// SYNX metadata — markers, args, constraints. Parity with Rust `Meta` / `Constraints`.
#pragma once

#include <cstddef>
#include <cstdint>
#include <functional>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "synx/key_index.hpp"

namespace synx {

/// Insertion-ordered string-keyed map — Rust `indexmap::IndexMap`.
///
/// Iteration follows insertion order; assigning to an existing key keeps its
/// position; `erase` shifts the later entries down. Small maps are searched
/// linearly; past `kIndexThreshold` entries a hash index of positions is kept.
template <typename V>
class IndexMap {
public:
    using value_type = std::pair<std::string, V>;
    using iterator = typename std::vector<value_type>::iterator;
    using const_iterator = typename std::vector<value_type>::const_iterator;

    IndexMap() = default;
    IndexMap(const IndexMap& o) : entries_(o.entries_) {}
    IndexMap(IndexMap&& o) noexcept : entries_(std::move(o.entries_)), index_(std::move(o.index_)) {
        o.entries_.clear();
        o.index_.clear();
    }
    IndexMap& operator=(const IndexMap& o) {
        if (this != &o) {
            entries_ = o.entries_;
            index_.clear();
        }
        return *this;
    }
    IndexMap& operator=(IndexMap&& o) noexcept {
        if (this != &o) {
            entries_ = std::move(o.entries_);
            index_ = std::move(o.index_);
            o.entries_.clear();
            o.index_.clear();
        }
        return *this;
    }

    iterator begin() noexcept { return entries_.begin(); }
    iterator end() noexcept { return entries_.end(); }
    const_iterator begin() const noexcept { return entries_.begin(); }
    const_iterator end() const noexcept { return entries_.end(); }
    size_t size() const noexcept { return entries_.size(); }
    bool empty() const noexcept { return entries_.empty(); }
    void clear() noexcept {
        entries_.clear();
        index_.clear();
    }

    /// Position of `key`, or `npos`.
    size_t index_of(std::string_view key) const noexcept {
        if (entries_.size() <= kIndexThreshold) {
            for (size_t i = 0; i < entries_.size(); ++i) {
                if (entries_[i].first == key) return i;
            }
            return npos;
        }
        auto key_of = [](const value_type& e) { return std::string_view(e.first); };
        index_.sync(entries_, key_of);
        size_t at = index_.find(entries_, key, key_of);
        return at == detail::KeyIndex::npos ? npos : at;
    }

    iterator find(std::string_view key) noexcept {
        size_t i = index_of(key);
        return i == npos ? entries_.end() : entries_.begin() + static_cast<std::ptrdiff_t>(i);
    }
    const_iterator find(std::string_view key) const noexcept {
        size_t i = index_of(key);
        return i == npos ? entries_.end() : entries_.begin() + static_cast<std::ptrdiff_t>(i);
    }
    size_t count(std::string_view key) const noexcept { return index_of(key) == npos ? 0 : 1; }
    bool contains(std::string_view key) const noexcept { return index_of(key) != npos; }

    V* get(std::string_view key) noexcept {
        size_t i = index_of(key);
        return i == npos ? nullptr : &entries_[i].second;
    }
    const V* get(std::string_view key) const noexcept {
        size_t i = index_of(key);
        return i == npos ? nullptr : &entries_[i].second;
    }

    /// The value for `key` — which must be present.
    V& at(std::string_view key) noexcept { return entries_[index_of(key)].second; }
    const V& at(std::string_view key) const noexcept { return entries_[index_of(key)].second; }

    /// The value for `key`, appended default-constructed when absent.
    V& operator[](std::string_view key) {
        size_t i = index_of(key);
        if (i != npos) return entries_[i].second;
        return push(std::string(key), V{})->second;
    }

    /// Insert when absent; an existing entry is left as it is.
    std::pair<iterator, bool> emplace(std::string key, V value) {
        size_t i = index_of(key);
        if (i != npos) return {entries_.begin() + static_cast<std::ptrdiff_t>(i), false};
        return {push(std::move(key), std::move(value)), true};
    }

    /// Insert or replace; a replaced entry keeps its position (`IndexMap::insert`).
    V& insert_or_assign(std::string key, V value) {
        size_t i = index_of(key);
        if (i != npos) {
            entries_[i].second = std::move(value);
            return entries_[i].second;
        }
        return push(std::move(key), std::move(value))->second;
    }

    /// Remove `key`, shifting later entries down (`IndexMap::shift_remove`).
    bool erase(std::string_view key) {
        size_t i = index_of(key);
        if (i == npos) return false;
        entries_.erase(entries_.begin() + static_cast<std::ptrdiff_t>(i));
        reindex();
        return true;
    }

    /// Keep only the entries for which `keep(key, value)` is true, in order.
    template <typename Pred>
    void retain(Pred keep) {
        size_t w = 0;
        for (size_t r = 0; r < entries_.size(); ++r) {
            if (keep(entries_[r].first, entries_[r].second)) {
                if (w != r) entries_[w] = std::move(entries_[r]);
                ++w;
            }
        }
        if (w != entries_.size()) {
            entries_.erase(entries_.begin() + static_cast<std::ptrdiff_t>(w), entries_.end());
            reindex();
        }
    }

    static constexpr size_t npos = static_cast<size_t>(-1);

private:
    static constexpr size_t kIndexThreshold = 12;

    iterator push(std::string key, V value) {
        entries_.emplace_back(std::move(key), std::move(value));
        return entries_.end() - 1;
    }

    void reindex() noexcept { index_.clear(); }

    std::vector<value_type> entries_;
    // Positions of `entries_`, synced lazily on lookup; cleared when entries move down.
    mutable detail::KeyIndex index_;
};

/// Constraints from `[min:3, max:30, required, type:int, pattern:^\d+$, enum:[a,b], readonly]`.
struct Constraints {
    std::optional<double> min;
    std::optional<double> max;
    std::optional<std::string> type_name;
    bool required = false;
    bool readonly = false;
    std::optional<std::string> pattern;
    std::optional<std::vector<std::string>> enum_values;

    bool has_any() const noexcept {
        return min.has_value() || max.has_value() || type_name.has_value() || required
            || readonly || pattern.has_value() || enum_values.has_value();
    }
};

/// Metadata attached to a single key.
struct Meta {
    std::vector<std::string> markers;
    /// Arguments taken from the value: `:random` weights, the `:inherit` parent.
    std::vector<std::string> args;
    std::optional<std::string> type_hint;
    std::optional<Constraints> constraints;

    bool has_marker(std::string_view name) const noexcept {
        for (const auto& m : markers) {
            if (m == name) return true;
        }
        return false;
    }

    int marker_index(std::string_view name) const noexcept {
        for (size_t i = 0; i < markers.size(); ++i) {
            if (markers[i] == name) return static_cast<int>(i);
        }
        return -1;
    }
};

/// `key -> Meta` for one object, in declaration order.
using MetaMap = IndexMap<Meta>;

/// Whole-tree metadata, keyed by the dot-path of the object that holds the keys:
/// `""` is the root, `server.db` a nested object, `servers.#2` the third item of
/// the list `servers`. A `.` or `\` inside a key is escaped with `\` (`a\.b`).
using MetadataTree = IndexMap<MetaMap>;

} // namespace synx
