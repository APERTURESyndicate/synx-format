// Canonical JSON serializer. Parity with `write_json` / `to_json` in synx-core lib.rs.
#pragma once

#include <cstddef>
#include <string>

#include "synx/value.hpp"

namespace synx {

constexpr size_t kMaxJsonDepth = 128;

/// Write `val` as JSON to `out`. Sorted keys; strings escaped to JSON;
/// secrets rendered as the literal `"[SECRET]"`.
void write_json(std::string& out, const Value& val);

/// Convenience: returns a fresh JSON string.
std::string to_json(const Value& val);

} // namespace synx
