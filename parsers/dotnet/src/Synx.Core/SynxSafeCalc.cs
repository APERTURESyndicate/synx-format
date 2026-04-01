using System.Globalization;
using System.Text;

namespace Synx;

/// <summary>Arithmetic-only evaluator (ported from <c>synx-core::calc::safe_calc</c>).</summary>
public static class SynxSafeCalc
{
    private enum TokType { Num, Op, Lp, Rp }

    private readonly struct Tok
    {
        internal Tok(TokType t, double n = 0, byte op = 0)
        {
            T = t;
            Num = n;
            Op = op;
        }
        internal TokType T { get; }
        internal double Num { get; }
        internal byte Op { get; }
    }

    public static double Eval(string expr)
    {
        var trimmed = expr.Trim();
        if (trimmed.Length == 0) return 0;
        var toks = Tokenize(trimmed);
        if (toks.Count == 0) return 0;
        var p = new Parser(toks);
        return p.ParseExpr();
    }

    /// <summary>Maps thrown tokenizer/parser errors to short CALC_ERR messages.</summary>
    public static bool TryEval(string expr, out double value, out string? errMsg)
    {
        try
        {
            value = Eval(expr);
            errMsg = null;
            return true;
        }
        catch (Exception e)
        {
            value = 0;
            var m = e.Message;
            if (m.StartsWith("SYNX :calc — ", StringComparison.Ordinal))
                m = m["SYNX :calc — ".Length..];
            errMsg = m;
            return false;
        }
    }

    private static List<Tok> Tokenize(string expr)
    {
        var bytes = Encoding.UTF8.GetBytes(expr);
        var len = bytes.Length;
        var tokens = new List<Tok>();
        var i = 0;
        while (i < len)
        {
            var ch = bytes[i];
            if (ch is (byte)' ' or (byte)'\t')
            {
                i++;
                continue;
            }

            if (IsDigit(ch)
                || (ch == (byte)'.' && i + 1 < len && IsDigit(bytes[i + 1]))
                || (ch == (byte)'-'
                    && (tokens.Count == 0 || IsOpOrLp(tokens[^1]))))
            {
                var start = i;
                if (ch == (byte)'-') i++;
                while (i < len && (IsDigit(bytes[i]) || bytes[i] == (byte)'.')) i++;
                var numStr = expr[start..i];
                if (!double.TryParse(numStr, CultureInfo.InvariantCulture, out var val))
                    throw new InvalidOperationException($"SYNX :calc — invalid number: '{numStr}'");
                tokens.Add(new Tok(TokType.Num, val));
                continue;
            }

            if (ch is (byte)'+' or (byte)'-' or (byte)'*' or (byte)'/' or (byte)'%')
            {
                tokens.Add(new Tok(TokType.Op, op: ch));
                i++;
                continue;
            }
            if (ch == (byte)'(')
            {
                tokens.Add(new Tok(TokType.Lp));
                i++;
                continue;
            }
            if (ch == (byte)')')
            {
                tokens.Add(new Tok(TokType.Rp));
                i++;
                continue;
            }

            throw new InvalidOperationException($"SYNX :calc — unexpected character: '{(char)ch}'");
        }
        return tokens;
    }

    private static bool IsDigit(byte b) => b is >= (byte)'0' and <= (byte)'9';

    private static bool IsOpOrLp(Tok t) => t.T is TokType.Op or TokType.Lp;

    private sealed class Parser
    {
        private readonly List<Tok> _t;
        private int _i;

        internal Parser(List<Tok> t) => _t = t;

        internal double ParseExpr()
        {
            var r = ParseTerm();
            while (_i < _t.Count)
            {
                if (_t[_i].T == TokType.Op && _t[_i].Op == (byte)'+')
                {
                    _i++;
                    r += ParseTerm();
                }
                else if (_t[_i].T == TokType.Op && _t[_i].Op == (byte)'-')
                {
                    _i++;
                    r -= ParseTerm();
                }
                else break;
            }
            if (_i != _t.Count)
                throw new InvalidOperationException($"SYNX :calc — unexpected token at position {_i}");
            return r;
        }

        private double ParseTerm()
        {
            var r = ParseFactor();
            while (_i < _t.Count)
            {
                if (_t[_i].T == TokType.Op && _t[_i].Op == (byte)'*')
                {
                    _i++;
                    r *= ParseFactor();
                }
                else if (_t[_i].T == TokType.Op && _t[_i].Op == (byte)'/')
                {
                    _i++;
                    var d = ParseFactor();
                    if (d == 0) throw new InvalidOperationException("SYNX :calc — division by zero");
                    r /= d;
                }
                else if (_t[_i].T == TokType.Op && _t[_i].Op == (byte)'%')
                {
                    _i++;
                    var d = ParseFactor();
                    if (d == 0) throw new InvalidOperationException("SYNX :calc — division by zero");
                    r %= d;
                }
                else break;
            }
            return r;
        }

        private double ParseFactor()
        {
            if (_i >= _t.Count)
                throw new InvalidOperationException("SYNX :calc — unexpected end of expression");
            if (_t[_i].T == TokType.Num)
            {
                var v = _t[_i].Num;
                _i++;
                return v;
            }
            if (_t[_i].T == TokType.Lp)
            {
                _i++;
                var v = ParseExpr();
                if (_i >= _t.Count || _t[_i].T != TokType.Rp)
                    throw new InvalidOperationException("SYNX :calc — missing closing parenthesis");
                _i++;
                return v;
            }
            throw new InvalidOperationException("SYNX :calc — unexpected token");
        }
    }
}
