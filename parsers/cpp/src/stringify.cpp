// Value → SYNX text serializer.
#include "synx/stringify.hpp"

#include <algorithm>
#include <cinttypes>
#include <cmath>
#include <cstdio>
#include <cstdint>
#include <string>
#include <vector>

namespace synx {

namespace {

constexpr size_t kMaxSerializeDepth = 128;

std::string format_primitive(const Value& v) {
    switch (v.kind()) {
        case Value::Kind::String:
            return *v.as_string();
        case Value::Kind::Int: {
            char buf[32];
            std::snprintf(buf, sizeof(buf), "%" PRId64, *v.as_int());
            return buf;
        }
        case Value::Kind::Float: {
            double f = *v.as_float();
            if (std::isnan(f) || std::isinf(f)) {
                return "null";
            }
            char buf[64];
            std::snprintf(buf, sizeof(buf), "%.17g", f);
            std::string s(buf);
            if (s.find('.') == std::string::npos
                && s.find('e') == std::string::npos
                && s.find('E') == std::string::npos) {
                s.append(".0");
            }
            return s;
        }
        case Value::Kind::Bool:
            return *v.as_bool() ? "true" : "false";
        case Value::Kind::Null:
            return "null";
        case Value::Kind::Array: {
            const Array& a = *v.as_array();
            std::string out = "[";
            for (size_t i = 0; i < a.size(); ++i) {
                if (i > 0) out.append(", ");
                out.append(format_primitive(a[i]));
            }
            out.push_back(']');
            return out;
        }
        case Value::Kind::Object:
            return "[Object]";
        case Value::Kind::Secret:
            return "[SECRET]";
    }
    return {};
}

void serialize_impl(const Value& v, size_t depth_lvl, std::string& out) {
    if (depth_lvl > kMaxSerializeDepth) {
        out.append("[synx:max-depth]\n");
        return;
    }
    if (v.kind() != Value::Kind::Object) {
        out.append(format_primitive(v));
        return;
    }
    const Object& map = *v.as_object();
    std::string indent(depth_lvl * 2, ' ');

    // Sort keys for deterministic output (matches Rust serialize).
    std::vector<const Pair*> entries;
    entries.reserve(map.size());
    for (const auto& p : map) entries.push_back(&p);
    std::sort(entries.begin(), entries.end(),
              [](const Pair* a, const Pair* b) { return a->key < b->key; });

    for (const Pair* p : entries) {
        const Value& val = p->value;
        if (val.is_array()) {
            const Array& arr = *val.as_array();
            out.append(indent);
            out.append(p->key);
            out.push_back('\n');
            for (const Value& item : arr) {
                if (item.is_object()) {
                    const Object& inner = *item.as_object();
                    auto first_it = inner.begin();
                    if (first_it != inner.end()) {
                        out.append(indent);
                        out.append("  - ");
                        out.append(first_it->key);
                        out.push_back(' ');
                        out.append(format_primitive(first_it->value));
                        out.push_back('\n');
                        for (auto it = first_it + 1; it != inner.end(); ++it) {
                            out.append(indent);
                            out.append("    ");
                            out.append(it->key);
                            out.push_back(' ');
                            out.append(format_primitive(it->value));
                            out.push_back('\n');
                        }
                    }
                } else {
                    out.append(indent);
                    out.append("  - ");
                    out.append(format_primitive(item));
                    out.push_back('\n');
                }
            }
        } else if (val.is_object()) {
            out.append(indent);
            out.append(p->key);
            out.push_back('\n');
            serialize_impl(val, depth_lvl + 1, out);
        } else if (val.is_string()) {
            const std::string& s = *val.as_string();
            if (s.find('\n') != std::string::npos) {
                out.append(indent);
                out.append(p->key);
                out.append(" |\n");
                size_t line_start = 0;
                for (size_t i = 0; i <= s.size(); ++i) {
                    if (i == s.size() || s[i] == '\n') {
                        out.append(indent);
                        out.append("  ");
                        out.append(s, line_start, i - line_start);
                        out.push_back('\n');
                        line_start = i + 1;
                    }
                }
            } else {
                out.append(indent);
                out.append(p->key);
                out.push_back(' ');
                out.append(format_primitive(val));
                out.push_back('\n');
            }
        } else {
            out.append(indent);
            out.append(p->key);
            out.push_back(' ');
            out.append(format_primitive(val));
            out.push_back('\n');
        }
    }
}

} // namespace

std::string stringify(const Value& val) {
    std::string out;
    out.reserve(2048);
    serialize_impl(val, 0, out);
    return out;
}

} // namespace synx
