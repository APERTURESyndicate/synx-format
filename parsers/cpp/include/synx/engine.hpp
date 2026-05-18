// SYNX `!active` engine — resolves markers, includes, packages, interpolation,
// constraints. Mirrors crates/synx-core/src/engine.rs.
#pragma once

#include "synx/parse_result.hpp"
#include "synx/options.hpp"

namespace synx {

/// Resolve `!active` markers and validate constraints in-place.
/// Safe to call on Static result (no-op). Honours `options.strict`.
void resolve(ParseResult& result, const Options& options);

} // namespace synx
