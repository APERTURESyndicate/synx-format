// Unicode helpers with Rust `char` / `str` semantics: White_Space trimming and
// indentation (SYNX §4), UTF-8 stepping, `char::is_alphanumeric` and
// `str::to_lowercase`.
#pragma once

#include <cstddef>
#include <cstdint>
#include <string>
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
    if (c < 0x80) return 0;
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
    if (end == 0) return 0;
    unsigned char last = static_cast<unsigned char>(s[end - 1]);
    if (last < 0x80) return ws_len_at(s, end - 1);
    for (size_t n = 2; n <= 3 && n <= end; ++n) {
        if (ws_len_at(s, end - n) == n) return n;
    }
    return 0;
}

inline bool starts_with(std::string_view s, std::string_view prefix) noexcept {
    return s.size() >= prefix.size() && s.compare(0, prefix.size(), prefix) == 0;
}

inline bool ends_with(std::string_view s, std::string_view suffix) noexcept {
    return s.size() >= suffix.size() && s.compare(s.size() - suffix.size(), suffix.size(), suffix) == 0;
}

inline bool is_ascii_ws(unsigned char c) noexcept { return c == 0x20 || (c >= 0x09 && c <= 0x0D); }

/// Rust `str::trim_start`.
inline std::string_view trim_start_view(std::string_view s) noexcept {
    size_t start = 0;
    while (start < s.size()) {
        size_t n = ws_len_at(s, start);
        if (n == 0) break;
        start += n;
    }
    return s.substr(start);
}

/// Rust `str::trim_end`.
inline std::string_view trim_end_view(std::string_view s) noexcept {
    size_t end = s.size();
    while (end > 0) {
        size_t n = ws_len_before(s, end);
        if (n == 0) break;
        end -= n;
    }
    return s.substr(0, end);
}

/// Rust `str::trim`, with an ASCII fast path at both edges.
inline std::string_view trim_view(std::string_view s) noexcept {
    size_t start = 0;
    size_t end = s.size();
    while (start < end && is_ascii_ws(static_cast<unsigned char>(s[start]))) ++start;
    if (start < end && static_cast<unsigned char>(s[start]) >= 0x80) {
        return trim_end_view(trim_start_view(s.substr(start)));
    }
    while (end > start && is_ascii_ws(static_cast<unsigned char>(s[end - 1]))) --end;
    if (end > start && static_cast<unsigned char>(s[end - 1]) >= 0x80) {
        return trim_end_view(s.substr(start, end - start));
    }
    return s.substr(start, end - start);
}

/// Byte length of the leading Unicode whitespace (the SYNX indentation).
inline size_t indent_len(std::string_view s) noexcept {
    size_t i = 0;
    while (i < s.size() && is_ascii_ws(static_cast<unsigned char>(s[i]))) ++i;
    if (i < s.size() && static_cast<unsigned char>(s[i]) >= 0x80) {
        return s.size() - trim_start_view(s).size();
    }
    return i;
}

/// Byte offset of the first character at or past `strip` bytes of leading
/// whitespace, never inside a character (Rust `strip_boundary`).
inline size_t strip_boundary(std::string_view s, size_t strip) noexcept {
    size_t off = 0;
    while (off < strip && off < s.size()) {
        size_t n = ws_len_at(s, off);
        if (n == 0) break;
        off += n;
    }
    return off;
}

/// Longest prefix of `s` that fits in `limit` bytes without splitting a character.
inline size_t utf8_prefix_len(std::string_view s, size_t limit) noexcept {
    if (s.size() <= limit) return s.size();
    size_t end = limit;
    while (end > 0 && (static_cast<unsigned char>(s[end]) & 0xC0) == 0x80) --end;
    return end;
}

/// Byte length of the UTF-8 sequence that starts with `lead` (1 for a stray byte).
inline size_t utf8_len(unsigned char lead) noexcept {
    if (lead < 0x80) return 1;
    if ((lead & 0xE0) == 0xC0) return 2;
    if ((lead & 0xF0) == 0xE0) return 3;
    if ((lead & 0xF8) == 0xF0) return 4;
    return 1;
}

/// The code point at `pos`; `len` receives its byte length (input is valid UTF-8).
uint32_t decode_utf8(std::string_view s, size_t pos, size_t& len) noexcept;

/// Append `cp` as UTF-8.
void push_utf8(std::string& out, uint32_t cp);

/// Is `s` well-formed UTF-8 (no overlongs, surrogates or code points past U+10FFFF)?
bool valid_utf8(std::string_view s) noexcept;

/// Number of Unicode scalar values (Rust `str::chars().count()`).
size_t char_count(std::string_view s) noexcept;

/// Rust `char::is_alphanumeric` (Alphabetic or Numeric).
bool is_alphanumeric(uint32_t cp) noexcept;

/// Rust `str::to_lowercase`, including the Final_Sigma rule for U+03A3.
std::string to_lowercase(std::string_view s);

/// Rust `split_whitespace`: `each` gets every run between Unicode whitespace.
template <typename F>
void split_whitespace(std::string_view s, F&& each) {
    size_t i = 0;
    while (i < s.size()) {
        size_t n = ws_len_at(s, i);
        if (n != 0) { i += n; continue; }
        size_t start = i;
        while (i < s.size() && ws_len_at(s, i) == 0) i += utf8_len(static_cast<unsigned char>(s[i]));
        each(s.substr(start, i - start));
    }
}

} // namespace detail
} // namespace synx
