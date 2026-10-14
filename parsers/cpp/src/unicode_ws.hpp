// Unicode whitespace helpers shared by the parser and `:calc`.
// Same set as Rust `char::is_whitespace` (White_Space), which is what
// `str::trim` strips in synx-core.
#pragma once

#include <cstddef>
#include <string_view>

namespace synx {
namespace detail {

// Byte length of the Unicode whitespace character starting at `pos`, or 0.
// Same set as Rust `char::is_whitespace` (White_Space): SYNX §4 measures indent
// over Unicode whitespace, so U+00A0 / U+3000 etc. are indent and trim too.
inline size_t ws_len_at(std::string_view s, size_t pos) noexcept {
    auto b = [&](size_t k) { return static_cast<unsigned char>(s[pos + k]); };
    size_t rest = s.size() - pos;
    unsigned char c = b(0);
    if (c == 0x20 || (c >= 0x09 && c <= 0x0D)) return 1;
    if (c == 0xC2 && rest >= 2 && (b(1) == 0x85 || b(1) == 0xA0)) return 2;
    if (c == 0xE1 && rest >= 3 && b(1) == 0x9A && b(2) == 0x80) return 3;           // U+1680
    if (c == 0xE2 && rest >= 3) {
        if (b(1) == 0x80 && ((b(2) >= 0x80 && b(2) <= 0x8A)                          // U+2000..200A
                             || b(2) == 0xA8 || b(2) == 0xA9 || b(2) == 0xAF)) {     // U+2028/2029/202F
            return 3;
        }
        if (b(1) == 0x81 && b(2) == 0x9F) return 3;                                  // U+205F
    }
    if (c == 0xE3 && rest >= 3 && b(1) == 0x80 && b(2) == 0x80) return 3;           // U+3000
    return 0;
}

// Byte length of the Unicode whitespace character ending at `end`, or 0.
inline size_t ws_len_before(std::string_view s, size_t end) noexcept {
    for (size_t n = 1; n <= 3 && n <= end; ++n) {
        if (ws_len_at(s, end - n) == n) return n;
    }
    return 0;
}

inline std::string_view trim_end_view(std::string_view s) noexcept {
    size_t end = s.size();
    while (end > 0) {
        size_t n = ws_len_before(s, end);
        if (n == 0) break;
        end -= n;
    }
    return s.substr(0, end);
}

inline std::string_view trim_view(std::string_view s) noexcept {
    size_t start = 0;
    while (start < s.size()) {
        size_t n = ws_len_at(s, start);
        if (n == 0) break;
        start += n;
    }
    return trim_end_view(s.substr(start));
}

} // namespace detail
} // namespace synx
