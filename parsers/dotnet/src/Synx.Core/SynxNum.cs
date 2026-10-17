using System.Globalization;
using System.Numerics;
using System.Text;

namespace Synx;

/// <summary>
/// Number text exactly as synx-core writes and reads it: Rust <c>f64</c> <c>Display</c>,
/// the ryu layout of <c>write_json</c>, <c>{:.N}</c> / <c>{:.Ne}</c> with exact
/// round-half-to-even, and the strict <c>str::parse</c> grammars.
/// </summary>
internal static class SynxNum
{
    // ── Shortest round-trip digits ──

    /// <summary>
    /// Shortest round-trip digits of a finite <paramref name="d"/>: the value is
    /// <c>0.digits × 10^point</c>. Zero is <c>("0", 1)</c>.
    /// </summary>
    private static (string Digits, int Point, bool Neg) Shortest(double d)
    {
        var neg = d < 0 || (d == 0 && double.IsNegative(d));
        if (d == 0) return ("0", 1, neg);
        // .NET Core 3.0+ "R" is the shortest round-trip form, e.g. "1E-07", "123.45".
        var r = Math.Abs(d).ToString("R", CultureInfo.InvariantCulture);
        var e = r.IndexOf('E');
        var exp = 0;
        var mant = r;
        if (e >= 0)
        {
            exp = int.Parse(r.AsSpan(e + 1), NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture);
            mant = r[..e];
        }
        var dot = mant.IndexOf('.');
        var digits = dot < 0 ? mant : string.Concat(mant.AsSpan(0, dot), mant.AsSpan(dot + 1));
        var point = (dot < 0 ? mant.Length : dot) + exp;
        var lead = 0;
        while (lead < digits.Length - 1 && digits[lead] == '0') lead++;
        if (lead > 0)
        {
            digits = digits[lead..];
            point -= lead;
        }
        var end = digits.Length;
        while (end > 1 && digits[end - 1] == '0') end--;
        return (digits[..end], point, neg);
    }

    /// <summary>Rust <c>f64</c> <c>Display</c>: shortest round-trip digits, plain decimal, never an exponent.</summary>
    internal static string Display(double d)
    {
        if (double.IsNaN(d)) return "NaN";
        if (double.IsInfinity(d)) return d > 0 ? "inf" : "-inf";
        var (digits, point, neg) = Shortest(d);
        var sb = new StringBuilder(digits.Length + 8);
        if (neg) sb.Append('-');
        if (point <= 0)
            sb.Append("0.").Append('0', -point).Append(digits);
        else if (point >= digits.Length)
            sb.Append(digits).Append('0', point - digits.Length);
        else
            sb.Append(digits, 0, point).Append('.').Append(digits, point, digits.Length - point);
        return sb.ToString();
    }

    /// <summary>
    /// synx-core <c>format_number</c>: a whole value below 2^63 prints as an integer,
    /// anything else as <see cref="Display"/>.
    /// </summary>
    internal static string FormatNumber(double n)
    {
        if (n % 1 == 0 && Math.Abs(n) < long.MaxValue)
            return ((long)n).ToString(CultureInfo.InvariantCulture);
        return Display(n);
    }

    /// <summary>The ryu layout <c>write_json</c> uses for a finite float (<c>1.0</c>, <c>1e-7</c>, <c>1.5e300</c>).</summary>
    internal static void AppendRyu(StringBuilder o, double d)
    {
        var (digits, point, neg) = Shortest(d);
        if (neg) o.Append('-');
        var len = digits.Length;
        var k = point - len;
        if (k >= 0 && point <= 16)
            o.Append(digits).Append('0', k).Append(".0");
        else if (point > 0 && point <= 16)
            o.Append(digits, 0, point).Append('.').Append(digits, point, len - point);
        else if (point > -5 && point <= 0)
            o.Append("0.").Append('0', -point).Append(digits);
        else
        {
            o.Append(digits[0]);
            if (len > 1) o.Append('.').Append(digits, 1, len - 1);
            o.Append('e').Append((point - 1).ToString(CultureInfo.InvariantCulture));
        }
    }

    /// <summary>Rust <c>{:e}</c>: shortest digits in exponent form (<c>1.5e-7</c>, <c>1e0</c>).</summary>
    internal static string LowerExpShortest(double d)
    {
        if (double.IsNaN(d)) return "NaN";
        if (double.IsInfinity(d)) return d > 0 ? "inf" : "-inf";
        var (digits, point, neg) = Shortest(d);
        var sb = new StringBuilder();
        if (neg) sb.Append('-');
        sb.Append(digits[0]);
        if (digits.Length > 1) sb.Append('.').Append(digits, 1, digits.Length - 1);
        var exp = digits == "0" ? 0 : point - 1;
        sb.Append('e').Append(exp.ToString(CultureInfo.InvariantCulture));
        return sb.ToString();
    }

    // ── Exact decimal expansion (for {:.N} and {:.Ne}) ──

    /// <summary>The exact decimal value of a finite |d| as <c>digits × 10^-scale</c>.</summary>
    private static (BigInteger Digits, int Scale) Exact(double d)
    {
        var bits = BitConverter.DoubleToInt64Bits(Math.Abs(d));
        var exp = (int)((bits >> 52) & 0x7FF);
        var mant = bits & 0xF_FFFF_FFFF_FFFFL;
        if (exp == 0) exp = 1;
        else mant |= 1L << 52;
        var e2 = exp - 1075;
        if (e2 >= 0) return (new BigInteger(mant) << e2, 0);
        return (new BigInteger(mant) * BigInteger.Pow(5, -e2), -e2);
    }

    /// <summary>Round the decimal string <paramref name="s"/> to its first <paramref name="keep"/> digits, ties to even.</summary>
    private static string RoundDigits(string s, int keep, out bool carry)
    {
        carry = false;
        if (keep >= s.Length) return s;
        if (keep < 0) return "";
        var up = false;
        var first = s[keep];
        if (first > '5') up = true;
        else if (first == '5')
        {
            var rest = false;
            for (var i = keep + 1; i < s.Length; i++)
                if (s[i] != '0') { rest = true; break; }
            up = rest || (keep > 0 ? (s[keep - 1] - '0') % 2 == 1 : false);
        }
        var kept = s[..keep].ToCharArray();
        if (up)
        {
            var i = kept.Length - 1;
            while (i >= 0 && kept[i] == '9') { kept[i] = '0'; i--; }
            if (i >= 0) kept[i]++;
            else carry = true;
        }
        return new string(kept);
    }

    /// <summary>Rust <c>format!("{:.prec$}", d)</c>.</summary>
    internal static string Fixed(double d, int prec)
    {
        if (double.IsNaN(d)) return "NaN";
        if (double.IsInfinity(d)) return d > 0 ? "inf" : "-inf";
        var neg = d < 0 || (d == 0 && double.IsNegative(d));
        var (big, scale) = Exact(d);
        var s = big.ToString(CultureInfo.InvariantCulture);
        string intPart, frac;
        if (scale <= prec)
        {
            if (s.Length <= scale) s = new string('0', scale - s.Length + 1) + s;
            intPart = s[..(s.Length - scale)];
            frac = s[(s.Length - scale)..] + new string('0', prec - scale);
        }
        else
        {
            if (s.Length <= scale) s = new string('0', scale - s.Length + 1) + s;
            var keep = s.Length - scale + prec;
            var r = RoundDigits(s, keep, out var carry);
            if (carry) r = "1" + r;
            intPart = r[..(r.Length - prec)];
            frac = r[(r.Length - prec)..];
        }
        var sb = new StringBuilder(intPart.Length + frac.Length + 2);
        if (neg) sb.Append('-');
        sb.Append(intPart);
        if (prec > 0) sb.Append('.').Append(frac);
        return sb.ToString();
    }

    /// <summary>Rust <c>format!("{:.prec$e}", d)</c>.</summary>
    internal static string ExpFixed(double d, int prec)
    {
        if (double.IsNaN(d)) return "NaN";
        if (double.IsInfinity(d)) return d > 0 ? "inf" : "-inf";
        var neg = d < 0 || (d == 0 && double.IsNegative(d));
        var sb = new StringBuilder();
        if (neg) sb.Append('-');
        if (d == 0)
        {
            sb.Append('0');
            if (prec > 0) sb.Append('.').Append('0', prec);
            return sb.Append("e0").ToString();
        }
        var (big, scale) = Exact(d);
        var s = big.ToString(CultureInfo.InvariantCulture);
        var exp = s.Length - 1 - scale;
        var r = RoundDigits(s, prec + 1, out var carry);
        if (carry)
        {
            r = "1" + r[..^1];
            exp++;
        }
        if (r.Length < prec + 1) r += new string('0', prec + 1 - r.Length);
        sb.Append(r[0]);
        if (prec > 0) sb.Append('.').Append(r, 1, prec);
        sb.Append('e').Append(exp.ToString(CultureInfo.InvariantCulture));
        return sb.ToString();
    }

    // ── Rust str::parse grammars ──

    /// <summary>Rust <c>str::parse::&lt;f64&gt;</c>: <c>[+-]? (digits[.digits?] | .digits) ([eE][+-]?digits)?</c>, or inf/infinity/nan.</summary>
    internal static bool TryParseF64(ReadOnlySpan<char> s, out double value)
    {
        value = 0;
        if (s.IsEmpty) return false;
        var i = 0;
        var neg = false;
        if (s[0] is '+' or '-')
        {
            neg = s[0] == '-';
            i = 1;
        }
        var body = s[i..];
        if (body.Equals("inf", StringComparison.OrdinalIgnoreCase) || body.Equals("infinity", StringComparison.OrdinalIgnoreCase))
        {
            value = neg ? double.NegativeInfinity : double.PositiveInfinity;
            return true;
        }
        if (body.Equals("nan", StringComparison.OrdinalIgnoreCase))
        {
            value = double.NaN;
            return true;
        }
        var intDigits = 0;
        while (i < s.Length && char.IsAsciiDigit(s[i])) { i++; intDigits++; }
        var fracDigits = 0;
        if (i < s.Length && s[i] == '.')
        {
            i++;
            while (i < s.Length && char.IsAsciiDigit(s[i])) { i++; fracDigits++; }
        }
        if (intDigits == 0 && fracDigits == 0) return false;
        if (i < s.Length && s[i] is 'e' or 'E')
        {
            i++;
            if (i < s.Length && s[i] is '+' or '-') i++;
            var expDigits = 0;
            while (i < s.Length && char.IsAsciiDigit(s[i])) { i++; expDigits++; }
            if (expDigits == 0) return false;
        }
        if (i != s.Length) return false;
        value = double.Parse(s, NumberStyles.Float, CultureInfo.InvariantCulture);
        return true;
    }

    /// <summary>Rust <c>str::parse::&lt;i64&gt;</c>: an optional sign and ASCII digits, no overflow.</summary>
    internal static bool TryParseI64(ReadOnlySpan<char> s, out long value)
    {
        value = 0;
        var i = 0;
        var neg = false;
        if (s.Length > 0 && s[0] is '+' or '-')
        {
            neg = s[0] == '-';
            i = 1;
        }
        if (i >= s.Length) return false;
        ulong acc = 0;
        for (; i < s.Length; i++)
        {
            var c = s[i];
            if (!char.IsAsciiDigit(c)) return false;
            if (acc > (ulong.MaxValue - 9) / 10) return false;
            acc = acc * 10 + (ulong)(c - '0');
            if (acc > (ulong)long.MaxValue + 1) return false;
        }
        if (neg)
        {
            value = acc == (ulong)long.MaxValue + 1 ? long.MinValue : -(long)acc;
            return true;
        }
        if (acc > long.MaxValue) return false;
        value = (long)acc;
        return true;
    }

    /// <summary>Rust <c>str::parse::&lt;u64&gt;</c> (also usize / u32 below their maximum): optional <c>+</c>, ASCII digits.</summary>
    internal static bool TryParseU64(ReadOnlySpan<char> s, ulong max, out ulong value)
    {
        value = 0;
        var i = 0;
        if (s.Length > 0 && s[0] == '+') i = 1;
        if (i >= s.Length) return false;
        for (; i < s.Length; i++)
        {
            var c = s[i];
            if (!char.IsAsciiDigit(c)) return false;
            var digit = (ulong)(c - '0');
            if (value > (max - digit) / 10) return false;
            value = value * 10 + digit;
        }
        return true;
    }

    /// <summary>Rust <c>f as i64</c>: saturating, NaN is 0.</summary>
    internal static long SaturatingI64(double d)
    {
        if (double.IsNaN(d)) return 0;
        if (d >= 9.2233720368547758E18) return long.MaxValue;
        if (d <= -9.2233720368547758E18) return long.MinValue;
        return (long)d;
    }

    /// <summary>UTF-8 byte length of a string (all SYNX limits are in UTF-8 bytes).</summary>
    internal static int Utf8Len(string s) => Encoding.UTF8.GetByteCount(s);

    /// <summary>UTF-8 byte length of one UTF-16 unit; a surrogate pair counts 4 over its two units.</summary>
    internal static int Utf8Len(char c) => c < 0x80 ? 1 : c < 0x800 ? 2 : char.IsSurrogate(c) ? 2 : 3;
}

/// <summary>Order by Unicode code point — Rust's byte order of UTF-8 strings (UTF-16 ordinal differs above the surrogates).</summary>
internal sealed class CodePointComparer : IComparer<string>
{
    internal static readonly CodePointComparer Instance = new();

    public int Compare(string? a, string? b) => SynxEngine.CompareCodePoints(a ?? "", b ?? "");
}

internal static class SynxSort
{
    /// <summary>
    /// Sort keys by code point. perf: when no key holds a unit at or above U+D800 —
    /// nearly always — UTF-16 ordinal order is code-point order and the vectorized
    /// ordinal comparer is used.
    /// </summary>
    internal static void Keys(string[] keys)
    {
        foreach (var k in keys)
        {
            if (k.AsSpan().IndexOfAnyInRange((char)0xD800, (char)0xFFFF) >= 0)
            {
                Array.Sort(keys, CodePointComparer.Instance);
                return;
            }
        }
        Array.Sort(keys, StringComparer.Ordinal);
    }
}
