using System.Buffers;
using System.Globalization;
using System.Text;
using System.Text.Json;

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
            case SynxValue.Secret s:
                WriteString(o, s.Value);
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
                var keys = obj.Map.Keys.OrderBy(k => k, StringComparer.Ordinal).ToList();
                for (var i = 0; i < keys.Count; i++)
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
        // Match ECMA/JavaScript shortest representation (same idea as Rust ryu in synx-core).
        var buf = new ArrayBufferWriter<byte>();
        using (var jw = new Utf8JsonWriter(buf))
            jw.WriteNumberValue(f);
        var span = buf.WrittenSpan;
        o.Append(Encoding.UTF8.GetString(span));
    }

    internal static void WriteString(StringBuilder o, string str)
    {
        o.Append('"');
        foreach (var ch in str)
        {
            switch (ch)
            {
                case '"': o.Append("\\\""); break;
                case '\\': o.Append("\\\\"); break;
                case '\n': o.Append("\\n"); break;
                case '\r': o.Append("\\r"); break;
                case '\t': o.Append("\\t"); break;
                default:
                    if (ch < 0x20)
                        o.AppendFormat(CultureInfo.InvariantCulture, "\\u{0:x4}", (uint)ch);
                    else
                        o.Append(ch);
                    break;
            }
        }
        o.Append('"');
    }
}
