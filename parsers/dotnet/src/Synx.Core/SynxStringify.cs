using System.Globalization;
using System.Text;

namespace Synx;

/// <summary>
/// Writes <see cref="SynxValue"/> trees as SYNX text that parses back to the same data —
/// the SYNXL §14.1 block writer of synx-core (<c>synxl::write_synx_entry</c>): strings that
/// would read as another type or be cut as a comment are quoted or carried as <c>|+</c>,
/// multi-line text keeps its relative indent, a list item's head is repeated when needed,
/// and secrets are written as their mask.
/// </summary>
internal static class SynxStringify
{
    private const int MaxDepth = 128;
    private const int MaxWriteDepth = SynxParserCore.MaxParseNestingDepth;

    internal static string Serialize(SynxValue value, int depth)
    {
        if (depth > MaxDepth)
            return "[synx:max-depth]\n";
        if (value is not SynxValue.Obj obj)
            return FormatPrimitive(value);

        var sb = new StringBuilder();
        foreach (var key in SortedKeys(obj.Map))
            WriteEntry(sb, key, obj.Map[key], depth * 2, depth);
        return sb.ToString();
    }

    private static string[] SortedKeys(Dictionary<string, SynxValue> map)
    {
        var keys = new string[map.Count];
        map.Keys.CopyTo(keys, 0);
        SynxSort.Keys(keys);
        return keys;
    }

    /// <summary>Write <c>key value</c> as SYNX at <paramref name="indent"/> columns.</summary>
    internal static void WriteEntry(StringBuilder o, string key, SynxValue value, int indent, int depth)
    {
        if (depth > MaxWriteDepth) return;
        switch (value)
        {
            case SynxValue.Obj obj:
                o.Append(' ', indent).Append(key).Append('\n');
                foreach (var k in SortedKeys(obj.Map))
                    WriteEntry(o, k, obj.Map[k], indent + 2, depth + 1);
                break;
            case SynxValue.Arr arr:
                o.Append(' ', indent).Append(key);
                // A bare key parses back as an empty object; `:join` is the only SYNX
                // surface that yields an empty array.
                if (arr.Items.Count == 0) o.Append(":join");
                o.Append('\n');
                foreach (var item in arr.Items)
                    WriteItem(o, item, indent + 2, depth + 1);
                break;
            case SynxValue.Str s when s.Value.Contains('\n'):
                WriteMultiline(o, key, s.Value, indent);
                break;
            default:
                if (Scalar(value) is { } text)
                    o.Append(' ', indent).Append(key).Append(' ').Append(text).Append('\n');
                else
                    // Not one SYNX token: an indent-preserving block, never comment-stripped nor cast.
                    WriteMultiline(o, key, value.AsString() ?? "", indent);
                break;
        }
    }

    /// <summary>Write one <c>- item</c> list entry.</summary>
    private static void WriteItem(StringBuilder o, SynxValue item, int indent, int depth)
    {
        if (depth > MaxWriteDepth) return;
        if (item is SynxValue.Obj obj)
        {
            var keys = SortedKeys(obj.Map);
            // An empty object in a list has no key for the `- ` line: a bare dash.
            if (keys.Length == 0)
            {
                o.Append(' ', indent).Append("-\n");
                return;
            }
            // The `- ` line carries a key whose value fits on it; SYNX attaches everything
            // deeper to the item, not to that key.
            var headSafe = Array.FindIndex(keys, k => DashLineSafe(obj.Map[k]));
            var head = headSafe < 0 ? 0 : headSafe;
            // §14.1 — an item is an object only when a deeper line follows its `- ` line;
            // writing the head again in full covers a one-key item and an item whose every
            // value is too big for the `- ` line (a repeated key is legal, the last wins).
            var repeatHead = headSafe < 0 || keys.Length == 1;
            var headKey = keys[head];
            o.Append(' ', indent).Append("- ").Append(headKey);
            if (Scalar(obj.Map[headKey]) is { Length: > 0 } text)
                o.Append(' ').Append(text);
            o.Append('\n');
            for (var i = 0; i < keys.Length; i++)
            {
                if (i == head && !repeatHead) continue;
                WriteEntry(o, keys[i], obj.Map[keys[i]], indent + 2, depth + 1);
            }
            return;
        }
        o.Append(' ', indent).Append("- ").Append(Scalar(item) ?? item.AsString() ?? "").Append('\n');
    }

    /// <summary>May this value share the <c>- </c> line of a list item?</summary>
    private static bool DashLineSafe(SynxValue value) => value switch
    {
        SynxValue.Obj o => o.Map.Count == 0,
        SynxValue.Arr => false,
        SynxValue.Str s => !s.Value.Contains('\n') && Scalar(value) != null && s.Value.Length > 0,
        _ => true,
    };

    /// <summary><c>key |+</c> plus an indented body (§14.3).</summary>
    private static void WriteMultiline(StringBuilder o, string key, string body, int indent)
    {
        o.Append(' ', indent).Append(key).Append(" |+\n");
        foreach (var line in body.Split('\n'))
            o.Append(' ', indent + 2).Append(line).Append('\n');
    }

    /// <summary>
    /// A scalar as one SYNX value token, or null when none can carry it. A value is
    /// comment-stripped at <c> #</c> / <c> //</c> before casting, so quoting cannot rescue those.
    /// </summary>
    private static string? Scalar(SynxValue value)
    {
        switch (value)
        {
            case SynxValue.Null: return "null";
            case SynxValue.Bool b: return b.Value ? "true" : "false";
            case SynxValue.Int n: return n.Value.ToString(CultureInfo.InvariantCulture);
            case SynxValue.Float f:
                // A non-finite float has no JSON form either; `null` is the projection-preserving token.
                if (!double.IsFinite(f.Value)) return "null";
                return WriteFloat(f.Value);
            // safety: a secret is written as its mask, never as the plaintext.
            case SynxValue.Secret: return "[SECRET]";
            case SynxValue.Str s:
            {
                var v = s.Value;
                if (v.Contains('\n') || v.Contains(" #", StringComparison.Ordinal) || v.Contains(" //", StringComparison.Ordinal))
                    return null;
                if (!NeedsQuote(v)) return v;
                if (!v.Contains('"')) return "\"" + v + "\"";
                if (!v.Contains('\'')) return "'" + v + "'";
                return null;
            }
            default:
                return null;
        }
    }

    /// <summary>A float the SYNX cast reads back as the same float: Rust <c>Display</c>, with <c>.0</c> when whole.</summary>
    private static string WriteFloat(double f)
    {
        var s = SynxNum.Display(f);
        return s.Contains('.') ? s : s + ".0";
    }

    /// <summary>Would this string be misread as a bare SYNX value?</summary>
    private static bool NeedsQuote(string s) =>
        s.Length == 0
        || SynxParserCore.IsWs(s[0]) || SynxParserCore.IsWs(s[^1])
        || s is "|" or "|+"
        || IsQuoteWrapped(s)
        || !AutoCastKeepsString(s);

    private static bool IsQuoteWrapped(string s) =>
        s.Length >= 2 && ((s[0] == '"' && s[^1] == '"') || (s[0] == '\'' && s[^1] == '\''));

    /// <summary>Does SYNX §8.3 casting leave <paramref name="s"/> a string?</summary>
    private static bool AutoCastKeepsString(string s)
    {
        if (IsQuoteWrapped(s)) return true;
        if (s is "true" or "false" or "null") return false;
        var start = s.Length > 0 && s[0] == '-' ? 1 : 0;
        if (s.Length <= start || !char.IsAsciiDigit(s[start])) return true;
        var dot = -1;
        for (var j = start; j < s.Length; j++)
        {
            var c = s[j];
            if (c == '.')
            {
                if (dot >= 0) return true;
                dot = j;
            }
            else if (!char.IsAsciiDigit(c)) return true;
        }
        if (dot < 0) return !SynxNum.TryParseI64(s, out _);
        return !(dot > start && dot < s.Length - 1);
    }

    internal static string FormatPrimitive(SynxValue value) => value switch
    {
        SynxValue.Str s => s.Value,
        SynxValue.Int i => i.Value.ToString(CultureInfo.InvariantCulture),
        SynxValue.Float f => WriteFloatPrimitive(f.Value),
        SynxValue.Bool b => b.Value ? "true" : "false",
        SynxValue.Null => "null",
        SynxValue.Arr a => "[" + string.Join(", ", a.Items.Select(FormatPrimitive)) + "]",
        SynxValue.Obj => "[Object]",
        SynxValue.Secret => "[SECRET]",
        _ => ""
    };

    private static string WriteFloatPrimitive(double f)
    {
        var s = SynxNum.Display(f);
        return s.Contains('.') ? s : s + ".0";
    }
}
