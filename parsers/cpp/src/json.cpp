// Canonical JSON serializer. Mirrors synx-core/src/lib.rs.
#include "synx/json.hpp"

#include <algorithm>
#include <cinttypes>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
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

// Shortest decimal digits that round-trip `f` (finite, non-zero, positive) and the
// decimal exponent `kk` such that value = 0.<digits> * 10^kk.
void shortest_digits(double f, std::string& digits, int& kk) {
    char buf[40];
    for (int prec = 1; prec <= 17; ++prec) {
        std::snprintf(buf, sizeof(buf), "%.*e", prec - 1, f);
        if (std::strtod(buf, nullptr) == f || prec == 17) break;
    }
    // buf is `d[.ddd]e[+-]XX`.
    digits.clear();
    const char* e = std::strchr(buf, 'e');
    for (const char* c = buf; c != e; ++c) {
        if (*c >= '0' && *c <= '9') digits.push_back(*c);
    }
    int exp10 = std::atoi(e + 1);
    while (digits.size() > 1 && digits.back() == '0') digits.pop_back();
    kk = exp10 + 1;
}

// Positional notation of `digits`/`kk`, always with a `.`.
std::string plain_digits(const std::string& digits, int kk) {
    int n = static_cast<int>(digits.size());
    if (kk <= 0) return "0." + std::string(static_cast<size_t>(-kk), '0') + digits;
    if (n <= kk) return digits + std::string(static_cast<size_t>(kk - n), '0') + ".0";
    return digits.substr(0, static_cast<size_t>(kk)) + "." + digits.substr(static_cast<size_t>(kk));
}

// Layout of Rust's `ryu::Buffer::format`: plain notation for 1e-5 <= |x| < 1e16
// (always with a `.`), else `d.ddde<exp>` with no `+` and no exponent padding.
void write_float(std::string& out, double f) {
    if (std::isnan(f) || std::isinf(f)) {
        out.append("null"); // JSON does not allow NaN/Inf; emit null for safety
        return;
    }
    if (f == 0.0) {
        out.append(std::signbit(f) ? "-0.0" : "0.0");
        return;
    }
    std::string digits;
    int kk = 0;
    shortest_digits(std::fabs(f), digits, kk);
    if (f < 0) out.push_back('-');
    if (kk > -5 && kk <= 16) {
        out.append(plain_digits(digits, kk));
        return;
    }
    out.push_back(digits[0]);
    if (digits.size() > 1) {
        out.push_back('.');
        out.append(digits, 1, std::string::npos);
    }
    out.push_back('e');
    out.append(std::to_string(kk - 1));
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

std::string float_to_display_string(double f) {
    if (std::isnan(f) || std::isinf(f)) return "null";
    if (f == 0.0) return std::signbit(f) ? "-0.0" : "0.0";
    std::string digits;
    int kk = 0;
    shortest_digits(std::fabs(f), digits, kk);
    return (f < 0 ? "-" : "") + plain_digits(digits, kk);
}

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
