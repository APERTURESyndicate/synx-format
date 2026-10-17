// Shared PRNG — port of crates/synx-core/src/rng.rs (xorshift64, per-thread seed).
// Not cryptographically secure; suitable for config-level randomness only.
#include "internal.hpp"

#include <chrono>
#include <cstdio>

namespace synx {
namespace detail {

namespace {

uint64_t initial_seed() noexcept {
    auto nanos = static_cast<uint64_t>(std::chrono::duration_cast<std::chrono::nanoseconds>(
        std::chrono::system_clock::now().time_since_epoch()).count());
    return nanos == 0 ? 0xcafedeadbeef1234ull : nanos;
}

uint64_t& state() noexcept {
    thread_local uint64_t s = initial_seed();
    return s;
}

} // namespace

uint64_t next_u64() noexcept {
    uint64_t& x = state();
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    return x;
}

void seed_rng(uint64_t seed) noexcept { state() = seed == 0 ? 0xcafedeadbeef1234ull : seed; }

size_t random_usize(size_t bound) noexcept {
    return bound == 0 ? 0 : static_cast<size_t>(next_u64() % bound);
}

double random_f64_01() noexcept { return static_cast<double>(next_u64() % 10000) / 10000.0; }

int64_t random_i64() noexcept { return static_cast<int64_t>(next_u64() % 2147483647ull); }

bool random_bool() noexcept { return next_u64() % 2 == 0; }

std::string generate_uuid() {
    uint64_t hi = next_u64();
    uint64_t lo = next_u64();
    uint64_t time_hi = ((hi >> 16) & 0x0FFF) | 0x4000;
    uint64_t clk_seq = ((lo >> 48) & 0x3FFF) | 0x8000;
    char buf[40];
    std::snprintf(buf, sizeof(buf), "%08x-%04x-%04x-%04x-%012llx",
                  static_cast<unsigned>(hi >> 32), static_cast<unsigned>((hi >> 16) & 0xFFFF),
                  static_cast<unsigned>(time_hi & 0xFFFF), static_cast<unsigned>(clk_seq & 0xFFFF),
                  static_cast<unsigned long long>(lo & 0x0000FFFFFFFFFFFFull));
    return buf;
}

} // namespace detail
} // namespace synx
