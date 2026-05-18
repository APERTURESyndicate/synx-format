// SYNX text-to-tree parser. Parity with crates/synx-core/src/parser.rs.
#pragma once

#include <cstddef>
#include <string_view>

#include "synx/parse_result.hpp"

namespace synx {

// ─── Resource limits (fuzz / hostile input) ──────────────────────────────────
constexpr size_t kMaxSynxInputBytes      = 16ull * 1024 * 1024;
constexpr size_t kMaxLineStarts          = 2'000'000;
constexpr size_t kMaxParseNestingDepth   = 128;
constexpr size_t kMaxMultilineBlockBytes = 1024 * 1024;
constexpr size_t kMaxListItems           = 1u << 20;
constexpr size_t kMaxIncludeDirectives   = 4096;
constexpr size_t kMaxConstraintEnumParts = 4096;
constexpr size_t kMaxMarkerChainSegments = 512;

/// Truncate to a UTF-8-safe prefix (used by `parse` and canonical `format`).
std::string_view clamp_synx_text(std::string_view text) noexcept;

/// Parse a SYNX text string into a value tree with metadata.
ParseResult parse(std::string_view text);

/// Reshape parsed tree for `!tool` mode.
///
/// Call mode (`!tool`):  `{ tool: "name", params: { ... } }`
/// Schema mode (`!tool` + `!schema`): `{ tools: [ { name, params }, ... ] }`
Value reshape_tool_output(const Value& root, bool schema);

} // namespace synx
