namespace Synx;

/// <summary>Arithmetic-only evaluator (ported from <c>synx-core::calc::safe_calc</c>).</summary>
public static class SynxSafeCalc
{
    private enum TokType { Num, Op, Lp, Rp }

    private readonly struct Tok(TokType t, double n = 0, char op = '\0')
    {
        internal TokType T { get; } = t;
        internal double Num { get; } = n;
        internal char Op { get; } = op;
    }

    private sealed class CalcException(string message) : Exception(message);

    /// <summary>Evaluate; throws <see cref="InvalidOperationException"/> with the synx-core message on error.</summary>
    public static double Eval(string expr)
    {
        if (TryEval(expr, out var value, out var err))
            return value;
        throw new InvalidOperationException(err);
    }

    /// <summary>
    /// Evaluate <paramref name="expr"/>. On failure <paramref name="errMsg"/> is the synx-core
    /// message (<c>SYNX :calc — division by zero</c>), which the engine writes as <c>CALC_ERR: …</c>.
    /// </summary>
    public static bool TryEval(string expr, out double value, out string? errMsg)
    {
        value = 0;
        errMsg = null;
        var trimmed = expr.Trim();
        if (trimmed.Length == 0) return true;
        try
        {
            var toks = Tokenize(trimmed);
            if (toks.Count == 0) return true;
            value = Evaluate(toks);
            return true;
        }
        catch (CalcException e)
        {
            errMsg = e.Message;
            return false;
        }
    }

    private static List<Tok> Tokenize(string expr)
    {
        var len = expr.Length;
        var tokens = new List<Tok>(16);
        var i = 0;
        while (i < len)
        {
            var ch = expr[i];
            if (ch is ' ' or '\t')
            {
                i++;
                continue;
            }

            // A number, including a negative one after an operator, `(` or the start.
            if (char.IsAsciiDigit(ch)
                || (ch == '.' && i + 1 < len && char.IsAsciiDigit(expr[i + 1]))
                || (ch == '-' && (tokens.Count == 0 || tokens[^1].T is TokType.Op or TokType.Lp)))
            {
                var start = i;
                if (ch == '-') i++;
                while (i < len && (char.IsAsciiDigit(expr[i]) || expr[i] == '.')) i++;
                var numStr = expr[start..i];
                if (!SynxNum.TryParseF64(numStr, out var val))
                    throw new CalcException($"SYNX :calc — invalid number: '{numStr}'");
                tokens.Add(new Tok(TokType.Num, val));
                continue;
            }

            if (ch is '+' or '-' or '*' or '/' or '%')
            {
                tokens.Add(new Tok(TokType.Op, op: ch));
                i++;
                continue;
            }
            if (ch == '(')
            {
                tokens.Add(new Tok(TokType.Lp));
                i++;
                continue;
            }
            if (ch == ')')
            {
                tokens.Add(new Tok(TokType.Rp));
                i++;
                continue;
            }

            // The whole character, a surrogate pair included.
            var shown = char.IsHighSurrogate(ch) && i + 1 < len && char.IsLowSurrogate(expr[i + 1])
                ? expr.Substring(i, 2)
                : ch.ToString();
            throw new CalcException($"SYNX :calc — unexpected character: '{shown}' in expression");
        }
        return tokens;
    }

    /// <summary>A token as written, for error messages (identical in every implementation).</summary>
    private static string TokenText(Tok t) => t.T switch
    {
        TokType.Num => SynxNum.Display(t.Num),
        TokType.Op => t.Op.ToString(),
        TokType.Lp => "(",
        _ => ")",
    };

    /// <summary>
    /// One open <c>expr</c> of the grammar — <c>expr → term (('+'|'-') term)*</c>,
    /// <c>term → factor (('*'|'/'|'%') factor)*</c>, <c>factor → NUMBER | '(' expr ')'</c> —
    /// the top level, or the inside of a pair of parentheses.
    /// </summary>
    private struct Frame
    {
        /// <summary>Sum so far and the <c>+</c> / <c>-</c> waiting for its right-hand term (<c>'\0'</c>: none).</summary>
        internal double ExprLeft;
        internal char ExprOp;
        /// <summary>Product so far and the <c>*</c> / <c>/</c> / <c>%</c> waiting for its factor.</summary>
        internal double TermLeft;
        internal char TermOp;
    }

    /// <summary>
    /// Evaluates a token stream.
    /// safety: parentheses used to recurse through Expr → Term → Factor, one group of stack
    /// frames per <c>(</c>. A stack overflow cannot be caught in .NET: <c>x:calc ((((…</c> — 1.6 KB
    /// of document on a thread-pool thread, 20 000 parentheses through <c>:ref:calc</c> anywhere —
    /// ended the whole process. Open parentheses now live in a list; the order of evaluation,
    /// and so every result and error text, is the one the recursive parser produced (and the
    /// one synx-core produces).
    /// </summary>
    private static double Evaluate(List<Tok> t)
    {
        var stack = new List<Frame> { default };
        var pos = 0;
        while (true)
        {
            // A factor is expected here.
            if (pos >= t.Count)
                throw new CalcException("SYNX :calc — unexpected end of expression");
            var tok = t[pos];
            if (tok.T == TokType.Lp)
            {
                pos++;
                stack.Add(default);
                continue;
            }
            if (tok.T != TokType.Num)
                throw new CalcException($"SYNX :calc — unexpected token: '{TokenText(tok)}'");
            pos++;
            var value = tok.Num;

            // Fold the factor into its term, the term into its expr, and close every pair of
            // parentheses that ends here.
            while (true)
            {
                var frame = stack[^1];
                var termOp = frame.TermOp;
                frame.TermOp = '\0';
                if (termOp == '\0') frame.TermLeft = value;
                else if (termOp == '*') frame.TermLeft *= value;
                else
                {
                    if (value == 0) throw new CalcException("SYNX :calc — division by zero");
                    frame.TermLeft = termOp == '/' ? frame.TermLeft / value : frame.TermLeft % value;
                }
                if (pos < t.Count && t[pos].T == TokType.Op && t[pos].Op is '*' or '/' or '%')
                {
                    frame.TermOp = t[pos].Op;
                    pos++;
                    stack[^1] = frame;
                    break;
                }

                var exprOp = frame.ExprOp;
                frame.ExprOp = '\0';
                if (exprOp == '\0') frame.ExprLeft = frame.TermLeft;
                else if (exprOp == '+') frame.ExprLeft += frame.TermLeft;
                else frame.ExprLeft -= frame.TermLeft;
                if (pos < t.Count && t[pos].T == TokType.Op && t[pos].Op is '+' or '-')
                {
                    frame.ExprOp = t[pos].Op;
                    pos++;
                    stack[^1] = frame;
                    break;
                }

                // This expr is complete.
                var result = frame.ExprLeft;
                if (stack.Count == 1)
                {
                    if (pos < t.Count)
                        throw new CalcException($"SYNX :calc — unexpected token at position {pos}");
                    return result;
                }
                stack.RemoveAt(stack.Count - 1);
                if (pos >= t.Count || t[pos].T != TokType.Rp)
                    throw new CalcException("SYNX :calc — missing closing parenthesis");
                pos++;
                value = result;
            }
        }
    }
}
