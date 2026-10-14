// SYNX `!active` markers — per-marker implementations.
// Mirrors crates/synx-core/src/engine.rs (marker half).
#include "synx/calc.hpp"
#include "synx/parser.hpp"
#include "engine_internal.hpp"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <optional>
#include <regex>
#include <sstream>
#include <string>
#include <string_view>
#include <unordered_map>
#include <unordered_set>
#include <vector>

namespace synx {
namespace detail {

namespace {

inline std::string_view trim_sv(std::string_view s) noexcept {
    size_t a = 0;
    while (a < s.size() && (s[a] == ' ' || s[a] == '\t' || s[a] == '\r')) ++a;
    size_t b = s.size();
    while (b > a && (s[b - 1] == ' ' || s[b - 1] == '\t' || s[b - 1] == '\r')) --b;
    return s.substr(a, b - a);
}

// Process environment lookup (used by `:env` when `Options::env` is not given).
std::optional<std::string> read_process_env(const std::string& name) {
#if defined(_MSC_VER)
    char* buf = nullptr;
    size_t len = 0;
    if (_dupenv_s(&buf, &len, name.c_str()) != 0 || buf == nullptr) return std::nullopt;
    std::string value(buf);
    std::free(buf);
    return value;
#else
    const char* raw = std::getenv(name.c_str());
    if (raw == nullptr) return std::nullopt;
    return std::string(raw);
#endif
}

// In-place word-boundary replace (used by :calc identifier substitution).
void replace_word(std::string& s, std::string_view word, std::string_view repl) {
    if (word.empty()) return;
    size_t i = 0;
    while (true) {
        size_t pos = s.find(std::string(word), i);
        if (pos == std::string::npos) return;
        bool left = (pos == 0)
            || !(std::isalnum(static_cast<unsigned char>(s[pos - 1])) || s[pos - 1] == '_');
        bool right = (pos + word.size() == s.size())
            || !(std::isalnum(static_cast<unsigned char>(s[pos + word.size()])) || s[pos + word.size()] == '_');
        if (left && right) {
            s.replace(pos, word.size(), std::string(repl));
            i = pos + repl.size();
        } else {
            i = pos + 1;
        }
    }
}

// printf-style `%d`, `%05d`, `%.2f`, `%e`, `%s`, `%%` formatting against numeric input.
std::string format_pattern(std::string_view pattern, double n, const std::string& s_input) {
    std::string out;
    out.reserve(pattern.size() + 16);
    for (size_t i = 0; i < pattern.size(); ) {
        char c = pattern[i];
        if (c != '%') { out.push_back(c); ++i; continue; }
        if (i + 1 < pattern.size() && pattern[i + 1] == '%') {
            out.push_back('%'); i += 2; continue;
        }
        // Read directive
        size_t end = i + 1;
        while (end < pattern.size()) {
            char k = pattern[end];
            if (k == 'd' || k == 'i' || k == 'f' || k == 'e' || k == 'g' || k == 's') break;
            ++end;
        }
        if (end >= pattern.size()) { out.append(pattern.substr(i)); break; }
        std::string spec(pattern.substr(i, end - i + 1));
        char kind = pattern[end];
        char buf[64];
        if (kind == 'd' || kind == 'i') {
            std::snprintf(buf, sizeof(buf), spec.c_str(), static_cast<long long>(n));
        } else if (kind == 'f' || kind == 'e' || kind == 'g') {
            std::snprintf(buf, sizeof(buf), spec.c_str(), n);
        } else if (kind == 's') {
            std::snprintf(buf, sizeof(buf), spec.c_str(), s_input.c_str());
        } else {
            buf[0] = '\0';
        }
        out.append(buf);
        i = end + 1;
    }
    return out;
}

} // namespace

// ─── markers ────────────────────────────────────────────────────────────────

void Resolver::apply_env(Value& v, const Meta& m) {
    // The value on the line is the env-var name; anything else is left alone.
    const std::string* name_ptr = v.as_string();
    if (!name_ptr) return;
    const std::string var = *name_ptr;

    std::optional<std::string> env_val;
    if (options.env.has_value()) {
        auto it = options.env->find(var);
        if (it != options.env->end()) env_val = it->second;
    } else {
        env_val = read_process_env(var);
    }

    // `(string)` / `[type:string]` keep the env text verbatim (`007`, 64-bit ids).
    const bool force_string = wants_string(m);
    if (env_val.has_value() && !env_val->empty()) {
        v = force_string ? Value::make_string(*env_val) : cast_primitive(*env_val);
        return;
    }
    // Fallback: every marker after `default`, rejoined with ':' (IPs, compound values).
    int di = m.marker_index("default");
    if (di >= 0 && static_cast<size_t>(di + 1) < m.markers.size()) {
        std::string fallback;
        for (size_t k = static_cast<size_t>(di) + 1; k < m.markers.size(); ++k) {
            if (k > static_cast<size_t>(di) + 1) fallback.push_back(':');
            fallback.append(m.markers[k]);
        }
        v = force_string ? Value::make_string(std::move(fallback)) : cast_primitive(fallback);
        return;
    }
    v = Value::make_null();
}

void Resolver::apply_default(Value& v, const Meta& m) {
    // If preceded by :env, :env handled the default fallback already.
    if (m.has_marker("env")) return;
    if (v.is_null() || (v.is_string() && v.as_string()->empty())) {
        // Default value is every marker after `default`, rejoined with ':'.
        int idx = m.marker_index("default");
        if (idx >= 0 && static_cast<size_t>(idx + 1) < m.markers.size()) {
            std::string fallback;
            for (size_t k = static_cast<size_t>(idx) + 1; k < m.markers.size(); ++k) {
                if (k > static_cast<size_t>(idx) + 1) fallback.push_back(':');
                fallback.append(m.markers[k]);
            }
            v = wants_string(m) ? Value::make_string(std::move(fallback)) : cast_primitive(fallback);
        }
    }
}

void Resolver::apply_calc(Value& v, const Meta& /*m*/, const std::string& key, const Object& local) {
    // Port of the Rust `:calc` handler: substitute numeric root keys, then numeric
    // siblings, then dotted paths, then evaluate. A failure writes the exact
    // `CALC_ERR: …` string into the value.
    constexpr size_t kMaxExprLen = 4096;
    constexpr size_t kMaxResolvedLen = 64 * 1024;

    const std::string* src = v.as_string();
    if (!src) return;
    const std::string expr = *src;
    if (expr.size() > kMaxExprLen) {
        v = Value::make_string("CALC_ERR: expression too long (" + std::to_string(expr.size())
                               + " chars, max " + std::to_string(kMaxExprLen) + ")");
        return;
    }
    auto too_long = [&v]() {
        v = Value::make_string("CALC_ERR: resolved expression too long (max "
                               + std::to_string(kMaxResolvedLen) + " bytes)");
    };
    // Only real numbers are substituted (Rust `value_as_number`): not bools or numeric text.
    auto number = [](const Value& x, double& d) {
        if (x.is_int()) { d = static_cast<double>(*x.as_int()); return true; }
        if (x.is_float()) { d = *x.as_float(); return true; }
        return false;
    };

    std::string resolved = expr;
    for (const auto& p : root) {
        double d = 0.0;
        if (!number(p.value, d)) continue;
        replace_word(resolved, p.key, format_number(d));
        if (resolved.size() > kMaxResolvedLen) { too_long(); return; }
    }
    for (const auto& p : local) {
        double d = 0.0;
        if (p.key == key || !number(p.value, d)) continue;
        replace_word(resolved, p.key, format_number(d));
        if (resolved.size() > kMaxResolvedLen) { too_long(); return; }
    }

    // Dotted references (`base.hp`). Every other byte is widened to U+00xx as
    // Rust's `byte as char` does, which is what a later error message shows.
    auto is_word = [](unsigned char b) { return std::isalnum(b) != 0 && b < 0x80 ? true : b == '_'; };
    std::string dot_resolved;
    size_t i = 0;
    while (i < resolved.size()) {
        if (is_word(static_cast<unsigned char>(resolved[i]))) {
            size_t start = i;
            bool has_dot = false;
            while (i < resolved.size()
                   && (is_word(static_cast<unsigned char>(resolved[i])) || resolved[i] == '.')) {
                if (resolved[i] == '.') has_dot = true;
                ++i;
            }
            std::string token = resolved.substr(start, i - start);
            if (has_dot) {
                const Value* val = nullptr;
                for (const auto& p : root) {
                    if (p.key == token) { val = &p.value; break; }
                }
                if (!val) val = deep_get(root, token);
                double d = 0.0;
                if (val && number(*val, d)) {
                    dot_resolved.append(format_number(d));
                    if (dot_resolved.size() > kMaxResolvedLen) { too_long(); return; }
                    continue;
                }
            }
            dot_resolved.append(token);
        } else {
            unsigned char b = static_cast<unsigned char>(resolved[i]);
            if (b < 0x80) {
                dot_resolved.push_back(static_cast<char>(b));
            } else {
                dot_resolved.push_back(static_cast<char>(0xC0 | (b >> 6)));
                dot_resolved.push_back(static_cast<char>(0x80 | (b & 0x3F)));
            }
            ++i;
        }
        if (dot_resolved.size() > kMaxResolvedLen) { too_long(); return; }
    }

    CalcResult r = safe_calc(dot_resolved);
    if (!r.ok) {
        v = Value::make_string("CALC_ERR: " + r.error);
        return;
    }
    if (std::trunc(r.value) == r.value && std::fabs(r.value) < 9.223372036854775807e18) {
        v = Value::make_int(static_cast<int64_t>(r.value));
    } else {
        v = Value::make_float(r.value);
    }
}

void Resolver::apply_ref(Value& v, const Meta& /*m*/) {
    std::string path = value_to_string(v);
    if (path.empty()) return;
    const Value* target = deep_get_with_namespaces(path);
    if (target) v = *target;
}

void Resolver::apply_alias(Value& v, const Meta& /*m*/) {
    // :alias is structural — its value should already be a reference.
    // We resolve as a :ref to keep the surface consistent.
    apply_ref(v, Meta{});
}

void Resolver::apply_secret(Value& v, const Meta& /*m*/) {
    // Mark string as Secret so JSON output is redacted.
    if (v.is_string()) {
        std::string s = *v.as_string();
        v = Value::make_secret(std::move(s));
    } else if (!v.is_secret() && !v.is_null()) {
        v = Value::make_secret(value_to_string(v));
    }
}

void Resolver::apply_random(Value& v, const Meta& m) {
    // Weights live in m.args (numeric strings). If the value was a list,
    // pick one item by weight; otherwise the value is the comma-separated list.
    std::vector<std::string> options_list;
    if (const Array* arr = v.as_array()) {
        for (const auto& item : *arr) options_list.push_back(value_to_string(item));
    } else if (v.is_string()) {
        std::string s = *v.as_string();
        size_t start = 0;
        while (start < s.size()) {
            size_t comma = s.find(',', start);
            options_list.emplace_back(trim_sv(std::string_view(s).substr(
                start, (comma == std::string::npos ? s.size() : comma) - start)));
            if (comma == std::string::npos) break;
            start = comma + 1;
        }
    }
    if (options_list.empty()) return;

    std::vector<double> weights;
    for (const auto& w : m.args) {
        weights.push_back(std::strtod(w.c_str(), nullptr));
    }
    while (weights.size() < options_list.size()) weights.push_back(1.0);
    double total = 0.0;
    for (double w : weights) total += w;
    if (total <= 0.0) return;
    std::uniform_real_distribution<double> dist(0.0, total);
    double pick = dist(rng);
    double acc = 0.0;
    for (size_t i = 0; i < options_list.size(); ++i) {
        acc += weights[i];
        if (pick <= acc) {
            v = Value::make_string(options_list[i]);
            return;
        }
    }
    v = Value::make_string(options_list.back());
}

void Resolver::apply_unique(Value& v, const Meta& /*m*/) {
    // De-duplicate an array, preserving first occurrence.
    Array* arr = v.as_array_mut();
    if (!arr) return;
    Array out;
    out.reserve(arr->size());
    for (auto& item : *arr) {
        bool dup = false;
        for (const auto& seen : out) {
            if (seen.equals(item)) { dup = true; break; }
        }
        if (!dup) out.push_back(item);
    }
    v = Value::make_array(std::move(out));
}

void Resolver::apply_geo(Value& v, const Meta& m) {
    // :geo picks a value matching the active region. Args: region=value pairs.
    std::string current = options.region.value_or("");
    if (current.empty()) return;
    // value may be the default; args carry "region:value".
    for (const auto& a : m.args) {
        size_t eq = a.find(':');
        if (eq == std::string::npos) continue;
        std::string_view r = std::string_view(a).substr(0, eq);
        std::string_view val = std::string_view(a).substr(eq + 1);
        if (r == current) {
            v = Value::make_string(std::string(val));
            return;
        }
    }
    // No match — leave the default.
}

void Resolver::apply_i18n(Value& v, const Meta& m) {
    std::string lang = options.lang.value_or("en");
    double n = 0.0;
    bool numeric = value_to_number(v, n);

    // Translations are encoded in args as `lang.category:text` (or `lang:text`).
    std::string match_lang = lang;
    std::string lang2 = lang.substr(0, 2);
    const char* category = numeric ? plural_category(lang, n) : "other";

    // Order of preference: exact "lang.category", "lang2.category", "lang", "lang2", "other".
    std::vector<std::string> keys = {
        match_lang + "." + category,
        lang2 + "." + category,
        match_lang,
        lang2,
        std::string("other"),
    };
    for (const auto& key : keys) {
        for (const auto& a : m.args) {
            size_t colon = a.find(':');
            if (colon == std::string::npos) continue;
            if (std::string_view(a).substr(0, colon) == key) {
                v = Value::make_string(std::string(std::string_view(a).substr(colon + 1)));
                return;
            }
        }
    }
}

void Resolver::apply_split(Value& v, const Meta& m) {
    if (!v.is_string()) return;
    std::string sep = ",";
    int idx = m.marker_index("split");
    if (idx >= 0 && static_cast<size_t>(idx + 1) < m.markers.size()) {
        sep = m.markers[idx + 1];
    }
    const std::string& s = *v.as_string();
    Array out;
    if (sep.empty()) {
        for (char c : s) out.push_back(Value::make_string(std::string(1, c)));
    } else {
        size_t start = 0;
        while (start <= s.size()) {
            size_t pos = s.find(sep, start);
            if (pos == std::string::npos) {
                out.push_back(Value::make_string(std::string(trim_sv(
                    std::string_view(s).substr(start)))));
                break;
            }
            out.push_back(Value::make_string(std::string(trim_sv(
                std::string_view(s).substr(start, pos - start)))));
            start = pos + sep.size();
        }
    }
    v = Value::make_array(std::move(out));
}

void Resolver::apply_join(Value& v, const Meta& m) {
    if (!v.is_array()) return;
    std::string sep = ",";
    int idx = m.marker_index("join");
    if (idx >= 0 && static_cast<size_t>(idx + 1) < m.markers.size()) {
        sep = m.markers[idx + 1];
    }
    std::string out;
    const Array& a = *v.as_array();
    for (size_t i = 0; i < a.size(); ++i) {
        if (i > 0) out.append(sep);
        out.append(value_to_string(a[i]));
    }
    v = Value::make_string(std::move(out));
}

void Resolver::apply_clamp(Value& v, const Meta& m) {
    double d = 0.0;
    if (!value_to_number(v, d)) return;
    double lo = -1e300, hi = 1e300;
    int idx = m.marker_index("clamp");
    if (idx >= 0 && static_cast<size_t>(idx + 2) < m.markers.size()) {
        lo = std::strtod(m.markers[idx + 1].c_str(), nullptr);
        hi = std::strtod(m.markers[idx + 2].c_str(), nullptr);
    }
    if (d < lo) d = lo;
    if (d > hi) d = hi;
    if (v.is_int()) v = Value::make_int(static_cast<int64_t>(d));
    else v = Value::make_float(d);
}

void Resolver::apply_round(Value& v, const Meta& m) {
    double d = 0.0;
    if (!value_to_number(v, d)) return;
    int digits = 0;
    int idx = m.marker_index("round");
    if (idx >= 0 && static_cast<size_t>(idx + 1) < m.markers.size()) {
        digits = static_cast<int>(std::strtol(m.markers[idx + 1].c_str(), nullptr, 10));
    }
    double factor = std::pow(10.0, digits);
    double r = std::round(d * factor) / factor;
    if (digits == 0) v = Value::make_int(static_cast<int64_t>(r));
    else v = Value::make_float(r);
}

void Resolver::apply_map(Value& v, const Meta& m) {
    // m.args carry `key:value` pairs. Lookup current value, substitute.
    std::string key = value_to_string(v);
    for (const auto& a : m.args) {
        size_t colon = a.find(':');
        if (colon == std::string::npos) continue;
        if (std::string_view(a).substr(0, colon) == key) {
            v = Value::make_string(std::string(std::string_view(a).substr(colon + 1)));
            return;
        }
    }
}

void Resolver::apply_format(Value& v, const Meta& m) {
    int idx = m.marker_index("format");
    if (idx < 0 || static_cast<size_t>(idx + 1) >= m.markers.size()) return;
    const std::string& pattern = m.markers[idx + 1];
    double n = 0.0;
    value_to_number(v, n);
    std::string s_in = value_to_string(v);
    std::string interp = interpolate(pattern);
    v = Value::make_string(format_pattern(interp, n, s_in));
}

void Resolver::apply_replace(Value& v, const Meta& m) {
    if (!v.is_string()) return;
    int idx = m.marker_index("replace");
    if (idx < 0 || static_cast<size_t>(idx + 2) >= m.markers.size()) return;
    const std::string& from = m.markers[idx + 1];
    const std::string& to = m.markers[idx + 2];
    std::string s = *v.as_string();
    if (from.empty()) { v = Value::make_string(std::move(s)); return; }
    size_t pos = 0;
    while ((pos = s.find(from, pos)) != std::string::npos) {
        s.replace(pos, from.size(), to);
        pos += to.size();
    }
    v = Value::make_string(std::move(s));
}

void Resolver::apply_sort(Value& v, const Meta& m) {
    Array* arr = v.as_array_mut();
    if (!arr) return;
    bool descending = false;
    int idx = m.marker_index("sort");
    if (idx >= 0 && static_cast<size_t>(idx + 1) < m.markers.size()) {
        descending = (m.markers[idx + 1] == "desc");
    }
    std::sort(arr->begin(), arr->end(), [&](const Value& a, const Value& b) {
        double da = 0.0, db = 0.0;
        bool na = value_to_number(a, da), nb = value_to_number(b, db);
        if (na && nb) return descending ? da > db : da < db;
        std::string sa = value_to_string(a), sb = value_to_string(b);
        return descending ? sa > sb : sa < sb;
    });
}

void Resolver::apply_sum(Value& v, const Meta& /*m*/) {
    const Array* arr = v.as_array();
    if (!arr) return;
    double total = 0.0;
    bool any_float = false;
    for (const auto& item : *arr) {
        double d = 0.0;
        if (value_to_number(item, d)) {
            total += d;
            if (item.is_float()) any_float = true;
        }
    }
    if (any_float) v = Value::make_float(total);
    else v = Value::make_int(static_cast<int64_t>(total));
}

void Resolver::apply_fallback(Value& v, const Meta& m) {
    if (!v.is_null() && !(v.is_string() && v.as_string()->empty())) return;
    int idx = m.marker_index("fallback");
    if (idx >= 0 && static_cast<size_t>(idx + 1) < m.markers.size()) {
        v = Value::make_string(m.markers[idx + 1]);
    }
}

void Resolver::apply_once(Value& v, const Meta& /*m*/,
                          const std::string& path, const std::string& key) {
    // Lazy-load .synx.lock on first use.
    if (!once_loaded) {
        once_loaded = true;
        std::string base = options.base_path.value_or(".");
        std::ifstream in(base + "/.synx.lock", std::ios::binary);
        if (in.good()) {
            std::string line;
            while (std::getline(in, line)) {
                if (!line.empty() && line.back() == '\r') line.pop_back();
                if (!line.empty()) once_keys.insert(line);
            }
        }
    }
    std::string lock_key = path.empty() ? key : path + "." + key;
    if (once_keys.count(lock_key)) {
        // Already set in a previous run — clear value to mark "skip".
        v = Value::make_null();
    } else {
        once_new_keys.insert(lock_key);
    }
}

void Resolver::apply_version(Value& v, const Meta& /*m*/) {
    // Accept the value as-is; only normalize string formatting.
    if (!v.is_string()) v = Value::make_string(value_to_string(v));
}

void Resolver::apply_watch(Value& v, const Meta& /*m*/) {
    // `:watch` loads a file and embeds its content as the value (utf-8 string).
    if (!v.is_string()) return;
    std::string base = options.base_path.value_or(".");
    std::string path = jail_path(base, *v.as_string());
    if (path.empty()) return;
    FileText ft = read_text_file(path);
    if (ft.ok) v = Value::make_string(std::move(ft.text));
}

void Resolver::apply_prompt(Value& v, const Meta& /*m*/) {
    // `:prompt` is an LLM-oriented passthrough — interpolate template only.
    if (!v.is_string()) return;
    v = Value::make_string(interpolate(*v.as_string()));
}

void Resolver::apply_vision(Value& v, const Meta& /*m*/) {
    // `:vision` — file URL → base64? In core we keep a path-string envelope
    // so consumers attach actual bytes. Accept any string, no transform.
    (void)v;
}

void Resolver::apply_audio(Value& v, const Meta& /*m*/) {
    (void)v;
}

void Resolver::apply_spam(Value& v, const Meta& /*m*/,
                          const std::string& /*path*/, const std::string& key) {
    // Simple rate-limit bucket: at most one resolution per key per process run.
    static std::unordered_set<std::string>* buckets = new std::unordered_set<std::string>();
    if (buckets->count(key)) {
        v = Value::make_null();
        return;
    }
    buckets->insert(key);
}

void Resolver::apply_custom(Value& v, const Meta& m, const std::string& key, const MarkerFn& fn) {
    if (!fn) return;
    v = fn(key, m.args, v);
}

} // namespace detail
} // namespace synx
