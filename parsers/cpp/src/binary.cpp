// .synxb compact binary format — string interning + raw DEFLATE.
// Port of crates/synx-core/src/binary.rs.
#include "synx/binary.hpp"

#include "internal.hpp"
#include "unicode.hpp"

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

#if defined(SYNX_HAVE_ZLIB)
#  include <zlib.h>
#endif

namespace synx {

namespace {

constexpr uint8_t kMagic[5] = {'S', 'Y', 'N', 'X', 'B'};
[[maybe_unused]] constexpr uint8_t kFormatVersion = 1;

/// safety: an untrusted header may declare up to 4 GiB and a deflate stream
/// expands far past its own length; decoding is bounded instead.
[[maybe_unused]] constexpr size_t kMaxDecodedBytes = 256u * 1024 * 1024;
/// A hostile file can nest arrays/objects arbitrarily; text caps at 128.
constexpr size_t kMaxDecodeDepth = 256;

constexpr uint8_t FLAG_ACTIVE = 0x01;
constexpr uint8_t FLAG_LOCKED = 0x02;
constexpr uint8_t FLAG_HAS_META = 0x04;
constexpr uint8_t FLAG_RESOLVED = 0x08;
constexpr uint8_t FLAG_TOOL = 0x10;
constexpr uint8_t FLAG_SCHEMA = 0x20;
constexpr uint8_t FLAG_LLM = 0x40;

constexpr uint8_t TAG_NULL = 0x00;
constexpr uint8_t TAG_FALSE = 0x01;
constexpr uint8_t TAG_TRUE = 0x02;
constexpr uint8_t TAG_INT = 0x03;
constexpr uint8_t TAG_FLOAT = 0x04;
constexpr uint8_t TAG_STRING = 0x05;
constexpr uint8_t TAG_ARRAY = 0x06;
constexpr uint8_t TAG_OBJECT = 0x07;
constexpr uint8_t TAG_SECRET = 0x08;

/// A count read from the file can never exceed the bytes left (every element
/// costs at least one); clamp reservations to that, and to 1024.
///
/// safety: capped at the bytes that remain alone, an 8 KB file of zeros still
/// reserved 128 MiB for a list it did not hold.
size_t safe_capacity(uint64_t count, size_t remaining) noexcept {
    return static_cast<size_t>(std::min<uint64_t>(std::min<uint64_t>(count, remaining), 1024));
}

/// What one decoded value costs against kMaxDecodedBytes, text aside.
constexpr uint64_t kDecodedValueCost = 64;
/// What one string table entry costs, text aside.
constexpr uint64_t kStringEntryCost = 16;

/// What decoding has built so far, in bytes.
///
/// safety: a value is one byte on the wire and tens of bytes in memory, a
/// string is stored once and referenced any number of times, and deflate
/// shrinks a run of either a thousandfold — 171 bytes of file decoded to
/// 2 GiB. The tree shares the payload's limit: every value at a fixed cost,
/// plus the bytes of each text it uses.
thread_local uint64_t t_decoded = 0;

bool charge(uint64_t n, std::string& err) {
    t_decoded += n;
    if (t_decoded > kMaxDecodedBytes) {
        err = "decoded document exceeds " + std::to_string(kMaxDecodedBytes) + " bytes \xE2\x80\x94 refusing to decode";
        return false;
    }
    return true;
}

// ─── Varint (LEB128) / zigzag ───────────────────────────────────────────────

void encode_varint(std::vector<uint8_t>& out, uint64_t val) {
    while (true) {
        uint8_t byte = static_cast<uint8_t>(val & 0x7F);
        val >>= 7;
        if (val == 0) {
            out.push_back(byte);
            return;
        }
        out.push_back(byte | 0x80);
    }
}

bool decode_varint(const uint8_t* data, size_t len, size_t& pos, uint64_t& out, std::string& err) {
    uint64_t result = 0;
    uint32_t shift = 0;
    while (true) {
        if (pos >= len) {
            err = "unexpected end of data in varint";
            return false;
        }
        uint8_t byte = data[pos++];
        result |= static_cast<uint64_t>(byte & 0x7F) << shift;
        if ((byte & 0x80) == 0) {
            out = result;
            return true;
        }
        shift += 7;
        if (shift >= 64) {
            err = "varint overflow";
            return false;
        }
    }
}

uint64_t zigzag_encode(int64_t n) noexcept {
    return (static_cast<uint64_t>(n) << 1) ^ static_cast<uint64_t>(n >> 63);
}

int64_t zigzag_decode(uint64_t n) noexcept {
    return static_cast<int64_t>(n >> 1) ^ -static_cast<int64_t>(n & 1);
}

void encode_f64_le(std::vector<uint8_t>& out, double f) {
    uint64_t bits = 0;
    std::memcpy(&bits, &f, sizeof(bits));
    for (int i = 0; i < 8; ++i) out.push_back(static_cast<uint8_t>(bits >> (8 * i)));
}

bool decode_f64_le(const uint8_t* data, size_t len, size_t& pos, double& out, std::string& err, const char* what) {
    if (pos > len || len - pos < 8) {
        err = what;
        return false;
    }
    uint64_t bits = 0;
    for (int i = 0; i < 8; ++i) bits |= static_cast<uint64_t>(data[pos + static_cast<size_t>(i)]) << (8 * i);
    std::memcpy(&out, &bits, sizeof(out));
    pos += 8;
    return true;
}

// ─── String table ───────────────────────────────────────────────────────────

struct StringTable {
    std::vector<std::string> strings;
    std::unordered_map<std::string, uint32_t> index;
    /// How many times each string is referenced, by first-seen index.
    std::vector<uint32_t> uses;

    void intern(const std::string& s) {
        auto it = index.find(s);
        if (it != index.end()) {
            ++uses[it->second];
            return;
        }
        index.emplace(s, static_cast<uint32_t>(strings.size()));
        strings.push_back(s);
        uses.push_back(1);
    }

    /// Renumber by descending use count, ties in first-seen order: frequent
    /// strings get one-byte indexes, and the same document always compiles to
    /// the same bytes (no hash order is involved).
    void order_by_use() {
        std::vector<uint32_t> order(strings.size());
        for (uint32_t i = 0; i < order.size(); ++i) order[i] = i;
        std::stable_sort(order.begin(), order.end(), [&](uint32_t a, uint32_t b) { return uses[a] > uses[b]; });
        std::vector<std::string> sorted;
        sorted.reserve(strings.size());
        for (uint32_t new_idx = 0; new_idx < order.size(); ++new_idx) {
            std::string& str = strings[order[new_idx]];
            index[str] = new_idx;
            sorted.push_back(std::move(str));
        }
        strings = std::move(sorted);
    }

    uint32_t get(const std::string& s) const {
        auto it = index.find(s);
        return it == index.end() ? 0 : it->second;
    }

    void collect_value(const Value& v) {
        switch (v.kind()) {
            case Value::Kind::String: intern(*v.as_string()); return;
            case Value::Kind::Secret: intern(*v.as_secret()); return;
            case Value::Kind::Array:
                for (const auto& item : *v.as_array()) collect_value(item);
                return;
            case Value::Kind::Object:
                for (const auto& p : *v.as_object()) {
                    intern(p.key);
                    collect_value(p.value);
                }
                return;
            default:
                return;
        }
    }

    void collect_metadata(const MetadataTree& metadata) {
        for (const auto& outer : metadata) {
            intern(outer.first);
            for (const auto& field : outer.second) {
                intern(field.first);
                const Meta& m = field.second;
                for (const auto& mk : m.markers) intern(mk);
                for (const auto& a : m.args) intern(a);
                if (m.type_hint) intern(*m.type_hint);
                if (m.constraints) {
                    const Constraints& c = *m.constraints;
                    if (c.type_name) intern(*c.type_name);
                    if (c.pattern) intern(*c.pattern);
                    if (c.enum_values) {
                        for (const auto& ev : *c.enum_values) intern(ev);
                    }
                }
            }
        }
    }

    void collect_includes(const std::vector<IncludeDirective>& incs) {
        for (const auto& inc : incs) {
            intern(inc.path);
            intern(inc.alias);
        }
    }

    void encode(std::vector<uint8_t>& out) const {
        encode_varint(out, strings.size());
        for (const auto& s : strings) {
            encode_varint(out, s.size());
            out.insert(out.end(), s.begin(), s.end());
        }
    }
};

struct StringTableReader {
    std::vector<std::string> strings;

    bool decode(const uint8_t* data, size_t len, size_t& pos, std::string& err) {
        uint64_t count = 0;
        if (!decode_varint(data, len, pos, count, err)) return false;
        strings.reserve(safe_capacity(count, len - pos));
        for (uint64_t i = 0; i < count; ++i) {
            uint64_t slen = 0;
            if (!decode_varint(data, len, pos, slen, err)) return false;
            if (slen > len - pos) {
                err = "unexpected end of data in string table";
                return false;
            }
            if (!charge(kStringEntryCost + slen, err)) return false;
            std::string str(reinterpret_cast<const char*>(data + pos), static_cast<size_t>(slen));
            if (!detail::valid_utf8(str)) {
                err = "invalid UTF-8 in string table";
                return false;
            }
            strings.push_back(std::move(str));
            pos += static_cast<size_t>(slen);
        }
        return true;
    }

    bool get(uint64_t idx, std::string& out, std::string& err) const {
        if (idx >= strings.size()) {
            err = "string index " + std::to_string(static_cast<uint32_t>(idx)) + " out of bounds (size "
                + std::to_string(strings.size()) + ")";
            return false;
        }
        if (!charge(strings[static_cast<size_t>(idx)].size(), err)) return false;
        out = strings[static_cast<size_t>(idx)];
        return true;
    }

    bool read(const uint8_t* data, size_t len, size_t& pos, std::string& out, std::string& err) const {
        uint64_t idx = 0;
        return decode_varint(data, len, pos, idx, err) && get(idx, out, err);
    }
};

std::vector<const Pair*> sorted_entries(const Object& obj) {
    std::vector<const Pair*> entries;
    entries.reserve(obj.size());
    for (const auto& p : obj) entries.push_back(&p);
    std::sort(entries.begin(), entries.end(), [](const Pair* a, const Pair* b) { return a->key < b->key; });
    return entries;
}

template <typename V>
std::vector<const std::pair<std::string, V>*> sorted_entries(const IndexMap<V>& map) {
    std::vector<const std::pair<std::string, V>*> entries;
    entries.reserve(map.size());
    for (const auto& e : map) entries.push_back(&e);
    std::sort(entries.begin(), entries.end(), [](const auto* a, const auto* b) { return a->first < b->first; });
    return entries;
}

// ─── Value encode / decode ──────────────────────────────────────────────────

void encode_value(std::vector<uint8_t>& out, const Value& v, const StringTable& st) {
    switch (v.kind()) {
        case Value::Kind::Null: out.push_back(TAG_NULL); return;
        case Value::Kind::Bool: out.push_back(*v.as_bool() ? TAG_TRUE : TAG_FALSE); return;
        case Value::Kind::Int:
            out.push_back(TAG_INT);
            encode_varint(out, zigzag_encode(*v.as_int()));
            return;
        case Value::Kind::Float:
            out.push_back(TAG_FLOAT);
            encode_f64_le(out, *v.as_float());
            return;
        case Value::Kind::String:
            out.push_back(TAG_STRING);
            encode_varint(out, st.get(*v.as_string()));
            return;
        case Value::Kind::Secret:
            out.push_back(TAG_SECRET);
            encode_varint(out, st.get(*v.as_secret()));
            return;
        case Value::Kind::Array: {
            const Array& a = *v.as_array();
            out.push_back(TAG_ARRAY);
            encode_varint(out, a.size());
            for (const auto& item : a) encode_value(out, item, st);
            return;
        }
        case Value::Kind::Object: {
            out.push_back(TAG_OBJECT);
            std::vector<const Pair*> entries = sorted_entries(*v.as_object());
            encode_varint(out, entries.size());
            for (const Pair* p : entries) {
                encode_varint(out, st.get(p->key));
                encode_value(out, p->value, st);
            }
            return;
        }
    }
}

bool decode_value(const uint8_t* data, size_t len, size_t& pos, const StringTableReader& st, Value& out,
                  std::string& err, size_t depth) {
    if (depth > kMaxDecodeDepth) {
        err = "nesting exceeds " + std::to_string(kMaxDecodeDepth) + " \xE2\x80\x94 refusing to decode";
        return false;
    }
    if (!charge(kDecodedValueCost, err)) return false;
    if (pos >= len) {
        err = "unexpected end of data";
        return false;
    }
    uint8_t tag = data[pos++];
    switch (tag) {
        case TAG_NULL: out = Value::make_null(); return true;
        case TAG_FALSE: out = Value::make_bool(false); return true;
        case TAG_TRUE: out = Value::make_bool(true); return true;
        case TAG_INT: {
            uint64_t raw = 0;
            if (!decode_varint(data, len, pos, raw, err)) return false;
            out = Value::make_int(zigzag_decode(raw));
            return true;
        }
        case TAG_FLOAT: {
            double f = 0.0;
            if (!decode_f64_le(data, len, pos, f, err, "unexpected end of data in float")) return false;
            out = Value::make_float(f);
            return true;
        }
        case TAG_STRING:
        case TAG_SECRET: {
            std::string s;
            if (!st.read(data, len, pos, s, err)) return false;
            out = tag == TAG_STRING ? Value::make_string(std::move(s)) : Value::make_secret(std::move(s));
            return true;
        }
        case TAG_ARRAY: {
            uint64_t count = 0;
            if (!decode_varint(data, len, pos, count, err)) return false;
            Array arr;
            arr.reserve(safe_capacity(count, len - pos));
            for (uint64_t i = 0; i < count; ++i) {
                Value item;
                if (!decode_value(data, len, pos, st, item, err, depth + 1)) return false;
                arr.push_back(std::move(item));
            }
            out = Value::make_array(std::move(arr));
            return true;
        }
        case TAG_OBJECT: {
            uint64_t count = 0;
            if (!decode_varint(data, len, pos, count, err)) return false;
            // A repeated key keeps its first position and its last value.
            IndexMap<Value> map;
            for (uint64_t i = 0; i < count; ++i) {
                std::string key;
                if (!st.read(data, len, pos, key, err)) return false;
                Value v;
                if (!decode_value(data, len, pos, st, v, err, depth + 1)) return false;
                map.insert_or_assign(std::move(key), std::move(v));
            }
            Object obj;
            obj.reserve(map.size());
            for (auto& e : map) obj.push_back(Pair{e.first, std::move(e.second)});
            out = Value::make_object(std::move(obj));
            return true;
        }
        default: {
            char buf[48];
            std::snprintf(buf, sizeof(buf), "unknown type tag: 0x%02x", tag);
            err = buf;
            return false;
        }
    }
}

// ─── Metadata encode / decode ───────────────────────────────────────────────

void encode_constraints(std::vector<uint8_t>& out, const Constraints& c, const StringTable& st) {
    uint8_t bits = 0;
    if (c.min) bits |= 0x01;
    if (c.max) bits |= 0x02;
    if (c.type_name) bits |= 0x04;
    if (c.required) bits |= 0x08;
    if (c.readonly) bits |= 0x10;
    if (c.pattern) bits |= 0x20;
    if (c.enum_values) bits |= 0x40;
    out.push_back(bits);
    if (c.min) encode_f64_le(out, *c.min);
    if (c.max) encode_f64_le(out, *c.max);
    if (c.type_name) encode_varint(out, st.get(*c.type_name));
    if (c.pattern) encode_varint(out, st.get(*c.pattern));
    if (c.enum_values) {
        encode_varint(out, c.enum_values->size());
        for (const auto& v : *c.enum_values) encode_varint(out, st.get(v));
    }
}

bool decode_constraints(const uint8_t* data, size_t len, size_t& pos, const StringTableReader& st, Constraints& out,
                        std::string& err) {
    if (pos >= len) {
        err = "unexpected end of data in constraints";
        return false;
    }
    uint8_t bits = data[pos++];
    if (bits & 0x01) {
        double v = 0.0;
        if (!decode_f64_le(data, len, pos, v, err, "truncated min")) return false;
        out.min = v;
    }
    if (bits & 0x02) {
        double v = 0.0;
        if (!decode_f64_le(data, len, pos, v, err, "truncated max")) return false;
        out.max = v;
    }
    if (bits & 0x04) {
        std::string s;
        if (!st.read(data, len, pos, s, err)) return false;
        out.type_name = std::move(s);
    }
    if (bits & 0x08) out.required = true;
    if (bits & 0x10) out.readonly = true;
    if (bits & 0x20) {
        std::string s;
        if (!st.read(data, len, pos, s, err)) return false;
        out.pattern = std::move(s);
    }
    if (bits & 0x40) {
        uint64_t count = 0;
        if (!decode_varint(data, len, pos, count, err)) return false;
        std::vector<std::string> vals;
        vals.reserve(safe_capacity(count, len - pos));
        for (uint64_t i = 0; i < count; ++i) {
            std::string s;
            if (!st.read(data, len, pos, s, err)) return false;
            vals.push_back(std::move(s));
        }
        out.enum_values = std::move(vals);
    }
    return true;
}

void encode_meta(std::vector<uint8_t>& out, const Meta& m, const StringTable& st) {
    encode_varint(out, m.markers.size());
    for (const auto& mk : m.markers) encode_varint(out, st.get(mk));
    encode_varint(out, m.args.size());
    for (const auto& a : m.args) encode_varint(out, st.get(a));
    if (m.type_hint) {
        out.push_back(1);
        encode_varint(out, st.get(*m.type_hint));
    } else {
        out.push_back(0);
    }
    if (m.constraints) {
        out.push_back(1);
        encode_constraints(out, *m.constraints, st);
    } else {
        out.push_back(0);
    }
}

bool decode_strings(const uint8_t* data, size_t len, size_t& pos, const StringTableReader& st,
                    std::vector<std::string>& out, std::string& err) {
    uint64_t count = 0;
    if (!decode_varint(data, len, pos, count, err)) return false;
    out.reserve(safe_capacity(count, len - pos));
    for (uint64_t i = 0; i < count; ++i) {
        std::string s;
        if (!charge(kDecodedValueCost, err) || !st.read(data, len, pos, s, err)) return false;
        out.push_back(std::move(s));
    }
    return true;
}

bool decode_meta(const uint8_t* data, size_t len, size_t& pos, const StringTableReader& st, Meta& m,
                 std::string& err) {
    if (!decode_strings(data, len, pos, st, m.markers, err)) return false;
    if (!decode_strings(data, len, pos, st, m.args, err)) return false;
    if (pos >= len) {
        err = "unexpected end in meta";
        return false;
    }
    if (data[pos++] != 0) {
        std::string s;
        if (!st.read(data, len, pos, s, err)) return false;
        m.type_hint = std::move(s);
    }
    if (pos >= len) {
        err = "unexpected end in meta";
        return false;
    }
    if (data[pos++] != 0) {
        Constraints c;
        if (!decode_constraints(data, len, pos, st, c, err)) return false;
        m.constraints = std::move(c);
    }
    return true;
}

void encode_metadata(std::vector<uint8_t>& out, const MetadataTree& metadata, const StringTable& st) {
    auto outer = sorted_entries(metadata);
    encode_varint(out, outer.size());
    for (const auto* path : outer) {
        encode_varint(out, st.get(path->first));
        auto inner = sorted_entries(path->second);
        encode_varint(out, inner.size());
        for (const auto* field : inner) {
            encode_varint(out, st.get(field->first));
            encode_meta(out, field->second, st);
        }
    }
}

bool decode_metadata(const uint8_t* data, size_t len, size_t& pos, const StringTableReader& st, MetadataTree& out,
                     std::string& err) {
    uint64_t outer = 0;
    if (!decode_varint(data, len, pos, outer, err)) return false;
    for (uint64_t i = 0; i < outer; ++i) {
        std::string path;
        if (!charge(kDecodedValueCost, err) || !st.read(data, len, pos, path, err)) return false;
        uint64_t inner = 0;
        if (!decode_varint(data, len, pos, inner, err)) return false;
        MetaMap mm;
        for (uint64_t j = 0; j < inner; ++j) {
            std::string field;
            if (!charge(kDecodedValueCost, err) || !st.read(data, len, pos, field, err)) return false;
            Meta m;
            if (!decode_meta(data, len, pos, st, m, err)) return false;
            mm.insert_or_assign(std::move(field), std::move(m));
        }
        out.insert_or_assign(std::move(path), std::move(mm));
    }
    return true;
}

void encode_includes(std::vector<uint8_t>& out, const std::vector<IncludeDirective>& incs, const StringTable& st) {
    encode_varint(out, incs.size());
    for (const auto& inc : incs) {
        encode_varint(out, st.get(inc.path));
        encode_varint(out, st.get(inc.alias));
    }
}

bool decode_includes(const uint8_t* data, size_t len, size_t& pos, const StringTableReader& st,
                     std::vector<IncludeDirective>& out, std::string& err) {
    uint64_t count = 0;
    if (!decode_varint(data, len, pos, count, err)) return false;
    out.reserve(safe_capacity(count, len - pos));
    for (uint64_t i = 0; i < count; ++i) {
        std::string path, alias;
        if (!charge(kDecodedValueCost, err) || !st.read(data, len, pos, path, err)
            || !st.read(data, len, pos, alias, err)) {
            return false;
        }
        out.push_back(IncludeDirective{std::move(path), std::move(alias)});
    }
    return true;
}

// ─── DEFLATE wrappers (raw, no zlib header) ─────────────────────────────────
#if defined(SYNX_HAVE_ZLIB)
bool deflate_raw(const std::vector<uint8_t>& in, std::vector<uint8_t>& out, std::string& err) {
    z_stream zs{};
    // -15 windowBits → raw DEFLATE (no zlib wrapper), as miniz_oxide writes;
    // level 9: a storage format favours size.
    if (deflateInit2(&zs, 9, Z_DEFLATED, -15, 8, Z_DEFAULT_STRATEGY) != Z_OK) {
        err = "deflate init failed";
        return false;
    }
    zs.next_in = const_cast<Bytef*>(in.data());
    zs.avail_in = static_cast<uInt>(in.size());
    out.resize(deflateBound(&zs, static_cast<uLong>(in.size())));
    zs.next_out = out.data();
    zs.avail_out = static_cast<uInt>(out.size());
    int rc = deflate(&zs, Z_FINISH);
    if (rc != Z_STREAM_END) {
        deflateEnd(&zs);
        err = "deflate failed";
        return false;
    }
    out.resize(zs.total_out);
    deflateEnd(&zs);
    return true;
}

bool inflate_raw(const uint8_t* in, size_t in_len, size_t expected, std::vector<uint8_t>& out, std::string& err) {
    z_stream zs{};
    if (inflateInit2(&zs, -15) != Z_OK) {
        err = "inflate init failed";
        return false;
    }
    zs.next_in = const_cast<Bytef*>(in);
    zs.avail_in = static_cast<uInt>(in_len);
    // One byte past the declared size, so a longer stream is caught as a mismatch.
    //
    // safety: the buffer grows with what the stream produces. It was reserved
    // from the declared size before a byte was inflated — 13 bytes of file
    // declaring 256 MiB cost 256 MiB each.
    const size_t limit = expected + 1;
    out.clear();
    out.reserve(std::min<size_t>(limit, 64 * 1024));
    uint8_t chunk[16 * 1024];
    int rc = Z_OK;
    while (out.size() < limit) {
        zs.next_out = chunk;
        zs.avail_out = static_cast<uInt>(std::min<size_t>(sizeof(chunk), limit - out.size()));
        const uInt room = zs.avail_out;
        rc = inflate(&zs, Z_NO_FLUSH);
        out.insert(out.end(), chunk, chunk + (room - zs.avail_out));
        if (rc == Z_STREAM_END) break;
        if (rc != Z_OK && !(rc == Z_BUF_ERROR && zs.avail_out == 0)) {
            inflateEnd(&zs);
            err = "decompression failed";
            return false;
        }
        if (zs.avail_in == 0 && zs.avail_out != 0) {
            // The input ended inside the stream.
            inflateEnd(&zs);
            err = "decompression failed";
            return false;
        }
    }
    inflateEnd(&zs);
    return true;
}
#endif

} // namespace

namespace detail {

std::vector<uint8_t> encode_synxb_payload(const ParseResult& result, bool resolved) {
    StringTable st;
    st.collect_value(result.root);
    const bool has_meta = !resolved && !result.metadata.empty();
    if (has_meta) {
        st.collect_metadata(result.metadata);
        st.collect_includes(result.includes);
    }
    st.order_by_use();
    std::vector<uint8_t> payload;
    payload.reserve(1024);
    st.encode(payload);
    encode_value(payload, result.root, st);
    if (has_meta) {
        encode_metadata(payload, result.metadata, st);
        encode_includes(payload, result.includes, st);
    }
    return payload;
}

uint8_t synxb_flags(const ParseResult& result, bool resolved) noexcept {
    uint8_t flags = 0;
    if (result.mode == Mode::Active) flags |= FLAG_ACTIVE;
    if (result.locked) flags |= FLAG_LOCKED;
    if (!resolved && !result.metadata.empty()) flags |= FLAG_HAS_META;
    if (resolved) flags |= FLAG_RESOLVED;
    if (result.tool) flags |= FLAG_TOOL;
    if (result.schema) flags |= FLAG_SCHEMA;
    if (result.llm) flags |= FLAG_LLM;
    return flags;
}

Result<ParseResult> decode_synxb_payload(uint8_t flags, const uint8_t* payload, size_t len) {
    std::string err;
    size_t pos = 0;
    t_decoded = 0;
    StringTableReader st;
    if (!st.decode(payload, len, pos, err)) return Result<ParseResult>::from_error(std::move(err));
    ParseResult out;
    if (!decode_value(payload, len, pos, st, out.root, err, 0)) return Result<ParseResult>::from_error(std::move(err));
    out.mode = (flags & FLAG_ACTIVE) ? Mode::Active : Mode::Static;
    out.locked = (flags & FLAG_LOCKED) != 0;
    out.tool = (flags & FLAG_TOOL) != 0;
    out.schema = (flags & FLAG_SCHEMA) != 0;
    out.llm = (flags & FLAG_LLM) != 0;
    if (flags & FLAG_HAS_META) {
        if (!decode_metadata(payload, len, pos, st, out.metadata, err)
            || !decode_includes(payload, len, pos, st, out.includes, err)) {
            return Result<ParseResult>::from_error(std::move(err));
        }
    }
    return Result<ParseResult>(std::move(out));
}

} // namespace detail

// ─── Public API ─────────────────────────────────────────────────────────────

bool is_synxb(const uint8_t* data, size_t len) noexcept {
    return len >= 5 && std::memcmp(data, kMagic, 5) == 0;
}
bool is_synxb(const std::vector<uint8_t>& data) noexcept { return is_synxb(data.data(), data.size()); }

Result<std::vector<uint8_t>> compile(const ParseResult& result, bool resolved) {
#if !defined(SYNX_HAVE_ZLIB)
    (void)result;
    (void)resolved;
    return Result<std::vector<uint8_t>>::from_error("synx: compile() requires SYNX_HAVE_ZLIB (link zlib at build time)");
#else
    std::vector<uint8_t> payload = detail::encode_synxb_payload(result, resolved);
    std::vector<uint8_t> compressed;
    std::string err;
    if (!deflate_raw(payload, compressed, err)) return Result<std::vector<uint8_t>>::from_error(std::move(err));
    std::vector<uint8_t> out;
    out.reserve(11 + compressed.size());
    out.insert(out.end(), kMagic, kMagic + 5);
    out.push_back(kFormatVersion);
    out.push_back(detail::synxb_flags(result, resolved));
    const uint32_t size = static_cast<uint32_t>(payload.size());
    for (int i = 0; i < 4; ++i) out.push_back(static_cast<uint8_t>(size >> (8 * i)));
    out.insert(out.end(), compressed.begin(), compressed.end());
    return Result<std::vector<uint8_t>>(std::move(out));
#endif
}

Result<ParseResult> decompile(const uint8_t* data, size_t len) {
#if !defined(SYNX_HAVE_ZLIB)
    (void)data;
    (void)len;
    return Result<ParseResult>::from_error("synx: decompile() requires SYNX_HAVE_ZLIB (link zlib at build time)");
#else
    if (len < 11) return Result<ParseResult>::from_error("file too small for .synxb header");
    if (std::memcmp(data, kMagic, 5) != 0) return Result<ParseResult>::from_error("invalid .synxb magic (expected SYNXB)");
    if (data[5] != kFormatVersion) {
        return Result<ParseResult>::from_error("unsupported .synxb version: " + std::to_string(data[5]) + " (expected "
                                               + std::to_string(kFormatVersion) + ")");
    }
    const uint8_t flags = data[6];
    const size_t declared = static_cast<size_t>(data[7]) | (static_cast<size_t>(data[8]) << 8)
                          | (static_cast<size_t>(data[9]) << 16) | (static_cast<size_t>(data[10]) << 24);
    if (declared > kMaxDecodedBytes) {
        return Result<ParseResult>::from_error("declared size " + std::to_string(declared) + " exceeds limit "
                                               + std::to_string(kMaxDecodedBytes));
    }
    std::vector<uint8_t> payload;
    std::string err;
    if (!inflate_raw(data + 11, len - 11, declared, payload, err)) return Result<ParseResult>::from_error(std::move(err));
    if (payload.size() != declared) {
        return Result<ParseResult>::from_error("size mismatch: expected " + std::to_string(declared) + ", got "
                                               + std::to_string(payload.size()));
    }
    return detail::decode_synxb_payload(flags, payload.data(), payload.size());
#endif
}

Result<ParseResult> decompile(const std::vector<uint8_t>& data) { return decompile(data.data(), data.size()); }

} // namespace synx
