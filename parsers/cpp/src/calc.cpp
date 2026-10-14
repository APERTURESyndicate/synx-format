// Safe arithmetic evaluator for `:calc`. Byte-for-byte port of crates/synx-core/src/calc.rs,
// including its error messages (`SYNX :calc — …`, with the em dash).
#include "synx/calc.hpp"
#include "unicode_ws.hpp"

#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <string>
#include <vector>

namespace synx {

namespace {

// "SYNX :calc " + U+2014 (EM DASH) + " "
constexpr const char* kPrefix = "SYNX :calc \xE2\x80\x94 ";

enum class TokenKind : uint8_t { Number, Op, LParen, RParen };

struct Token {
    TokenKind kind;
    double number = 0.0;
    unsigned char op = 0;
};

inline bool is_digit(char c) noexcept { return c >= '0' && c <= '9'; }

// Rust `ch as char`: one byte widened to U+00xx, then UTF-8 encoded.
std::string widen_byte(unsigned char b) {
    std::string out;
    if (b < 0x80) {
        out.push_back(static_cast<char>(b));
    } else {
        out.push_back(static_cast<char>(0xC0 | (b >> 6)));
        out.push_back(static_cast<char>(0x80 | (b & 0x3F)));
    }
    return out;
}

// Rust `str::parse::<f64>` for the shapes the tokenizer can produce: an optional
// `-`, digits and dots, at least one digit, at most one dot (`5.`, `.5`, `-.5` are fine).
bool parse_number(const std::string& s, double& out) {
    size_t i = 0;
    bool neg = false;
    if (i < s.size() && s[i] == '-') { neg = true; ++i; }
    std::string int_part, frac_part;
    bool seen_dot = false;
    for (; i < s.size(); ++i) {
        if (s[i] == '.') {
            if (seen_dot) return false;
            seen_dot = true;
        } else if (seen_dot) {
            frac_part.push_back(s[i]);
        } else {
            int_part.push_back(s[i]);
        }
    }
    if (int_part.empty() && frac_part.empty()) return false;
    std::string norm = (int_part.empty() ? "0" : int_part) + "." + (frac_part.empty() ? "0" : frac_part);
    out = std::strtod(norm.c_str(), nullptr);
    if (neg) out = -out;
    return true;
}

bool tokenize(std::string_view expr, std::vector<Token>& out, std::string& err) {
    size_t i = 0;
    const size_t len = expr.size();
    out.reserve(16);

    while (i < len) {
        char ch = expr[i];
        if (ch == ' ' || ch == '\t') {
            ++i;
            continue;
        }

        const bool number_start =
            is_digit(ch)
            || (ch == '.' && i + 1 < len && is_digit(expr[i + 1]))
            || (ch == '-'
                && (out.empty()
                    || out.back().kind == TokenKind::Op
                    || out.back().kind == TokenKind::LParen));

        if (number_start) {
            size_t start = i;
            if (ch == '-') ++i;
            while (i < len && (is_digit(expr[i]) || expr[i] == '.')) {
                ++i;
            }
            std::string num_str(expr.substr(start, i - start));
            double v = 0.0;
            if (!parse_number(num_str, v)) {
                err = std::string(kPrefix) + "invalid number: '" + num_str + "'";
                return false;
            }
            out.push_back(Token{TokenKind::Number, v, 0});
            continue;
        }

        if (ch == '+' || ch == '-' || ch == '*' || ch == '/' || ch == '%') {
            out.push_back(Token{TokenKind::Op, 0.0, static_cast<unsigned char>(ch)});
            ++i;
            continue;
        }
        if (ch == '(') {
            out.push_back(Token{TokenKind::LParen, 0.0, 0});
            ++i;
            continue;
        }
        if (ch == ')') {
            out.push_back(Token{TokenKind::RParen, 0.0, 0});
            ++i;
            continue;
        }

        err = std::string(kPrefix) + "unexpected character: '"
            + widen_byte(static_cast<unsigned char>(ch)) + "' in expression";
        return false;
    }
    return true;
}

class ExprParser {
public:
    explicit ExprParser(std::vector<Token> toks) : tokens_(std::move(toks)) {}

    bool parse(double& result, std::string& err) {
        if (!expr(result, err)) return false;
        if (pos_ < tokens_.size()) {
            err = std::string(kPrefix) + "unexpected token at position " + std::to_string(pos_);
            return false;
        }
        return true;
    }

private:
    bool is_op(unsigned char c) const {
        return tokens_[pos_].kind == TokenKind::Op && tokens_[pos_].op == c;
    }

    bool expr(double& out, std::string& err) {
        double left = 0.0;
        if (!term(left, err)) return false;
        while (pos_ < tokens_.size()) {
            if (is_op('+')) {
                ++pos_;
                double right = 0.0;
                if (!term(right, err)) return false;
                left += right;
            } else if (is_op('-')) {
                ++pos_;
                double right = 0.0;
                if (!term(right, err)) return false;
                left -= right;
            } else {
                break;
            }
        }
        out = left;
        return true;
    }

    bool term(double& out, std::string& err) {
        double left = 0.0;
        if (!factor(left, err)) return false;
        while (pos_ < tokens_.size()) {
            if (is_op('*')) {
                ++pos_;
                double right = 0.0;
                if (!factor(right, err)) return false;
                left *= right;
            } else if (is_op('/')) {
                ++pos_;
                double right = 0.0;
                if (!factor(right, err)) return false;
                if (right == 0.0) {
                    err = std::string(kPrefix) + "division by zero";
                    return false;
                }
                left /= right;
            } else if (is_op('%')) {
                ++pos_;
                double right = 0.0;
                if (!factor(right, err)) return false;
                if (right == 0.0) {
                    err = std::string(kPrefix) + "division by zero";
                    return false;
                }
                left = std::fmod(left, right);
            } else {
                break;
            }
        }
        out = left;
        return true;
    }

    bool factor(double& out, std::string& err) {
        if (pos_ >= tokens_.size()) {
            err = std::string(kPrefix) + "unexpected end of expression";
            return false;
        }
        const Token& t = tokens_[pos_];
        if (t.kind == TokenKind::Number) {
            out = t.number;
            ++pos_;
            return true;
        }
        if (t.kind == TokenKind::LParen) {
            ++pos_;
            double v = 0.0;
            if (!expr(v, err)) return false;
            if (pos_ >= tokens_.size() || tokens_[pos_].kind != TokenKind::RParen) {
                err = std::string(kPrefix) + "missing closing parenthesis";
                return false;
            }
            ++pos_;
            out = v;
            return true;
        }
        // Mirrors Rust's `{:?}` of the offending token.
        if (t.kind == TokenKind::Op) {
            err = std::string(kPrefix) + "unexpected token: Op(" + std::to_string(static_cast<unsigned>(t.op)) + ")";
        } else {
            err = std::string(kPrefix) + "unexpected token: RParen";
        }
        return false;
    }

    std::vector<Token> tokens_;
    size_t pos_ = 0;
};

} // namespace

CalcResult safe_calc(std::string_view expr) {
    // Unicode trim, as Rust `str::trim` in `safe_calc`.
    std::string_view trimmed = detail::trim_view(expr);
    if (trimmed.empty()) {
        return CalcResult::success(0.0);
    }

    std::vector<Token> tokens;
    std::string err;
    if (!tokenize(trimmed, tokens, err)) {
        return CalcResult::failure(std::move(err));
    }
    if (tokens.empty()) {
        return CalcResult::success(0.0);
    }
    ExprParser parser(std::move(tokens));
    double v = 0.0;
    if (!parser.parse(v, err)) {
        return CalcResult::failure(std::move(err));
    }
    return CalcResult::success(v);
}

} // namespace synx
