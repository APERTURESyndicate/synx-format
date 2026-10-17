// Metadata paths shared by the parser and the engine (Rust engine.rs
// push_key_segment / push_item_segment / meta_path_segments).
#pragma once

#include <cstddef>
#include <string>
#include <string_view>
#include <vector>

#include "numfmt.hpp"

namespace synx {
namespace detail {

/// Append `key` as one path segment: `.` and `\` inside it are escaped with `\`,
/// so the object `a.b` and the object `b` inside `a` have different paths.
inline void push_key_segment(std::string& path, std::string_view key) {
    if (key.find_first_of(".\\") == std::string_view::npos) {
        path.append(key);
        return;
    }
    for (char c : key) {
        if (c == '.' || c == '\\') path.push_back('\\');
        path.push_back(c);
    }
}

/// `key` as one path segment.
inline std::string key_segment(std::string_view key) {
    std::string out;
    push_key_segment(out, key);
    return out;
}

/// Append the segment of list item `idx`: `.#<idx>` (a key never starts with `#`).
inline void push_item_segment(std::string& path, size_t idx) {
    path.append(".#");
    append_i64(path, static_cast<int64_t>(idx));
}

/// The unescaped segments of a path: `a\.b.c` is `["a.b", "c"]`.
inline std::vector<std::string> meta_path_segments(std::string_view path) {
    std::vector<std::string> out;
    if (path.empty()) return out;
    std::string seg;
    for (size_t i = 0; i < path.size(); ++i) {
        char c = path[i];
        if (c == '\\') {
            if (i + 1 < path.size()) seg.push_back(path[++i]);
        } else if (c == '.') {
            out.push_back(std::move(seg));
            seg.clear();
        } else {
            seg.push_back(c);
        }
    }
    out.push_back(std::move(seg));
    return out;
}

} // namespace detail
} // namespace synx
