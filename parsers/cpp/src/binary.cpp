// .synxb compact binary format — string interning + raw DEFLATE.
// Mirrors crates/synx-core/src/binary.rs.
#include "synx/binary.hpp"

#include <algorithm>
#include <cstdint>
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

constexpr uint8_t kMagic[5]      = {'S', 'Y', 'N', 'X', 'B'};
constexpr uint8_t kFormatVersion = 1;

constexpr uint8_t FLAG_ACTIVE   = 0x01;
constexpr uint8_t FLAG_LOCKED   = 0x02;
constexpr uint8_t FLAG_HAS_META = 0x04;
constexpr uint8_t FLAG_RESOLVED = 0x08;
constexpr uint8_t FLAG_TOOL     = 0x10;
constexpr uint8_t FLAG_SCHEMA   = 0x20;
constexpr uint8_t FLAG_LLM      = 0x40;

constexpr uint8_t TAG_NULL   = 0x00;
constexpr uint8_t TAG_FALSE  = 0x01;
constexpr uint8_t TAG_TRUE   = 0x02;
constexpr uint8_t TAG_INT    = 0x03;
constexpr uint8_t TAG_FLOAT  = 0x04;
constexpr uint8_t TAG_STRING = 0x05;
constexpr uint8_t TAG_ARRAY  = 0x06;
constexpr uint8_t TAG_OBJECT = 0x07;
constexpr uint8_t TAG_SECRET = 0x08;

// ─── Varint (LEB128) ────────────────────────────────────────────────────────
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
        result |= (static_cast<uint64_t>(byte & 0x7F) << shift);
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
    return static_cast<uint64_t>((n << 1) ^ (n >> 63));
}

int64_t zigzag_decode(uint64_t n) noexcept {
    return static_cast<int64_t>(n >> 1) ^ -static_cast<int64_t>(n & 1);
}

void append_bytes(std::vector<uint8_t>& out, const void* data, size_t len) {
    const uint8_t* p = static_cast<const uint8_t*>(data);
    out.insert(out.end(), p, p + len);
}

void encode_f64_le(std::vector<uint8_t>& out, double f) {
    uint8_t buf[8];
    std::memcpy(buf, &f, sizeof(double));
    out.insert(out.end(), buf, buf + 8);
}

bool decode_f64_le(const uint8_t* data, size_t len, size_t& pos, double& out, std::string& err) {
    if (pos + 8 > len) {
        err = "unexpected end of data in float";
        return false;
    }
    std::memcpy(&out, data + pos, sizeof(double));
    pos += 8;
    return true;
}

// ─── String table ───────────────────────────────────────────────────────────
struct StringTable {
    std::vector<std::string> strings;
    std::unordered_map<std::string, uint32_t> index;

    uint32_t intern(const std::string& s) {
        auto it = index.find(s);
        if (it != index.end()) return it->second;
        uint32_t idx = static_cast<uint32_t>(strings.size());
        strings.push_back(s);
        index.emplace(s, idx);
        return idx;
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
                if (m.type_hint.has_value()) intern(*m.type_hint);
                if (m.constraints.has_value()) {
                    const Constraints& c = *m.constraints;
                    if (c.type_name.has_value()) intern(*c.type_name);
                    if (c.pattern.has_value()) intern(*c.pattern);
                    if (c.enum_values.has_value()) {
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
            append_bytes(out, s.data(), s.size());
        }
    }
};

struct StringTableReader {
    std::vector<std::string> strings;

    bool decode(const uint8_t* data, size_t len, size_t& pos, std::string& err) {
        uint64_t count = 0;
        if (!decode_varint(data, len, pos, count, err)) return false;
        strings.reserve(static_cast<size_t>(count));
        for (uint64_t i = 0; i < count; ++i) {
            uint64_t slen = 0;
            if (!decode_varint(data, len, pos, slen, err)) return false;
            if (pos + slen > len) {
                err = "unexpected end of data in string table";
                return false;
            }
            strings.emplace_back(reinterpret_cast<const char*>(data + pos), slen);
            pos += slen;
        }
        return true;
    }

    bool get(uint32_t idx, std::string& out, std::string& err) const {
        if (idx >= strings.size()) {
            err = "string index out of bounds";
            return false;
        }
        out = strings[idx];
        return true;
    }
};

// ─── Value encode / decode ──────────────────────────────────────────────────
void encode_value(std::vector<uint8_t>& out, const Value& v, const StringTable& st) {
    switch (v.kind()) {
        case Value::Kind::Null:  out.push_back(TAG_NULL);  return;
        case Value::Kind::Bool:  out.push_back(*v.as_bool() ? TAG_TRUE : TAG_FALSE); return;
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
            const Object& obj = *v.as_object();
            std::vector<const Pair*> entries;
            entries.reserve(obj.size());
            for (const auto& p : obj) entries.push_back(&p);
            std::sort(entries.begin(), entries.end(),
                      [](const Pair* a, const Pair* b) { return a->key < b->key; });
            encode_varint(out, entries.size());
            for (const Pair* p : entries) {
                encode_varint(out, st.get(p->key));
                encode_value(out, p->value, st);
            }
            return;
        }
    }
}

bool decode_value(const uint8_t* data, size_t len, size_t& pos,
                  const StringTableReader& st, Value& out, std::string& err);

bool decode_value(const uint8_t* data, size_t len, size_t& pos,
                  const StringTableReader& st, Value& out, std::string& err) {
    if (pos >= len) {
        err = "unexpected end of data";
        return false;
    }
    uint8_t tag = data[pos++];
    switch (tag) {
        case TAG_NULL:  out = Value::make_null();         return true;
        case TAG_FALSE: out = Value::make_bool(false);    return true;
        case TAG_TRUE:  out = Value::make_bool(true);     return true;
        case TAG_INT: {
            uint64_t raw = 0;
            if (!decode_varint(data, len, pos, raw, err)) return false;
            out = Value::make_int(zigzag_decode(raw));
            return true;
        }
        case TAG_FLOAT: {
            double f = 0.0;
            if (!decode_f64_le(data, len, pos, f, err)) return false;
            out = Value::make_float(f);
            return true;
        }
        case TAG_STRING: {
            uint64_t idx = 0;
            if (!decode_varint(data, len, pos, idx, err)) return false;
            std::string s;
            if (!st.get(static_cast<uint32_t>(idx), s, err)) return false;
            out = Value::make_string(std::move(s));
            return true;
        }
        case TAG_SECRET: {
            uint64_t idx = 0;
            if (!decode_varint(data, len, pos, idx, err)) return false;
            std::string s;
            if (!st.get(static_cast<uint32_t>(idx), s, err)) return false;
            out = Value::make_secret(std::move(s));
            return true;
        }
        case TAG_ARRAY: {
            uint64_t count = 0;
            if (!decode_varint(data, len, pos, count, err)) return false;
            Array arr;
            arr.reserve(static_cast<size_t>(count));
            for (uint64_t i = 0; i < count; ++i) {
                Value item;
                if (!decode_value(data, len, pos, st, item, err)) return false;
                arr.push_back(std::move(item));
            }
            out = Value::make_array(std::move(arr));
            return true;
        }
        case TAG_OBJECT: {
            uint64_t count = 0;
            if (!decode_varint(data, len, pos, count, err)) return false;
            Object obj;
            obj.reserve(static_cast<size_t>(count));
            for (uint64_t i = 0; i < count; ++i) {
                uint64_t key_idx = 0;
                if (!decode_varint(data, len, pos, key_idx, err)) return false;
                std::string key;
                if (!st.get(static_cast<uint32_t>(key_idx), key, err)) return false;
                Value v;
                if (!decode_value(data, len, pos, st, v, err)) return false;
                obj.push_back(Pair{std::move(key), std::move(v)});
            }
            out = Value::make_object(std::move(obj));
            return true;
        }
        default:
            err = "unknown type tag";
            return false;
    }
}

// ─── Metadata encode / decode ───────────────────────────────────────────────
void encode_constraints(std::vector<uint8_t>& out, const Constraints& c, const StringTable& st) {
    uint8_t bits = 0;
    if (c.min.has_value())         bits |= 0x01;
    if (c.max.has_value())         bits |= 0x02;
    if (c.type_name.has_value())   bits |= 0x04;
    if (c.required)                bits |= 0x08;
    if (c.readonly)                bits |= 0x10;
    if (c.pattern.has_value())     bits |= 0x20;
    if (c.enum_values.has_value()) bits |= 0x40;
    out.push_back(bits);

    if (c.min.has_value())       encode_f64_le(out, *c.min);
    if (c.max.has_value())       encode_f64_le(out, *c.max);
    if (c.type_name.has_value()) encode_varint(out, st.get(*c.type_name));
    if (c.pattern.has_value())   encode_varint(out, st.get(*c.pattern));
    if (c.enum_values.has_value()) {
        encode_varint(out, c.enum_values->size());
        for (const auto& v : *c.enum_values) encode_varint(out, st.get(v));
    }
}

bool decode_constraints(const uint8_t* data, size_t len, size_t& pos,
                        const StringTableReader& st, Constraints& out, std::string& err) {
    if (pos >= len) { err = "unexpected end in constraints"; return false; }
    uint8_t bits = data[pos++];
    if (bits & 0x01) {
        double v = 0.0;
        if (!decode_f64_le(data, len, pos, v, err)) return false;
        out.min = v;
    }
    if (bits & 0x02) {
        double v = 0.0;
        if (!decode_f64_le(data, len, pos, v, err)) return false;
        out.max = v;
    }
    if (bits & 0x04) {
        uint64_t idx = 0;
        if (!decode_varint(data, len, pos, idx, err)) return false;
        std::string s;
        if (!st.get(static_cast<uint32_t>(idx), s, err)) return false;
        out.type_name = std::move(s);
    }
    if (bits & 0x08) out.required = true;
    if (bits & 0x10) out.readonly = true;
    if (bits & 0x20) {
        uint64_t idx = 0;
        if (!decode_varint(data, len, pos, idx, err)) return false;
        std::string s;
        if (!st.get(static_cast<uint32_t>(idx), s, err)) return false;
        out.pattern = std::move(s);
    }
    if (bits & 0x40) {
        uint64_t count = 0;
        if (!decode_varint(data, len, pos, count, err)) return false;
        std::vector<std::string> vals;
        vals.reserve(static_cast<size_t>(count));
        for (uint64_t i = 0; i < count; ++i) {
            uint64_t idx = 0;
            if (!decode_varint(data, len, pos, idx, err)) return false;
            std::string s;
            if (!st.get(static_cast<uint32_t>(idx), s, err)) return false;
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
    if (m.type_hint.has_value()) {
        out.push_back(1);
        encode_varint(out, st.get(*m.type_hint));
    } else {
        out.push_back(0);
    }
    if (m.constraints.has_value()) {
        out.push_back(1);
        encode_constraints(out, *m.constraints, st);
    } else {
        out.push_back(0);
    }
}

bool decode_meta(const uint8_t* data, size_t len, size_t& pos,
                 const StringTableReader& st, Meta& m, std::string& err) {
    uint64_t mc = 0;
    if (!decode_varint(data, len, pos, mc, err)) return false;
    m.markers.reserve(static_cast<size_t>(mc));
    for (uint64_t i = 0; i < mc; ++i) {
        uint64_t idx = 0;
        if (!decode_varint(data, len, pos, idx, err)) return false;
        std::string s;
        if (!st.get(static_cast<uint32_t>(idx), s, err)) return false;
        m.markers.push_back(std::move(s));
    }
    uint64_t ac = 0;
    if (!decode_varint(data, len, pos, ac, err)) return false;
    m.args.reserve(static_cast<size_t>(ac));
    for (uint64_t i = 0; i < ac; ++i) {
        uint64_t idx = 0;
        if (!decode_varint(data, len, pos, idx, err)) return false;
        std::string s;
        if (!st.get(static_cast<uint32_t>(idx), s, err)) return false;
        m.args.push_back(std::move(s));
    }
    if (pos >= len) { err = "unexpected end in meta (type_hint flag)"; return false; }
    uint8_t has_th = data[pos++];
    if (has_th) {
        uint64_t idx = 0;
        if (!decode_varint(data, len, pos, idx, err)) return false;
        std::string s;
        if (!st.get(static_cast<uint32_t>(idx), s, err)) return false;
        m.type_hint = std::move(s);
    }
    if (pos >= len) { err = "unexpected end in meta (constraints flag)"; return false; }
    uint8_t has_c = data[pos++];
    if (has_c) {
        Constraints c;
        if (!decode_constraints(data, len, pos, st, c, err)) return false;
        m.constraints = std::move(c);
    }
    return true;
}

[[maybe_unused]]
void encode_metadata(std::vector<uint8_t>& out, const MetadataTree& metadata, const StringTable& st) {
    std::vector<const std::string*> outer_keys;
    outer_keys.reserve(metadata.size());
    for (const auto& it : metadata) outer_keys.push_back(&it.first);
    std::sort(outer_keys.begin(), outer_keys.end(),
              [](const std::string* a, const std::string* b) { return *a < *b; });
    encode_varint(out, outer_keys.size());
    for (const std::string* path : outer_keys) {
        encode_varint(out, st.get(*path));
        const MetaMap& mm = metadata.at(*path);
        std::vector<const std::string*> inner_keys;
        inner_keys.reserve(mm.size());
        for (const auto& it : mm) inner_keys.push_back(&it.first);
        std::sort(inner_keys.begin(), inner_keys.end(),
                  [](const std::string* a, const std::string* b) { return *a < *b; });
        encode_varint(out, inner_keys.size());
        for (const std::string* fk : inner_keys) {
            encode_varint(out, st.get(*fk));
            encode_meta(out, mm.at(*fk), st);
        }
    }
}

[[maybe_unused]]
bool decode_metadata(const uint8_t* data, size_t len, size_t& pos,
                     const StringTableReader& st, MetadataTree& out, std::string& err) {
    uint64_t outer = 0;
    if (!decode_varint(data, len, pos, outer, err)) return false;
    out.reserve(static_cast<size_t>(outer));
    for (uint64_t i = 0; i < outer; ++i) {
        uint64_t path_idx = 0;
        if (!decode_varint(data, len, pos, path_idx, err)) return false;
        std::string path;
        if (!st.get(static_cast<uint32_t>(path_idx), path, err)) return false;
        uint64_t inner = 0;
        if (!decode_varint(data, len, pos, inner, err)) return false;
        MetaMap mm;
        mm.reserve(static_cast<size_t>(inner));
        for (uint64_t j = 0; j < inner; ++j) {
            uint64_t fk_idx = 0;
            if (!decode_varint(data, len, pos, fk_idx, err)) return false;
            std::string fk;
            if (!st.get(static_cast<uint32_t>(fk_idx), fk, err)) return false;
            Meta m;
            if (!decode_meta(data, len, pos, st, m, err)) return false;
            mm.emplace(std::move(fk), std::move(m));
        }
        out.emplace(std::move(path), std::move(mm));
    }
    return true;
}

[[maybe_unused]]
void encode_includes(std::vector<uint8_t>& out, const std::vector<IncludeDirective>& incs, const StringTable& st) {
    encode_varint(out, incs.size());
    for (const auto& inc : incs) {
        encode_varint(out, st.get(inc.path));
        encode_varint(out, st.get(inc.alias));
    }
}

[[maybe_unused]]
bool decode_includes(const uint8_t* data, size_t len, size_t& pos,
                     const StringTableReader& st, std::vector<IncludeDirective>& out, std::string& err) {
    uint64_t count = 0;
    if (!decode_varint(data, len, pos, count, err)) return false;
    out.reserve(static_cast<size_t>(count));
    for (uint64_t i = 0; i < count; ++i) {
        uint64_t pi = 0;
        if (!decode_varint(data, len, pos, pi, err)) return false;
        uint64_t ai = 0;
        if (!decode_varint(data, len, pos, ai, err)) return false;
        std::string path;
        std::string alias;
        if (!st.get(static_cast<uint32_t>(pi), path, err)) return false;
        if (!st.get(static_cast<uint32_t>(ai), alias, err)) return false;
        out.push_back(IncludeDirective{std::move(path), std::move(alias)});
    }
    return true;
}

// ─── DEFLATE wrappers (raw, no zlib header) ─────────────────────────────────
#if defined(SYNX_HAVE_ZLIB)
bool deflate_raw(const std::vector<uint8_t>& in, std::vector<uint8_t>& out, std::string& err) {
    z_stream zs{};
    // -15 windowBits → raw DEFLATE (no zlib wrapper) — matches Rust miniz_oxide output.
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

bool inflate_raw(const uint8_t* in, size_t in_len, size_t expected,
                 std::vector<uint8_t>& out, std::string& err) {
    z_stream zs{};
    if (inflateInit2(&zs, -15) != Z_OK) {
        err = "inflate init failed";
        return false;
    }
    zs.next_in = const_cast<Bytef*>(in);
    zs.avail_in = static_cast<uInt>(in_len);
    out.resize(expected);
    zs.next_out = out.data();
    zs.avail_out = static_cast<uInt>(out.size());
    int rc = inflate(&zs, Z_FINISH);
    if (rc != Z_STREAM_END) {
        inflateEnd(&zs);
        err = "decompression failed";
        return false;
    }
    out.resize(zs.total_out);
    inflateEnd(&zs);
    return true;
}
#endif

} // namespace

// ─── Public API ─────────────────────────────────────────────────────────────

bool is_synxb(const uint8_t* data, size_t len) noexcept {
    return len >= 5 && std::memcmp(data, kMagic, 5) == 0;
}
bool is_synxb(const std::vector<uint8_t>& data) noexcept {
    return is_synxb(data.data(), data.size());
}

Result<std::vector<uint8_t>> compile(const ParseResult& result, bool resolved) {
#if !defined(SYNX_HAVE_ZLIB)
    (void)result; (void)resolved;
    return Result<std::vector<uint8_t>>::from_error(
        "synx: compile() requires SYNX_HAVE_ZLIB (link zlib at build time)");
#else
    StringTable st;
    st.collect_value(result.root);
    bool has_meta = !resolved && !result.metadata.empty();
    if (has_meta) {
        st.collect_metadata(result.metadata);
        st.collect_includes(result.includes);
    }

    std::vector<uint8_t> payload;
    payload.reserve(1024);
    st.encode(payload);
    encode_value(payload, result.root, st);
    if (has_meta) {
        encode_metadata(payload, result.metadata, st);
        encode_includes(payload, result.includes, st);
    }

    std::vector<uint8_t> compressed;
    std::string err;
    if (!deflate_raw(payload, compressed, err)) {
        return Result<std::vector<uint8_t>>::from_error(std::move(err));
    }

    std::vector<uint8_t> out;
    out.reserve(11 + compressed.size());
    out.insert(out.end(), kMagic, kMagic + 5);
    out.push_back(kFormatVersion);

    uint8_t flags = 0;
    if (result.mode == Mode::Active) flags |= FLAG_ACTIVE;
    if (result.locked)               flags |= FLAG_LOCKED;
    if (has_meta)                    flags |= FLAG_HAS_META;
    if (resolved)                    flags |= FLAG_RESOLVED;
    if (result.tool)                 flags |= FLAG_TOOL;
    if (result.schema)               flags |= FLAG_SCHEMA;
    if (result.llm)                  flags |= FLAG_LLM;
    out.push_back(flags);

    uint32_t uncomp = static_cast<uint32_t>(payload.size());
    uint8_t size_bytes[4] = {
        static_cast<uint8_t>(uncomp & 0xFF),
        static_cast<uint8_t>((uncomp >> 8) & 0xFF),
        static_cast<uint8_t>((uncomp >> 16) & 0xFF),
        static_cast<uint8_t>((uncomp >> 24) & 0xFF),
    };
    out.insert(out.end(), size_bytes, size_bytes + 4);

    out.insert(out.end(), compressed.begin(), compressed.end());
    return Result<std::vector<uint8_t>>(std::move(out));
#endif
}

Result<ParseResult> decompile(const uint8_t* data, size_t len) {
#if !defined(SYNX_HAVE_ZLIB)
    (void)data; (void)len;
    return Result<ParseResult>::from_error(
        "synx: decompile() requires SYNX_HAVE_ZLIB (link zlib at build time)");
#else
    if (len < 11) {
        return Result<ParseResult>::from_error("file too small for .synxb header");
    }
    if (std::memcmp(data, kMagic, 5) != 0) {
        return Result<ParseResult>::from_error("invalid .synxb magic (expected SYNXB)");
    }
    if (data[5] != kFormatVersion) {
        return Result<ParseResult>::from_error("unsupported .synxb version");
    }
    uint8_t flags = data[6];

    uint32_t uncomp = static_cast<uint32_t>(data[7])
                    | (static_cast<uint32_t>(data[8]) << 8)
                    | (static_cast<uint32_t>(data[9]) << 16)
                    | (static_cast<uint32_t>(data[10]) << 24);

    std::vector<uint8_t> payload;
    std::string err;
    if (!inflate_raw(data + 11, len - 11, uncomp, payload, err)) {
        return Result<ParseResult>::from_error(std::move(err));
    }
    if (payload.size() != uncomp) {
        return Result<ParseResult>::from_error("size mismatch in decompressed payload");
    }

    size_t pos = 0;
    StringTableReader st;
    if (!st.decode(payload.data(), payload.size(), pos, err)) {
        return Result<ParseResult>::from_error(std::move(err));
    }

    ParseResult out;
    if (!decode_value(payload.data(), payload.size(), pos, st, out.root, err)) {
        return Result<ParseResult>::from_error(std::move(err));
    }
    out.mode = (flags & FLAG_ACTIVE) ? Mode::Active : Mode::Static;
    out.locked = (flags & FLAG_LOCKED) != 0;
    out.tool = (flags & FLAG_TOOL) != 0;
    out.schema = (flags & FLAG_SCHEMA) != 0;
    out.llm = (flags & FLAG_LLM) != 0;

    if (flags & FLAG_HAS_META) {
        if (!decode_metadata(payload.data(), payload.size(), pos, st, out.metadata, err)) {
            return Result<ParseResult>::from_error(std::move(err));
        }
        if (!decode_includes(payload.data(), payload.size(), pos, st, out.includes, err)) {
            return Result<ParseResult>::from_error(std::move(err));
        }
    }
    return Result<ParseResult>(std::move(out));
#endif
}

Result<ParseResult> decompile(const std::vector<uint8_t>& data) {
    return decompile(data.data(), data.size());
}

} // namespace synx
