// SYNX safe calculator — sandboxed +, -, *, /, %, parens for `:calc`.
// Parity with crates/synx-core/src/calc.rs.
#pragma once

#include <string>
#include <string_view>

namespace synx {

struct CalcResult {
    bool ok = false;
    double value = 0.0;
    std::string error;

    static CalcResult success(double v) { return {true, v, {}}; }
    static CalcResult failure(std::string msg) { return {false, 0.0, std::move(msg)}; }
};

/// Evaluate an arithmetic expression. All variable references must be substituted
/// with numeric literals before calling.
CalcResult safe_calc(std::string_view expr);

} // namespace synx
