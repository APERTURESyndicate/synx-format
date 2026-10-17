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

/// A float as Rust's `Value` Display writes it: `f64` Display (shortest
/// round-trip digits, plain decimal, never an exponent) with `.0` appended when
/// there is no `.` (`5.0`, `0.25`, `1000000000000000000000.0`).
std::string float_to_display_string(double f);

/// Convenience: returns a fresh JSON string.
std::string to_json(const Value& val);

} // namespace synx
