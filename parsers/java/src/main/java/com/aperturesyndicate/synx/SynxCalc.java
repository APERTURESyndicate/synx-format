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
        return evaluateTokens(tokens);
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

    // ─── evaluation ────────────────────────────────────────────────────────

    /**
     * One open {@code expr} of the grammar: the top level, or the inside of a pair of
     * parentheses.
     */
    private static final class Frame {
        /** Sum so far and the {@code +} / {@code -} waiting for its right-hand term (0 = none). */
        double exprLeft;
        char exprOp;
        /** Product so far and the {@code *} / {@code /} / {@code %} waiting for its factor (0 = none). */
        double termLeft;
        char termOp;
    }

    private static boolean isOp(List<Token> tokens, int pos, char a, char b, char c) {
        if (pos >= tokens.size()) return false;
        Token t = tokens.get(pos);
        return t.kind == Kind.OP && (t.op == a || t.op == b || t.op == c);
    }

    /**
     * Evaluates a token stream.
     *
     * <p>safety: parentheses used to recurse through expr → term → factor, one group of stack
     * frames per {@code (}. {@code x:calc ((((…} ended in a StackOverflowError — an Error,
     * which {@code catch (Exception e)} around a parse does not catch. Open parentheses now
     * live in a list; the order of evaluation, and so every result and error text, is the one
     * the recursive parser produced (and the one synx-core produces).
     */
    private static Result evaluateTokens(List<Token> tokens) {
        ArrayList<Frame> stack = new ArrayList<>();
        stack.add(new Frame());
        int pos = 0;
        while (true) {
            // A factor is expected here.
            if (pos >= tokens.size()) return Result.failure("SYNX :calc — unexpected end of expression");
            Token t = tokens.get(pos);
            if (t.kind == Kind.LPAREN) {
                pos++;
                stack.add(new Frame());
                continue;
            }
            if (t.kind != Kind.NUMBER) {
                return Result.failure("SYNX :calc — unexpected token: '" + tokenText(t) + "'");
            }
            pos++;
            double value = t.number;

            // Fold the factor into its term, the term into its expr, and close every pair of
            // parentheses that ends here.
            while (true) {
                Frame frame = stack.get(stack.size() - 1);
                char termOp = frame.termOp;
                frame.termOp = 0;
                if (termOp == 0) {
                    frame.termLeft = value;
                } else if (termOp == '*') {
                    frame.termLeft *= value;
                } else {
                    if (value == 0.0) return Result.failure("SYNX :calc — division by zero");
                    frame.termLeft = termOp == '/' ? frame.termLeft / value : frame.termLeft % value;
                }
                if (isOp(tokens, pos, '*', '/', '%')) {
                    frame.termOp = tokens.get(pos).op;
                    pos++;
                    break;
                }

                char exprOp = frame.exprOp;
                frame.exprOp = 0;
                if (exprOp == 0) frame.exprLeft = frame.termLeft;
                else if (exprOp == '+') frame.exprLeft += frame.termLeft;
                else frame.exprLeft -= frame.termLeft;
                if (isOp(tokens, pos, '+', '-', '+')) {
                    frame.exprOp = tokens.get(pos).op;
                    pos++;
                    break;
                }

                // This expr is complete.
                double result = frame.exprLeft;
                if (stack.size() == 1) {
                    if (pos < tokens.size()) return Result.failure("SYNX :calc — unexpected token at position " + pos);
                    return Result.success(result);
                }
                stack.remove(stack.size() - 1);
                if (pos >= tokens.size() || tokens.get(pos).kind != Kind.RPAREN) {
                    return Result.failure("SYNX :calc — missing closing parenthesis");
                }
                pos++;
                value = result;
            }
        }
    }
}
