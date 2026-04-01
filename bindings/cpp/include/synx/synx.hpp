/**
 * SYNX C++ SDK — thin wrapper over the `synx-c` C FFI (Rust `synx-core` 3.6.x).
 * Same grammar, markers, `!tool`, `.synxb`, and canonical JSON as the Rust engine.
 *
 * Requires: C++17, `synx.h` on the include path, link `synx-c` (cdylib or staticlib).
 * See bindings/cpp/README.md.
 */
#pragma once

#include <synx.h>

#include <cstddef>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace synx {
namespace detail {

inline std::optional<std::string> take_cstring(char *p) noexcept {
    if (p == nullptr) {
        return std::nullopt;
    }
    std::string out(p);
    synx_free(p);
    return out;
}

} // namespace detail

/** Parse SYNX → canonical JSON string (static mode). */
inline std::optional<std::string> parse(std::string_view input) noexcept {
    std::string buf(input);
    return detail::take_cstring(synx_parse(buf.c_str()));
}

/** Parse with `!active` engine resolution. */
inline std::optional<std::string> parse_active(std::string_view input) noexcept {
    std::string buf(input);
    return detail::take_cstring(synx_parse_active(buf.c_str()));
}

/** JSON (UTF-8) → SYNX text. */
inline std::optional<std::string> stringify(std::string_view json_utf8) noexcept {
    std::string buf(json_utf8);
    return detail::take_cstring(synx_stringify(buf.c_str()));
}

/** Canonical SYNX reformat. */
inline std::optional<std::string> format(std::string_view input) noexcept {
    std::string buf(input);
    return detail::take_cstring(synx_format(buf.c_str()));
}

/** `!tool` reshape → JSON. */
inline std::optional<std::string> parse_tool(std::string_view input) noexcept {
    std::string buf(input);
    return detail::take_cstring(synx_parse_tool(buf.c_str()));
}

/** Structural diff of two SYNX documents → JSON. */
inline std::optional<std::string> diff(std::string_view a, std::string_view b) noexcept {
    std::string ca(a);
    std::string cb(b);
    return detail::take_cstring(synx_diff(ca.c_str(), cb.c_str()));
}

/** Compile SYNX → `.synxb` bytes. */
inline std::optional<std::vector<unsigned char>>
compile(std::string_view input, bool resolved = false) noexcept {
    std::string buf(input);
    size_t len = 0;
    unsigned char *p = synx_compile(buf.c_str(), resolved ? 1 : 0, &len);
    if (p == nullptr) {
        return std::nullopt;
    }
    std::vector<unsigned char> out(p, p + len);
    synx_free_bytes(p, len);
    return out;
}

/** Decompile `.synxb` → SYNX text. */
inline std::optional<std::string> decompile(const unsigned char *data,
                                            std::size_t len) noexcept {
    if (data == nullptr || len == 0) {
        return std::nullopt;
    }
    return detail::take_cstring(synx_decompile(data, len));
}

inline std::optional<std::string>
decompile(const std::vector<unsigned char> &bytes) noexcept {
    return decompile(bytes.data(), bytes.size());
}

/** True if buffer begins with `.synxb` magic. */
inline bool is_synxb(const unsigned char *data, std::size_t len) noexcept {
    if (data == nullptr) {
        return false;
    }
    return synx_is_synxb(data, len) != 0;
}

inline bool is_synxb(const std::vector<unsigned char> &bytes) noexcept {
    return is_synxb(bytes.data(), bytes.size());
}

} // namespace synx
