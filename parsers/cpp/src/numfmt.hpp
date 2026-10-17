// Numbers with Rust semantics: `str::parse` for i64 / u64 / f64, and the text
// Rust writes for f64 — Display (`{}`), `{:.N}`, `{:e}`, `{:.Ne}` — and the
// `ryu` layout the JSON projection uses.
#pragma once

#include <cstddef>
#include <cstdint>
#include <string>
#include <string_view>

namespace synx {
namespace detail {

/// Rust `i64::from_str`: optional `+`/`-`, ASCII digits, no overflow.
bool parse_i64(std::string_view s, int64_t& out) noexcept;

/// Rust `u64::from_str` (also `usize` / `u32` after a range check).
bool parse_u64(std::string_view s, uint64_t& out) noexcept;

/// Rust `f64::from_str`: `[+-]? (inf | infinity | nan | digits[.digits][e[+-]digits])`,
/// case-insensitive words, no whitespace, no hex. Overflow is ±inf, underflow ±0.
bool parse_f64(std::string_view s, double& out) noexcept;

/// Rust `f64` Display: shortest round-trip digits, plain decimal, never an
/// exponent; a whole number has no fraction (`5`, `-0`, `1e21` → `1000000000000000000000`).
std::string display_f64(double f);

/// Rust `format_number` (engine): a whole float below 2^63 prints as an integer,
/// anything else as Display.
std::string format_number(double n);

/// Rust `{:.prec$}` — exact decimal, rounded half to even.
std::string fixed_f64(double f, size_t prec);

/// Rust `{:e}` — shortest digits, `1.5e3`, `1e0`, `1e-7`.
std::string exp_f64(double f);

/// Rust `{:.prec$e}` — `prec` digits after the point, exponent as in `{:e}`.
std::string exp_prec_f64(double f, size_t prec);

/// Append `f` in `ryu::Buffer::format` layout (`1.0`, `0.001`, `1e-7`, `1.5e16`).
/// `f` must be finite.
void append_ryu(std::string& out, double f);

/// Append `n` in decimal.
void append_i64(std::string& out, int64_t n);
std::string i64_to_string(int64_t n);

/// Rust `f64 as i64`: saturating, NaN is 0.
int64_t saturating_i64(double f) noexcept;

/// Rust `f64::powi` (compiler-rt `__powidf2`: repeated squaring).
double powi(double base, int exp) noexcept;

/// Rust `f64::fract() == 0.0` (false for NaN and ±inf).
bool is_whole(double f) noexcept;

} // namespace detail
} // namespace synx
