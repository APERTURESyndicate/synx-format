package com.aperturesyndicate.synx;

import java.util.ArrayList;
import java.util.List;

/**
 * Safe arithmetic evaluator for {@code :calc}: +, -, *, /, %, parentheses, integers, floats and
 * unary minus. Mirrors {@code crates/synx-core/src/calc.rs}, error texts included.
 */
public final class SynxCalc {

    private SynxCalc() {}

    public static final class Result {
        public final boolean ok;
        public final double value;
        public final String error;
        Result(boolean ok, double value, String error) {
            this.ok = ok; this.value = value; this.error = error;
        }
        public static Result success(double v) { return new Result(true, v, ""); }
        public static Result failure(String msg) { return new Result(false, 0, msg); }
    }

    public static Result evaluate(String expr) {
        String t = expr == null ? "" : SynxParser.trim(expr);
        if (t.isEmpty()) return Result.success(0);
        List<Token> tokens = new ArrayList<>();
        String err = tokenize(t, tokens);
        if (err != null) return Result.failure(err);
        if (tokens.isEmpty()) return Result.success(0);
        return new ExprParser(tokens).parse();
    }

    // ─── tokens ─────────────────────────────────────────────────────────────

    private enum Kind { NUMBER, OP, LPAREN, RPAREN }

    private static final class Token {
        final Kind kind;
        final double number;
        final char op;
        Token(Kind k, double n, char op) { this.kind = k; this.number = n; this.op = op; }
    }

    private static String tokenize(String expr, List<Token> tokens) {
        int i = 0;
        int len = expr.length();
        while (i < len) {
            char c = expr.charAt(i);
            if (c == ' ' || c == '\t') { i++; continue; }
            boolean isDigit = c >= '0' && c <= '9';
            boolean isDotNum = c == '.' && i + 1 < len && expr.charAt(i + 1) >= '0' && expr.charAt(i + 1) <= '9';
            boolean isUnary = false;
            if (c == '-') {
                if (tokens.isEmpty()) isUnary = true;
                else {
                    Kind last = tokens.get(tokens.size() - 1).kind;
                    isUnary = last == Kind.OP || last == Kind.LPAREN;
                }
            }
            if (isDigit || isDotNum || isUnary) {
                int start = i;
                if (c == '-') i++;
                while (i < len && ((expr.charAt(i) >= '0' && expr.charAt(i) <= '9') || expr.charAt(i) == '.')) i++;
                String s = expr.substring(start, i);
                Double d = SynxNumbers.parseF64(s);
                if (d == null) return "SYNX :calc — invalid number: '" + s + "'";
                tokens.add(new Token(Kind.NUMBER, d, '\0'));
                continue;
            }
            switch (c) {
                case '+': case '-': case '*': case '/': case '%':
                    tokens.add(new Token(Kind.OP, 0, c)); i++; continue;
                case '(':
                    tokens.add(new Token(Kind.LPAREN, 0, '\0')); i++; continue;
                case ')':
                    tokens.add(new Token(Kind.RPAREN, 0, '\0')); i++; continue;
                default:
                    break;
            }
            // The whole character, not its first UTF-16 unit.
            String shown = new String(Character.toChars(expr.codePointAt(i)));
            return "SYNX :calc — unexpected character: '" + shown + "' in expression";
        }
        return null;
    }

    private static String tokenText(Token t) {
        switch (t.kind) {
            case NUMBER: return SynxNumbers.display(t.number);
            case OP:     return String.valueOf(t.op);
            case LPAREN: return "(";
            default:     return ")";
        }
    }

    // ─── recursive descent ─────────────────────────────────────────────────

    private static final class CalcException extends RuntimeException {
        CalcException(String msg) { super(msg, null, false, false); }
    }

    private static final class ExprParser {
        final List<Token> tokens;
        int pos;

        ExprParser(List<Token> tokens) { this.tokens = tokens; }

        Result parse() {
            try {
                double v = expr();
                if (pos < tokens.size()) return Result.failure("SYNX :calc — unexpected token at position " + pos);
                return Result.success(v);
            } catch (CalcException e) {
                return Result.failure(e.getMessage());
            }
        }

        double expr() {
            double left = term();
            while (pos < tokens.size()) {
                Token t = tokens.get(pos);
                if (t.kind != Kind.OP || (t.op != '+' && t.op != '-')) break;
                pos++;
                double right = term();
                left = t.op == '+' ? left + right : left - right;
            }
            return left;
        }

        double term() {
            double left = factor();
            while (pos < tokens.size()) {
                Token t = tokens.get(pos);
                if (t.kind != Kind.OP || (t.op != '*' && t.op != '/' && t.op != '%')) break;
                pos++;
                double right = factor();
                if (t.op == '*') {
                    left *= right;
                } else {
                    if (right == 0.0) throw new CalcException("SYNX :calc — division by zero");
                    left = t.op == '/' ? left / right : left % right;
                }
            }
            return left;
        }

        double factor() {
            if (pos >= tokens.size()) throw new CalcException("SYNX :calc — unexpected end of expression");
            Token t = tokens.get(pos);
            if (t.kind == Kind.NUMBER) {
                pos++;
                return t.number;
            }
            if (t.kind == Kind.LPAREN) {
                pos++;
                double v = expr();
                if (pos >= tokens.size() || tokens.get(pos).kind != Kind.RPAREN) {
                    throw new CalcException("SYNX :calc — missing closing parenthesis");
                }
                pos++;
                return v;
            }
            throw new CalcException("SYNX :calc — unexpected token: '" + tokenText(t) + "'");
        }
    }
}
