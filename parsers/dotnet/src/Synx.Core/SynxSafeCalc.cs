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
            var p = new Parser(toks);
            value = p.Parse();
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
    /// Recursive descent: <c>expr → term (('+'|'-') term)*</c>,
    /// <c>term → factor (('*'|'/'|'%') factor)*</c>, <c>factor → NUMBER | '(' expr ')'</c>.
    /// </summary>
    private sealed class Parser(List<Tok> t)
    {
        private int _i;

        internal double Parse()
        {
            var r = Expr();
            if (_i < t.Count)
                throw new CalcException($"SYNX :calc — unexpected token at position {_i}");
            return r;
        }

        private double Expr()
        {
            var r = Term();
            while (_i < t.Count && t[_i].T == TokType.Op && t[_i].Op is '+' or '-')
            {
                var op = t[_i].Op;
                _i++;
                var right = Term();
                r = op == '+' ? r + right : r - right;
            }
            return r;
        }

        private double Term()
        {
            var r = Factor();
            while (_i < t.Count && t[_i].T == TokType.Op && t[_i].Op is '*' or '/' or '%')
            {
                var op = t[_i].Op;
                _i++;
                var d = Factor();
                if (op == '*')
                {
                    r *= d;
                    continue;
                }
                if (d == 0) throw new CalcException("SYNX :calc — division by zero");
                r = op == '/' ? r / d : r % d;
            }
            return r;
        }

        private double Factor()
        {
            if (_i >= t.Count)
                throw new CalcException("SYNX :calc — unexpected end of expression");
            var tok = t[_i];
            if (tok.T == TokType.Num)
            {
                _i++;
                return tok.Num;
            }
            if (tok.T == TokType.Lp)
            {
                _i++;
                var v = Expr();
                if (_i >= t.Count || t[_i].T != TokType.Rp)
                    throw new CalcException("SYNX :calc — missing closing parenthesis");
                _i++;
                return v;
            }
            throw new CalcException($"SYNX :calc — unexpected token: '{TokenText(tok)}'");
        }
    }
}
