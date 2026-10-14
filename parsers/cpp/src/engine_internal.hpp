// Internal-only declarations shared between engine.cpp and engine_markers.cpp.
// Not part of the public include tree.
#pragma once

#include <cstdint>
#include <optional>
#include <random>
#include <string>
#include <string_view>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include "synx/options.hpp"
#include "synx/parse_result.hpp"
#include "synx/value.hpp"

namespace synx {
namespace detail {

constexpr size_t kMaxResolveDepth = 512;

/// Per-resolve mutable state. Carries the loaded sub-trees from `:use` /
/// `:include`, type/constraint registries, RNG, and per-invocation caches.
struct Resolver {
    ParseResult& result;
    const Options& options;
    Object& root;

    /// `alias -> Object*` for `!include` / `!use` sub-trees that resolve()
    /// has loaded into the root tree (transient; lookup only).
    std::unordered_map<std::string, const Object*> namespaces;

    /// Aliases of loaded `!include` files (not `!use` packages) — the scope
    /// that `{key:include}` resolves against when exactly one is present.
    std::vector<std::string> include_aliases;

    /// Type aliases declared by `type:` blocks. `name -> Constraints`.
    std::unordered_map<std::string, Constraints> type_registry;

    /// Per-resolve RNG (deterministic if `options.env["SYNX_SEED"]` is set).
    std::mt19937_64 rng;

    /// `:once` lock state (read from `.synx.lock` on first marker).
    bool once_loaded = false;
    std::unordered_set<std::string> once_keys;
    std::unordered_set<std::string> once_new_keys; // pending writes

    /// Path stack used for marker context (e.g. interpolation root).
    std::vector<std::string> path_stack;

    Resolver(ParseResult& r, const Options& o);

    // ─── helpers (engine.cpp) ────────────────────────────────────────────────
    /// Sandbox a relative path against a base directory. Returns empty string
    /// on rejection (rooted/absolute/traversal/`res://`/`user://`).
    std::string jail_path(const std::string& base, const std::string& rel) const;

    /// Deep dot-path lookup. Returns nullptr if any segment is missing or wrong type.
    const Value* deep_get(const Object& root, std::string_view path) const;
    const Value* deep_get_with_namespaces(std::string_view path) const;

    /// Substitute `{key}`, `{key.path}`, `{key:alias}` placeholders in `s`.
    std::string interpolate(std::string_view s) const;

    /// Auto-`{}` interpolation (Rust engine's third pass): resolve `{key}`,
    /// `{a.b}`, `{key:alias}`, `{key:include}` in every string of `obj`.
    void interpolate_level(Object& obj);
    std::string resolve_interpolation(std::string_view tpl, const Object& local) const;

    /// `:secret` pre-pass: wrap literal secret values before any resolution so
    /// an `{interpolation}` can never read the plaintext.
    void mask_secret_literals();

    /// Re-point `namespaces` at the installed sub-objects (root may reallocate).
    void refresh_namespaces();

    /// Apply markers to `value` at the given metadata key, mutating in place.
    /// `path` is the dot-path of the *containing* object.
    void resolve_value(Value& value, const Object& local, const std::string& path, const std::string& key, int depth);

    /// Recursive walk: applies markers to every keyed leaf and recurses into objects/arrays.
    void walk(Value& v, const std::string& path, int depth);

    /// Run `:inherit` pre-pass — must happen before regular marker application.
    void apply_inherit_pass();

    /// Strip top-level `_`-prefixed helper keys from root.
    void strip_underscore_keys();

    /// Validate constraints across the whole tree.
    void validate_field_constraints();
    void validate_field_types();
    void report_violation(const std::string& message) const;

    /// Load `!use` packages (file-system resolved against options.packages_path).
    void load_packages();

    /// Load `!include` directives (file-system resolved against options.base_path).
    void load_includes();

    /// Persist `:once` keys to `.synx.lock` if any new keys were written.
    void flush_once();

    // ─── markers (engine_markers.cpp) ────────────────────────────────────────
    void apply_env(Value& v, const Meta& m);
    void apply_default(Value& v, const Meta& m);
    void apply_calc(Value& v, const Meta& m, const std::string& key, const Object& local);
    void apply_ref(Value& v, const Meta& m);
    void apply_alias(Value& v, const Meta& m);
    void apply_secret(Value& v, const Meta& m);
    void apply_random(Value& v, const Meta& m);
    void apply_unique(Value& v, const Meta& m);
    void apply_geo(Value& v, const Meta& m);
    void apply_i18n(Value& v, const Meta& m);
    void apply_split(Value& v, const Meta& m);
    void apply_join(Value& v, const Meta& m);
    void apply_clamp(Value& v, const Meta& m);
    void apply_round(Value& v, const Meta& m);
    void apply_map(Value& v, const Meta& m);
    void apply_format(Value& v, const Meta& m);
    void apply_replace(Value& v, const Meta& m);
    void apply_sort(Value& v, const Meta& m);
    void apply_sum(Value& v, const Meta& m);
    void apply_fallback(Value& v, const Meta& m);
    void apply_once(Value& v, const Meta& m, const std::string& path, const std::string& key);
    void apply_version(Value& v, const Meta& m);
    void apply_watch(Value& v, const Meta& m);
    void apply_prompt(Value& v, const Meta& m);
    void apply_vision(Value& v, const Meta& m);
    void apply_audio(Value& v, const Meta& m);
    void apply_spam(Value& v, const Meta& m, const std::string& path, const std::string& key);
    void apply_custom(Value& v, const Meta& m, const std::string& key, const MarkerFn& fn);
};

/// Read whole text file from `path`. Returns empty string + ok=false on error.
struct FileText {
    std::string text;
    bool ok = false;
};
FileText read_text_file(const std::string& path);

/// Normalize a value to a string for marker pipelines / interpolation.
std::string value_to_string(const Value& v);

/// Does `m` ask for the text to stay text (`(string)` or `[type:string]`)?
bool wants_string(const Meta& m);

/// Rust `cast_primitive`: quotes strip, `true`/`false`/`null`, exact int64, then double.
Value cast_primitive(std::string_view val);

/// Convert a value to a double if possible. Returns false for non-numeric.
bool value_to_number(const Value& v, double& out);

/// CLDR plural category for `n` in the given language. Returns one of
/// "zero" / "one" / "two" / "few" / "many" / "other".
const char* plural_category(std::string_view lang, double n);

/// "now" as ISO 8601 timestamp (UTC, second precision).
std::string current_iso_timestamp();

/// Builtin marker names — used to filter `options.marker_fns` so user code
/// cannot override builtins.
bool is_builtin_marker(const std::string& name);

/// Rust `format_number`: whole numbers print without a fraction, others as shortest `Display`.
std::string format_number(double n);

/// Rust `f64` Display inside messages: no `.0` on whole numbers (`5`, `0.25`, `70000`).
std::string display_number(double n);

/// First failing constraint for `val` under `c` (Rust order: required, type, enum,
/// min/max, pattern) as the full `CONSTRAINT_ERR: …` string, or nullopt.
std::optional<std::string> constraint_error(const std::string& key, const Value& val,
                                            const Constraints& c);

/// Does the pattern repeat something that already repeats (`(a+)+`)? Port of
/// `hasNestedQuantifier` in packages/synx-js/src/synxl.ts.
bool has_nested_quantifier(std::string_view pattern);

/// Test `value` against `pattern`, refusing patterns that can backtrack
/// catastrophically. Returns -1 for a refused / uncompilable pattern (constraint
/// skipped), 0 for no match, 1 for a match.
int matches_linear(const std::string& pattern, const std::string& value);

/// Conservative syntax pre-check for ECMAScript-grammar `std::regex` patterns.
/// Accepts only patterns `std::regex` is certain to compile; with exceptions
/// disabled a bad pattern would abort the process, so anything doubtful
/// (unbalanced groups/classes, bad `{m,n}`, trailing `\`, a quantifier with
/// nothing to repeat, back-references, unknown escapes) is rejected and the
/// constraint is skipped, like an invalid pattern in Rust.
bool regex_pattern_is_supported(std::string_view pattern);

/// Strict regex check using std::regex. Returns true on match, or true on
/// regex compile error (the value is *not* rejected if the pattern is invalid).
bool regex_match_strict(const std::string& value, const std::string& pattern);

} // namespace detail
} // namespace synx
