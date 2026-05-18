// SYNX metadata — markers, args, constraints. Parity with Rust `Meta` / `Constraints`.
#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <vector>
#include <unordered_map>

namespace synx {

/// Constraints from `[min:3, max:30, required, type:int, pattern:^\d+$, enum:[a,b], readonly]`.
struct Constraints {
    std::optional<double> min;
    std::optional<double> max;
    std::optional<std::string> type_name;
    bool required = false;
    bool readonly = false;
    std::optional<std::string> pattern;
    std::optional<std::vector<std::string>> enum_values;

    bool has_any() const noexcept {
        return min.has_value() || max.has_value() || type_name.has_value() || required
            || readonly || pattern.has_value() || enum_values.has_value();
    }
};

/// Metadata attached to a single key.
struct Meta {
    std::vector<std::string> markers;
    /// One arg list per marker (same length as `markers`).
    std::vector<std::string> args;
    std::optional<std::string> type_hint;
    std::optional<Constraints> constraints;

    bool has_marker(const std::string& name) const noexcept {
        for (const auto& m : markers) {
            if (m == name) {
                return true;
            }
        }
        return false;
    }

    int marker_index(const std::string& name) const noexcept {
        for (size_t i = 0; i < markers.size(); ++i) {
            if (markers[i] == name) {
                return static_cast<int>(i);
            }
        }
        return -1;
    }
};

/// Map of `key -> Meta` for one object nesting level. Keyed by simple key name.
using MetaMap = std::unordered_map<std::string, Meta>;

/// Whole-tree metadata, keyed by dot-path prefix.
/// `""` = root level, `"server"` = `server.*` keys, `"server.db"` = nested, etc.
using MetadataTree = std::unordered_map<std::string, MetaMap>;

} // namespace synx
