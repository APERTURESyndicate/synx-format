package com.aperturesyndicate.synx;

import java.util.ArrayList;
import java.util.List;

/** Safe arithmetic evaluator for {@code :calc}. Mirrors {@code crates/synx-core/src/calc.rs}. */
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
        String t = expr == null ? "" : expr.strip();
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
                    Token last = tokens.get(tokens.size() - 1);
                    if (last.kind == Kind.OP || last.kind == Kind.LPAREN) isUnary = true;
                }
            }
            if (isDigit || isDotNum || isUnary) {
                int start = i;
                if (c == '-') i++;
                while (i < len) {
                    char x = expr.charAt(i);
                    if ((x >= '0' && x <= '9') || x == '.') i++;
                    else break;
                }
                String s = expr.substring(start, i);
                try {
                    tokens.add(new Token(Kind.NUMBER, Double.parseDouble(s), '\0'));
                } catch (NumberFormatException e) {
                    return "SYNX :calc - invalid number: '" + s + "'";
                }
                continue;
            }
            if (c == '+' || c == '-' || c == '*' || c == '/' || c == '%') {
                tokens.add(new Token(Kind.OP, 0, c)); i++; continue;
            }
            if (c == '(') { tokens.add(new Token(Kind.LPAREN, 0, '\0')); i++; continue; }
            if (c == ')') { tokens.add(new Token(Kind.RPAREN, 0, '\0')); i++; continue; }
            return "SYNX :calc - unexpected character: '" + c + "'";
        }
        return null;
    }

    private static final class ExprParser {
        final List<Token> tokens;
        int pos;
        ExprParser(List<Token> t) { this.tokens = t; }

        Result parse() {
            double[] out = { 0 };
            String e = expr(out);
            if (e != null) return Result.failure(e);
            if (pos < tokens.size()) {
                return Result.failure("SYNX :calc - unexpected token at position " + pos);
            }
            return Result.success(out[0]);
        }

        String expr(double[] out) {
            double[] left = { 0 };
            String e = term(left);
            if (e != null) return e;
            while (pos < tokens.size()) {
                Token t = tokens.get(pos);
                if (t.kind == Kind.OP && t.op == '+') {
                    pos++;
                    double[] r = { 0 };
                    e = term(r); if (e != null) return e;
                    left[0] += r[0];
                } else if (t.kind == Kind.OP && t.op == '-') {
                    pos++;
                    double[] r = { 0 };
                    e = term(r); if (e != null) return e;
                    left[0] -= r[0];
                } else break;
            }
            out[0] = left[0];
            return null;
        }

        String term(double[] out) {
            double[] left = { 0 };
            String e = factor(left);
            if (e != null) return e;
            while (pos < tokens.size()) {
                Token t = tokens.get(pos);
                if (t.kind != Kind.OP) break;
                switch (t.op) {
                    case '*': {
                        pos++;
                        double[] r = { 0 };
                        e = factor(r); if (e != null) return e;
                        left[0] *= r[0]; break;
                    }
                    case '/': {
                        pos++;
                        double[] r = { 0 };
                        e = factor(r); if (e != null) return e;
                        if (r[0] == 0) return "SYNX :calc - division by zero";
                        left[0] /= r[0]; break;
                    }
                    case '%': {
                        pos++;
                        double[] r = { 0 };
                        e = factor(r); if (e != null) return e;
                        if (r[0] == 0) return "SYNX :calc - division by zero";
                        left[0] = left[0] % r[0]; break;
                    }
                    default: out[0] = left[0]; return null;
                }
            }
            out[0] = left[0];
            return null;
        }

        String factor(double[] out) {
            if (pos >= tokens.size()) return "SYNX :calc - unexpected end of expression";
            Token t = tokens.get(pos);
            if (t.kind == Kind.NUMBER) { pos++; out[0] = t.number; return null; }
            if (t.kind == Kind.LPAREN) {
                pos++;
                double[] v = { 0 };
                String e = expr(v);
                if (e != null) return e;
                if (pos >= tokens.size()) return "SYNX :calc - missing closing parenthesis";
                if (tokens.get(pos).kind != Kind.RPAREN) {
                    return "SYNX :calc - missing closing parenthesis";
                }
                pos++; out[0] = v[0]; return null;
            }
            return "SYNX :calc - unexpected token";
        }
    }
}
