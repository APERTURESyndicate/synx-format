using System.Globalization;
using System.Text;

namespace Synx;

/// <summary>Deterministic JSON matching <c>synx_core::write_json</c> (sorted object keys, same escapes).</summary>
public static class SynxJson
{
    public static string ToJson(SynxValue value)
    {
        var sb = new StringBuilder(2048);
        WriteJson(sb, value);
        return sb.ToString();
    }

    public static string ToJson(Dictionary<string, SynxValue> map)
    {
        return ToJson(new SynxValue.Obj(map));
    }

    internal static void WriteJson(StringBuilder o, SynxValue val)
    {
        switch (val)
        {
            case SynxValue.Null:
                o.Append("null");
                break;
            case SynxValue.Bool b:
                o.Append(b.Value ? "true" : "false");
                break;
            case SynxValue.Int n:
                o.Append(n.Value.ToString(CultureInfo.InvariantCulture));
                break;
            case SynxValue.Float f:
                WriteDouble(o, f.Value);
                break;
            case SynxValue.Str s:
                WriteString(o, s.Value);
                break;
            case SynxValue.Secret _:
                // Secrets MUST never appear in JSON output. Match synx_core::write_json:
                // emit a fixed redaction marker; the typed Value API must be used to access
                // the underlying secret directly.
                o.Append("\"[SECRET]\"");
                break;
            case SynxValue.Arr a:
                o.Append('[');
                for (var i = 0; i < a.Items.Count; i++)
                {
                    if (i > 0) o.Append(',');
                    WriteJson(o, a.Items[i]);
                }
                o.Append(']');
                break;
            case SynxValue.Obj obj:
                o.Append('{');
                var keys = new string[obj.Map.Count];
                obj.Map.Keys.CopyTo(keys, 0);
                SynxSort.Keys(keys);
                for (var i = 0; i < keys.Length; i++)
                {
                    if (i > 0) o.Append(',');
                    WriteString(o, keys[i]);
                    o.Append(':');
                    WriteJson(o, obj.Map[keys[i]]);
                }
                o.Append('}');
                break;
            default:
                throw new ArgumentOutOfRangeException(nameof(val));
        }
    }

    internal static void WriteDouble(StringBuilder o, double f)
    {
        // JSON has no NaN / ±Infinity literals — emit null so output stays valid.
        if (double.IsNaN(f) || double.IsInfinity(f))
        {
            o.Append("null");
            return;
        }
        // The ryu layout synx-core writes: 1.0, 0.25, 1e-7, 1.2345678901234569e23.
        SynxNum.AppendRyu(o, f);
    }

    internal static void WriteString(StringBuilder o, string str)
    {
        o.Append('"');
        // perf: unescaped runs are copied whole.
        var start = 0;
        for (var i = 0; i < str.Length; i++)
        {
            var ch = str[i];
            if (ch >= 0x20 && ch != '"' && ch != '\\') continue;
            o.Append(str, start, i - start);
            switch (ch)
            {
                case '"': o.Append("\\\""); break;
                case '\\': o.Append("\\\\"); break;
                case '\n': o.Append("\\n"); break;
                case '\r': o.Append("\\r"); break;
                case '\t': o.Append("\\t"); break;
                default:
                    o.Append("\\u00").Append(Hex[ch >> 4]).Append(Hex[ch & 0xF]);
                    break;
            }
            start = i + 1;
        }
        o.Append(str, start, str.Length - start);
        o.Append('"');
    }

    private const string Hex = "0123456789abcdef";
}
