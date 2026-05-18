// Structural diff between two SYNX values. Parity with synx-core/src/diff.rs.
#pragma once

#include <string>
#include <vector>

#include "synx/value.hpp"

namespace synx {

struct DiffChange {
    Value from;
    Value to;
};

struct DiffResult {
    Object added;
    Object removed;
    std::vector<std::pair<std::string, DiffChange>> changed;
    std::vector<std::string> unchanged;
};

/// Compute a structural diff between two SYNX objects (top-level keys).
DiffResult diff(const Object& a, const Object& b);

/// Convert a DiffResult into an Object suitable for JSON serialisation.
Value diff_to_value(const DiffResult& d);

} // namespace synx
