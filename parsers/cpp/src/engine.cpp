// SYNX `!active` engine — resolution order, pre-passes, registries, validation.
// Port of crates/synx-core/src/engine.rs (control-flow half).
#include "synx/engine.hpp"

#include "engine_internal.hpp"
#include "internal.hpp"
#include "numfmt.hpp"
#include "paths.hpp"
#include "pattern.hpp"
#include "synx/parser.hpp"
#include "unicode.hpp"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <fstream>
#include <memory>
#include <sstream>
#include <string>
#include <string_view>
#include <system_error>
#include <unordered_map>
#include <utility>
#include <vector>

namespace synx {
namespace detail {

// ─── value helpers ──────────────────────────────────────────────────────────

const Value* ObjectLookup::get(std::string_view key) const {
    const Object& obj = *obj_;
    if (obj.size() < 16) {
        for (const auto& p : obj) {
            if (p.key == key) return &p.value;
        }
        return nullptr;
    }
    auto key_of = [](const Pair& p) { return std::string_view(p.key); };
    index_.sync(obj, key_of);
    size_t at = index_.find(obj, key, key_of);
    return at == KeyIndex::npos ? nullptr : &obj[at].value;
}

std::string value_to_string(const Value& v) {
    switch (v.kind()) {
        case Value::Kind::String: return *v.as_string();
        case Value::Kind::Int: return i64_to_string(*v.as_int());
        case Value::Kind::Float: return format_number(*v.as_float());
        case Value::Kind::Bool: return *v.as_bool() ? "true" : "false";
        case Value::Kind::Null: return "null";
        // safety: a `:secret` value MUST NOT reach a string sink as plaintext.
        case Value::Kind::Secret: return "[SECRET]";
        case Value::Kind::Array:
        case Value::Kind::Object: return std::string();
    }
    return std::string();
}

bool value_as_number(const Value& v, double& out) noexcept {
    if (const int64_t* n = v.as_int()) { out = static_cast<double>(*n); return true; }
    if (const double* f = v.as_float()) { out = *f; return true; }
    return false;
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
    // spec: `inf`, `NaN` and `1e400` are text. Read as floats they became
    // `null` in JSON: a host named `inf` lost its value.
    double f = 0.0;
    if (parse_f64(val, f) && std::isfinite(f)) return Value::make_float(f);
    return Value::make_string(std::string(val));
}

Value external_value(std::string text, bool as_string, bool secret) {
    if (as_string) return Value::make_string(std::move(text));
    Value cast = cast_primitive(text);
    // spec: a secret is its text. Cast first, `00123` became the secret `123`.
    if (secret && (cast.is_int() || cast.is_float() || cast.is_bool())) return Value::make_string(std::move(text));
    return cast;
}

bool wants_string(const Meta& m) {
    return (m.type_hint && *m.type_hint == "string")
        || (m.constraints && m.constraints->type_name && *m.constraints->type_name == "string");
}

const Value* deep_get(const Value& root, std::string_view path, const ObjectLookup* root_lookup) {
    const Object* map = root.as_object();
    if (map) {
        const Value* direct = nullptr;
        if (root_lookup && &root_lookup->object() == map) {
            direct = root_lookup->get(path);
        } else {
            for (const auto& p : *map) {
                if (p.key == path) { direct = &p.value; break; }
            }
        }
        if (direct) return direct;
    }
    const Value* current = &root;
    size_t start = 0;
    while (true) {
        size_t dot = path.find('.', start);
        std::string_view part = path.substr(start, (dot == std::string_view::npos ? path.size() : dot) - start);
        const Object* obj = current->as_object();
        if (!obj) return nullptr;
        const Value* next = nullptr;
        if (root_lookup && &root_lookup->object() == obj) {
            next = root_lookup->get(part);
        } else {
            for (const auto& p : *obj) {
                if (p.key == part) { next = &p.value; break; }
            }
        }
        if (!next) return nullptr;
        current = next;
        if (dot == std::string_view::npos) return current;
        start = dot + 1;
    }
}

uint32_t marker_bits(const std::vector<std::string>& markers) noexcept {
    static const std::pair<std::string_view, uint32_t> kNames[] = {
        {"alias", mk::ALIAS}, {"calc", mk::CALC}, {"clamp", mk::CLAMP}, {"default", mk::DEFAULT},
        {"env", mk::ENV}, {"fallback", mk::FALLBACK}, {"format", mk::FORMAT}, {"geo", mk::GEO},
        {"i18n", mk::I18N}, {"import", mk::IMPORT}, {"include", mk::INCLUDE}, {"join", mk::JOIN},
        {"map", mk::MAP}, {"once", mk::ONCE}, {"prompt", mk::PROMPT}, {"random", mk::RANDOM},
        {"ref", mk::REF}, {"replace", mk::REPLACE}, {"round", mk::ROUND}, {"secret", mk::SECRET},
        {"sort", mk::SORT}, {"spam", mk::SPAM}, {"split", mk::SPLIT}, {"sum", mk::SUM},
        {"unique", mk::UNIQUE}, {"version", mk::VERSION}, {"watch", mk::WATCH},
    };
    uint32_t bits = 0;
    for (const auto& m : markers) {
        for (const auto& n : kNames) {
            if (m == n.first) { bits |= n.second; break; }
        }
    }
    return bits;
}

bool is_calc_special_key(std::string_view k) noexcept {
    bool all_word = true;
    bool all_digit = true;
    for (char c : k) {
        unsigned char b = static_cast<unsigned char>(c);
        bool word = (b >= '0' && b <= '9') || (b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z') || b == '_';
        all_word = all_word && word;
        all_digit = all_digit && b >= '0' && b <= '9';
    }
    return !all_word || all_digit || k == "NaN" || k == "inf";
}

// ─── files ──────────────────────────────────────────────────────────────────

namespace fs = std::filesystem;

fs::path path_from_utf8(std::string_view s) {
#if defined(__cpp_char8_t)
    std::u8string u(s.begin(), s.end());
    return fs::path(u);
#else
    return fs::u8path(s.begin(), s.end());
#endif
}

std::string path_to_utf8(const fs::path& p) {
#if defined(__cpp_char8_t)
    std::u8string u = p.u8string();
    return std::string(u.begin(), u.end());
#else
    return p.u8string();
#endif
}

namespace {

// Rust `Path::starts_with`: whole components.
bool path_starts_with(const fs::path& full, const fs::path& base) {
    auto f = full.begin();
    for (auto b = base.begin(); b != base.end(); ++b, ++f) {
        if (b->empty()) continue;  // trailing separator
        if (f == full.end() || *f != *b) return false;
    }
    return true;
}

// The components of a path as the document wrote it, split at `/` and `\`.
template <typename F>
void each_segment(std::string_view p, F&& f) {
    size_t start = 0;
    while (true) {
        size_t sep = p.find_first_of("/\\", start);
        f(p.substr(start, (sep == std::string_view::npos ? p.size() : sep) - start));
        if (sep == std::string_view::npos) return;
        start = sep + 1;
    }
}

#if defined(_WIN32)
// A path component that names a DOS device, with or without an extension or a
// stream: `NUL`, `con.txt`, `sub/COM1`, `aux:x`.
bool has_device_name(std::string_view p) {
    bool found = false;
    each_segment(p, [&](std::string_view part) {
        size_t cut = part.find_first_of(".:");
        std::string stem(part.substr(0, cut));
        while (!stem.empty() && stem.back() == ' ') stem.pop_back();
        for (char& ch : stem) {
            if (ch >= 'a' && ch <= 'z') ch = static_cast<char>(ch - 'a' + 'A');
        }
        if (stem == "CON" || stem == "PRN" || stem == "AUX" || stem == "NUL" || stem == "CONIN$" || stem == "CONOUT$") {
            found = true;
        }
        if (stem.size() >= 4 && (stem.compare(0, 3, "COM") == 0 || stem.compare(0, 3, "LPT") == 0)) {
            std::string_view rest = std::string_view(stem).substr(3);
            if ((rest.size() == 1 && rest[0] >= '0' && rest[0] <= '9') || rest == "\xC2\xB9" || rest == "\xC2\xB2"
                || rest == "\xC2\xB3") {
                found = true;
            }
        }
    });
    return found;
}
#endif

/// What a top-level resolve shares with the resolves it starts for included
/// documents and packages (synx-core keeps the same state per thread).
struct ResolveState {
    size_t depth = 0;
    std::vector<fs::path> chain;
    size_t reads = 0;
    uint64_t read_bytes = 0;
    size_t expansion = 0;
};

ResolveState& state() {
    thread_local ResolveState s;
    return s;
}

/// Held for one `resolve` call; the outermost one starts the budgets afresh.
struct ResolveScope {
    ResolveScope() {
        ResolveState& s = state();
        if (s.depth == 0) {
            s.chain.clear();
            s.reads = 0;
            s.read_bytes = 0;
            s.expansion = 0;
        }
        ++s.depth;
    }
    ~ResolveScope() { --state().depth; }
    ResolveScope(const ResolveScope&) = delete;
    ResolveScope& operator=(const ResolveScope&) = delete;
};

} // namespace

bool charge_expansion(size_t bytes) noexcept {
    ResolveState& s = state();
    if (bytes <= kMaxResolveExpansionBytes - std::min(s.expansion, kMaxResolveExpansionBytes)) {
        s.expansion += bytes;
        return true;
    }
    s.expansion = kMaxResolveExpansionBytes;
    return false;
}

size_t expansion_room() noexcept {
    return kMaxResolveExpansionBytes - std::min(state().expansion, kMaxResolveExpansionBytes);
}

std::pair<size_t, size_t> value_cost(const Value& v) {
    if (const std::string* s = v.as_string()) return {kValueCost + s->size(), 0};
    if (v.is_secret()) return {kValueCost + v.as_secret()->size(), 0};
    if (const Array* a = v.as_array()) {
        size_t cost = kValueCost;
        size_t depth = 0;
        for (const auto& item : *a) {
            auto [c, d] = value_cost(item);
            cost += c;
            depth = std::max(depth, d);
        }
        return {cost, depth + 1};
    }
    if (const Object* o = v.as_object()) {
        size_t cost = kValueCost;
        size_t depth = 0;
        for (const auto& p : *o) {
            auto [c, d] = value_cost(p.value);
            cost += c + p.key.size();
            depth = std::max(depth, d);
        }
        return {cost, depth + 1};
    }
    return {kValueCost, 0};
}

Value budgeted_copy(const Value& v, size_t depth) {
    auto [cost, nesting] = value_cost(v);
    if (depth + nesting >= kMaxResolveDepth) return Value::make_string(kNestingErr);
    if (!charge_expansion(cost)) return Value::make_string(kLimitErr);
    return Value(v);
}

Value placed_document(Value v, size_t depth, bool secret) {
    if (depth + value_cost(v).second >= kMaxResolveDepth) return Value::make_string(kNestingErr);
    if (secret) mask_secret_value(v);
    return v;
}

bool is_engine_error(std::string_view s) noexcept {
    static constexpr std::string_view kPrefixes[] = {
        "INCLUDE_ERR:", "WATCH_ERR:", "CALC_ERR:", "SPAM_ERR:", "CONSTRAINT_ERR:",
        "ALIAS_ERR:", "NESTING_ERR:", "LIMIT_ERR:", "TYPE_ERR:", "WASM_ERR:",
    };
    if (s.empty() || s[0] < 'A' || s[0] > 'Z') return false;
    for (std::string_view p : kPrefixes) {
        if (s.substr(0, p.size()) == p) return true;
    }
    return false;
}

void mask_secret_value(Value& v) {
    if (v.is_secret() || v.is_null()) return;
    if (Array* a = v.as_array_mut()) {
        for (auto& item : *a) mask_secret_value(item);
        return;
    }
    if (Object* o = v.as_object_mut()) {
        for (auto& p : *o) mask_secret_value(p.value);
        return;
    }
    if (const std::string* s = v.as_string()) {
        if (is_engine_error(*s)) return;
    }
    v = Value::make_secret(value_to_string(v));
}

JailResult jail_path(std::string_view base, std::string_view file_path) {
    JailResult r;
    std::string quoted = "'" + std::string(file_path) + "'";
    if (!file_path.empty() && (file_path[0] == '/' || file_path[0] == '\\')) {
        r.error = "SECURITY: rooted paths are not allowed: " + quoted;
        return r;
    }
    fs::path fp = path_from_utf8(file_path);
    if (fp.is_absolute()) {
        r.error = "SECURITY: absolute paths are not allowed: " + quoted;
        return r;
    }
#if defined(_WIN32)
    // safety: `NUL`, `con.txt` or `COM1` in any directory opens a device on
    // Windows; synx-core reaches the disk through a verbatim path, where these
    // are ordinary missing files.
    if (has_device_name(file_path)) {
        r.error = "file not found: " + std::string(file_path);
        return r;
    }
#endif
    std::error_code ec;
    // safety: a base that cannot be resolved refuses instead of jailing nothing.
    if (base.empty()) base = ".";
    fs::path base_path = path_from_utf8(base);
    fs::path base_canonical = fs::canonical(base_path, ec);
    if (ec) {
        r.error = "SECURITY: base directory not found: '" + std::string(base) + "'";
        return r;
    }
    const bool canonical_base = true;
    fs::path full = base_canonical / fp;
#if defined(_WIN32)
    // Rust's canonical path on Windows is verbatim (`\\?\C:\…`), and pushing
    // onto a verbatim path resolves `.` and `..` lexically — so `../x` is
    // reported as escaping the base rather than as traversal, as on Unix.
    if (canonical_base) full = full.lexically_normal();
#else
    (void)canonical_base;
#endif
    ec.clear();
    fs::path full_canonical = fs::canonical(full, ec);
    if (ec) {
        // The file may not exist yet — at least verify no `..` escapes, judged
        // by the path as the document wrote it: a base named `a..b` is no
        // traversal.
        bool traversal = false;
        each_segment(file_path, [&](std::string_view seg) { traversal = traversal || seg == ".."; });
        if (traversal) {
            r.error = "SECURITY: path traversal detected: " + quoted;
            return r;
        }
        if (!path_starts_with(full, base_canonical)) {
            r.error = "SECURITY: path escapes base directory: " + quoted;
            return r;
        }
        r.ok = true;
        r.path = full;
        return r;
    }
    if (!path_starts_with(full_canonical, base_canonical)) {
        r.error = "SECURITY: path escapes base directory: " + quoted;
        return r;
    }
    r.ok = true;
    r.path = full_canonical;
    return r;
}

std::string check_file_size(const fs::path& p) {
    std::error_code ec;
    uintmax_t len = fs::file_size(p, ec);
    if (!ec && len > kMaxFileSize) {
        return "SECURITY: file too large (" + std::to_string(len) + " bytes, max "
             + std::to_string(kMaxFileSize) + ")";
    }
    return std::string();
}

// spec: the error text never carries the operating system's wording or the
// absolute path, so the same document resolves to the same string everywhere.
bool read_text(const fs::path& p, std::string_view ctx, std::string& out, std::string& error) {
    std::error_code ec;
    fs::file_status st = fs::status(p, ec);
    if (ec || !fs::exists(st)) {
        error = "file not found: " + std::string(ctx);
        return false;
    }
    // A directory reads as "permission denied" on Windows and "is a directory"
    // elsewhere; it is neither.
    if (fs::is_directory(st)) {
        error = "not a file: " + std::string(ctx);
        return false;
    }
    std::ifstream in(p, std::ios::binary);
    if (!in.good()) {
        error = "permission denied: " + std::string(ctx);
        return false;
    }
    std::ostringstream ss;
    ss << in.rdbuf();
    if (in.bad()) {
        error = "cannot read file: " + std::string(ctx);
        return false;
    }
    out = ss.str();
    if (!valid_utf8(out)) {
        error = "file is not valid UTF-8: " + std::string(ctx);
        return false;
    }
    return true;
}

bool read_budgeted(const fs::path& p, std::string_view shown, std::string& out, std::string& error) {
    std::error_code ec;
    uintmax_t size = fs::is_regular_file(p, ec) ? fs::file_size(p, ec) : 0;
    if (ec) size = 0;
    if (state().read_bytes + size > kMaxResolveReadBytes) {
        error = "read budget (" + std::to_string(kMaxResolveReadBytes) + " bytes) exceeded";
        return false;
    }
    if (!read_text(p, shown, out, error)) return false;
    state().read_bytes += out.size();
    return true;
}

bool include_file(const fs::path& full, std::string_view shown, const Options& options, Value& out,
                  std::string& error) {
    ResolveState& s = state();
    for (const auto& open : s.chain) {
        if (open == full) {
            error = "circular include: '" + std::string(shown) + "'";
            return false;
        }
    }
    if (s.reads >= kMaxIncludeReads) {
        error = "include budget (" + std::to_string(kMaxIncludeReads) + " files) exceeded";
        return false;
    }
    ++s.reads;
    s.chain.push_back(full);
    std::string text;
    bool ok = read_budgeted(full, shown, text, error);
    if (ok) out = load_synx_file(text, full, options);
    state().chain.pop_back();
    return ok;
}

Value load_synx_file(const std::string& text, const fs::path& file, const Options& options) {
    ParseResult included = parse(text);
    if (included.mode == Mode::Active) {
        Options child = options;
        child._include_depth += 1;
        fs::path parent = file.parent_path();
        if (!parent.empty()) child.base_path = path_to_utf8(parent);
        resolve(included, child);
    }
    return std::move(included.root);
}

// ─── [pattern] ──────────────────────────────────────────────────────────────

bool has_nested_quantifier(std::string_view p) {
    // Characters, not bytes, as in Rust: a `\` escapes one whole character.
    std::vector<uint32_t> chars;
    for (size_t i = 0; i < p.size();) {
        size_t len = 0;
        chars.push_back(decode_utf8(p, i, len));
        i += len;
    }
    std::vector<size_t> opens;
    size_t i = 0;
    while (i < chars.size()) {
        uint32_t c = chars[i];
        if (c == '\\') {
            i += 1;
        } else if (c == '[') {
            // A character class is skipped wholesale; quantifiers in it are literal.
            while (i < chars.size() && chars[i] != ']') {
                if (chars[i] == '\\') i += 1;
                i += 1;
            }
        } else if (c == '(') {
            opens.push_back(i);
        } else if (c == ')') {
            if (!opens.empty()) {
                size_t start = opens.back();
                opens.pop_back();
                if (i + 1 < chars.size() && (chars[i + 1] == '*' || chars[i + 1] == '+' || chars[i + 1] == '{')) {
                    std::vector<uint32_t> body;
                    size_t j = start + 1;
                    while (j < i) {
                        if (chars[j] == '\\') { j += 2; continue; }
                        body.push_back(chars[j]);
                        j += 1;
                    }
                    for (size_t k = 0; k < body.size(); ++k) {
                        if (body[k] == '*' || body[k] == '+') return true;
                        if (body[k] == '{' && k + 1 < body.size() && body[k + 1] >= '0' && body[k + 1] <= '9') {
                            return true;
                        }
                    }
                }
            }
        }
        i += 1;
    }
    return false;
}

bool pattern_rejects(const std::string& pattern, const std::string& s) {
    // perf: compiled patterns are cached per thread — a pattern declared on a
    // field name was compiled again for every value checked against it.
    // safety: bounded, so a document of distinct patterns cannot grow it forever.
    constexpr size_t kMaxCachedPatterns = 256;
    thread_local std::unordered_map<std::string, std::unique_ptr<Pattern>> cache;
    auto it = cache.find(pattern);
    if (it == cache.end()) {
        if (cache.size() >= kMaxCachedPatterns) cache.clear();
        // A backtracking-prone shape is refused everywhere, as in synx-core.
        std::unique_ptr<Pattern> compiled = has_nested_quantifier(pattern) ? nullptr : Pattern::compile(pattern);
        it = cache.emplace(pattern, std::move(compiled)).first;
    }
    if (!it->second) return false;
    return !it->second->is_match(s);
}

// ─── constraints ────────────────────────────────────────────────────────────

std::optional<std::string> constraint_violation(std::string_view key_sv, const Value& val, const Constraints& c) {
    const std::string key(key_sv);
    auto join_enum = [&]() {
        std::string joined;
        for (size_t i = 0; i < c.enum_values->size(); ++i) {
            if (i > 0) joined.push_back('|');
            joined.append((*c.enum_values)[i]);
        }
        return joined;
    };
    // A key written without a value parses as an empty group; for `[required]`
    // that is the missing value, not a present object.
    if (c.required && (val.is_null() || (val.is_string() && val.as_string()->empty())
                       || (val.is_object() && val.as_object()->empty()))) {
        return "CONSTRAINT_ERR: '" + key + "' is required";
    }
    // A secret is checked by its text; no message quotes the value.
    const std::string* text_of = val.is_secret() ? val.as_secret() : val.as_string();
    // An absent value (null) is checked against `enum` only, read as an empty
    // string; type, range and pattern apply to values that exist.
    if (val.is_null()) {
        if (c.enum_values) {
            bool has_empty = false;
            for (const auto& e : *c.enum_values) has_empty = has_empty || e.empty();
            if (!has_empty) return "CONSTRAINT_ERR: '" + key + "' must be one of [" + join_enum() + "]";
        }
        return std::nullopt;
    }
    if (c.type_name) {
        const std::string& tn = *c.type_name;
        bool ok = true;
        if (tn == "int") ok = val.is_int();
        else if (tn == "float") ok = val.is_float() || val.is_int();
        else if (tn == "bool") ok = val.is_bool();
        else if (tn == "string") ok = val.is_string() || val.is_secret();  // a secret is a masked string
        if (!ok) return "CONSTRAINT_ERR: '" + key + "' expected type '" + tn + "'";
    }
    if (c.enum_values) {
        std::string text;
        if (text_of) text = *text_of;
        else if (val.is_int()) text = i64_to_string(*val.as_int());
        else if (val.is_float()) text = display_f64(*val.as_float());
        else if (val.is_bool()) text = *val.as_bool() ? "true" : "false";
        bool found = false;
        for (const auto& e : *c.enum_values) {
            if (e == text) { found = true; break; }
        }
        if (!found) return "CONSTRAINT_ERR: '" + key + "' must be one of [" + join_enum() + "]";
    }
    // Numbers by value; strings by their count of Unicode scalar values.
    bool have_n = false;
    double n = 0.0;
    if (val.is_int()) { n = static_cast<double>(*val.as_int()); have_n = true; }
    else if (val.is_float()) { n = *val.as_float(); have_n = true; }
    else if (text_of && (c.min || c.max)) { n = static_cast<double>(char_count(*text_of)); have_n = true; }
    if (have_n) {
        if (c.min && n < *c.min) {
            return "CONSTRAINT_ERR: '" + key + "' value " + display_f64(n) + " is below min " + display_f64(*c.min);
        }
        if (c.max && n > *c.max) {
            return "CONSTRAINT_ERR: '" + key + "' value " + display_f64(n) + " exceeds max " + display_f64(*c.max);
        }
    }
    if (c.pattern && c.pattern->size() <= 256 && text_of) {
        if (pattern_rejects(*c.pattern, *text_of)) {
            return "CONSTRAINT_ERR: '" + key + "' does not match pattern /" + *c.pattern + "/";
        }
    }
    return std::nullopt;
}

// ─── resolution walk ────────────────────────────────────────────────────────

void Resolver::resolve_value(Value& value, std::string& path, size_t depth) {
    Object* map = value.as_object_mut();
    if (!map) return;
    if (depth >= kMaxResolveDepth) {
        for (auto& p : *map) p.value = Value::make_string(kNestingErr);
        return;
    }
    // One pass in document order: each key's subtree, then the key's own
    // markers — so a marker sees every key before it resolved, at any depth.
    const MetaMap* mm = metadata.get(path);
    ObjectLookup own(*map);
    const ObjectLookup& local = map == root.as_object() ? root_lookup : own;
    const size_t base_len = path.size();
    const size_t count = map->size();
    // What this object's `:secret` keys held before the pre-pass.
    std::unordered_map<std::string, Value> originals;
    if (!secret_originals.empty()) {
        auto it = secret_originals.find(path);
        if (it != secret_originals.end()) {
            originals = std::move(it->second);
            secret_originals.erase(it);
        }
    }
    for (size_t i = 0; i < count; ++i) {
        Value& child = (*map)[i].value;
        if (!originals.empty()) {
            auto it = originals.find((*map)[i].key);
            if (it != originals.end()) {
                child = std::move(it->second);
                originals.erase(it);
                if (child.is_object() || child.is_array()) {
                    // Secrets nested in it stay masked until their own turn.
                    if (base_len > 0) path.push_back('.');
                    push_key_segment(path, (*map)[i].key);
                    mask_secret_subtree(child, metadata, path, secret_originals);
                    path.resize(base_len);
                }
            }
        }
        if (child.is_object() || child.is_array()) {
            if (base_len > 0) path.push_back('.');
            push_key_segment(path, (*map)[i].key);
            if (child.is_object()) {
                resolve_value(child, path, depth + 1);
            } else {
                Array& arr = *child.as_array_mut();
                const size_t list_len = path.size();
                for (size_t idx = 0; idx < arr.size(); ++idx) {
                    if (!arr[idx].is_object()) continue;
                    push_item_segment(path, idx);
                    resolve_value(arr[idx], path, depth + 1);
                    path.resize(list_len);
                }
            }
            path.resize(base_len);
        }
        if (!mm) continue;
        const Meta* meta = mm->get((*map)[i].key);
        if (!meta) continue;
        apply_markers(*map, i, local, *meta, path, depth);
    }
    // Then `{…}` interpolation over the object's strings.
    for (size_t i = 0; i < count; ++i) {
        const std::string* s = (*map)[i].value.as_string();
        if (!s || s->find('{') == std::string::npos) continue;
        // safety: the text may grow by what is left of the budget and no
        // further — one 4 MiB value named from 600 keys was 2.4 GiB.
        const size_t before = s->size();
        std::optional<std::string> result = resolve_interpolation(*s, local, before + expansion_room());
        if (!result) {
            charge_expansion(static_cast<size_t>(-1));
            (*map)[i].value = Value::make_string(kLimitErr);
        } else if (*result != *s) {
            charge_expansion(result->size() > before ? result->size() - before : 0);
            (*map)[i].value = Value::make_string(std::move(*result));
        }
    }
}

std::optional<std::string> Resolver::resolve_interpolation(std::string_view tpl, const ObjectLookup& local,
                                                           size_t cap) const {
    std::string result;
    result.reserve(std::min(tpl.size(), kMaxScratchString));
    auto push_capped = [&](std::string_view s) {
        size_t room = kMaxScratchString > result.size() ? kMaxScratchString - result.size() : 0;
        if (room > 0) result.append(s.data(), utf8_prefix_len(s, std::min(s.size(), room)));
    };
    // Unresolved: written back as `{inner}`, within the cap.
    auto push_literal = [&](std::string_view inner) {
        result.push_back('{');
        size_t rem = kMaxScratchString > result.size() + 1 ? kMaxScratchString - (result.size() + 1) : 0;
        if (rem > 0) result.append(inner.data(), utf8_prefix_len(inner, std::min(inner.size(), rem)));
        if (result.size() < kMaxScratchString) result.push_back('}');
    };
    auto valid_name = [](std::string_view name) {
        for (size_t k = 0; k < name.size();) {
            size_t len = 0;
            uint32_t cp = decode_utf8(name, k, len);
            if (!(is_alphanumeric(cp) || cp == '_' || cp == '.')) return false;
            k += len;
        }
        return true;
    };
    size_t i = 0;
    while (i < tpl.size()) {
        if (result.size() >= kMaxScratchString) break;
        if (tpl[i] == '{') {
            size_t close = tpl.find('}', i + 1);
            if (close != std::string_view::npos) {
                std::string_view inner = tpl.substr(i + 1, close - i - 1);
                size_t colon = inner.find(':');
                std::string_view ref = colon == std::string_view::npos ? inner : inner.substr(0, colon);
                if (valid_name(ref)) {
                    const Value* val = nullptr;
                    if (colon != std::string_view::npos) {
                        std::string_view scope = inner.substr(colon + 1);
                        if (scope == "include") {
                            // {key:include} — the first and only include.
                            if (includes.size() == 1) val = deep_get(includes.begin()->second, ref, nullptr);
                        } else if (const Value* inc = includes.get(scope)) {
                            val = deep_get(*inc, ref, nullptr);
                        }
                        if (val) push_capped(value_to_string(*val));
                        else push_literal(inner);
                    } else {
                        val = deep_get(root, ref, &root_lookup);
                        if (!val) val = local.get(ref);
                        if (val) push_capped(value_to_string(*val));
                        else push_literal(ref);
                    }
                    if (result.size() > cap) return std::nullopt;
                    i = close + 1;
                    continue;
                }
            }
        }
        // One whole character of literal text.
        size_t len = utf8_len(static_cast<unsigned char>(tpl[i]));
        if (result.size() < kMaxScratchString) result.append(tpl.substr(i, len));
        i += len;
    }
    if (result.size() > cap) return std::nullopt;
    return result;
}

namespace {

// ─── pre-passes ─────────────────────────────────────────────────────────────

void merge_constraints(Constraints& base, const Constraints& in) {
    if (in.required) base.required = true;
    if (in.readonly) base.readonly = true;
    if (in.min) base.min = base.min ? std::max(*base.min, *in.min) : *in.min;
    if (in.max) base.max = base.max ? std::min(*base.max, *in.max) : *in.max;
    if (!base.type_name) base.type_name = in.type_name;
    if (!base.pattern) base.pattern = in.pattern;
    if (!base.enum_values) base.enum_values = in.enum_values;
}

// Registries from the live tree: each object's own declarations first, then
// its children in document order. The first type for a name wins.
void collect_registries(const Value& value, const MetadataTree& metadata, std::string& path,
                        IndexMap<std::string>& types, IndexMap<Constraints>& constraints, size_t depth) {
    if (depth > kMaxResolveDepth) return;
    const Object* map = value.as_object();
    if (!map) return;
    if (const MetaMap* mm = metadata.get(path)) {
        for (const auto& e : *mm) {
            const Meta& meta = e.second;
            if (meta.type_hint && !types.contains(e.first)) types.insert_or_assign(e.first, *meta.type_hint);
            if (meta.constraints) {
                if (Constraints* existing = constraints.get(e.first)) merge_constraints(*existing, *meta.constraints);
                else constraints.insert_or_assign(e.first, *meta.constraints);
            }
        }
    }
    const size_t base_len = path.size();
    for (const auto& p : *map) {
        if (!p.value.is_object() && !p.value.is_array()) continue;
        if (base_len > 0) path.push_back('.');
        push_key_segment(path, p.key);
        if (p.value.is_object()) {
            collect_registries(p.value, metadata, path, types, constraints, depth + 1);
        } else {
            const Array& items = *p.value.as_array();
            const size_t list_len = path.size();
            for (size_t idx = 0; idx < items.size(); ++idx) {
                if (!items[idx].is_object()) continue;
                push_item_segment(path, idx);
                collect_registries(items[idx], metadata, path, types, constraints, depth + 1);
                path.resize(list_len);
            }
        }
        path.resize(base_len);
    }
}

Value* find_in(Object& obj, std::string_view key) {
    for (auto& p : obj) {
        if (p.key == key) return &p.value;
    }
    return nullptr;
}

void set_in(Object& obj, std::string_view key, Value v) {
    if (Value* existing = find_in(obj, key)) *existing = std::move(v);
    else obj.push_back(Pair{std::string(key), std::move(v)});
}

// What copying one key's declarations costs against the budget (synx-core `meta_cost`).
size_t meta_cost(std::string_view key, const Meta& meta) {
    size_t cost = 2 * kValueCost + key.size();
    for (const auto& s : meta.markers) cost += 24 + s.size();
    for (const auto& s : meta.args) cost += 24 + s.size();
    if (meta.type_hint) cost += meta.type_hint->size();
    if (meta.constraints) {
        const Constraints& c = *meta.constraints;
        if (c.type_name) cost += 24 + c.type_name->size();
        if (c.pattern) cost += 24 + c.pattern->size();
        if (c.enum_values) {
            for (const auto& s : *c.enum_values) cost += 24 + s.size();
        }
    }
    return cost;
}

size_t meta_map_cost(const MetaMap& mm) {
    size_t cost = kValueCost;
    for (const auto& e : mm) cost += meta_cost(e.first, e.second);
    return cost;
}

// The first two segments of a metadata path (`parent.field`), or empty for a
// path of one segment. A `.` escaped with `\` is part of its segment.
std::string_view two_segment_head(std::string_view path) {
    size_t dots = 0;
    for (size_t i = 0; i < path.size(); ++i) {
        if (path[i] == '\\') {
            ++i;
        } else if (path[i] == '.') {
            if (++dots == 2) return path.substr(0, i);
        }
    }
    return dots == 1 ? path : std::string_view();
}

// `:inherit` — several parents merged left to right, the child overrides; each
// field keeps the declaration of the object its value came from; inherited
// nested subtrees are copies carrying their own declarations.
//
// safety: every inherited field and declaration is a copy, charged to the
// document's budget — 3 000 children of a 3 000-field parent are nine million
// fields from a 130 KB file.
void apply_inheritance(Value& root, MetadataTree& metadata) {
    const MetaMap* root_meta = metadata.get("");
    if (!root_meta) return;
    std::vector<std::pair<std::string, std::vector<std::string>>> inherits;
    for (const auto& e : *root_meta) {
        const Meta& meta = e.second;
        int idx = meta.marker_index("inherit");
        if (idx < 0) continue;
        std::vector<std::string> parents(meta.markers.begin() + idx + 1, meta.markers.end());
        if (parents.empty() && !meta.args.empty()) parents = meta.args;
        if (!parents.empty()) inherits.emplace_back(e.first, std::move(parents));
    }
    Object* map = root.as_object_mut();
    if (!map || inherits.empty()) return;
    // perf: the declarations nested under each `object.field`, found once.
    std::unordered_map<std::string, std::vector<std::string>> nested_index;
    for (const auto& me : metadata) {
        std::string_view head = two_segment_head(me.first);
        if (!head.empty()) nested_index[std::string(head)].push_back(me.first);
    }
    for (const auto& inh : inherits) {
        const std::string& child_key = inh.first;
        Value* child_val = find_in(*map, child_key);
        // Only an object inherits; a list or a scalar is left as written.
        if (!child_val || !child_val->is_object()) continue;
        IndexMap<Value> merged;
        IndexMap<const std::string*> source;  // nullptr = the child itself
        bool within_budget = true;
        for (const std::string& parent_name : inh.second) {
            const Value* pv = find_in(*map, parent_name);
            if (!pv || !pv->is_object()) continue;
            for (const auto& p : *pv->as_object()) {
                if (!charge_expansion(value_cost(p.value).first + p.key.size())) {
                    within_budget = false;
                    break;
                }
                merged.insert_or_assign(p.key, p.value);
                source.insert_or_assign(p.key, &parent_name);
            }
            if (!within_budget) break;
        }
        if (!within_budget) {
            set_in(*map, child_key, Value::make_string(kLimitErr));
            continue;
        }
        child_val = find_in(*map, child_key);
        for (const auto& p : *child_val->as_object()) {
            merged.insert_or_assign(p.key, p.value);
            source.insert_or_assign(p.key, nullptr);
        }
        MetaMap top;
        std::vector<std::pair<std::string, MetaMap>> nested;
        for (const auto& s : source) {
            const std::string& k = s.first;
            const std::string& from = s.second ? *s.second : child_key;
            if (const MetaMap* mm = metadata.get(key_segment(from))) {
                if (const Meta* m = mm->get(k)) {
                    if (s.second && !charge_expansion(meta_cost(k, *m))) {
                        within_budget = false;
                        break;
                    }
                    top.insert_or_assign(k, *m);
                }
            }
            if (s.second) {
                // The inherited subtree is a copy; its nested markers move with it.
                std::string prefix = key_segment(*s.second) + "." + key_segment(k);
                auto found = nested_index.find(prefix);
                if (found == nested_index.end()) continue;
                for (const std::string& p : found->second) {
                    const MetaMap* mm = metadata.get(p);
                    if (!mm) continue;
                    if (!charge_expansion(meta_map_cost(*mm) + p.size())) {
                        within_budget = false;
                        break;
                    }
                    std::string_view rest = std::string_view(p).substr(prefix.size());
                    nested.emplace_back(key_segment(child_key) + "." + key_segment(k) + std::string(rest), *mm);
                }
                if (!within_budget) break;
            }
        }
        if (!within_budget) {
            set_in(*map, child_key, Value::make_string(kLimitErr));
            continue;
        }
        Object merged_obj;
        merged_obj.reserve(merged.size());
        for (auto& e : merged) merged_obj.push_back(Pair{e.first, std::move(e.second)});
        set_in(*map, child_key, Value::make_object(std::move(merged_obj)));
        if (top.empty()) metadata.erase(key_segment(child_key));
        else metadata.insert_or_assign(key_segment(child_key), std::move(top));
        for (auto& n : nested) {
            // A later child may inherit from this one: its copies are indexed too.
            if (!metadata.contains(n.first)) {
                std::string_view head = two_segment_head(n.first);
                if (!head.empty()) nested_index[std::string(head)].push_back(n.first);
            }
            metadata.insert_or_assign(std::move(n.first), std::move(n.second));
        }
    }
}

} // namespace

// The `:secret` pre-pass: every value under `:secret` is masked before any
// resolution. What the key held is kept in `originals` and comes back for the
// key's own turn (its subtree and its marker chain); what that produces is
// masked again by the `:secret` marker.
//
// safety: a literal next to `:default`, `:replace`, `:format`, `:split`, … was
// left in the clear until its own turn, and the fields of a group under
// `:secret` were never masked here: `leak:alias password` above
// `password:secret:default:x hunter2` read `hunter2`.
void mask_secret_literals(Value& value, const MetadataTree& metadata, std::string& path, SecretOriginals& originals) {
    Object* map = value.as_object_mut();
    if (!map) return;
    if (const MetaMap* mm = metadata.get(path)) {
        for (const auto& e : *mm) {
            if (!e.second.has_marker("secret")) continue;
            if (Value* v = find_in(*map, e.first)) {
                if (v->is_secret() || v->is_null()) continue;
                Value masked = *v;
                mask_secret_value(masked);
                originals[path][e.first] = std::move(*v);
                *v = std::move(masked);
            }
        }
    }
    const size_t base_len = path.size();
    for (auto& p : *map) {
        if (!p.value.is_object() && !p.value.is_array()) continue;
        if (base_len > 0) path.push_back('.');
        push_key_segment(path, p.key);
        mask_secret_subtree(p.value, metadata, path, originals);
        path.resize(base_len);
    }
}

void mask_secret_subtree(Value& child, const MetadataTree& metadata, std::string& path, SecretOriginals& originals) {
    if (child.is_object()) {
        mask_secret_literals(child, metadata, path, originals);
    } else if (Array* arr = child.as_array_mut()) {
        const size_t list_len = path.size();
        for (size_t idx = 0; idx < arr->size(); ++idx) {
            if (!(*arr)[idx].is_object()) continue;
            push_item_segment(path, idx);
            mask_secret_literals((*arr)[idx], metadata, path, originals);
            path.resize(list_len);
        }
    }
}

namespace {

bool value_matches_type(const Value& v, const std::string& t) {
    if (t == "int") return v.is_int();
    if (t == "float") return v.is_float() || v.is_int();
    if (t == "bool") return v.is_bool();
    if (t == "string") return v.is_string() || v.is_secret();
    if (t == "array") return v.is_array();
    if (t == "object") return v.is_object();
    return true;  // unknown types are accepted
}

void report(const Options& options, const std::string& message) {
    if (options.strict) std::fprintf(stderr, "synx: %s\n", message.c_str());
}

// Constraints then type for every field of the tree. A value that already holds
// a CONSTRAINT_ERR / TYPE_ERR is not measured again against constraints.
void validate_fields(Value& value, const IndexMap<Constraints>& constraints, const IndexMap<std::string>& types,
                     const Options& options) {
    Object* map = value.as_object_mut();
    if (!map) return;
    for (auto& p : *map) {
        Value& val = p.value;
        if (const Constraints* c = constraints.get(p.key)) {
            const std::string* s = val.as_string();
            bool already = s && (s->rfind("CONSTRAINT_ERR:", 0) == 0 || s->rfind("TYPE_ERR:", 0) == 0);
            if (!already) {
                if (auto err = constraint_violation(p.key, val, *c)) {
                    report(options, *err);
                    val = Value::make_string(std::move(*err));
                }
            }
        }
        if (const std::string* expected = types.get(p.key)) {
            if (!value_matches_type(val, *expected)) {
                std::string err = "TYPE_ERR: '" + p.key + "' expected " + *expected + " but got " + val.type_name();
                report(options, err);
                val = Value::make_string(std::move(err));
            }
        }
        if (val.is_object()) {
            validate_fields(val, constraints, types, options);
        } else if (Array* arr = val.as_array_mut()) {
            for (auto& item : *arr) {
                if (item.is_object()) validate_fields(item, constraints, types, options);
            }
        }
    }
}

// ─── !include / !use ────────────────────────────────────────────────────────

IndexMap<Value> load_includes(const std::vector<IncludeDirective>& directives, const Options& options) {
    IndexMap<Value> map;
    const std::string base = options.base_path.value_or(".");
    if (options._include_depth >= options.max_include_depth.value_or(kDefaultMaxIncludeDepth)) return map;
    for (const auto& inc : directives) {
        JailResult j = jail_path(base, inc.path);
        if (!j.ok || !check_file_size(j.path).empty()) continue;
        Value included;
        std::string err;
        if (include_file(j.path, inc.path, options, included, err)) map.insert_or_assign(inc.alias, std::move(included));
    }
    return map;
}

// Does `path` name something inside its base directory: relative, with no
// root, drive or `..` segment? Judged by its text, so a package directory that
// is itself a symlink keeps working (Rust `is_contained_relative`).
// safety: `!use @a/../../secret` and a manifest `main ../../x` read files from
// anywhere — the escape `:include` refuses through `jail_path`.
bool is_contained_relative(std::string_view path) {
    if (path.empty() || path[0] == '/' || path[0] == '\\' || path.find(':') != std::string_view::npos
        || path_from_utf8(path).is_absolute()) {
        return false;
    }
    size_t start = 0;
    while (true) {
        size_t sep = path.find_first_of("/\\", start);
        if (path.substr(start, (sep == std::string_view::npos ? path.size() : sep) - start) == "..") return false;
        if (sep == std::string_view::npos) return true;
        start = sep + 1;
    }
}

// `main` / `entry` of `synx-pkg.synx`, if the manifest names a contained one.
std::optional<fs::path> read_manifest_main(const fs::path& pkg_dir) {
    std::string text, err;
    if (!read_text(pkg_dir / "synx-pkg.synx", "", text, err)) return std::nullopt;
    size_t start = 0;
    while (start <= text.size()) {
        size_t nl = text.find('\n', start);
        std::string_view line = std::string_view(text).substr(start, (nl == std::string::npos ? text.size() : nl) - start);
        start = nl == std::string::npos ? text.size() + 1 : nl + 1;
        std::string_view t = trim_view(line);
        for (std::string_view prefix : {std::string_view("main "), std::string_view("entry ")}) {
            if (t.substr(0, prefix.size()) == prefix) {
                std::string_view entry = trim_view(t.substr(prefix.size()));
                if (is_contained_relative(entry)) return pkg_dir / path_from_utf8(entry);
            }
        }
    }
    return std::nullopt;
}

// `<base>/<packages_path>/@scope/name` → its manifest's entry or `src/main.synx`.
IndexMap<Value> load_packages(const std::vector<UseDirective>& directives, const Options& options) {
    IndexMap<Value> map;
    // safety: a package may `!use` another; packages using one another recursed
    // until the process ran out of stack.
    if (directives.empty() || options._include_depth >= options.max_include_depth.value_or(kDefaultMaxIncludeDepth)) {
        return map;
    }
    const fs::path pkg_root = path_from_utf8(options.base_path.value_or("."))
                            / path_from_utf8(options.packages_path.value_or("./synx_packages"));
    for (const auto& ud : directives) {
        if (!is_contained_relative(ud.package)) continue;
        fs::path pkg_dir = pkg_root / path_from_utf8(ud.package);
        fs::path entry = read_manifest_main(pkg_dir).value_or(pkg_dir / "src" / "main.synx");
        std::error_code ec;
        if (!fs::is_regular_file(entry, ec) || !check_file_size(entry).empty()) continue;
        fs::path opened = fs::canonical(entry, ec);
        if (ec) opened = entry;
        Value included;
        std::string err;
        if (include_file(opened, ud.package, options, included, err)) map.insert_or_assign(ud.alias, std::move(included));
    }
    return map;
}

} // namespace
} // namespace detail

// ─── public API ─────────────────────────────────────────────────────────────

void resolve(ParseResult& result, const Options& options) {
    using namespace detail;
    if (result.mode != Mode::Active) return;
    ResolveScope scope_guard;
    MetadataTree metadata = std::move(result.metadata);
    result.metadata.clear();
    std::vector<IncludeDirective> include_directives = std::move(result.includes);
    result.includes.clear();
    std::vector<UseDirective> use_directives = std::move(result.uses);
    result.uses.clear();

    // `options.env["SYNX_SEED"]` makes `:random` and friends reproducible (C++ extension).
    if (options.env) {
        auto it = options.env->find("SYNX_SEED");
        uint64_t seed = 0;
        if (it != options.env->end() && parse_u64(it->second, seed)) seed_rng(seed);
    }

    IndexMap<Value> packages = load_packages(use_directives, options);
    IndexMap<Value> includes = load_includes(include_directives, options);
    IndexMap<Value> preresolved;

    // `:include` / `:import` root keys are include aliases too, so
    // `{leaf:<key>}` interpolation works for `db:include ./common.synx`.
    if (Object* root_map = result.root.as_object_mut()) {
        if (const MetaMap* mm = metadata.get("")) {
            const std::string base = options.base_path.value_or(".");
            for (const auto& p : *root_map) {
                const Meta* meta = mm->get(p.key);
                if (!meta || !(meta->has_marker("include") || meta->has_marker("import"))) continue;
                // safety: the included document's own pre-pass includes again;
                // without the depth cap a file that includes itself never ends.
                if (options._include_depth >= options.max_include_depth.value_or(kDefaultMaxIncludeDepth)) continue;
                const std::string* file = p.value.as_string();
                if (!file) continue;
                JailResult j = jail_path(base, *file);
                if (!j.ok || !check_file_size(j.path).empty()) continue;
                Value included;
                std::string err;
                if (!include_file(j.path, *file, options, included, err)) continue;
                // perf: the key's `:include` marker takes this result instead of
                // resolving the file again (each level twice cost 2^depth).
                Array pre;
                pre.push_back(Value::make_string(*file));
                pre.push_back(included);
                preresolved.insert_or_assign(p.key, Value::make_array(std::move(pre)));
                // safety: the tree is also what `{leaf:key}` reads. Under
                // `:secret` that scope gets the masked tree, or `x {password:db}`
                // printed what `db:secret:include` hid.
                if (meta->has_marker("secret")) mask_secret_value(included);
                includes.emplace(p.key, std::move(included));
            }
        }
        for (auto& pkg : packages) {
            if (!find_in(*root_map, pkg.first)) root_map->push_back(Pair{pkg.first, pkg.second});
        }
    }

    IndexMap<std::string> type_registry;
    IndexMap<Constraints> constraint_registry;
    {
        std::string path;
        collect_registries(result.root, metadata, path, type_registry, constraint_registry, 0);
    }

    apply_inheritance(result.root, metadata);
    if (Object* root_map = result.root.as_object_mut()) {
        root_map->erase(std::remove_if(root_map->begin(), root_map->end(),
                                       [](const Pair& p) { return !p.key.empty() && p.key[0] == '_'; }),
                        root_map->end());
    }
    SecretOriginals secret_originals;
    {
        std::string path;
        mask_secret_literals(result.root, metadata, path, secret_originals);
    }

    if (Object* root_map = result.root.as_object_mut()) {
        // Resolution replaces values but never adds or removes keys: the root's
        // key set and entry buffer are fixed from here on.
        ObjectLookup root_lookup(*root_map);
        CalcScope scope;
        for (const auto& p : *root_map) {
            if (is_calc_special_key(p.key)) scope.root_special.push_back(p.key);
        }
        Resolver r{options, result.root, root_lookup, metadata, includes, preresolved, scope, secret_originals};
        std::string path;
        r.resolve_value(result.root, path, 0);
    }

    if (!constraint_registry.empty() || !type_registry.empty()) {
        validate_fields(result.root, constraint_registry, type_registry, options);
    }
    result.metadata = std::move(metadata);
    result.includes = std::move(include_directives);
}

} // namespace synx
