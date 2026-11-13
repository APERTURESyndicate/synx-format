// Safe arithmetic evaluator for `:calc`. Byte-for-byte port of crates/synx-core/src/calc.rs,
// including its error messages (`SYNX :calc — …`, with the em dash).
#include "synx/calc.hpp"
#include "numfmt.hpp"
#include "unicode.hpp"

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
            if (!detail::parse_f64(num_str, v)) {
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

        // The whole character, not its first byte read as Latin-1.
        err = std::string(kPrefix) + "unexpected character: '"
            + std::string(expr.substr(i, detail::utf8_len(static_cast<unsigned char>(ch)))) + "' in expression";
        return false;
    }
    return true;
}

// `expr -> term (('+'|'-') term)*`, `term -> factor (('*'|'/'|'%') factor)*`,
// `factor -> NUMBER | '(' expr ')'`, one frame per open parenthesis.
//
// safety: parentheses used to recurse through expr -> term -> factor, one group
// of stack frames per `(`; 20 000 of them through `:ref:calc` overflowed the
// stack and took the process down. Open parentheses now live in a vector; the
// order of evaluation, and so every result and error, is the one the recursive
// parser produced (synx-core `calc.rs evaluate`).
struct Frame {
    double expr_left = 0.0;
    unsigned char expr_op = 0;
    double term_left = 0.0;
    unsigned char term_op = 0;
};

bool evaluate(const std::vector<Token>& tokens, double& result, std::string& err) {
    std::vector<Frame> stack(1);
    size_t pos = 0;
    while (true) {
        // A factor is expected here.
        if (pos >= tokens.size()) {
            err = std::string(kPrefix) + "unexpected end of expression";
            return false;
        }
        const Token& t = tokens[pos];
        double value = 0.0;
        if (t.kind == TokenKind::Number) {
            value = t.number;
            ++pos;
        } else if (t.kind == TokenKind::LParen) {
            ++pos;
            stack.emplace_back();
            continue;
        } else {
            // The token as written (Rust `token_text`).
            std::string text = t.kind == TokenKind::Op ? std::string(1, static_cast<char>(t.op)) : std::string(")");
            err = std::string(kPrefix) + "unexpected token: '" + text + "'";
            return false;
        }
        // Fold the factor into its term, the term into its expr, and close every
        // pair of parentheses that ends here.
        while (true) {
            Frame& f = stack.back();
            const unsigned char top = f.term_op;
            f.term_op = 0;
            if (top == 0) {
                f.term_left = value;
            } else if (top == '*') {
                f.term_left *= value;
            } else {
                if (value == 0.0) {
                    err = std::string(kPrefix) + "division by zero";
                    return false;
                }
                f.term_left = top == '/' ? f.term_left / value : std::fmod(f.term_left, value);
            }
            if (pos < tokens.size() && tokens[pos].kind == TokenKind::Op
                && (tokens[pos].op == '*' || tokens[pos].op == '/' || tokens[pos].op == '%')) {
                f.term_op = tokens[pos].op;
                ++pos;
                break;
            }
            const unsigned char eop = f.expr_op;
            f.expr_op = 0;
            if (eop == 0) f.expr_left = f.term_left;
            else if (eop == '+') f.expr_left += f.term_left;
            else f.expr_left -= f.term_left;
            if (pos < tokens.size() && tokens[pos].kind == TokenKind::Op
                && (tokens[pos].op == '+' || tokens[pos].op == '-')) {
                f.expr_op = tokens[pos].op;
                ++pos;
                break;
            }
            // This expr is complete.
            const double done = f.expr_left;
            if (stack.size() == 1) {
                if (pos < tokens.size()) {
                    err = std::string(kPrefix) + "unexpected token at position " + std::to_string(pos);
                    return false;
                }
                result = done;
                return true;
            }
            stack.pop_back();
            if (pos < tokens.size() && tokens[pos].kind == TokenKind::RParen) {
                ++pos;
            } else {
                err = std::string(kPrefix) + "missing closing parenthesis";
                return false;
            }
            value = done;
        }
    }
}

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
    double v = 0.0;
    if (!evaluate(tokens, v, err)) {
        return CalcResult::failure(std::move(err));
    }
    return CalcResult::success(v);
}

} // namespace synx
