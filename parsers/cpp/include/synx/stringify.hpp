// Value → SYNX text. Parity with `serialize` / `format_primitive` in synx-core/src/lib.rs.
#pragma once

#include <string>

#include "synx/value.hpp"

namespace synx {

/// Serialize a Value back to SYNX text. Object keys are emitted in their
/// stored order (insertion-ordered Object means callers control determinism).
std::string stringify(const Value& val);

} // namespace synx
