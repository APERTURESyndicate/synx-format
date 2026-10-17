// Canonical JSON serializer. Mirrors `write_json` / `to_json` in synx-core/src/lib.rs.
#include "synx/json.hpp"

#include "numfmt.hpp"

#include <algorithm>
#include <cmath>
#include <string>
#include <vector>

namespace synx {

namespace {

// Bytes that need escaping: control characters, `"` and `\` — all ASCII.
struct EscapeTable {
    bool needs[256] = {};
    constexpr EscapeTable() {
        for (int c = 0; c < 0x20; ++c) needs[c] = true;
        needs[static_cast<unsigned char>('"')] = true;
        needs[static_cast<unsigned char>('\\')] = true;
    }
};
constexpr EscapeTable kEscape{};

// perf: unescaped runs are copied whole.
void write_json_str(std::string& out, const std::string& s) {
    static const char kHex[] = "0123456789abcdef";
    out.push_back('"');
    const char* data = s.data();
    size_t start = 0;
    for (size_t i = 0; i < s.size(); ++i) {
        unsigned char b = static_cast<unsigned char>(data[i]);
        if (!kEscape.needs[b]) continue;
        out.append(data + start, i - start);
        switch (b) {
            case '"': out.append("\\\"", 2); break;
            case '\\': out.append("\\\\", 2); break;
            case '\n': out.append("\\n", 2); break;
            case '\r': out.append("\\r", 2); break;
            case '\t': out.append("\\t", 2); break;
            default: {
                const char u[6] = {'\\', 'u', '0', '0', kHex[b >> 4], kHex[b & 0xF]};
                out.append(u, 6);
            }
        }
        start = i + 1;
    }
    out.append(data + start, s.size() - start);
    out.push_back('"');
}

void write_value(std::string& out, const Value& v, size_t depth) {
    if (depth > kMaxJsonDepth) {
        out.append("null");
        return;
    }
    switch (v.kind()) {
        case Value::Kind::Null: out.append("null"); return;
        case Value::Kind::Bool: out.append(*v.as_bool() ? "true" : "false"); return;
        case Value::Kind::Int: detail::append_i64(out, *v.as_int()); return;
        case Value::Kind::Float: {
            double f = *v.as_float();
            // JSON has no NaN / ±Infinity.
            if (!std::isfinite(f)) out.append("null");
            else detail::append_ryu(out, f);
            return;
        }
        case Value::Kind::String: write_json_str(out, *v.as_string()); return;
        // Never leak the underlying value — the `:secret` contract.
        case Value::Kind::Secret: out.append("\"[SECRET]\""); return;
        case Value::Kind::Array: {
            const Array& arr = *v.as_array();
            out.push_back('[');
            for (size_t i = 0; i < arr.size(); ++i) {
                if (i > 0) out.push_back(',');
                write_value(out, arr[i], depth + 1);
            }
            out.push_back(']');
            return;
        }
        case Value::Kind::Object: {
            const Object& obj = *v.as_object();
            // perf: one sort buffer per depth, reused across objects.
            thread_local std::vector<std::vector<const Pair*>> scratch;
            if (scratch.capacity() < kMaxJsonDepth + 2) scratch.reserve(kMaxJsonDepth + 2);
            if (scratch.size() <= depth) scratch.resize(depth + 1);
            std::vector<const Pair*>& entries = scratch[depth];
            entries.clear();
            for (const auto& p : obj) entries.push_back(&p);
            std::sort(entries.begin(), entries.end(),
                      [](const Pair* a, const Pair* b) { return a->key < b->key; });
            out.push_back('{');
            for (size_t i = 0; i < entries.size(); ++i) {
                if (i > 0) out.push_back(',');
                write_json_str(out, entries[i]->key);
                out.push_back(':');
                write_value(out, entries[i]->value, depth + 1);
            }
            out.push_back('}');
            return;
        }
    }
}

} // namespace

std::string float_to_display_string(double f) {
    std::string s = detail::display_f64(f);
    if (s.find('.') == std::string::npos) s.append(".0");
    return s;
}

void write_json(std::string& out, const Value& val) { write_value(out, val, 0); }

std::string to_json(const Value& val) {
    std::string out;
    out.reserve(2048);
    write_value(out, val, 0);
    return out;
}

} // namespace synx
