// Safe arithmetic evaluator for `:calc`. Mirrors crates/synx-core/src/calc.rs.
#include "synx/calc.hpp"

#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <string>
#include <vector>

namespace synx {

namespace {

enum class TokenKind : uint8_t { Number, Op, LParen, RParen };

struct Token {
    TokenKind kind;
    double number = 0.0;
    char op = 0;
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
            char* end = nullptr;
            double v = std::strtod(num_str.c_str(), &end);
            if (!end || end == num_str.c_str() || *end != '\0') {
                err = "SYNX :calc - invalid number: '" + num_str + "'";
                return false;
            }
            out.push_back(Token{TokenKind::Number, v, 0});
            continue;
        }

        if (ch == '+' || ch == '-' || ch == '*' || ch == '/' || ch == '%') {
            out.push_back(Token{TokenKind::Op, 0.0, ch});
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

        err = std::string("SYNX :calc - unexpected character: '") + ch + "'";
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
            err = "SYNX :calc - unexpected token at position " + std::to_string(pos_);
            return false;
        }
        return true;
    }

private:
    bool expr(double& out, std::string& err) {
        double left = 0.0;
        if (!term(left, err)) return false;
        while (pos_ < tokens_.size()) {
            const Token& t = tokens_[pos_];
            if (t.kind == TokenKind::Op && t.op == '+') {
                ++pos_;
                double right = 0.0;
                if (!term(right, err)) return false;
                left += right;
            } else if (t.kind == TokenKind::Op && t.op == '-') {
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
            const Token& t = tokens_[pos_];
            if (t.kind == TokenKind::Op && t.op == '*') {
                ++pos_;
                double right = 0.0;
                if (!factor(right, err)) return false;
                left *= right;
            } else if (t.kind == TokenKind::Op && t.op == '/') {
                ++pos_;
                double right = 0.0;
                if (!factor(right, err)) return false;
                if (right == 0.0) {
                    err = "SYNX :calc - division by zero";
                    return false;
                }
                left /= right;
            } else if (t.kind == TokenKind::Op && t.op == '%') {
                ++pos_;
                double right = 0.0;
                if (!factor(right, err)) return false;
                if (right == 0.0) {
                    err = "SYNX :calc - division by zero";
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
            err = "SYNX :calc - unexpected end of expression";
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
                err = "SYNX :calc - missing closing parenthesis";
                return false;
            }
            ++pos_;
            out = v;
            return true;
        }
        err = "SYNX :calc - unexpected token";
        return false;
    }

    std::vector<Token> tokens_;
    size_t pos_ = 0;
};

} // namespace

CalcResult safe_calc(std::string_view expr) {
    // trim
    size_t start = 0;
    while (start < expr.size() && (expr[start] == ' ' || expr[start] == '\t')) ++start;
    size_t end = expr.size();
    while (end > start && (expr[end - 1] == ' ' || expr[end - 1] == '\t')) --end;
    std::string_view trimmed = expr.substr(start, end - start);
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
