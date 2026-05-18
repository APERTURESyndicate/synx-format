// SYNX active-mode resolver options. Parity with Rust `Options`.
#pragma once

#include <cstdint>
#include <functional>
#include <optional>
#include <string>
#include <unordered_map>
#include <vector>

#include "synx/value.hpp"

namespace synx {

/// `:include path [alias]` directive parsed from the file.
struct IncludeDirective {
    std::string path;
    std::string alias;
};

/// `:use @scope/name [as alias]` package directive.
struct UseDirective {
    /// Full package name, e.g. `@aperture/synx-defaults`.
    std::string package;
    /// Namespace alias (defaults to last path segment).
    std::string alias;
};

/// User-supplied custom marker.
///
/// Signature: (key, args, current_value) -> resolved_value.
///   * `key`   — name of the field being resolved
///   * `args`  — argument list parsed from `:marker:arg1:arg2`
///   * `value` — value already on the field (may be Null)
///
/// Returning `Value::make_null()` is valid. Throwing is not allowed (exceptions
/// are disabled in the build).
using MarkerFn = std::function<Value(const std::string& key,
                                     const std::vector<std::string>& args,
                                     const Value& value)>;

/// Resolution options. Default-constructed Options is fine for static parsing.
struct Options {
    std::optional<std::unordered_map<std::string, std::string>> env;
    std::optional<std::string> region;
    std::optional<std::string> lang;
    /// Base directory for `:include` / `:import` / `:use` file lookups (default: current dir).
    std::optional<std::string> base_path;
    /// Max nesting for `:include` / `:import` / `:watch` chains. Default 16.
    std::optional<size_t> max_include_depth;
    /// Path to installed packages root. Default: `./synx_packages`.
    std::optional<std::string> packages_path;
    /// Strict mode — fail on unknown markers, missing required env, etc.
    bool strict = false;
    /// Custom (non-builtin) markers. Looked up by marker name.
    std::unordered_map<std::string, MarkerFn> marker_fns;
    /// Internal counter — do not set manually.
    size_t _include_depth = 0;
};

} // namespace synx
