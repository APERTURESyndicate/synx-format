// Numbers with Rust semantics — see numfmt.hpp.
#include "numfmt.hpp"

#include <charconv>
#include <cmath>
#include <cstring>
#include <limits>
#include <system_error>

namespace synx {
namespace detail {

namespace {

inline bool is_digit(char c) noexcept { return c >= '0' && c <= '9'; }

bool eq_ignore_ascii_case(std::string_view a, std::string_view b) noexcept {
    if (a.size() != b.size()) return false;
    for (size_t i = 0; i < a.size(); ++i) {
        char x = a[i];
        if (x >= 'A' && x <= 'Z') x = static_cast<char>(x + 32);
        if (x != b[i]) return false;
    }
    return true;
}

// Shortest round-trip digits of finite, non-zero |f|: value = 0.<digits> × 10^point.
struct Shortest {
    char digits[24];
    int len = 0;
    int point = 0;
};

Shortest shortest(double f) noexcept {
    char buf[40];
    auto res = std::to_chars(buf, buf + sizeof(buf), std::fabs(f), std::chars_format::scientific);
    Shortest s;
    const char* p = buf;
    for (; p < res.ptr && *p != 'e'; ++p) {
        if (is_digit(*p)) s.digits[s.len++] = *p;
    }
    int exp = 0;
    std::from_chars(p + 1 + (p[1] == '+' ? 1 : 0), res.ptr, exp);
    while (s.len > 1 && s.digits[s.len - 1] == '0') --s.len;
    s.point = exp + 1;
    return s;
}

// `e+05` / `e-07` from to_chars → Rust's `e5` / `e-7`.
void append_rust_exponent(std::string& out, const char* e, const char* end) {
    out.push_back('e');
    const char* p = e + 1;
    if (p < end && *p == '-') { out.push_back('-'); ++p; }
    else if (p < end && *p == '+') { ++p; }
    while (p + 1 < end && *p == '0') ++p;
    out.append(p, end);
}

bool non_finite(double f, std::string& out) {
    if (std::isnan(f)) { out = "NaN"; return true; }
    if (std::isinf(f)) { out = f < 0 ? "-inf" : "inf"; return true; }
    return false;
}

} // namespace

bool parse_i64(std::string_view s, int64_t& out) noexcept {
    size_t i = 0;
    bool neg = false;
    if (!s.empty() && (s[0] == '-' || s[0] == '+')) {
        neg = s[0] == '-';
        i = 1;
    }
    if (i >= s.size()) return false;
    uint64_t v = 0;
    const uint64_t limit = neg ? 9223372036854775808ull : 9223372036854775807ull;
    for (; i < s.size(); ++i) {
        if (!is_digit(s[i])) return false;
        uint64_t d = static_cast<uint64_t>(s[i] - '0');
        if (v > (limit - d) / 10) return false;
        v = v * 10 + d;
    }
    out = neg ? static_cast<int64_t>(0 - v) : static_cast<int64_t>(v);
    return true;
}

bool parse_u64(std::string_view s, uint64_t& out) noexcept {
    size_t i = (!s.empty() && s[0] == '+') ? 1 : 0;
    if (i >= s.size()) return false;
    uint64_t v = 0;
    for (; i < s.size(); ++i) {
        if (!is_digit(s[i])) return false;
        uint64_t d = static_cast<uint64_t>(s[i] - '0');
        if (v > (UINT64_MAX - d) / 10) return false;
        v = v * 10 + d;
    }
    out = v;
    return true;
}

bool parse_f64(std::string_view s, double& out) noexcept {
    size_t i = 0;
    bool neg = false;
    if (!s.empty() && (s[0] == '-' || s[0] == '+')) {
        neg = s[0] == '-';
        i = 1;
    }
    std::string_view rest = s.substr(i);
    if (eq_ignore_ascii_case(rest, "inf") || eq_ignore_ascii_case(rest, "infinity")) {
        out = neg ? -std::numeric_limits<double>::infinity() : std::numeric_limits<double>::infinity();
        return true;
    }
    if (eq_ignore_ascii_case(rest, "nan")) {
        out = std::numeric_limits<double>::quiet_NaN();
        return true;
    }
    // Validate the decimal grammar and rebuild it in a shape every from_chars takes.
    size_t p = 0;
    size_t int_start = p;
    while (p < rest.size() && is_digit(rest[p])) ++p;
    std::string_view int_part = rest.substr(int_start, p - int_start);
    std::string_view frac_part;
    if (p < rest.size() && rest[p] == '.') {
        size_t fs = ++p;
        while (p < rest.size() && is_digit(rest[p])) ++p;
        frac_part = rest.substr(fs, p - fs);
    }
    if (int_part.empty() && frac_part.empty()) return false;
    std::string_view exp_part;
    if (p < rest.size() && (rest[p] == 'e' || rest[p] == 'E')) {
        size_t es = ++p;
        if (p < rest.size() && (rest[p] == '+' || rest[p] == '-')) ++p;
        size_t ds = p;
        while (p < rest.size() && is_digit(rest[p])) ++p;
        if (p == ds) return false;
        exp_part = rest.substr(es, p - es);
    }
    if (p != rest.size()) return false;

    std::string norm;
    norm.reserve(rest.size() + 4);
    norm.append(int_part.empty() ? std::string_view("0") : int_part);
    if (!frac_part.empty()) {
        norm.push_back('.');
        norm.append(frac_part);
    }
    if (!exp_part.empty()) {
        norm.push_back('e');
        norm.append(exp_part[0] == '+' ? exp_part.substr(1) : exp_part);
    }
    double v = 0.0;
    auto res = std::from_chars(norm.data(), norm.data() + norm.size(), v, std::chars_format::general);
    if (res.ec == std::errc::result_out_of_range) {
        // Overflow is ±inf, underflow ±0 (Rust never fails on range). Decide by
        // where the first significant digit sits relative to the decimal point.
        long long exp10 = 0;
        if (!exp_part.empty()) {
            std::string_view e = exp_part[0] == '+' ? exp_part.substr(1) : exp_part;
            bool eneg = !e.empty() && e[0] == '-';
            if (eneg) e.remove_prefix(1);
            for (char c : e) exp10 = exp10 > 100000 ? exp10 : exp10 * 10 + (c - '0');
            if (eneg) exp10 = -exp10;
        }
        std::string all = std::string(int_part) + std::string(frac_part);
        size_t z = all.find_first_not_of('0');
        long long magnitude = static_cast<long long>(int_part.size()) + exp10 - static_cast<long long>(z);
        v = magnitude > 0 ? std::numeric_limits<double>::infinity() : 0.0;
    } else if (res.ec != std::errc() || res.ptr != norm.data() + norm.size()) {
        return false;
    }
    out = neg ? -v : v;
    return true;
}

std::string display_f64(double f) {
    std::string out;
    if (non_finite(f, out)) return out;
    if (f == 0.0) return std::signbit(f) ? "-0" : "0";
    Shortest s = shortest(f);
    if (f < 0) out.push_back('-');
    if (s.point <= 0) {
        out.append("0.");
        out.append(static_cast<size_t>(-s.point), '0');
        out.append(s.digits, static_cast<size_t>(s.len));
    } else if (s.point < s.len) {
        out.append(s.digits, static_cast<size_t>(s.point));
        out.push_back('.');
        out.append(s.digits + s.point, static_cast<size_t>(s.len - s.point));
    } else {
        out.append(s.digits, static_cast<size_t>(s.len));
        out.append(static_cast<size_t>(s.point - s.len), '0');
    }
    return out;
}

std::string format_number(double n) {
    if (is_whole(n) && std::fabs(n) < 9223372036854775808.0) {
        return i64_to_string(static_cast<int64_t>(n));
    }
    return display_f64(n);
}

std::string fixed_f64(double f, size_t prec) {
    std::string out;
    if (non_finite(f, out)) return out;
    out.resize(330 + prec);
    auto res = std::to_chars(&out[0], &out[0] + out.size(), f, std::chars_format::fixed,
                             static_cast<int>(prec));
    out.resize(static_cast<size_t>(res.ptr - out.data()));
    return out;
}

std::string exp_f64(double f) {
    std::string out;
    if (non_finite(f, out)) return out;
    if (f == 0.0) return std::signbit(f) ? "-0e0" : "0e0";
    char buf[40];
    auto res = std::to_chars(buf, buf + sizeof(buf), f, std::chars_format::scientific);
    const char* e = static_cast<const char*>(std::memchr(buf, 'e', static_cast<size_t>(res.ptr - buf)));
    out.append(buf, static_cast<size_t>(e - buf));
    append_rust_exponent(out, e, res.ptr);
    return out;
}

std::string exp_prec_f64(double f, size_t prec) {
    std::string out;
    if (non_finite(f, out)) return out;
    std::string buf(prec + 40, '\0');
    auto res = std::to_chars(&buf[0], &buf[0] + buf.size(), f, std::chars_format::scientific,
                             static_cast<int>(prec));
    const char* start = buf.data();
    const char* e = static_cast<const char*>(std::memchr(start, 'e', static_cast<size_t>(res.ptr - start)));
    out.append(start, static_cast<size_t>(e - start));
    append_rust_exponent(out, e, res.ptr);
    return out;
}

void append_ryu(std::string& out, double f) {
    if (f == 0.0) {
        out.append(std::signbit(f) ? "-0.0" : "0.0");
        return;
    }
    Shortest s = shortest(f);
    if (f < 0) out.push_back('-');
    const int len = s.len;
    const int kk = s.point;
    if (kk >= len && kk <= 16) {
        out.append(s.digits, static_cast<size_t>(len));
        out.append(static_cast<size_t>(kk - len), '0');
        out.append(".0");
    } else if (kk > 0 && kk <= 16) {
        out.append(s.digits, static_cast<size_t>(kk));
        out.push_back('.');
        out.append(s.digits + kk, static_cast<size_t>(len - kk));
    } else if (kk > -5 && kk <= 0) {
        out.append("0.");
        out.append(static_cast<size_t>(-kk), '0');
        out.append(s.digits, static_cast<size_t>(len));
    } else {
        out.push_back(s.digits[0]);
        if (len > 1) {
            out.push_back('.');
            out.append(s.digits + 1, static_cast<size_t>(len - 1));
        }
        out.push_back('e');
        append_i64(out, kk - 1);
    }
}

void append_i64(std::string& out, int64_t n) {
    char buf[24];
    auto res = std::to_chars(buf, buf + sizeof(buf), n);
    out.append(buf, res.ptr);
}

std::string i64_to_string(int64_t n) {
    std::string out;
    append_i64(out, n);
    return out;
}

int64_t saturating_i64(double f) noexcept {
    if (std::isnan(f)) return 0;
    if (f >= 9223372036854775807.0) return INT64_MAX;
    if (f <= -9223372036854775808.0) return INT64_MIN;
    return static_cast<int64_t>(f);
}

double powi(double base, int exp) noexcept {
    const bool recip = exp < 0;
    double r = 1.0;
    while (true) {
        if (exp & 1) r *= base;
        exp /= 2;
        if (exp == 0) break;
        base *= base;
    }
    return recip ? 1.0 / r : r;
}

bool is_whole(double f) noexcept {
    return std::isfinite(f) && std::trunc(f) == f;
}

} // namespace detail
} // namespace synx
