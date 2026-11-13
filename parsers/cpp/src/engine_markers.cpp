// SYNX `!active` markers. Port of `apply_markers` and its helpers in
// crates/synx-core/src/engine.rs — same order, same early returns, same texts.
#include "engine_internal.hpp"

#include "internal.hpp"
#include "numfmt.hpp"
#include "paths.hpp"
#include "synx/calc.hpp"
#include "synx/parser.hpp"
#include "unicode.hpp"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdlib>
#include <ctime>
#include <fstream>
#include <mutex>
#include <optional>
#include <sstream>
#include <string>
#include <string_view>
#include <unordered_map>
#include <unordered_set>
#include <vector>

namespace synx {
namespace detail {

namespace {

namespace fs = std::filesystem;

const std::string* marker_after(const std::vector<std::string>& markers, std::string_view name, size_t offset = 1) {
    for (size_t i = 0; i < markers.size(); ++i) {
        if (markers[i] == name) return i + offset < markers.size() ? &markers[i + offset] : nullptr;
    }
    return nullptr;
}

int marker_pos(const std::vector<std::string>& markers, std::string_view name) {
    for (size_t i = 0; i < markers.size(); ++i) {
        if (markers[i] == name) return static_cast<int>(i);
    }
    return -1;
}

std::string join_from(const std::vector<std::string>& markers, size_t from) {
    std::string out;
    for (size_t k = from; k < markers.size(); ++k) {
        if (k > from) out.push_back(':');
        out.append(markers[k]);
    }
    return out;
}

/// Int when the result is a whole number below 2^63, else Float.
Value number_value(double r) {
    if (is_whole(r) && std::fabs(r) < 9223372036854775807.0) return Value::make_int(static_cast<int64_t>(r));
    return Value::make_float(r);
}

std::string delimiter_from_keyword(const std::string& keyword) {
    if (keyword == "space") return " ";
    if (keyword == "pipe") return "|";
    if (keyword == "dash") return "-";
    if (keyword == "dot") return ".";
    if (keyword == "semi") return ";";
    if (keyword == "tab") return "\t";
    if (keyword == "slash") return "/";
    return keyword;
}

std::string replace_all(std::string_view s, std::string_view from, std::string_view to) {
    std::string out;
    size_t pos = 0;
    while (true) {
        size_t hit = s.find(from, pos);
        if (hit == std::string_view::npos) break;
        out.append(s.substr(pos, hit - pos));
        out.append(to);
        pos = hit + from.size();
    }
    out.append(s.substr(pos));
    return out;
}

/// The value of an environment variable as UTF-8 (Rust `std::env::var`).
std::optional<std::string> read_process_env(const std::string& name) {
#if defined(_WIN32)
    std::wstring wname;
    for (size_t i = 0; i < name.size();) {
        size_t len = 0;
        uint32_t cp = decode_utf8(name, i, len);
        i += len;
        if (cp >= 0x10000) {
            cp -= 0x10000;
            wname.push_back(static_cast<wchar_t>(0xD800 + (cp >> 10)));
            wname.push_back(static_cast<wchar_t>(0xDC00 + (cp & 0x3FF)));
        } else {
            wname.push_back(static_cast<wchar_t>(cp));
        }
    }
#  if defined(_MSC_VER)
    wchar_t* owned = nullptr;
    size_t owned_len = 0;
    if (_wdupenv_s(&owned, &owned_len, wname.c_str()) != 0 || !owned) return std::nullopt;
    std::wstring value(owned);
    std::free(owned);
    const wchar_t* raw = value.c_str();
#  else
    const wchar_t* raw = _wgetenv(wname.c_str());
    if (!raw) return std::nullopt;
#  endif
    std::string out;
    for (const wchar_t* p = raw; *p; ++p) {
        uint32_t cp = static_cast<uint16_t>(*p);
        if (cp >= 0xD800 && cp <= 0xDBFF && p[1] >= 0xDC00 && p[1] <= 0xDFFF) {
            cp = 0x10000 + ((cp - 0xD800) << 10) + (static_cast<uint16_t>(p[1]) - 0xDC00);
            ++p;
        } else if (cp >= 0xD800 && cp <= 0xDFFF) {
            return std::nullopt;  // not Unicode: Rust's env::var fails
        }
        push_utf8(out, cp);
    }
    return out;
#else
    const char* raw = std::getenv(name.c_str());
    if (!raw) return std::nullopt;
    return std::string(raw);
#endif
}

// ─── :spam ──────────────────────────────────────────────────────────────────

bool allow_spam_access(const std::string& bucket_key, size_t max_calls, uint64_t window_sec) {
    using clock = std::chrono::steady_clock;
    static std::mutex mu;
    static std::unordered_map<std::string, std::vector<clock::time_point>>* buckets =
        new std::unordered_map<std::string, std::vector<clock::time_point>>();
    const auto now = clock::now();
    const auto window = std::chrono::seconds(static_cast<long long>(std::max<uint64_t>(window_sec, 1)));
    std::lock_guard<std::mutex> lock(mu);
    auto& calls = (*buckets)[bucket_key];
    calls.erase(std::remove_if(calls.begin(), calls.end(), [&](clock::time_point ts) { return now - ts > window; }),
                calls.end());
    if (calls.size() >= max_calls) return false;
    calls.push_back(now);
    return true;
}

// ─── :random ────────────────────────────────────────────────────────────────

Value weighted_random(const Array& items, const std::vector<double>& weights) {
    std::vector<double> w = weights;
    if (w.size() < items.size()) {
        double assigned = 0.0;
        for (double x : w) assigned += x;
        // Weights above 100 give the rest their average; room below 100 is shared.
        double per_item = assigned < 100.0 ? (100.0 - assigned) / static_cast<double>(items.size() - w.size())
                                           : assigned / static_cast<double>(w.size());
        while (w.size() < items.size()) w.push_back(per_item);
    }
    double total = 0.0;
    for (double x : w) total += x;
    if (total <= 0.0) return items[random_usize(items.size())];
    double rand_val = random_f64_01();
    double cumulative = 0.0;
    for (size_t i = 0; i < items.size(); ++i) {
        cumulative += w[i] / total;
        if (rand_val <= cumulative) return items[i];
    }
    return items.empty() ? Value::make_null() : items.back();
}

// ─── :i18n ──────────────────────────────────────────────────────────────────

const char* plural_category(std::string_view lang, int64_t n) {
    uint64_t abs_n = n < 0 ? static_cast<uint64_t>(0) - static_cast<uint64_t>(n) : static_cast<uint64_t>(n);
    uint64_t n10 = abs_n % 10;
    uint64_t n100 = abs_n % 100;
    if (lang == "ru" || lang == "uk" || lang == "be" || lang == "pl") {
        if (n10 == 1 && n100 != 11) return "one";
        if (n10 >= 2 && n10 <= 4 && !(n100 >= 12 && n100 <= 14)) return "few";
        return "many";
    }
    if (lang == "cs" || lang == "sk") {
        if (abs_n == 1) return "one";
        if (abs_n >= 2 && abs_n <= 4) return "few";
        return "other";
    }
    if (lang == "ar") {
        if (abs_n == 0) return "zero";
        if (abs_n == 1) return "one";
        if (abs_n == 2) return "two";
        if (n100 >= 3 && n100 <= 10) return "few";
        if (n100 >= 11 && n100 <= 99) return "many";
        return "other";
    }
    if (lang == "fr" || lang == "pt") return abs_n <= 1 ? "one" : "other";
    if (lang == "ja" || lang == "zh" || lang == "ko" || lang == "vi" || lang == "th") return "other";
    return abs_n == 1 ? "one" : "other";
}

const Value* object_get(const Object& obj, std::string_view key) {
    for (const auto& p : obj) {
        if (p.key == key) return &p.value;
    }
    return nullptr;
}

// ─── :calc ──────────────────────────────────────────────────────────────────

inline bool is_word_char(unsigned char b) noexcept {
    return (b >= '0' && b <= '9') || (b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z') || b == '_';
}

// Whole-word replacement of `word` (Rust `replace_word`, with the scratch cap).
std::string replace_word(std::string_view hay, std::string_view word, std::string_view repl) {
    if (word.empty() || word.size() > hay.size()) return std::string(hay);
    std::string result;
    result.reserve(std::min(hay.size(), kMaxScratchString));
    size_t i = 0;
    while (i <= hay.size() - word.size()) {
        if (result.size() >= kMaxScratchString) break;
        if (hay.compare(i, word.size(), word) == 0) {
            bool before_ok = i == 0 || !is_word_char(static_cast<unsigned char>(hay[i - 1]));
            bool after_ok = i + word.size() >= hay.size()
                         || !is_word_char(static_cast<unsigned char>(hay[i + word.size()]));
            if (before_ok && after_ok) {
                size_t room = kMaxScratchString - result.size();
                result.append(repl.data(), utf8_prefix_len(repl, std::min(repl.size(), room)));
                i += word.size();
                continue;
            }
        }
        size_t len = utf8_len(static_cast<unsigned char>(hay[i]));
        if (result.size() < kMaxScratchString) result.append(hay.substr(i, len));
        i += len;
    }
    if (i < hay.size()) {
        size_t room = kMaxScratchString > result.size() ? kMaxScratchString - result.size() : 0;
        std::string_view tail = hay.substr(i);
        result.append(tail.data(), utf8_prefix_len(tail, std::min(room, tail.size())));
    }
    return result;
}

// Names in a `:calc` expression: a key of the same object first (it shadows the
// root), then a root key. nullopt past kMaxCalcResolvedLen.
std::optional<std::string> substitute_calc_vars(std::string_view expr, const ObjectLookup& local,
                                                const std::string& self_key, const ObjectLookup& root,
                                                const CalcScope& scope) {
    const bool local_is_root = &local.object() == &root.object();
    auto numeric = [](const ObjectLookup& m, std::string_view k, double& out) {
        const Value* v = m.get(k);
        return v && value_as_number(*v, out);
    };

    bool special_in_play = false;
    double tmp = 0.0;
    for (const auto& k : scope.root_special) {
        if (numeric(root, k, tmp)) { special_in_play = true; break; }
    }
    if (!special_in_play && !local_is_root) {
        for (const auto& p : local.object()) {
            if (p.key != self_key && value_as_number(p.value, tmp) && is_calc_special_key(p.key)) {
                special_in_play = true;
                break;
            }
        }
    }

    if (special_in_play) {
        // Keys a token lookup cannot see (`max-hp`, `a.b`, `2`): exact sequential
        // whole-word replacement, object before root.
        std::string resolved(expr);
        const Object* maps[2] = {&local.object(), local_is_root ? nullptr : &root.object()};
        for (const Object* m : maps) {
            if (!m) continue;
            for (const auto& p : *m) {
                if (m == &local.object() && p.key == self_key) continue;
                double n = 0.0;
                if (!value_as_number(p.value, n)) continue;
                resolved = replace_word(resolved, p.key, format_number(n));
                if (resolved.size() > kMaxCalcResolvedLen) return std::nullopt;
            }
        }
        return resolved;
    }

    std::string out;
    out.reserve(expr.size() + 16);
    size_t i = 0;
    while (i < expr.size()) {
        if (is_word_char(static_cast<unsigned char>(expr[i]))) {
            size_t start = i;
            while (i < expr.size() && is_word_char(static_cast<unsigned char>(expr[i]))) ++i;
            std::string_view token = expr.substr(start, i - start);
            double n = 0.0;
            bool found = false;
            if (token != self_key) found = numeric(local, token, n);
            if (!found) found = numeric(root, token, n);
            if (found) out.append(format_number(n));
            else out.append(token);
        } else {
            size_t len = utf8_len(static_cast<unsigned char>(expr[i]));
            out.append(expr.substr(i, len));
            i += len;
        }
        if (out.size() > kMaxCalcResolvedLen) return std::nullopt;
    }
    return out;
}

// ─── :format ────────────────────────────────────────────────────────────────

std::string pad_left(std::string s, size_t width, char fill) {
    size_t chars = char_count(s);
    if (chars < width) s.insert(0, width - chars, fill);
    return s;
}

std::string format_int_pattern(std::string_view pattern, int64_t n) {
    // Guardrail: a user-controlled width can be enormous.
    constexpr uint64_t kMaxWidth = 4096;
    if (!pattern.empty() && pattern[0] == '%') {
        std::string_view s = pattern.substr(1);
        if (!s.empty() && (s.back() == 'd' || s.back() == 'i')) {
            std::string_view inner = s.substr(0, s.size() - 1);
            uint64_t width = 0;
            if (!inner.empty() && inner[0] == '0' && parse_u64(inner.substr(1), width)) {
                return pad_left(i64_to_string(n), static_cast<size_t>(std::min(width, kMaxWidth)), '0');
            }
            if (parse_u64(inner, width)) {
                return pad_left(i64_to_string(n), static_cast<size_t>(std::min(width, kMaxWidth)), ' ');
            }
        }
    }
    return i64_to_string(n);
}

std::string format_float_pattern(std::string_view pattern, double f) {
    constexpr uint64_t kMaxPrec = 1024;
    if (!pattern.empty() && pattern[0] == '%') {
        std::string_view s = pattern.substr(1);
        // `%e` — the JS `toExponential()` shape: `1.23456789e+5`.
        if (s == "e") {
            if (f == 0.0) return "0e+0";
            std::string raw = exp_f64(f);
            size_t epos = raw.rfind('e');
            if (epos == std::string::npos) return raw;
            std::string mantissa = raw.substr(0, epos);
            std::string_view exp = std::string_view(raw).substr(epos + 1);
            char sign = '+';
            if (!exp.empty() && (exp[0] == '+' || exp[0] == '-')) {
                sign = exp[0];
                exp.remove_prefix(1);
            }
            if (mantissa.size() >= 2 && mantissa.compare(mantissa.size() - 2, 2, ".0") == 0) mantissa.resize(mantissa.size() - 2);
            return mantissa + "e" + sign + std::string(exp);
        }
        if (!s.empty() && (s.back() == 'f' || s.back() == 'e')) {
            std::string_view inner = s.substr(0, s.size() - 1);
            uint64_t prec = 0;
            if (!inner.empty() && inner[0] == '.' && parse_u64(inner.substr(1), prec)) {
                size_t p = static_cast<size_t>(std::min(prec, kMaxPrec));
                return s.back() == 'e' ? exp_prec_f64(f, p) : fixed_f64(f, p);
            }
        }
    }
    return display_f64(f);
}

std::string apply_format_pattern(std::string_view pattern, const Value& value) {
    auto has = [&](char c) { return pattern.find(c) != std::string_view::npos; };
    if (const int64_t* n = value.as_int()) {
        if (has('d') || has('i')) return format_int_pattern(pattern, *n);
        if (has('f') || has('e')) return format_float_pattern(pattern, static_cast<double>(*n));
        return i64_to_string(*n);
    }
    if (const double* f = value.as_float()) {
        if (has('f') || has('e')) return format_float_pattern(pattern, *f);
        // A whole float takes an integer pattern: `12.0` under `%05d` is `00012`.
        if ((has('d') || has('i')) && is_whole(*f) && std::fabs(*f) < 9.0e15) {
            return format_int_pattern(pattern, static_cast<int64_t>(*f));
        }
        return format_number(*f);
    }
    if (const std::string* s = value.as_string()) return *s;
    return value_to_string(value);
}

// ─── :once ──────────────────────────────────────────────────────────────────

std::vector<std::string_view> rust_lines(std::string_view text) {
    std::vector<std::string_view> lines;
    size_t start = 0;
    while (start < text.size()) {
        size_t nl = text.find('\n', start);
        size_t end = nl == std::string_view::npos ? text.size() : nl;
        std::string_view line = text.substr(start, end - start);
        if (!line.empty() && line.back() == '\r') line.remove_suffix(1);
        lines.push_back(line);
        start = end + 1;
    }
    return lines;
}

std::optional<std::string> read_lock_value(const fs::path& lock_path, const std::string& key) {
    std::string content;
    std::string err;
    if (!read_text(lock_path, "", content, err)) return std::nullopt;
    for (std::string_view line : rust_lines(content)) {
        if (line.substr(0, key.size()) == key && line.size() > key.size() && line[key.size()] == ' ') {
            return std::string(trim_start_view(line.substr(key.size())));
        }
    }
    return std::nullopt;
}

void write_lock_value(const fs::path& lock_path, const std::string& key, const std::string& value) {
    std::string content, err;
    if (!read_text(lock_path, "", content, err)) content.clear();
    std::vector<std::string> lines;
    for (std::string_view l : rust_lines(content)) lines.emplace_back(l);
    std::string new_line = key + " " + value;
    bool found = false;
    for (auto& line : lines) {
        if (line.compare(0, key.size(), key) == 0 && line.size() > key.size() && line[key.size()] == ' ') {
            line = new_line;
            found = true;
            break;
        }
    }
    if (!found) lines.push_back(new_line);
    std::string out;
    for (size_t i = 0; i < lines.size(); ++i) {
        if (i > 0) out.push_back('\n');
        out.append(lines[i]);
    }
    out.push_back('\n');
    std::ofstream f(lock_path, std::ios::binary | std::ios::trunc);
    f << out;
}

// ─── :version ───────────────────────────────────────────────────────────────

// The integer at the start of `s` (after whitespace, with a sign), or 0.
int64_t leading_int(std::string_view s) {
    std::string_view t = trim_start_view(s);
    bool neg = false;
    if (!t.empty() && (t[0] == '-' || t[0] == '+')) {
        neg = t[0] == '-';
        t.remove_prefix(1);
    }
    int64_t n = 0;
    for (char c : t) {
        if (c < '0' || c > '9') break;
        int64_t d = c - '0';
        n = n > (INT64_MAX - d) / 10 ? INT64_MAX : n * 10 + d;
    }
    return neg ? -n : n;
}

bool compare_versions(std::string_view current, std::string_view op, std::string_view required) {
    // Positional parts, each read for its leading integer: `1.2.3-beta` is 1.2.3.
    auto parse_ver = [](std::string_view s) {
        std::vector<int64_t> parts;
        size_t start = 0;
        while (true) {
            size_t dot = s.find('.', start);
            parts.push_back(leading_int(s.substr(start, (dot == std::string_view::npos ? s.size() : dot) - start)));
            if (dot == std::string_view::npos) break;
            start = dot + 1;
        }
        return parts;
    };
    std::vector<int64_t> cv = parse_ver(current);
    std::vector<int64_t> rv = parse_ver(required);
    int ord = 0;
    for (size_t i = 0; i < std::max(cv.size(), rv.size()); ++i) {
        int64_t a = i < cv.size() ? cv[i] : 0;
        int64_t b = i < rv.size() ? rv[i] : 0;
        if (a != b) { ord = a < b ? -1 : 1; break; }
    }
    if (op == ">=") return ord >= 0;
    if (op == "<=") return ord <= 0;
    if (op == ">") return ord > 0;
    if (op == "<") return ord < 0;
    if (op == "==" || op == "=") return ord == 0;
    if (op == "!=") return ord != 0;
    return false;
}

// ─── :watch ─────────────────────────────────────────────────────────────────

std::optional<Value> extract_from_file_content(const std::string& content, const std::string& key_path,
                                               const std::string& ext) {
    std::optional<Value> root;
    if (ext == "json") {
        root = parse_json(content);
        if (!root) return std::nullopt;
    } else {
        root = std::move(parse(content).root);
    }
    const Value* current = &*root;
    size_t start = 0;
    while (true) {
        size_t dot = key_path.find('.', start);
        std::string_view part = std::string_view(key_path).substr(start, (dot == std::string::npos ? key_path.size() : dot) - start);
        const Object* obj = current->as_object();
        if (!obj) return std::nullopt;
        current = object_get(*obj, part);
        if (!current) return std::nullopt;
        if (dot == std::string::npos) break;
        start = dot + 1;
    }
    return *current;
}

// ─── :prompt ────────────────────────────────────────────────────────────────

std::string stringify_value(const Value& value, size_t indent) {
    std::string spaces(indent, ' ');
    std::string out;
    if (const Object* map = value.as_object()) {
        std::vector<const Pair*> keys;
        for (const auto& p : *map) keys.push_back(&p);
        std::sort(keys.begin(), keys.end(), [](const Pair* a, const Pair* b) { return a->key < b->key; });
        for (const Pair* p : keys) {
            if (p->value.is_object()) {
                out += spaces + p->key + "\n";
                out += stringify_value(p->value, indent + 2);
            } else if (const Array* arr = p->value.as_array()) {
                out += spaces + p->key + "\n";
                for (const auto& item : *arr) out += spaces + "  - " + value_to_string(item) + "\n";
            } else {
                out += spaces + p->key + " " + value_to_string(p->value) + "\n";
            }
        }
        return out;
    }
    if (const Array* arr = value.as_array()) {
        for (const auto& item : *arr) out += spaces + "  - " + value_to_string(item) + "\n";
        return out;
    }
    return spaces + value_to_string(value) + "\n";
}

// ─── custom markers (C++ API; dispatched where Rust dispatches WASM markers) ─

bool is_builtin_marker(const std::string& name) {
    static const char* const kBuiltins[] = {
        "spam", "include", "import", "env", "random", "ref", "i18n", "calc", "alias", "secret",
        "unique", "geo", "template", "split", "join", "default", "clamp", "round", "map", "format",
        "fallback", "once", "version", "watch", "prompt", "vision", "audio", "replace", "sort", "sum",
        "inherit",
    };
    for (const char* b : kBuiltins) {
        if (name == b) return true;
    }
    return false;
}

// One element of a `:sort` input, with its comparison keys taken once.
struct SortItem {
    const Value* value;
    bool numeric;
    double number;
    std::string text;
};

// A total order: numbers first, by value (NaN after the rest of them), then
// everything else by its text, by code point. Comparing a number with text as
// text made `10 < 1a < 9 < 10` a cycle, and the result depended on the sort.
bool sort_less(const SortItem& a, const SortItem& b) {
    if (a.numeric && b.numeric) {
        if (std::isnan(a.number) || std::isnan(b.number)) return !std::isnan(a.number) && std::isnan(b.number);
        return a.number < b.number;
    }
    if (a.numeric != b.numeric) return a.numeric;
    return a.text < b.text;  // byte order of UTF-8 is code-point order
}

// Non-overlapping occurrences of `needle` in `hay`, left to right.
size_t count_matches(std::string_view hay, std::string_view needle) {
    size_t n = 0;
    for (size_t at = hay.find(needle); at != std::string_view::npos; at = hay.find(needle, at + needle.size())) ++n;
    return n;
}

void merge_sort(std::vector<SortItem>& v, std::vector<SortItem>& tmp, size_t lo, size_t hi) {
    if (hi - lo < 2) return;
    size_t mid = lo + (hi - lo) / 2;
    merge_sort(v, tmp, lo, mid);
    merge_sort(v, tmp, mid, hi);
    size_t i = lo, j = mid, k = lo;
    while (i < mid && j < hi) {
        // Stable: the right element goes first only when strictly smaller.
        if (sort_less(v[j], v[i])) tmp[k++] = std::move(v[j++]);
        else tmp[k++] = std::move(v[i++]);
    }
    while (i < mid) tmp[k++] = std::move(v[i++]);
    while (j < hi) tmp[k++] = std::move(v[j++]);
    for (size_t x = lo; x < hi; ++x) v[x] = std::move(tmp[x]);
}

} // namespace

// ─── apply_markers ──────────────────────────────────────────────────────────

void Resolver::apply_markers(Object& map, size_t index, const ObjectLookup& local, const Meta& meta,
                             const std::string& path, size_t depth) {
    const std::vector<std::string>& markers = meta.markers;
    const uint32_t mb = marker_bits(markers);
    const bool secret = (mb & mk::SECRET) != 0;
    const std::string key = map[index].key;
    Value& cur = map[index].value;

    // ── :spam — key:spam:MAX_CALLS[:WINDOW_SEC] target ──
    if (mb & mk::SPAM) {
        uint64_t max_calls = 0;
        uint64_t window_sec = 1;
        if (const std::string* s = marker_after(markers, "spam")) {
            if (!parse_u64(*s, max_calls)) max_calls = 0;
        }
        if (const std::string* s = marker_after(markers, "spam", 2)) {
            if (!parse_u64(*s, window_sec)) window_sec = 1;
        }
        if (max_calls == 0) {
            cur = Value::make_string("SPAM_ERR: invalid limit, use :spam:MAX[:WINDOW_SEC]");
            return;
        }
        std::string target = value_to_string(cur);
        if (!allow_spam_access(key + "::" + target, static_cast<size_t>(max_calls), window_sec)) {
            cur = Value::make_string("SPAM_ERR: '" + target + "' exceeded " + std::to_string(max_calls)
                                     + " calls per " + std::to_string(window_sec) + "s");
            return;
        }
        const Value* resolved = deep_get(root, target, &root_lookup);
        if (!resolved) resolved = local.get(target);
        if (resolved) cur = budgeted_copy(*resolved, depth);
    }

    // ── :include / :import ──
    if ((mb & mk::INCLUDE) || (mb & mk::IMPORT)) {
        if (const std::string* file_path = cur.as_string()) {
            const std::string file = *file_path;
            size_t max_depth = options.max_include_depth.value_or(kDefaultMaxIncludeDepth);
            if (options._include_depth >= max_depth) {
                cur = Value::make_string("INCLUDE_ERR: max include depth (" + std::to_string(max_depth) + ") exceeded");
                return;
            }
            // A root key the pre-pass already resolved for the same path.
            if (path.empty()) {
                if (const Value* pre = preresolved.get(key)) {
                    const Array* a = pre->as_array();
                    const std::string* pre_path = a && a->size() == 2 ? (*a)[0].as_string() : nullptr;
                    if (pre_path && *pre_path == file) {
                        cur = placed_document((*a)[1], depth, secret);
                        return;
                    }
                }
            }
            JailResult j = jail_path(options.base_path.value_or("."), file);
            if (!j.ok) {
                cur = Value::make_string("INCLUDE_ERR: " + j.error);
                return;
            }
            std::string size_err = check_file_size(j.path);
            if (!size_err.empty()) {
                cur = Value::make_string("INCLUDE_ERR: " + size_err);
                return;
            }
            Value included;
            std::string err;
            if (include_file(j.path, file, options, included, err)) cur = placed_document(std::move(included), depth, secret);
            else cur = Value::make_string("INCLUDE_ERR: " + err);
        }
        // safety: the chain ends here, so `:secret` is applied here — a value
        // that was not a path (`k:secret:include 5`) stayed in the clear.
        if (secret) mask_secret_value(cur);
        return;
    }

    // ── :env ──
    if (mb & mk::ENV) {
        if (const std::string* name = cur.as_string()) {
            std::optional<std::string> env_val;
            if (options.env) {
                auto it = options.env->find(*name);
                if (it != options.env->end()) env_val = it->second;
            } else {
                env_val = read_process_env(*name);
            }
            const bool force_string = wants_string(meta);
            const int di = marker_pos(markers, "default");
            if (env_val && !env_val->empty()) {
                cur = external_value(*env_val, force_string, secret);
            } else if (di >= 0 && markers.size() > static_cast<size_t>(di) + 1) {
                // Every part after `default`, rejoined with ':' (IPs, compound values).
                cur = external_value(join_from(markers, static_cast<size_t>(di) + 1), force_string, secret);
            } else {
                cur = Value::make_null();
            }
        }
    }

    // ── :random ──
    if (mb & mk::RANDOM) {
        if (const Array* arr = cur.as_array()) {
            if (arr->empty()) {
                cur = Value::make_null();
                return;
            }
            Value picked;
            if (!meta.args.empty()) {
                std::vector<double> weights;
                for (const auto& s : meta.args) {
                    double w = 0.0;
                    if (parse_f64(s, w)) weights.push_back(w);
                }
                picked = weighted_random(*arr, weights);
            } else {
                picked = (*arr)[random_usize(arr->size())];
            }
            cur = std::move(picked);
        }
    }

    // ── :ref — like :alias, feeding the value into the next markers;
    //    `:ref:calc:*2 base` applies `VALUE * 2` ──
    if (mb & mk::REF) {
        if (const std::string* target_p = cur.as_string()) {
            const std::string target = *target_p;
            const Value* found = deep_get(root, target, &root_lookup);
            if (!found) found = local.get(target);
            Value resolved = found ? budgeted_copy(*found, depth) : Value::make_null();
            double n = 0.0;
            const std::string* calc_expr = marker_after(markers, "calc");
            if ((mb & mk::CALC) && value_as_number(resolved, n) && calc_expr) {
                char first = calc_expr->empty() ? ' ' : (*calc_expr)[0];
                if (std::string_view("+-*/%").find(first) != std::string_view::npos) {
                    CalcResult r = safe_calc(format_number(n) + " " + *calc_expr);
                    cur = r.ok ? number_value(r.value) : Value::make_string("CALC_ERR: " + r.error);
                } else {
                    cur = std::move(resolved);
                }
            } else {
                cur = std::move(resolved);
            }
        }
    }

    // ── :i18n[:COUNT_FIELD] — a localized value, with plural forms ──
    if (mb & mk::I18N) {
        if (const Object* translations = cur.as_object()) {
            const std::string lang = options.lang.value_or("en");
            const Value* pick = object_get(*translations, lang);
            if (!pick) pick = object_get(*translations, "en");
            if (!pick && !translations->empty()) pick = &translations->front().value;
            Value val = pick ? Value(*pick) : Value::make_null();
            const std::string* count_field = marker_after(markers, "i18n");
            if (count_field && val.is_object()) {
                double cn = 0.0;
                bool have = false;
                if (const Value* lv = local.get(*count_field)) have = value_as_number(*lv, cn);
                if (!have) {
                    if (const Value* rv = deep_get(root, *count_field, &root_lookup)) have = value_as_number(*rv, cn);
                }
                const int64_t count = saturating_i64(have ? cn : 0.0);
                const Object& forms = *val.as_object();
                const Value* chosen = object_get(forms, plural_category(lang, count));
                if (!chosen) chosen = object_get(forms, "other");
                if (!chosen && !forms.empty()) chosen = &forms.front().value;
                Value out = chosen ? Value(*chosen) : Value::make_null();
                if (const std::string* s = out.as_string()) {
                    cur = Value::make_string(replace_all(*s, "{count}", i64_to_string(count)));
                } else {
                    cur = std::move(out);
                }
            } else {
                cur = std::move(val);
            }
        }
    }

    // ── :calc ──
    if (mb & mk::CALC) {
        if (const std::string* expr_p = cur.as_string()) {
            const std::string expr = *expr_p;
            auto too_long = [&] {
                cur = Value::make_string("CALC_ERR: resolved expression too long (max "
                                         + std::to_string(kMaxCalcResolvedLen) + " bytes)");
            };
            if (expr.size() > kMaxCalcExprLen) {
                cur = Value::make_string("CALC_ERR: expression too long (" + std::to_string(expr.size())
                                         + " chars, max " + std::to_string(kMaxCalcExprLen) + ")");
                return;
            }
            std::optional<std::string> sub = substitute_calc_vars(expr, local, key, root_lookup, calc_scope);
            if (!sub) {
                too_long();
                return;
            }
            // Dotted references (`base.hp`, `server.port`).
            const std::string& resolved = *sub;
            std::string dot_resolved;
            size_t i = 0;
            while (i < resolved.size()) {
                if (is_word_char(static_cast<unsigned char>(resolved[i]))) {
                    size_t start = i;
                    bool has_dot = false;
                    while (i < resolved.size()
                           && (is_word_char(static_cast<unsigned char>(resolved[i])) || resolved[i] == '.')) {
                        has_dot = has_dot || resolved[i] == '.';
                        ++i;
                    }
                    std::string_view token = std::string_view(resolved).substr(start, i - start);
                    if (has_dot) {
                        const Value* v = deep_get(root, token, &root_lookup);
                        double n = 0.0;
                        if (v && value_as_number(*v, n)) {
                            dot_resolved.append(format_number(n));
                            if (dot_resolved.size() > kMaxCalcResolvedLen) { too_long(); return; }
                            continue;
                        }
                    }
                    dot_resolved.append(token);
                } else {
                    size_t len = utf8_len(static_cast<unsigned char>(resolved[i]));
                    dot_resolved.append(resolved, i, len);
                    i += len;
                }
                if (dot_resolved.size() > kMaxCalcResolvedLen) { too_long(); return; }
            }
            CalcResult r = safe_calc(dot_resolved);
            cur = r.ok ? number_value(r.value) : Value::make_string("CALC_ERR: " + r.error);
        }
    }

    // ── :alias ──
    if (mb & mk::ALIAS) {
        if (const std::string* target_p = cur.as_string()) {
            const std::string target = *target_p;
            // The key's full dot-path; a list item's `#<index>` is not part of it.
            std::string current_path;
            for (const auto& seg : meta_path_segments(path)) {
                if (!seg.empty() && seg[0] == '#') continue;
                current_path += seg;
                current_path.push_back('.');
            }
            current_path += key;
            if (target == key || target == current_path) {
                cur = Value::make_string("ALIAS_ERR: self-referential alias: " + current_path + " \xE2\x86\x92 " + target);
            } else {
                const Value* tv = deep_get(root, target, &root_lookup);
                Value target_val = tv ? budgeted_copy(*tv, depth) : Value::make_null();
                size_t dot = target.rfind('.');
                std::string target_parent = dot == std::string::npos ? std::string() : target.substr(0, dot);
                std::string target_key = dot == std::string::npos ? target : target.substr(dot + 1);
                bool target_has_alias = false;
                if (const MetaMap* mm = metadata.get(target_parent)) {
                    if (const Meta* m = mm->get(target_key)) target_has_alias = m->has_marker("alias");
                }
                const std::string* ts = tv ? tv->as_string() : nullptr;
                bool is_cycle = target_has_alias && ts && (*ts == key || *ts == current_path);
                if (is_cycle) {
                    // Order-independent: both participants produce the same text.
                    const std::string& a = current_path <= target ? current_path : target;
                    const std::string& b = current_path <= target ? target : current_path;
                    cur = Value::make_string("ALIAS_ERR: circular alias detected: " + a + " \xE2\x86\x92 " + b);
                } else {
                    cur = std::move(target_val);
                }
            }
        }
    }


    // ── :unique ──
    if (mb & mk::UNIQUE) {
        if (const Array* arr = cur.as_array()) {
            std::unordered_set<std::string> seen;
            Array unique;
            for (const auto& item : *arr) {
                if (seen.insert(value_to_string(item)).second) unique.push_back(item);
            }
            cur = Value::make_array(std::move(unique));
        }
    }

    // ── :geo — `- REGION value` items, by options.region ──
    if (mb & mk::GEO) {
        if (const Array* arr = cur.as_array()) {
            const std::string prefix = options.region.value_or("US") + " ";
            const std::string* found = nullptr;
            for (const auto& item : *arr) {
                const std::string* s = item.as_string();
                if (s && s->compare(0, prefix.size(), prefix) == 0) { found = s; break; }
            }
            Value result;
            if (found) {
                result = Value::make_string(std::string(trim_view(std::string_view(*found).substr(prefix.size()))));
            } else if (!arr->empty()) {
                const Value& first = arr->front();
                const std::string* s = first.as_string();
                size_t space = s ? s->find(' ') : std::string::npos;
                result = space != std::string::npos
                    ? Value::make_string(std::string(trim_view(std::string_view(*s).substr(space + 1))))
                    : first;
            }
            cur = std::move(result);
        }
    }

    // ── :split[:SEP] ──
    if (mb & mk::SPLIT) {
        if (const std::string* sp = cur.as_string()) {
            const std::string* kw = marker_after(markers, "split");
            const std::string sep = kw ? delimiter_from_keyword(*kw) : std::string(",");
            const std::string s = *sp;
            Array items;
            auto add = [&](std::string_view part) {
                std::string_view t = trim_view(part);
                if (!t.empty()) items.push_back(cast_primitive(t));
            };
            if (sep.empty()) {
                // Rust `str::split("")` yields every character.
                for (size_t i = 0; i < s.size();) {
                    size_t len = utf8_len(static_cast<unsigned char>(s[i]));
                    add(std::string_view(s).substr(i, len));
                    i += len;
                }
            } else {
                size_t start = 0;
                while (true) {
                    size_t hit = s.find(sep, start);
                    add(std::string_view(s).substr(start, (hit == std::string::npos ? s.size() : hit) - start));
                    if (hit == std::string::npos) break;
                    start = hit + sep.size();
                }
            }
            cur = Value::make_array(std::move(items));
        }
    }

    // ── :join[:SEP] ──
    if (mb & mk::JOIN) {
        if (const Array* arr = cur.as_array()) {
            const std::string* kw = marker_after(markers, "join");
            const std::string sep = kw ? delimiter_from_keyword(*kw) : std::string(",");
            // safety: the separator is as long as the document likes and is
            // written once per item.
            const size_t separators = sep.size() * (arr->empty() ? 0 : arr->size() - 1);
            std::string joined;
            if (separators > arr->size() && !charge_expansion(separators)) {
                joined = kLimitErr;
            } else {
                for (size_t i = 0; i < arr->size(); ++i) {
                    if (i > 0) joined.append(sep);
                    joined.append(value_to_string((*arr)[i]));
                }
            }
            cur = Value::make_string(std::move(joined));
        }
    }

    // ── :default (without :env) ──
    if ((mb & mk::DEFAULT) && !(mb & mk::ENV)) {
        bool empty = cur.is_null() || (cur.is_string() && cur.as_string()->empty());
        const int di = marker_pos(markers, "default");
        if (empty && markers.size() > static_cast<size_t>(di) + 1) {
            cur = external_value(join_from(markers, static_cast<size_t>(di) + 1), wants_string(meta), secret);
        }
    }

    // ── :clamp:MIN:MAX ──
    if (mb & mk::CLAMP) {
        const std::string* min_s = marker_after(markers, "clamp");
        const std::string* max_s = marker_after(markers, "clamp", 2);
        // A bound must be a finite number: `nan`, `inf`, `1e400`, `5abc` skip the marker.
        double lo = 0.0, hi = 0.0;
        if (parse_f64(min_s ? *min_s : "", lo) && parse_f64(max_s ? *max_s : "", hi) && std::isfinite(lo)
            && std::isfinite(hi)) {
            double n = 0.0;
            if (lo > hi) {
                cur = Value::make_string("CONSTRAINT_ERR: clamp min (" + display_f64(lo) + ") > max ("
                                         + display_f64(hi) + ")");
            } else if (value_as_number(cur, n)) {
                cur = number_value(std::isnan(n) ? n : std::min(std::max(n, lo), hi));
            }
        }
    }

    // ── :round[:N] ──
    if (mb & mk::ROUND) {
        uint64_t decimals = 0;
        const std::string* d = marker_after(markers, "round");
        if (!d || !parse_u64(*d, decimals) || decimals > UINT32_MAX) decimals = 0;
        double n = 0.0;
        if (value_as_number(cur, n)) {
            // A scale past the range of f64 rounds nothing: the number stays.
            double factor = powi(10.0, static_cast<int>(std::min<uint64_t>(decimals, 400)));
            double scaled = n * factor;
            double rounded = std::isfinite(scaled) ? std::round(scaled) / factor : n;
            // safety: only a whole number that fits is an integer. The cast
            // alone saturated: `:round` of 1e30 was 9223372036854775807 and of
            // NaN was 0 — invented values.
            if (decimals == 0 && std::isfinite(rounded) && is_whole(rounded)
                && std::fabs(rounded) < 9223372036854775807.0) {
                cur = Value::make_int(static_cast<int64_t>(rounded));
            } else {
                cur = Value::make_float(rounded);
            }
        }
    }

    // ── :map[:SOURCE_KEY] — `- lookup result` items ──
    if (mb & mk::MAP) {
        if (const Array* arr = cur.as_array()) {
            const std::string* source_key = marker_after(markers, "map");
            std::string lookup_val;
            if (source_key && !source_key->empty()) {
                const Value* v = deep_get(root, *source_key, &root_lookup);
                if (!v) v = local.get(*source_key);
                if (v) lookup_val = value_to_string(*v);
            }
            Value result;
            for (const auto& item : *arr) {
                const std::string* s = item.as_string();
                if (!s) continue;
                size_t space = s->find(' ');
                if (space == std::string::npos) continue;
                if (trim_view(std::string_view(*s).substr(0, space)) == lookup_val) {
                    result = cast_primitive(trim_view(std::string_view(*s).substr(space + 1)));
                    break;
                }
            }
            cur = std::move(result);
        }
    }

    // ── :format:PATTERN — printf-style %.2f, %d, %05d, %e ──
    if (mb & mk::FORMAT) {
        const std::string* pattern = marker_after(markers, "format");
        std::string formatted = apply_format_pattern(pattern ? *pattern : std::string("%s"), cur);
        // safety: a width of 4096 on a twenty-byte line is a 200-fold growth.
        if ((cur.is_int() || cur.is_float()) && !charge_expansion(formatted.size())) formatted = kLimitErr;
        cur = Value::make_string(std::move(formatted));
    }

    // ── :replace:FROM:TO ──
    if (mb & mk::REPLACE) {
        if (const std::string* s = cur.as_string()) {
            const std::string* from = marker_after(markers, "replace");
            const std::string* to = marker_after(markers, "replace", 2);
            if (from && !from->empty()) {
                const std::string with = to ? *to : std::string();
                // safety: `:replace:a:<long>` on a run of `a` multiplies the two
                // lengths; the growth is known before a byte is written.
                const size_t grown = with.size() > from->size() ? count_matches(*s, *from) * (with.size() - from->size()) : 0;
                cur = grown > 0 && !charge_expansion(grown) ? Value::make_string(kLimitErr)
                                                            : Value::make_string(replace_all(*s, *from, with));
            }
        }
    }

    // ── :sort[:desc] — numbers numerically, anything else by code point; stable ──
    if (mb & mk::SORT) {
        if (const Array* arr = cur.as_array()) {
            const std::string* dir = marker_after(markers, "sort");
            const bool desc = dir && *dir == "desc";
            std::vector<SortItem> items;
            items.reserve(arr->size());
            for (const auto& v : *arr) {
                SortItem it{&v, false, 0.0, std::string()};
                it.numeric = value_as_number(v, it.number);
                it.text = value_to_string(v);
                items.push_back(std::move(it));
            }
            std::vector<SortItem> tmp(items.size());
            merge_sort(items, tmp, 0, items.size());
            Array sorted;
            sorted.reserve(items.size());
            for (const auto& it : items) sorted.push_back(*it.value);
            if (desc) std::reverse(sorted.begin(), sorted.end());
            cur = Value::make_array(std::move(sorted));
        }
    }

    // ── :sum — Int when every summand is whole, Float otherwise ──
    if (mb & mk::SUM) {
        if (const Array* arr = cur.as_array()) {
            double total = 0.0;
            bool all_int = true;
            for (const auto& v : *arr) {
                if (const int64_t* n = v.as_int()) {
                    total += static_cast<double>(*n);
                } else if (const double* f = v.as_float()) {
                    total += *f;
                    if (!is_whole(*f)) all_int = false;
                } else if (const std::string* s = v.as_string()) {
                    // `inf` and `nan` are text here as everywhere (see cast_primitive).
                    double parsed = 0.0;
                    if (parse_f64(*s, parsed) && std::isfinite(parsed)) {
                        total += parsed;
                        if (!is_whole(parsed)) all_int = false;
                    }
                }
            }
            cur = (all_int && is_whole(total) && std::fabs(total) < 9223372036854775807.0)
                ? Value::make_int(static_cast<int64_t>(total)) : Value::make_float(total);
        }
    }

    // ── :fallback:DEFAULT — a missing file path takes the default ──
    if (mb & mk::FALLBACK) {
        const std::string* dv = marker_after(markers, "fallback");
        bool use_fallback = false;
        if (cur.is_null()) {
            use_fallback = true;
        } else if (const std::string* s = cur.as_string()) {
            if (s->empty()) {
                use_fallback = true;
            } else {
                JailResult j = jail_path(options.base_path.value_or("."), *s);
                std::error_code ec;
                use_fallback = !j.ok || !fs::exists(j.path, ec);
            }
        }
        if (use_fallback && dv && !dv->empty()) cur = Value::make_string(*dv);
    }

    // ── :once[:uuid|timestamp|random] — generated once, kept in `.synx.lock` ──
    if (mb & mk::ONCE) {
        const std::string* gt = marker_after(markers, "once");
        const std::string gen_type = gt ? *gt : std::string("uuid");
        const fs::path lock_path = options.base_path ? path_from_utf8(*options.base_path) / ".synx.lock"
                                                     : fs::path(".synx.lock");
        if (std::optional<std::string> locked = read_lock_value(lock_path, key)) {
            cur = Value::make_string(std::move(*locked));
        } else {
            std::string generated;
            if (gen_type == "timestamp") {
                generated = std::to_string(static_cast<long long>(std::chrono::duration_cast<std::chrono::seconds>(
                    std::chrono::system_clock::now().time_since_epoch()).count()));
            } else if (gen_type == "random") {
                generated = std::to_string(random_usize(UINT32_MAX));
            } else {
                generated = generate_uuid();
            }
            write_lock_value(lock_path, key, generated);
            cur = Value::make_string(std::move(generated));
        }
    }

    // ── :version:OP:REQUIRED ──
    if (mb & mk::VERSION) {
        if (const std::string* current = cur.as_string()) {
            const std::string* op = marker_after(markers, "version");
            const std::string* required = marker_after(markers, "version", 2);
            cur = Value::make_bool(compare_versions(*current, op ? *op : std::string(">="),
                                                    required ? *required : std::string()));
        }
    }

    // ── :watch[:KEY_PATH] — a file read at resolution time ──
    if (mb & mk::WATCH) {
        if (const std::string* file_p = cur.as_string()) {
            const std::string file = *file_p;
            size_t max_depth = options.max_include_depth.value_or(kDefaultMaxIncludeDepth);
            if (options._include_depth >= max_depth) {
                cur = Value::make_string("WATCH_ERR: max include depth (" + std::to_string(max_depth) + ") exceeded");
                return;
            }
            JailResult j = jail_path(options.base_path.value_or("."), file);
            if (!j.ok) {
                cur = Value::make_string("WATCH_ERR: " + j.error);
                return;
            }
            std::string size_err = check_file_size(j.path);
            if (!size_err.empty()) {
                cur = Value::make_string("WATCH_ERR: " + size_err);
                return;
            }
            const std::string* key_path = marker_after(markers, "watch");
            std::string content, err;
            if (read_budgeted(j.path, file, content, err)) {
                if (key_path) {
                    std::string ext = path_to_utf8(j.path.extension());
                    if (!ext.empty() && ext[0] == '.') ext.erase(0, 1);
                    std::optional<Value> v = extract_from_file_content(content, *key_path, ext);
                    cur = v ? placed_document(std::move(*v), depth, false) : Value::make_null();
                } else {
                    cur = Value::make_string(std::string(trim_view(content)));
                }
            } else {
                cur = Value::make_string("WATCH_ERR: " + err);
            }
        }
    }

    // ── :secret ──
    // Last of the markers that produce the value, so whatever they resolved is
    // what gets masked. It used to sit after `:alias`: `t:secret:once` came out
    // as plain text and `:secret:format` formatted the mask. `:prompt` and
    // custom markers below still see only the mask.
    if (secret) mask_secret_value(cur);

    // ── :prompt[:LABEL] — the subtree as SYNX in a labelled code fence ──
    if (mb & mk::PROMPT) {
        const std::string* label = marker_after(markers, "prompt");
        std::string text = stringify_value(cur, 0);
        std::string block = (label ? *label : key) + " (SYNX):\n```synx\n" + text + "```";
        cur = Value::make_string(charge_expansion(block.size()) ? std::move(block) : std::string(kLimitErr));
    }

    // ── custom markers: the first non-builtin one registered, with the
    //    markers after it as arguments ──
    if (!options.marker_fns.empty()) {
        for (size_t i = 0; i < markers.size(); ++i) {
            if (is_builtin_marker(markers[i])) continue;
            auto fn = options.marker_fns.find(markers[i]);
            if (fn == options.marker_fns.end() || !fn->second) continue;
            std::vector<std::string> args(markers.begin() + static_cast<std::ptrdiff_t>(i) + 1, markers.end());
            cur = fn->second(key, args, cur);
            break;
        }
    }

    // ── constraints, always last ──
    if (meta.constraints) {
        if (auto err = constraint_violation(key, cur, *meta.constraints)) {
            if (options.strict) std::fprintf(stderr, "synx: %s\n", err->c_str());
            cur = Value::make_string(std::move(*err));
        }
    }
}

} // namespace detail
} // namespace synx
