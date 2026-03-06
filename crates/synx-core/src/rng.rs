//! Shared PRNG for synx-core — xorshift64, thread-local seed.
//!
//! Replaces the two independent copies that lived in parser.rs and engine.rs.
//! Not cryptographically secure; suitable for config-level randomness only.

use std::cell::Cell;
use std::time::SystemTime;

thread_local! {
    static SEED: Cell<u64> = Cell::new({
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        // 0 is a fixed point for xorshift64 — use a non-zero fallback.
        if nanos == 0 { 0xcafe_dead_beef_1234 } else { nanos }
    });
}

/// Advance the xorshift64 PRNG and return the next raw u64.
#[inline]
pub(crate) fn next_u64() -> u64 {
    SEED.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        x
    })
}

/// Random `usize` in `[0, bound)`.  Returns 0 if bound is 0.
#[inline]
pub(crate) fn random_usize(bound: usize) -> usize {
    if bound == 0 { return 0; }
    (next_u64() as usize) % bound
}

/// Random `f64` in `[0.0, 1.0)` with 1/10 000 granularity.
#[inline]
pub(crate) fn random_f64_01() -> f64 {
    (next_u64() % 10_000) as f64 / 10_000.0
}

/// Random `i64` in `[0, 2 147 483 647)`.
#[inline]
pub(crate) fn random_i64() -> i64 {
    (next_u64() % 2_147_483_647) as i64
}

/// Random `bool`.
#[inline]
pub(crate) fn random_bool() -> bool {
    next_u64() % 2 == 0
}

/// Generate a UUID v4 string (non-cryptographic).
pub(crate) fn generate_uuid() -> String {
    let hi = next_u64();
    let lo = next_u64();
    // Set version bits (4) and variant bits (10xx)
    let time_hi  = ((hi >> 16) & 0x0FFF) | 0x4000;
    let clk_seq  = ((lo >> 48) & 0x3FFF) | 0x8000;
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (hi >> 32) as u32,
        (hi >> 16) as u16,
        time_hi as u16,
        clk_seq as u16,
        lo & 0x0000_FFFF_FFFF_FFFF,
    )
}
