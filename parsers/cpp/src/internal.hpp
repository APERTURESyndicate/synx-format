// Internal declarations shared across the library's translation units.
#pragma once

#include <cstddef>
#include <cstdint>
#include <string>
#include <string_view>

#include <vector>

#include "synx/meta.hpp"
#include "synx/parse_result.hpp"
#include "synx/result.hpp"
#include "synx/value.hpp"

namespace synx {
namespace detail {

// ─── parser.cpp ──────────────────────────────────────────────────────────────

/// SYNX §8.3 automatic casting (Rust `parser::cast`).
Value cast_value(std::string_view val);

/// SYNX §8.3 typed casting (Rust `parser::cast_typed_with`, hints enabled).
Value cast_typed(std::string_view val, std::string_view hint);

/// The body of a `[…]` constraint block.
Constraints parse_constraints(std::string_view raw);

/// Cut an inline ` //` or ` #` comment and trailing whitespace off a value.
std::string_view strip_comment(std::string_view val) noexcept;

// ─── rng.cpp — Rust rng.rs: xorshift64, per-thread seed ──────────────────────

uint64_t next_u64() noexcept;
/// Reseed this thread's generator (0 is replaced, it is a fixed point).
void seed_rng(uint64_t seed) noexcept;
/// `[0, bound)`; 0 when `bound` is 0.
size_t random_usize(size_t bound) noexcept;
/// `[0, 1)` with 1/10 000 granularity.
double random_f64_01() noexcept;
/// `[0, 2 147 483 647)`.
int64_t random_i64() noexcept;
bool random_bool() noexcept;
/// UUID v4 text (non-cryptographic).
std::string generate_uuid();

// ─── synx_writer.cpp — SYNXL §14.1 block writer ──────────────────────────────

/// Write `key value` as SYNX at `indent` columns (Rust `synxl::write_synx_entry`).
void write_synx_entry(std::string& out, std::string_view key, const Value& value,
                      size_t indent, size_t depth);

// ─── binary.cpp — the `.synxb` payload, before / after DEFLATE ───────────────

std::vector<uint8_t> encode_synxb_payload(const ParseResult& result, bool resolved);
uint8_t synxb_flags(const ParseResult& result, bool resolved) noexcept;
Result<ParseResult> decode_synxb_payload(uint8_t flags, const uint8_t* payload, size_t len);

} // namespace detail
} // namespace synx
