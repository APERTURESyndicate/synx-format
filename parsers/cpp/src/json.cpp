// Canonical JSON serializer. Mirrors synx-core/src/lib.rs.
#include "synx/json.hpp"

#include <algorithm>
#include <cinttypes>
#include <cmath>
#include <cstdio>
#include <cstdint>
#include <string>
#include <vector>

namespace synx {

namespace {

void escape_string(std::string& out, const std::string& s) {
    for (unsigned char ch : s) {
        switch (ch) {
            case '"':  out.append("\\\""); break;
            case '\\': out.append("\\\\"); break;
            case '\n': out.append("\\n");  break;
            case '\r': out.append("\\r");  break;
            case '\t': out.append("\\t");  break;
            default:
                if (ch < 0x20) {
                    char buf[8];
                    std::snprintf(buf, sizeof(buf), "\\u%04x", static_cast<unsigned>(ch));
                    out.append(buf);
                } else {
                    out.push_back(static_cast<char>(ch));
                }
        }
    }
}

void write_int(std::string& out, int64_t n) {
    char buf[32];
    std::snprintf(buf, sizeof(buf), "%" PRId64, n);
    out.append(buf);
}

void write_float(std::string& out, double f) {
    if (std::isnan(f) || std::isinf(f)) {
        out.append("null"); // JSON does not allow NaN/Inf; emit null for safety
        return;
    }
    char buf[64];
    // 17 significant digits — round-trip safe for IEEE 754 doubles.
    std::snprintf(buf, sizeof(buf), "%.17g", f);
    out.append(buf);
    // Round-trip parity with Rust ryu: ensure a decimal point so JSON parsers
    // round-trip back to a Float, not an Int.
    bool has_marker = false;
    for (char c : std::string_view(buf)) {
        if (c == '.' || c == 'e' || c == 'E') { has_marker = true; break; }
    }
    if (!has_marker) {
        out.append(".0");
    }
}

void write_value(std::string& out, const Value& v, size_t depth) {
    if (depth > kMaxJsonDepth) {
        out.append("null");
        return;
    }
    switch (v.kind()) {
        case Value::Kind::Null:
            out.append("null");
            return;
        case Value::Kind::Bool:
            out.append(*v.as_bool() ? "true" : "false");
            return;
        case Value::Kind::Int:
            write_int(out, *v.as_int());
            return;
        case Value::Kind::Float:
            write_float(out, *v.as_float());
            return;
        case Value::Kind::String: {
            out.push_back('"');
            escape_string(out, *v.as_string());
            out.push_back('"');
            return;
        }
        case Value::Kind::Secret:
            // Never leak the underlying value — see SYNX `:secret` contract.
            out.append("\"[SECRET]\"");
            return;
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
            out.push_back('{');
            std::vector<const Pair*> entries;
            entries.reserve(obj.size());
            for (const auto& p : obj) entries.push_back(&p);
            std::sort(entries.begin(), entries.end(),
                      [](const Pair* a, const Pair* b) { return a->key < b->key; });
            bool first = true;
            for (const Pair* p : entries) {
                if (!first) out.push_back(',');
                first = false;
                out.push_back('"');
                escape_string(out, p->key);
                out.append("\":");
                write_value(out, p->value, depth + 1);
            }
            out.push_back('}');
            return;
        }
    }
}

} // namespace

void write_json(std::string& out, const Value& val) {
    write_value(out, val, 0);
}

std::string to_json(const Value& val) {
    std::string out;
    out.reserve(2048);
    write_value(out, val, 0);
    return out;
}

} // namespace synx
