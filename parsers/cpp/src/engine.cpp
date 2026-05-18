// SYNX `!active` engine — control flow, helpers, marker dispatch.
// Mirrors crates/synx-core/src/engine.rs (control-flow half).
#include "synx/engine.hpp"
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
std::string value_to_string(const Value& v) {
    switch (v.kind()) {
        case Value::Kind::Null:   return "null";
        case Value::Kind::Bool:   return *v.as_bool() ? "true" : "false";
        case Value::Kind::Int: {
            char buf[32];
            std::snprintf(buf, sizeof(buf), "%lld", static_cast<long long>(*v.as_int()));
            return buf;
        }
        case Value::Kind::Float: {
            double f = *v.as_float();
            if (std::isnan(f) || std::isinf(f)) return "null";
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
        case Value::Kind::String:
            return *v.as_string();
        case Value::Kind::Secret:
            return *v.as_secret();
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

bool regex_match_strict(const std::string& value, const std::string& pattern) {
#if defined(__cpp_exceptions) && __cpp_exceptions
    try {
        std::regex re(pattern);
        return std::regex_search(value, re);
    } catch (...) {
        return true; // Invalid pattern — do not reject the value.
    }
#else
    // With exceptions disabled an invalid pattern would abort the process.
    // Callers compiled with SYNX_NO_EXCEPTIONS=ON must ensure patterns are
    // well-formed (validated at edit time / CI).
    std::regex re(pattern);
    return std::regex_search(value, re);
#endif
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

// ─── walking + dispatch ─────────────────────────────────────────────────────
void Resolver::resolve_value(Value& value, const std::string& path,
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
        else if (m == "calc")     apply_calc(value, meta, path);
        else if (m == "ref")      apply_ref(value, meta);
        else if (m == "alias")    apply_alias(value, meta);
        else if (m == "secret")   apply_secret(value, meta);
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

    // Apply runtime type cast for type hints (when no marker rewrote the value).
    if (meta.type_hint.has_value()) {
        const std::string& th = *meta.type_hint;
        if (th == "int" && !value.is_int()) {
            double d = 0.0;
            if (value_to_number(value, d)) value = Value::make_int(static_cast<int64_t>(d));
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
}

void Resolver::walk(Value& v, const std::string& path, int depth) {
    if (depth > static_cast<int>(kMaxResolveDepth)) return;
    if (Object* obj = v.as_object_mut()) {
        for (auto& p : *obj) {
            resolve_value(p.value, path, p.key, depth);
            if (p.value.is_object() || p.value.is_array()) {
                std::string sub = path.empty() ? p.key : path + "." + p.key;
                walk(p.value, sub, depth + 1);
            }
        }
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

void Resolver::validate_all() {
    for (auto& path_entry : result.metadata) {
        const std::string& path = path_entry.first;
        Value* container = (path.empty()) ? &result.root :
            const_cast<Value*>(deep_get(root, path));
        if (!container || !container->is_object()) continue;
        Object& obj = *container->as_object_mut();

        for (auto& field_entry : path_entry.second) {
            const std::string& fk = field_entry.first;
            const Meta& m = field_entry.second;
            if (!m.constraints.has_value()) continue;
            const Constraints& c = *m.constraints;
            Value* fv = nullptr;
            for (auto& p : obj) {
                if (p.key == fk) { fv = &p.value; break; }
            }
            if (!fv) {
                if (c.required && options.strict) {
                    std::fprintf(stderr, "synx: required key '%s.%s' missing\n",
                                 path.c_str(), fk.c_str());
                }
                continue;
            }
            // Type check
            if (c.type_name.has_value()) {
                const std::string& tn = *c.type_name;
                bool match =
                    (tn == "int" && fv->is_int())
                 || (tn == "float" && fv->is_float())
                 || (tn == "bool" && fv->is_bool())
                 || (tn == "string" && fv->is_string())
                 || (tn == "array" && fv->is_array())
                 || (tn == "object" && fv->is_object());
                if (!match && options.strict) {
                    std::fprintf(stderr, "synx: type mismatch for '%s.%s' (want %s, got %s)\n",
                                 path.c_str(), fk.c_str(), tn.c_str(), fv->type_name());
                }
            }
            // Min/max
            double dv = 0.0;
            if ((c.min.has_value() || c.max.has_value()) && value_to_number(*fv, dv)) {
                if (c.min.has_value() && dv < *c.min) {
                    if (options.strict) std::fprintf(stderr,
                        "synx: '%s.%s' below min (%.6g < %.6g)\n",
                        path.c_str(), fk.c_str(), dv, *c.min);
                }
                if (c.max.has_value() && dv > *c.max) {
                    if (options.strict) std::fprintf(stderr,
                        "synx: '%s.%s' above max (%.6g > %.6g)\n",
                        path.c_str(), fk.c_str(), dv, *c.max);
                }
            }
            // Enum
            if (c.enum_values.has_value() && fv->is_string()) {
                const std::string& s = *fv->as_string();
                bool found = false;
                for (const auto& v : *c.enum_values) {
                    if (v == s) { found = true; break; }
                }
                if (!found && options.strict) std::fprintf(stderr,
                    "synx: '%s.%s' value '%s' not in enum\n",
                    path.c_str(), fk.c_str(), s.c_str());
            }
            // Pattern
            if (c.pattern.has_value() && fv->is_string()) {
                if (!regex_match_strict(*fv->as_string(), *c.pattern) && options.strict) {
                    std::fprintf(stderr, "synx: '%s.%s' fails pattern '%s'\n",
                                 path.c_str(), fk.c_str(), c.pattern->c_str());
                }
            }
        }
    }
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
            // Refresh namespace pointer to the now-installed location.
            for (auto& p : root) {
                if (p.key == use.alias) {
                    namespaces[use.alias] = p.value.as_object();
                    break;
                }
            }
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
        for (auto& p : root) {
            if (p.key == inc.alias) {
                namespaces[inc.alias] = p.value.as_object();
                break;
            }
        }
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
    // 5. Resolve markers across the whole tree.
    r.walk(result.root, std::string{}, 0);
    // 6. Validate constraints.
    r.validate_all();
    // 7. Persist `:once` writes.
    r.flush_once();
}

} // namespace synx
