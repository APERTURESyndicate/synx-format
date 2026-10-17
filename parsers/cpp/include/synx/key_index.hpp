// Hash index over the keys of an insertion-ordered entry list.
//
// It stores positions only — the keys stay in the entries themselves — so it
// never dangles when the entry buffer moves, and costs one allocation per
// rehash instead of one per key. Open addressing, linear probing, load ≤ 1/2.
#pragma once

#include <cstddef>
#include <cstdint>
#include <string_view>
#include <vector>

namespace synx {
namespace detail {

class KeyIndex {
public:
    static constexpr size_t npos = static_cast<size_t>(-1);

    /// Position of `key` in `entries` (indexed so far), or npos. `key_of(e)`
    /// returns an entry's key as a string_view.
    template <typename Entries, typename KeyOf>
    size_t find(const Entries& entries, std::string_view key, KeyOf key_of) const noexcept {
        if (slots_.empty()) return npos;
        const size_t mask = slots_.size() - 1;
        for (size_t s = hash(key) & mask;; s = (s + 1) & mask) {
            uint32_t v = slots_[s];
            if (v == 0) return npos;
            if (key_of(entries[v - 1]) == key) return v - 1;
        }
    }

    /// Index every entry past the ones already indexed (keys are distinct).
    template <typename Entries, typename KeyOf>
    void sync(const Entries& entries, KeyOf key_of) {
        const size_t n = entries.size();
        if (n * 2 > slots_.size()) {
            size_t cap = 32;
            while (cap < n * 2) cap *= 2;
            slots_.assign(cap, 0);
            count_ = 0;
        }
        const size_t mask = slots_.size() - 1;
        for (; count_ < n; ++count_) {
            size_t s = hash(key_of(entries[count_])) & mask;
            while (slots_[s] != 0) s = (s + 1) & mask;
            slots_[s] = static_cast<uint32_t>(count_ + 1);
        }
    }

    void clear() noexcept {
        slots_.clear();
        count_ = 0;
    }

    bool empty() const noexcept { return slots_.empty(); }

private:
    // FNV-1a from a per-process random basis: keys are mostly short, and the
    // basis keeps a document from being built to collide. Nothing observable
    // depends on it — iteration order is the entry order.
    static size_t hash(std::string_view s) noexcept {
        uint64_t h = basis();
        for (unsigned char c : s) {
            h ^= c;
            h *= 1099511628211ull;
        }
        return static_cast<size_t>(h ^ (h >> 29));
    }

    static uint64_t basis() noexcept {
        static const uint64_t b = [] {
            uint64_t local = 0;
            uint64_t x = reinterpret_cast<uintptr_t>(&local) ^ 14695981039346656037ull;
            x ^= x >> 33;
            x *= 0xff51afd7ed558ccdull;
            x ^= x >> 33;
            return x;
        }();
        return b;
    }

    std::vector<uint32_t> slots_;
    size_t count_ = 0;
};

} // namespace detail
} // namespace synx
