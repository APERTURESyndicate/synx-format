// SYNX `!active` engine — control flow, helpers, marker dispatch.
// Mirrors crates/synx-core/src/engine.rs (control-flow half).
#include "synx/engine.hpp"
#include "synx/json.hpp"
#include "synx/parser.hpp"
#include "engine_internal.hpp"

#include <algorithm>
#include <cctype>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstdint>
#include <cstring>
#include <ctime>
#include <fstream>
#include <regex>
#include <sstream>
#include <string>
#include <string_view>
#include <unordered_set>
#include <utility>
#include <vector>

namespace synx {
namespace detail {

namespace {

// ─── string helpers ──────────────────────────────────────────────────────────
inline std::string_view trim_sv(std::string_view s) noexcept {
    size_t a = 0;
    while (a < s.size() && (s[a] == ' ' || s[a] == '\t' || s[a] == '\r')) ++a;
    size_t b = s.size();
    while (b > a && (s[b - 1] == ' ' || s[b - 1] == '\t' || s[b - 1] == '\r')) --b;
    return s.substr(a, b - a);
}

inline std::string trim_str(const std::string& s) {
    return std::string(trim_sv(s));
}

// Longest prefix of `s` that fits in `limit` bytes without splitting a character.
inline size_t utf8_prefix_len(std::string_view s, size_t limit) noexcept {
    if (s.size() <= limit) return s.size();
    size_t end = limit;
    while (end > 0 && (static_cast<unsigned char>(s[end]) & 0xC0) == 0x80) --end;
    return end;
}

constexpr size_t kMaxScratchString = 4u * 1024 * 1024;

// `{name}` reference names: alphanumerics (any non-ASCII byte counts), `_` and `.`.
bool is_ref_name(std::string_view s) noexcept {
    for (unsigned char c : s) {
        if (!(std::isalnum(c) || c == '_' || c == '.' || c >= 0x80)) return false;
    }
    return true;
}

// Markers that must resolve the value before it can become a secret.
bool needs_resolution_before_secret(const Meta& m) {
    static const char* const kResolving[] = {
        "env", "include", "import", "watch", "ref", "alias", "calc", "map", "i18n", "default",
        "fallback", "once", "split", "join", "format", "replace",
    };
    for (const auto& marker : m.markers) {
        for (const char* r : kResolving) {
            if (marker == r) return true;
        }
    }
    return false;
}

// Exact int64 parse of the whole string (Rust `str::parse::<i64>`): optional sign, digits only.
bool parse_i64(std::string_view s, int64_t& out) noexcept {
    size_t i = 0;
    bool neg = false;
    if (!s.empty() && (s[0] == '-' || s[0] == '+')) {
        neg = s[0] == '-';
        i = 1;
    }
    if (i >= s.size()) return false;
    uint64_t v = 0;
    const uint64_t limit = neg ? 9223372036854775808ull : 9223372036854775807ull;
    for (; i < s.size(); ++i) {
        if (s[i] < '0' || s[i] > '9') return false;
        uint64_t d = static_cast<uint64_t>(s[i] - '0');
        if (v > (limit - d) / 10) return false;
        v = v * 10 + d;
    }
    out = neg ? static_cast<int64_t>(0 - v) : static_cast<int64_t>(v);
    return true;
}

bool starts_with(std::string_view s, std::string_view prefix) noexcept {
    return s.size() >= prefix.size()
        && std::memcmp(s.data(), prefix.data(), prefix.size()) == 0;
}

std::string join_paths(const std::string& base, const std::string& rel) {
    if (base.empty()) return rel;
    std::string out = base;
    if (out.back() != '/' && out.back() != '\\') out.push_back('/');
    out.append(rel);
    return out;
}

inline std::string dirname_of(const std::string& path) {
    size_t pos = path.find_last_of("/\\");
    return pos == std::string::npos ? std::string{} : path.substr(0, pos);
}

} // namespace

// ─── file IO ────────────────────────────────────────────────────────────────
FileText read_text_file(const std::string& path) {
    std::ifstream in(path, std::ios::binary);
    if (!in.good()) return {};
    std::ostringstream ss;
    ss << in.rdbuf();
    return FileText{ss.str(), true};
}

// ─── value helpers ──────────────────────────────────────────────────────────
bool wants_string(const Meta& m) {
    return (m.type_hint.has_value() && *m.type_hint == "string")
        || (m.constraints.has_value() && m.constraints->type_name.has_value()
            && *m.constraints->type_name == "string");
}

Value cast_primitive(std::string_view val) {
    if (val.size() >= 2
        && ((val.front() == '"' && val.back() == '"') || (val.front() == '\'' && val.back() == '\''))) {
        return Value::make_string(std::string(val.substr(1, val.size() - 2)));
    }
    if (val == "true") return Value::make_bool(true);
    if (val == "false") return Value::make_bool(false);
    if (val == "null") return Value::make_null();
    int64_t n = 0;
    if (parse_i64(val, n)) return Value::make_int(n);
    // Decimal floats only: strtod also takes leading blanks, hex and `nan(...)`.
    if (!val.empty() && !std::isspace(static_cast<unsigned char>(val.front()))
        && val.find_first_of("xX(") == std::string_view::npos) {
        std::string tmp(val);
        char* end = nullptr;
        double d = std::strtod(tmp.c_str(), &end);
        if (end != tmp.c_str() && *end == '\0') return Value::make_float(d);
    }
    return Value::make_string(std::string(val));
}

std::string value_to_string(const Value& v) {
    switch (v.kind()) {
        case Value::Kind::Null:   return "null";
        case Value::Kind::Bool:   return *v.as_bool() ? "true" : "false";
        case Value::Kind::Int: {
            char buf[32];
            std::snprintf(buf, sizeof(buf), "%lld", static_cast<long long>(*v.as_int()));
            return buf;
        }
        case Value::Kind::Float:
            return format_number(*v.as_float());
        case Value::Kind::String:
            return *v.as_string();
        case Value::Kind::Secret:
            // safety: a `:secret` value MUST NOT reach a string sink as plaintext.
            return "[SECRET]";
        case Value::Kind::Array: {
            std::string out = "[";
            const auto& a = *v.as_array();
            for (size_t i = 0; i < a.size(); ++i) {
                if (i > 0) out.append(", ");
                out.append(value_to_string(a[i]));
            }
            out.push_back(']');
            return out;
        }
        case Value::Kind::Object: return "[Object]";
    }
    return {};
}

std::string display_number(double n) {
    if (std::isnan(n)) return "NaN";
    if (std::isinf(n)) return n < 0 ? "-inf" : "inf";
    std::string s = float_to_display_string(n);
    if (s.size() >= 2 && s.compare(s.size() - 2, 2, ".0") == 0) s.resize(s.size() - 2);
    return s;
}

std::string format_number(double n) {
    if (std::isfinite(n) && std::fmod(n, 1.0) == 0.0 && std::fabs(n) < 9.223372036854775807e18) {
        return std::to_string(static_cast<long long>(n));
    }
    return display_number(n);
}

bool value_to_number(const Value& v, double& out) {
    if (v.is_int()) { out = static_cast<double>(*v.as_int()); return true; }
    if (v.is_float()) { out = *v.as_float(); return true; }
    if (v.is_string()) {
        const std::string& s = *v.as_string();
        char* end = nullptr;
        double d = std::strtod(s.c_str(), &end);
        if (end == s.c_str() || *end != '\0') return false;
        out = d;
        return true;
    }
    if (v.is_bool()) { out = *v.as_bool() ? 1.0 : 0.0; return true; }
    return false;
}

// ─── CLDR plural rules (subset of crates/synx-core/src/engine.rs) ────────────
const char* plural_category(std::string_view lang, double n) {
    auto two_letter = lang.substr(0, 2);
    long long ll = static_cast<long long>(std::floor(std::fabs(n)));
    long long mod10 = ll % 10;
    long long mod100 = ll % 100;
    bool int_n = (n == std::floor(n));

    if (two_letter == "ru" || two_letter == "uk" || two_letter == "be") {
        if (int_n && mod10 == 1 && mod100 != 11) return "one";
        if (int_n && mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)) return "few";
        if (int_n && (mod10 == 0 || (mod10 >= 5 && mod10 <= 9)
                      || (mod100 >= 11 && mod100 <= 14))) return "many";
        return "other";
    }
    if (two_letter == "pl") {
        if (int_n && n == 1) return "one";
        if (int_n && mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)) return "few";
        if (int_n && n != 1 && (mod10 == 0 || mod10 == 1
                                || (mod10 >= 5 && mod10 <= 9)
                                || (mod100 >= 12 && mod100 <= 14))) return "many";
        return "other";
    }
    if (two_letter == "cs" || two_letter == "sk") {
        if (int_n && n == 1) return "one";
        if (int_n && n >= 2 && n <= 4) return "few";
        if (!int_n) return "many";
        return "other";
    }
    if (two_letter == "ar") {
        if (n == 0) return "zero";
        if (n == 1) return "one";
        if (n == 2) return "two";
        if (int_n && mod100 >= 3 && mod100 <= 10) return "few";
        if (int_n && mod100 >= 11) return "many";
        return "other";
    }
    if (two_letter == "fr" || two_letter == "pt") {
        if (n >= 0 && n < 2) return "one";
        return "other";
    }
    if (two_letter == "ja" || two_letter == "zh" || two_letter == "ko"
        || two_letter == "vi" || two_letter == "th") {
        return "other";
    }
    // English-like default
    if (n == 1) return "one";
    return "other";
}

std::string current_iso_timestamp() {
    auto t = std::chrono::system_clock::to_time_t(std::chrono::system_clock::now());
    std::tm tm{};
#if defined(_WIN32)
    gmtime_s(&tm, &t);
#else
    gmtime_r(&t, &tm);
#endif
    char buf[32];
    std::snprintf(buf, sizeof(buf), "%04d-%02d-%02dT%02d:%02d:%02dZ",
                  tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday,
                  tm.tm_hour, tm.tm_min, tm.tm_sec);
    return buf;
}

bool is_builtin_marker(const std::string& name) {
    static const std::unordered_set<std::string> builtins = {
        "env","default","calc","ref","alias","secret","random","unique","geo","i18n",
        "split","join","clamp","round","map","format","replace","sort","sum","fallback",
        "once","version","watch","prompt","vision","audio","include","import","inherit","spam"
    };
    return builtins.count(name) != 0;
}

bool regex_pattern_is_supported(std::string_view p) {
    enum class Prev { None, Atom, Assertion, Quant };
    constexpr size_t kMaxGroupDepth = 32;
    constexpr unsigned kMaxRepeat = 1000;

    auto is_hex = [](char c) {
        return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
    };
    auto is_ascii_alpha = [](char c) { return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z'); };

    // Escape at p[i] == '\\'. Returns the index after it, or npos if unsupported.
    // `class_escape` reports \d \w \s \D \W \S (valid atoms but not range endpoints).
    auto escape_end = [&](size_t i, bool& class_escape) -> size_t {
        class_escape = false;
        if (i + 1 >= p.size()) return std::string_view::npos;      // trailing backslash
        char e = p[i + 1];
        if (e == 'd' || e == 'D' || e == 'w' || e == 'W' || e == 's' || e == 'S') {
            class_escape = true;
            return i + 2;
        }
        if (e == 'n' || e == 'r' || e == 't' || e == 'f' || e == 'v') return i + 2;
        if (e == 'x' || e == 'u') {
            size_t digits = e == 'x' ? 2 : 4;
            if (i + 2 + digits > p.size()) return std::string_view::npos;
            for (size_t k = 0; k < digits; ++k) {
                if (!is_hex(p[i + 2 + k])) return std::string_view::npos;
            }
            return i + 2 + digits;
        }
        // Back-references, \0, \c, \k, \p, unknown letters: not worth the risk.
        if (is_ascii_alpha(e) || (e >= '0' && e <= '9')) return std::string_view::npos;
        if (static_cast<unsigned char>(e) >= 0x80) return std::string_view::npos;
        return i + 2; // identity escape of punctuation
    };

    Prev prev = Prev::None;
    bool lazy_ok = false;
    std::vector<bool> lookahead; // one entry per open group
    size_t i = 0;
    const size_t n = p.size();
    while (i < n) {
        char c = p[i];
        switch (c) {
            case '\\': {
                if (p[std::min(i + 1, n - 1)] == 'b' || p[std::min(i + 1, n - 1)] == 'B') {
                    if (i + 1 >= n) return false;
                    prev = Prev::Assertion;
                    i += 2;
                    break;
                }
                bool cls = false;
                size_t next = escape_end(i, cls);
                if (next == std::string_view::npos) return false;
                prev = Prev::Atom;
                i = next;
                break;
            }
            case '(': {
                bool look = false;
                size_t skip = 1;
                if (i + 1 < n && p[i + 1] == '?') {
                    if (i + 2 >= n) return false;
                    char k = p[i + 2];
                    if (k == ':') { skip = 3; }
                    else if (k == '=' || k == '!') { skip = 3; look = true; }
                    else return false; // lookbehind, named groups, inline flags
                }
                if (lookahead.size() >= kMaxGroupDepth) return false;
                lookahead.push_back(look);
                prev = Prev::None;
                i += skip;
                break;
            }
            case ')':
                if (lookahead.empty()) return false;
                prev = lookahead.back() ? Prev::Assertion : Prev::Atom;
                lookahead.pop_back();
                ++i;
                break;
            case '|':
                prev = Prev::None;
                ++i;
                break;
            case '^':
            case '$':
                prev = Prev::Assertion;
                ++i;
                break;
            case '*':
            case '+':
            case '?':
                if (prev == Prev::Quant) {
                    // `?` right after a quantifier makes it lazy, once.
                    if (c == '?' && lazy_ok) { lazy_ok = false; ++i; break; }
                    return false;
                }
                if (prev != Prev::Atom) return false; // nothing to repeat
                prev = Prev::Quant;
                lazy_ok = true;
                ++i;
                break;
            case '{': {
                if (prev != Prev::Atom) return false;
                size_t k = i + 1;
                unsigned lo = 0, hi = 0;
                bool have_lo = false, have_hi = false, comma = false;
                while (k < n && p[k] >= '0' && p[k] <= '9') {
                    lo = lo * 10 + static_cast<unsigned>(p[k] - '0');
                    if (lo > kMaxRepeat) return false;
                    have_lo = true;
                    ++k;
                }
                if (!have_lo) return false;
                if (k < n && p[k] == ',') {
                    comma = true;
                    ++k;
                    while (k < n && p[k] >= '0' && p[k] <= '9') {
                        hi = hi * 10 + static_cast<unsigned>(p[k] - '0');
                        if (hi > kMaxRepeat) return false;
                        have_hi = true;
                        ++k;
                    }
                }
                if (k >= n || p[k] != '}') return false;
                if (comma && have_hi && lo > hi) return false;
                prev = Prev::Quant;
                lazy_ok = true;
                i = k + 1;
                break;
            }
            case '}':
                return false;
            case '[': {
                ++i;
                if (i < n && p[i] == '^') ++i;
                if (i >= n || p[i] == ']') return false; // unclosed or empty class
                // last: -1 none, otherwise the single-character code of the previous item
                // (0x100 marks a class escape / posix class that cannot start a range).
                int last = -1;
                bool dash_pending = false;
                bool closed = false;
                while (i < n) {
                    char k = p[i];
                    if (k == ']') {
                        if (dash_pending) return false;
                        closed = true;
                        ++i;
                        break;
                    }
                    int item = 0;
                    if (k == '\\') {
                        bool cls = false;
                        size_t next = escape_end(i, cls);
                        if (next == std::string_view::npos) return false;
                        // Only simple escapes may bound a range; keep it ASCII-only.
                        item = cls ? 0x100 : (next == i + 2 ? static_cast<unsigned char>(p[i + 1]) : 0x100);
                        i = next;
                    } else if (k == '[') {
                        // Only `[:name:]` POSIX classes.
                        if (i + 1 >= n || p[i + 1] != ':') return false;
                        size_t close = p.find(":]", i + 2);
                        if (close == std::string_view::npos) return false;
                        std::string_view name = p.substr(i + 2, close - (i + 2));
                        static const char* const kNames[] = {
                            "alpha", "digit", "alnum", "upper", "lower", "space", "punct",
                            "print", "graph", "cntrl", "xdigit", "blank", "w", "d", "s",
                        };
                        bool known = false;
                        for (const char* nm : kNames) {
                            if (name == nm) { known = true; break; }
                        }
                        if (!known) return false;
                        item = 0x100;
                        i = close + 2;
                    } else if (k == '-') {
                        // A range operator only between two single characters.
                        bool at_end = i + 1 < n && p[i + 1] == ']';
                        if (last == -1 || at_end) {
                            item = '-';
                            ++i;
                        } else {
                            if (last == 0x100 || dash_pending) return false;
                            dash_pending = true;
                            ++i;
                            continue;
                        }
                    } else {
                        item = static_cast<unsigned char>(k);
                        ++i;
                    }
                    if (dash_pending) {
                        // `last-item` must be a plain ASCII range with start <= end.
                        if (item == 0x100 || last < 0 || last >= 0x80 || item >= 0x80 || last > item) return false;
                        dash_pending = false;
                        last = -1; // a finished range cannot chain into another
                    } else {
                        last = item;
                    }
                }
                if (!closed) return false;
                prev = Prev::Atom;
                break;
            }
            default:
                prev = Prev::Atom;
                ++i;
                break;
        }
    }
    return lookahead.empty();
}

bool regex_match_strict(const std::string& value, const std::string& pattern) {
    // An unsupported or invalid pattern never rejects the value, and is never
    // handed to std::regex (which aborts the process when exceptions are off).
    if (!regex_pattern_is_supported(pattern)) return true;
#if defined(__cpp_exceptions) && __cpp_exceptions
    try {
        std::regex re(pattern);
        return std::regex_search(value, re);
    } catch (...) {
        return true; // Invalid pattern — do not reject the value.
    }
#else
    std::regex re(pattern);
    return std::regex_search(value, re);
#endif
}

// ─── constraint enforcement ─────────────────────────────────────────────────
bool has_nested_quantifier(std::string_view p) {
    std::vector<size_t> opens;
    for (size_t i = 0; i < p.size(); ++i) {
        char ch = p[i];
        if (ch == '\\') { ++i; continue; }
        if (ch == '[') {
            // Skip a character class wholesale; quantifiers inside it are literal.
            while (i < p.size() && p[i] != ']') {
                if (p[i] == '\\') ++i;
                ++i;
            }
            continue;
        }
        if (ch == '(') { opens.push_back(i); continue; }
        if (ch == ')') {
            if (opens.empty()) continue;
            size_t start = opens.back();
            opens.pop_back();
            if (i + 1 < p.size() && (p[i + 1] == '*' || p[i + 1] == '+' || p[i + 1] == '{')) {
                // Body with escapes (`\x`) removed, then look for `*`, `+` or `{<digit>`.
                std::string body;
                for (size_t k = start + 1; k < i; ++k) {
                    if (p[k] == '\\') { ++k; continue; }
                    body.push_back(p[k]);
                }
                for (size_t n = 0; n < body.size(); ++n) {
                    if (body[n] == '*' || body[n] == '+') return true;
                    if (body[n] == '{' && n + 1 < body.size() && body[n + 1] >= '0' && body[n + 1] <= '9') {
                        return true;
                    }
                }
            }
        }
    }
    return false;
}

int matches_linear(const std::string& pattern, const std::string& value) {
    if (has_nested_quantifier(pattern)) return -1;
    if (!regex_pattern_is_supported(pattern)) return -1;
    return regex_match_strict(value, pattern) ? 1 : 0;
}

std::optional<std::string> constraint_error(const std::string& key, const Value& val,
                                            const Constraints& c) {
    if (c.required) {
        bool empty = val.is_null() || (val.is_string() && val.as_string()->empty());
        if (empty) return "CONSTRAINT_ERR: '" + key + "' is required";
    }

    if (c.type_name.has_value()) {
        const std::string& tn = *c.type_name;
        bool ok = true;
        if (tn == "int") ok = val.is_int();
        else if (tn == "float") ok = val.is_float() || val.is_int();
        else if (tn == "bool") ok = val.is_bool();
        else if (tn == "string") ok = val.is_string() || val.is_secret(); // secret = masked string
        if (!ok) return "CONSTRAINT_ERR: '" + key + "' expected type '" + tn + "'";
    }

    if (c.enum_values.has_value()) {
        std::string text;
        if (val.is_string()) text = *val.as_string();
        else if (val.is_int()) text = std::to_string(static_cast<long long>(*val.as_int()));
        else if (val.is_float()) text = display_number(*val.as_float());
        else if (val.is_bool()) text = *val.as_bool() ? "true" : "false";
        bool found = false;
        for (const auto& e : *c.enum_values) {
            if (e == text) { found = true; break; }
        }
        if (!found) {
            std::string joined;
            for (size_t i = 0; i < c.enum_values->size(); ++i) {
                if (i > 0) joined.push_back('|');
                joined.append((*c.enum_values)[i]);
            }
            return "CONSTRAINT_ERR: '" + key + "' must be one of [" + joined + "]";
        }
    }

    // Numbers compare by value; strings by count of Unicode scalar values.
    bool have_n = false;
    double n = 0.0;
    if (val.is_int()) { n = static_cast<double>(*val.as_int()); have_n = true; }
    else if (val.is_float()) { n = *val.as_float(); have_n = true; }
    else if (val.is_string() && (c.min.has_value() || c.max.has_value())) {
        size_t count = 0;
        for (unsigned char ch : *val.as_string()) {
            if ((ch & 0xC0) != 0x80) ++count;
        }
        n = static_cast<double>(count);
        have_n = true;
    }
    if (have_n) {
        if (c.min.has_value() && n < *c.min) {
            return "CONSTRAINT_ERR: '" + key + "' value " + display_number(n)
                 + " is below min " + display_number(*c.min);
        }
        if (c.max.has_value() && n > *c.max) {
            return "CONSTRAINT_ERR: '" + key + "' value " + display_number(n)
                 + " exceeds max " + display_number(*c.max);
        }
    }

    if (c.pattern.has_value() && c.pattern->size() <= 256 && val.is_string()) {
        // -1 = refused or uncompilable pattern: skipped silently.
        if (matches_linear(*c.pattern, *val.as_string()) == 0) {
            return "CONSTRAINT_ERR: '" + key + "' does not match pattern /" + *c.pattern + "/";
        }
    }
    return std::nullopt;
}

// ─── Resolver ───────────────────────────────────────────────────────────────
Resolver::Resolver(ParseResult& r, const Options& o)
    : result(r), options(o), root(*r.root.as_object_mut()) {
    // RNG seeding: SYNX_SEED env if present, else random_device.
    uint64_t seed = std::random_device{}();
    if (options.env.has_value()) {
        auto it = options.env->find("SYNX_SEED");
        if (it != options.env->end()) {
            char* end = nullptr;
            uint64_t parsed = std::strtoull(it->second.c_str(), &end, 10);
            if (end != it->second.c_str() && *end == '\0') {
                seed = parsed;
            }
        }
    }
    rng.seed(seed);
}

std::string Resolver::jail_path(const std::string& base, const std::string& rel) const {
    if (rel.empty()) return {};
    // Reject anything that escapes the sandbox.
    if (rel[0] == '/' || rel[0] == '\\') return {};
    if (rel.size() >= 2 && rel[1] == ':') return {}; // Windows drive letter (C:\...)
    if (starts_with(rel, "res://") || starts_with(rel, "user://")) return {};
    // Reject path traversal segments
    std::string normalized;
    normalized.reserve(rel.size());
    for (size_t i = 0; i < rel.size(); ++i) {
        char c = rel[i];
        if (c == '\\') c = '/';
        normalized.push_back(c);
    }
    // Scan segments for ".."
    size_t start = 0;
    while (start < normalized.size()) {
        size_t slash = normalized.find('/', start);
        std::string_view seg = std::string_view(normalized).substr(
            start, (slash == std::string::npos ? normalized.size() : slash) - start);
        if (seg == ".." || seg == "...") return {};
        if (slash == std::string::npos) break;
        start = slash + 1;
    }
    return join_paths(base, normalized);
}

const Value* Resolver::deep_get(const Object& obj, std::string_view path) const {
    if (path.empty()) return nullptr;
    const Object* current = &obj;
    const Value* last = nullptr;
    size_t start = 0;
    while (start < path.size()) {
        size_t dot = path.find('.', start);
        std::string_view seg = path.substr(start,
            (dot == std::string_view::npos ? path.size() : dot) - start);
        const Value* found = nullptr;
        for (const auto& p : *current) {
            if (p.key == seg) { found = &p.value; break; }
        }
        if (!found) return nullptr;
        last = found;
        if (dot == std::string_view::npos) return found;
        const Object* next = found->as_object();
        if (!next) return nullptr;
        current = next;
        start = dot + 1;
    }
    return last;
}

const Value* Resolver::deep_get_with_namespaces(std::string_view path) const {
    // First: namespace.key.path? Try splitting at first dot.
    size_t dot = path.find('.');
    if (dot != std::string_view::npos) {
        std::string_view ns = path.substr(0, dot);
        std::string_view rest = path.substr(dot + 1);
        auto it = namespaces.find(std::string(ns));
        if (it != namespaces.end() && it->second) {
            const Value* v = deep_get(*it->second, rest);
            if (v) return v;
        }
    }
    return deep_get(root, path);
}

std::string Resolver::interpolate(std::string_view s) const {
    std::string out;
    out.reserve(s.size());
    for (size_t i = 0; i < s.size(); ) {
        if (s[i] == '{') {
            size_t end = s.find('}', i + 1);
            if (end == std::string_view::npos) {
                out.push_back('{');
                ++i;
                continue;
            }
            std::string_view inner = trim_sv(s.substr(i + 1, end - i - 1));
            const Value* v = deep_get_with_namespaces(inner);
            if (v) {
                out.append(value_to_string(*v));
            } else {
                out.push_back('{');
                out.append(inner);
                out.push_back('}');
            }
            i = end + 1;
        } else {
            out.push_back(s[i]);
            ++i;
        }
    }
    return out;
}

static const Value* find_in_object(const Object& obj, std::string_view key) {
    for (const auto& p : obj) {
        if (p.key == key) return &p.value;
    }
    return nullptr;
}

std::string Resolver::resolve_interpolation(std::string_view tpl, const Object& local) const {
    std::string out;
    out.reserve(std::min(tpl.size(), kMaxScratchString));
    auto push = [&](std::string_view piece) {
        size_t room = kMaxScratchString > out.size() ? kMaxScratchString - out.size() : 0;
        out.append(piece.data(), utf8_prefix_len(piece, room));
    };
    size_t i = 0;
    while (i < tpl.size()) {
        if (out.size() >= kMaxScratchString) break;
        if (tpl[i] == '{') {
            size_t close = tpl.find('}', i + 1);
            if (close != std::string_view::npos) {
                std::string_view inner = tpl.substr(i + 1, close - i - 1);
                size_t colon = inner.find(':');
                std::string_view ref = colon == std::string_view::npos ? inner : inner.substr(0, colon);
                if (is_ref_name(ref)) {
                    const Value* val = nullptr;
                    if (colon != std::string_view::npos) {
                        std::string_view scope = inner.substr(colon + 1);
                        const Object* ns = nullptr;
                        if (scope == "include") {
                            if (include_aliases.size() == 1) {
                                auto it = namespaces.find(include_aliases[0]);
                                if (it != namespaces.end()) ns = it->second;
                            }
                        } else {
                            auto it = namespaces.find(std::string(scope));
                            if (it != namespaces.end()) ns = it->second;
                        }
                        if (ns) val = find_in_object(*ns, ref) ? find_in_object(*ns, ref) : deep_get(*ns, ref);
                    } else {
                        val = find_in_object(root, ref);
                        if (!val) val = deep_get(root, ref);
                        if (!val) val = find_in_object(local, ref);
                    }
                    if (val) {
                        push(value_to_string(*val));
                    } else {
                        push("{");
                        push(inner);
                        push("}");
                    }
                    i = close + 1;
                    continue;
                }
            }
        }
        // Whole bytes of literal text, so multi-byte characters survive intact.
        out.push_back(tpl[i]);
        ++i;
    }
    return out;
}

void Resolver::interpolate_level(Object& obj) {
    for (auto& p : obj) {
        const std::string* s = p.value.as_string();
        if (!s || s->find('{') == std::string::npos) continue;
        std::string resolved = resolve_interpolation(*s, obj);
        if (resolved != *s) p.value = Value::make_string(std::move(resolved));
    }
}

void Resolver::mask_secret_literals() {
    for (auto& path_entry : result.metadata) {
        Value* container = path_entry.first.empty()
            ? &result.root
            : const_cast<Value*>(deep_get(root, path_entry.first));
        if (!container) continue;
        Object* obj = container->as_object_mut();
        if (!obj) continue;
        for (auto& field_entry : path_entry.second) {
            const Meta& m = field_entry.second;
            if (!m.has_marker("secret") || needs_resolution_before_secret(m)) continue;
            for (auto& p : *obj) {
                if (p.key != field_entry.first) continue;
                if (!p.value.is_secret() && !p.value.is_object() && !p.value.is_array()) {
                    p.value = Value::make_secret(value_to_string(p.value));
                }
                break;
            }
        }
    }
}

void Resolver::refresh_namespaces() {
    for (auto& ns : namespaces) {
        for (auto& p : root) {
            if (p.key == ns.first) {
                ns.second = p.value.as_object();
                break;
            }
        }
    }
}

// ─── walking + dispatch ─────────────────────────────────────────────────────
void Resolver::resolve_value(Value& value, const Object& local, const std::string& path,
                             const std::string& key, int depth) {
    if (depth > static_cast<int>(kMaxResolveDepth)) return;

    auto path_it = result.metadata.find(path);
    if (path_it == result.metadata.end()) return;
    auto key_it = path_it->second.find(key);
    if (key_it == path_it->second.end()) return;
    const Meta& meta = key_it->second;

    // Apply markers in declared order.
    for (size_t i = 0; i < meta.markers.size(); ++i) {
        const std::string& m = meta.markers[i];
        if (m == "env")           apply_env(value, meta);
        else if (m == "default")  apply_default(value, meta);
        else if (m == "calc")     apply_calc(value, meta, key, local);
        else if (m == "ref")      apply_ref(value, meta);
        else if (m == "alias")    apply_alias(value, meta);
        else if (m == "secret")   { /* applied after the chain, see below */ }
        else if (m == "random")   apply_random(value, meta);
        else if (m == "unique")   apply_unique(value, meta);
        else if (m == "geo")      apply_geo(value, meta);
        else if (m == "i18n")     apply_i18n(value, meta);
        else if (m == "split")    apply_split(value, meta);
        else if (m == "join")     apply_join(value, meta);
        else if (m == "clamp")    apply_clamp(value, meta);
        else if (m == "round")    apply_round(value, meta);
        else if (m == "map")      apply_map(value, meta);
        else if (m == "format")   apply_format(value, meta);
        else if (m == "replace")  apply_replace(value, meta);
        else if (m == "sort")     apply_sort(value, meta);
        else if (m == "sum")      apply_sum(value, meta);
        else if (m == "fallback") apply_fallback(value, meta);
        else if (m == "once")     apply_once(value, meta, path, key);
        else if (m == "version")  apply_version(value, meta);
        else if (m == "watch")    apply_watch(value, meta);
        else if (m == "prompt")   apply_prompt(value, meta);
        else if (m == "vision")   apply_vision(value, meta);
        else if (m == "audio")    apply_audio(value, meta);
        else if (m == "spam")     apply_spam(value, meta, path, key);
        else if (m == "inherit")  { /* handled in pre-pass */ }
        else if (m == "include" || m == "import") { /* directives, handled separately */ }
        else {
            // User-defined marker?
            if (!is_builtin_marker(m)) {
                auto fn_it = options.marker_fns.find(m);
                if (fn_it != options.marker_fns.end()) {
                    apply_custom(value, meta, key, fn_it->second);
                }
            }
        }
    }

    // `:secret` masks the *resolved* value, so it runs after every other marker
    // wherever it sits in the chain (`:secret:env`, `:env:secret`).
    if (meta.has_marker("secret")) apply_secret(value, meta);

    // Apply runtime type cast for type hints (when no marker rewrote the value).
    if (meta.type_hint.has_value() && !value.is_secret()) {
        const std::string& th = *meta.type_hint;
        if (th == "int" && !value.is_int()) {
            int64_t exact = 0;
            double d = 0.0;
            if (value.is_string() && parse_i64(*value.as_string(), exact)) {
                value = Value::make_int(exact);
            } else if (value_to_number(value, d)) {
                value = Value::make_int(static_cast<int64_t>(d));
            }
        } else if (th == "float" && !value.is_float()) {
            double d = 0.0;
            if (value_to_number(value, d)) value = Value::make_float(d);
        } else if (th == "string" && !value.is_string()) {
            value = Value::make_string(value_to_string(value));
        } else if (th == "bool" && !value.is_bool()) {
            const std::string s = value_to_string(value);
            value = Value::make_bool(s == "true" || s == "1");
        }
    }

    // Constraint validation always runs last, after every marker has resolved;
    // a failure replaces the value with its error string.
    if (meta.constraints.has_value()) {
        if (auto err = constraint_error(key, value, *meta.constraints)) {
            report_violation(*err);
            value = Value::make_string(std::move(*err));
        }
    }
}

void Resolver::walk(Value& v, const std::string& path, int depth) {
    if (depth > static_cast<int>(kMaxResolveDepth)) return;
    if (Object* obj = v.as_object_mut()) {
        for (auto& p : *obj) {
            resolve_value(p.value, *obj, path, p.key, depth);
            if (p.value.is_object() || p.value.is_array()) {
                std::string sub = path.empty() ? p.key : path + "." + p.key;
                walk(p.value, sub, depth + 1);
            }
        }
        // Auto-`{}` interpolation, after this level's markers and children are resolved.
        interpolate_level(*obj);
    } else if (Array* arr = v.as_array_mut()) {
        for (auto& item : *arr) {
            if (item.is_object() || item.is_array()) {
                walk(item, path, depth + 1);
            }
        }
    }
}

// ─── inherit pre-pass ───────────────────────────────────────────────────────
namespace {
void merge_object_into(Object& dst, const Object& src) {
    for (const auto& sp : src) {
        bool found = false;
        for (auto& dp : dst) {
            if (dp.key == sp.key) {
                if (dp.value.is_object() && sp.value.is_object()) {
                    merge_object_into(*dp.value.as_object_mut(), *sp.value.as_object());
                } else {
                    // Child wins — don't overwrite existing values from inherited parent.
                }
                found = true;
                break;
            }
        }
        if (!found) dst.push_back(sp);
    }
}
} // namespace

void Resolver::apply_inherit_pass() {
    // For each metadata entry that has the `inherit` marker, look up the
    // referenced parent key in the same parent object and merge missing keys.
    for (auto& path_entry : result.metadata) {
        const std::string& path = path_entry.first;
        for (auto& field_entry : path_entry.second) {
            const Meta& m = field_entry.second;
            if (!m.has_marker("inherit")) continue;
            if (m.args.empty()) continue;

            // Locate the parent object containing this key.
            Value* parent_val = (path.empty()) ? &result.root :
                const_cast<Value*>(deep_get(root, path));
            if (!parent_val) continue;
            Object* parent_obj = parent_val->as_object_mut();
            if (!parent_obj) continue;

            // Find the inheriting object.
            Object* target = nullptr;
            for (auto& p : *parent_obj) {
                if (p.key == field_entry.first) {
                    target = p.value.as_object_mut();
                    break;
                }
            }
            if (!target) continue;

            for (const std::string& parent_name : m.args) {
                for (const auto& sib : *parent_obj) {
                    if (sib.key == parent_name) {
                        const Object* src = sib.value.as_object();
                        if (src) merge_object_into(*target, *src);
                        break;
                    }
                }
            }
        }
    }
}

void Resolver::strip_underscore_keys() {
    for (auto it = root.begin(); it != root.end(); ) {
        if (!it->key.empty() && it->key[0] == '_') {
            it = root.erase(it);
        } else {
            ++it;
        }
    }
}

void Resolver::report_violation(const std::string& message) const {
    if (options.strict) std::fprintf(stderr, "synx: %s\n", message.c_str());
}

namespace {

bool is_error_string(const Value& v) {
    const std::string* s = v.as_string();
    return s && (s->rfind("CONSTRAINT_ERR:", 0) == 0 || s->rfind("TYPE_ERR:", 0) == 0);
}

// Merge repeated `[…]` declarations of one field name: stricter bounds win,
// the first type / pattern / enum wins.
void merge_constraints(Constraints& base, const Constraints& in) {
    if (in.required) base.required = true;
    if (in.readonly) base.readonly = true;
    if (in.min.has_value()) base.min = base.min.has_value() ? std::max(*base.min, *in.min) : *in.min;
    if (in.max.has_value()) base.max = base.max.has_value() ? std::min(*base.max, *in.max) : *in.max;
    if (!base.type_name.has_value()) base.type_name = in.type_name;
    if (!base.pattern.has_value()) base.pattern = in.pattern;
    if (!base.enum_values.has_value()) base.enum_values = in.enum_values;
}

} // namespace

void Resolver::validate_field_constraints() {
    std::unordered_map<std::string, Constraints> registry;
    std::vector<std::string> paths;
    for (const auto& pe : result.metadata) paths.push_back(pe.first);
    std::sort(paths.begin(), paths.end());
    for (const auto& path : paths) {
        const MetaMap& fields = result.metadata.at(path);
        std::vector<std::string> keys;
        for (const auto& fe : fields) keys.push_back(fe.first);
        std::sort(keys.begin(), keys.end());
        for (const auto& key : keys) {
            const Meta& m = fields.at(key);
            if (!m.constraints.has_value()) continue;
            auto it = registry.find(key);
            if (it == registry.end()) registry.emplace(key, *m.constraints);
            else merge_constraints(it->second, *m.constraints);
        }
    }
    if (registry.empty()) return;

    // Skips values already holding an error string, so the message is not re-judged.
    struct Pass {
        Resolver& self;
        const std::unordered_map<std::string, Constraints>& registry;
        void run(Object& obj) {
            for (auto& p : obj) {
                auto it = registry.find(p.key);
                if (it != registry.end() && !is_error_string(p.value)) {
                    if (auto err = constraint_error(p.key, p.value, it->second)) {
                        self.report_violation(*err);
                        p.value = Value::make_string(std::move(*err));
                    }
                }
                if (Object* child = p.value.as_object_mut()) {
                    run(*child);
                } else if (Array* arr = p.value.as_array_mut()) {
                    for (auto& item : *arr) {
                        if (Object* o = item.as_object_mut()) run(*o);
                    }
                }
            }
        }
    };
    Pass{*this, registry}.run(root);
}

void Resolver::validate_field_types() {
    std::unordered_map<std::string, std::string> registry;
    std::vector<std::string> paths;
    for (const auto& pe : result.metadata) paths.push_back(pe.first);
    std::sort(paths.begin(), paths.end());
    for (const auto& path : paths) {
        const MetaMap& fields = result.metadata.at(path);
        std::vector<std::string> keys;
        for (const auto& fe : fields) keys.push_back(fe.first);
        std::sort(keys.begin(), keys.end());
        for (const auto& key : keys) {
            const Meta& m = fields.at(key);
            if (m.type_hint.has_value() && registry.find(key) == registry.end()) {
                registry.emplace(key, *m.type_hint);
            }
        }
    }
    if (registry.empty()) return;

    struct Pass {
        Resolver& self;
        const std::unordered_map<std::string, std::string>& registry;
        static bool matches(const Value& v, const std::string& t) {
            if (t == "int") return v.is_int();
            if (t == "float") return v.is_float() || v.is_int();
            if (t == "bool") return v.is_bool();
            if (t == "string") return v.is_string() || v.is_secret();
            if (t == "array") return v.is_array();
            if (t == "object") return v.is_object();
            return true; // unknown types are accepted
        }
        void run(Object& obj) {
            for (auto& p : obj) {
                auto it = registry.find(p.key);
                if (it != registry.end() && !matches(p.value, it->second)) {
                    std::string err = "TYPE_ERR: '" + p.key + "' expected " + it->second
                                    + " but got " + p.value.type_name();
                    self.report_violation(err);
                    p.value = Value::make_string(std::move(err));
                }
                if (Object* child = p.value.as_object_mut()) {
                    run(*child);
                } else if (Array* arr = p.value.as_array_mut()) {
                    for (auto& item : *arr) {
                        if (Object* o = item.as_object_mut()) run(*o);
                    }
                }
            }
        }
    };
    Pass{*this, registry}.run(root);
}

void Resolver::load_packages() {
    if (result.uses.empty()) return;
    std::string base = options.packages_path.value_or("./synx_packages");
    for (const auto& use : result.uses) {
        // Convert `@scope/name` → `<base>/@scope/name/synx.synx`
        std::string rel = use.package;
        // Reject suspicious package names.
        if (rel.empty() || rel.find("..") != std::string::npos) continue;
        std::string path = join_paths(base, rel + "/synx.synx");
        FileText ft = read_text_file(path);
        if (!ft.ok) continue;
        ParseResult sub = parse(ft.text);
        if (Object* sub_obj = sub.root.as_object_mut()) {
            // Make the sub-object available via alias for interpolation/ref.
            // Move into root under alias so interpolation can resolve `{alias.key}`.
            root.push_back(Pair{use.alias, Value::make_object(std::move(*sub_obj))});
            namespaces[use.alias] = nullptr;
            // `root` may have reallocated: refresh every namespace pointer.
            refresh_namespaces();
        }
    }
}

void Resolver::load_includes() {
    if (result.includes.empty()) return;
    if (options._include_depth >= options.max_include_depth.value_or(16)) return;

    std::string base = options.base_path.value_or(".");
    for (const auto& inc : result.includes) {
        std::string path = jail_path(base, inc.path);
        if (path.empty()) continue;
        FileText ft = read_text_file(path);
        if (!ft.ok) continue;

        Options sub_opts = options;
        sub_opts.base_path = dirname_of(path);
        sub_opts._include_depth = options._include_depth + 1;

        ParseResult sub = parse(ft.text);
        if (sub.mode == Mode::Active) {
            resolve(sub, sub_opts);
        }
        Object* sub_obj = sub.root.as_object_mut();
        if (!sub_obj) continue;
        root.push_back(Pair{inc.alias, Value::make_object(std::move(*sub_obj))});
        namespaces[inc.alias] = nullptr;
        include_aliases.push_back(inc.alias);
        // `root` may have reallocated: earlier namespace pointers are stale.
        refresh_namespaces();
    }
}

void Resolver::flush_once() {
    if (once_new_keys.empty()) return;
    std::string base = options.base_path.value_or(".");
    std::string path = join_paths(base, ".synx.lock");
    // Merge with existing.
    std::unordered_set<std::string> all = once_keys;
    for (const auto& k : once_new_keys) all.insert(k);
    std::ofstream out(path, std::ios::binary | std::ios::trunc);
    if (!out.good()) return;
    for (const auto& k : all) out << k << "\n";
}

} // namespace detail

// ─── public API ─────────────────────────────────────────────────────────────
void resolve(ParseResult& result, const Options& options) {
    if (result.mode != Mode::Active) return;
    if (!result.root.is_object()) return;

    detail::Resolver r(result, options);

    // 1. Load packages (`!use`) — non-fatal on missing.
    r.load_packages();
    // 2. Load includes (`!include`).
    r.load_includes();
    // 3. Inheritance pre-pass.
    r.apply_inherit_pass();
    // 4. Strip `_`-prefixed helper keys at top level.
    r.strip_underscore_keys();
    // 5. Mask literal `:secret` values before anything can interpolate them.
    r.mask_secret_literals();
    // 6. Resolve markers across the whole tree.
    r.walk(result.root, std::string{}, 0);
    // 7. Validate constraints.
    r.validate_field_constraints();
    r.validate_field_types();
    // 8. Persist `:once` writes.
    r.flush_once();
}

} // namespace synx
