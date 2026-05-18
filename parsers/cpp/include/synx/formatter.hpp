// Canonical SYNX text reformatter. Parity with `fmt_canonical` in synx-core/src/lib.rs.
#pragma once

#include <string>
#include <string_view>

namespace synx {

constexpr size_t kMaxFmtParseDepth = 128;

/// Reformat `.synx` text into canonical form:
///   * Keys sorted alphabetically at every nesting level
///   * 2-space indentation
///   * One blank line between top-level blocks
///   * Comments stripped
///   * Directives (`!active`, `!lock`, etc.) preserved at the top
std::string format(std::string_view text);

} // namespace synx
