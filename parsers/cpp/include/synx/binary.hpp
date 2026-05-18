// .synxb compact binary serializer / deserializer. Wire-compatible with synx-core 3.6.x.
//
// Layout:
//   HEADER       5 bytes magic "SYNXB"
//                1 byte  version (currently 1)
//                1 byte  flags   (active/locked/has_meta/resolved/tool/schema/llm)
//   uncomp_size  4 bytes little-endian u32
//   payload      raw DEFLATE compressed bytes containing:
//                 - varint string table count + (varint len + UTF-8)*
//                 - root Value (tagged, recursive)
//                 - [metadata]   if HAS_META flag is set
//                 - [includes]   if HAS_META flag is set
//
// Requires zlib (SYNX_HAVE_ZLIB). Without zlib, compile/decompile return errors.
#pragma once

#include <cstddef>
#include <cstdint>
#include <vector>

#include "synx/parse_result.hpp"
#include "synx/result.hpp"

namespace synx {

/// True if `data` starts with the `.synxb` magic prefix.
bool is_synxb(const std::vector<uint8_t>& data) noexcept;
bool is_synxb(const uint8_t* data, size_t len) noexcept;

/// Compile a `ParseResult` into `.synxb` bytes. If `resolved` is true,
/// metadata and includes are stripped (post-engine output).
Result<std::vector<uint8_t>> compile(const ParseResult& result, bool resolved);

/// Decompile `.synxb` bytes back into a `ParseResult`.
Result<ParseResult> decompile(const std::vector<uint8_t>& data);
Result<ParseResult> decompile(const uint8_t* data, size_t len);

} // namespace synx
