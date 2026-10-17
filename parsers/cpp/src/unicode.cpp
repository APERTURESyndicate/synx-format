// Unicode helpers — see unicode.hpp. Property tables come from Rust's std.
#include "unicode.hpp"

#include <algorithm>
#include <cstdint>

namespace synx {
namespace detail {

namespace {

#include "unicode_tables.inc"

template <size_t N>
bool in_ranges(const uint32_t (&ranges)[N][2], uint32_t cp) noexcept {
    size_t lo = 0;
    size_t hi = N;
    while (lo < hi) {
        size_t mid = (lo + hi) / 2;
        if (cp < ranges[mid][0]) hi = mid;
        else if (cp > ranges[mid][1]) lo = mid + 1;
        else return true;
    }
    return false;
}

bool is_cased(uint32_t cp) noexcept { return in_ranges(kCasedRanges, cp); }
bool is_case_ignorable(uint32_t cp) noexcept { return in_ranges(kCaseIgnorableRanges, cp); }

// Code point ending at byte `end` (input is valid UTF-8).
uint32_t decode_before(std::string_view s, size_t end, size_t& len) noexcept {
    size_t start = end - 1;
    while (start > 0 && (static_cast<unsigned char>(s[start]) & 0xC0) == 0x80) --start;
    return decode_utf8(s, start, len);
}

// Rust `case_ignorable_then_cased` over the text before `at`.
bool cased_before(std::string_view s, size_t at) noexcept {
    size_t end = at;
    while (end > 0) {
        size_t len = 0;
        uint32_t cp = decode_before(s, end, len);
        if (!is_case_ignorable(cp)) return is_cased(cp);
        end -= len;
    }
    return false;
}

// Rust `case_ignorable_then_cased` over the text from `at`.
bool cased_after(std::string_view s, size_t at) noexcept {
    size_t pos = at;
    while (pos < s.size()) {
        size_t len = 0;
        uint32_t cp = decode_utf8(s, pos, len);
        if (!is_case_ignorable(cp)) return is_cased(cp);
        pos += len;
    }
    return false;
}

} // namespace

uint32_t decode_utf8(std::string_view s, size_t pos, size_t& len) noexcept {
    const auto b = [&](size_t k) { return static_cast<uint32_t>(static_cast<unsigned char>(s[pos + k])); };
    uint32_t c = b(0);
    len = utf8_len(static_cast<unsigned char>(c));
    if (pos + len > s.size()) { len = 1; return c; }
    switch (len) {
        case 2: return ((c & 0x1F) << 6) | (b(1) & 0x3F);
        case 3: return ((c & 0x0F) << 12) | ((b(1) & 0x3F) << 6) | (b(2) & 0x3F);
        case 4: return ((c & 0x07) << 18) | ((b(1) & 0x3F) << 12) | ((b(2) & 0x3F) << 6) | (b(3) & 0x3F);
        default: return c;
    }
}

void push_utf8(std::string& out, uint32_t cp) {
    if (cp < 0x80) {
        out.push_back(static_cast<char>(cp));
    } else if (cp < 0x800) {
        out.push_back(static_cast<char>(0xC0 | (cp >> 6)));
        out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
    } else if (cp < 0x10000) {
        out.push_back(static_cast<char>(0xE0 | (cp >> 12)));
        out.push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
    } else {
        out.push_back(static_cast<char>(0xF0 | (cp >> 18)));
        out.push_back(static_cast<char>(0x80 | ((cp >> 12) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
    }
}

bool valid_utf8(std::string_view s) noexcept {
    size_t i = 0;
    while (i < s.size()) {
        unsigned char c = static_cast<unsigned char>(s[i]);
        if (c < 0x80) { ++i; continue; }
        size_t n = (c & 0xE0) == 0xC0 ? 2 : (c & 0xF0) == 0xE0 ? 3 : (c & 0xF8) == 0xF0 ? 4 : 0;
        if (n == 0 || i + n > s.size()) return false;
        uint32_t cp = n == 2 ? (c & 0x1F) : n == 3 ? (c & 0x0F) : (c & 0x07);
        for (size_t k = 1; k < n; ++k) {
            unsigned char cc = static_cast<unsigned char>(s[i + k]);
            if ((cc & 0xC0) != 0x80) return false;
            cp = (cp << 6) | (cc & 0x3F);
        }
        if ((n == 2 && cp < 0x80) || (n == 3 && cp < 0x800) || (n == 4 && (cp < 0x10000 || cp > 0x10FFFF))
            || (cp >= 0xD800 && cp <= 0xDFFF)) {
            return false;
        }
        i += n;
    }
    return true;
}

size_t char_count(std::string_view s) noexcept {
    size_t n = 0;
    for (unsigned char c : s) {
        if ((c & 0xC0) != 0x80) ++n;
    }
    return n;
}

bool is_alphanumeric(uint32_t cp) noexcept {
    if (cp < 0x80) {
        return (cp >= '0' && cp <= '9') || (cp >= 'a' && cp <= 'z') || (cp >= 'A' && cp <= 'Z');
    }
    return in_ranges(kAlnumRanges, cp);
}

std::string to_lowercase(std::string_view s) {
    std::string out;
    out.reserve(s.size());
    size_t pos = 0;
    while (pos < s.size()) {
        unsigned char c = static_cast<unsigned char>(s[pos]);
        if (c < 0x80) {
            out.push_back(static_cast<char>(c >= 'A' && c <= 'Z' ? c + 32 : c));
            ++pos;
            continue;
        }
        size_t len = 0;
        uint32_t cp = decode_utf8(s, pos, len);
        if (cp == 0x3A3) {
            // Σ is ς at the end of a word (Unicode Final_Sigma), σ elsewhere.
            bool final = cased_before(s, pos) && !cased_after(s, pos + len);
            push_utf8(out, final ? 0x3C2 : 0x3C3);
        } else {
            const auto* end = std::end(kLowerMap);
            const auto* it = std::lower_bound(std::begin(kLowerMap), end, cp,
                [](const uint32_t (&e)[3], uint32_t v) { return e[0] < v; });
            if (it != end && (*it)[0] == cp) {
                push_utf8(out, (*it)[1]);
                if ((*it)[2] != 0) push_utf8(out, (*it)[2]);
            } else {
                out.append(s.substr(pos, len));
            }
        }
        pos += len;
    }
    return out;
}

} // namespace detail
} // namespace synx
