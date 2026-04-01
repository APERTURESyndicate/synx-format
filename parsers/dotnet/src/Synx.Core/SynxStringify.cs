using System.Globalization;
using System.Text;

namespace Synx;

/// <summary>
/// Serializes <see cref="SynxValue"/> trees back to canonical SYNX text.
/// Matches Rust <c>synx_core::serialize</c> output byte-for-byte:
/// sorted keys, 2-space indent, <c>- </c> list items, <c>|</c> multiline strings.
/// </summary>
internal static class SynxStringify
{
    private const int MaxDepth = 128;

    internal static string Serialize(SynxValue value, int depth)
    {
        if (depth > MaxDepth)
            return "[synx:max-depth]\n";

        if (value is not SynxValue.Obj obj)
            return FormatPrimitive(value);

        var sb = new StringBuilder();
        var indent = new string(' ', depth * 2);

        // Sort keys for deterministic output (matches Rust)
        var keys = obj.Map.Keys.ToList();
        keys.Sort(StringComparer.Ordinal);

        foreach (var key in keys)
        {
            var val = obj.Map[key];

            switch (val)
            {
                case SynxValue.Arr arr:
                    sb.Append(indent);
                    sb.Append(key);
                    sb.Append('\n');
                    foreach (var item in arr.Items)
                    {
                        if (item is SynxValue.Obj inner)
                        {
                            var entries = inner.Map.ToList();
                            if (entries.Count > 0)
                            {
                                sb.Append(indent);
                                sb.Append("  - ");
                                sb.Append(entries[0].Key);
                                sb.Append(' ');
                                sb.Append(FormatPrimitive(entries[0].Value));
                                sb.Append('\n');
                                for (int i = 1; i < entries.Count; i++)
                                {
                                    sb.Append(indent);
                                    sb.Append("    ");
                                    sb.Append(entries[i].Key);
                                    sb.Append(' ');
                                    sb.Append(FormatPrimitive(entries[i].Value));
                                    sb.Append('\n');
                                }
                            }
                        }
                        else
                        {
                            sb.Append(indent);
                            sb.Append("  - ");
                            sb.Append(FormatPrimitive(item));
                            sb.Append('\n');
                        }
                    }
                    break;

                case SynxValue.Obj _:
                    sb.Append(indent);
                    sb.Append(key);
                    sb.Append('\n');
                    sb.Append(Serialize(val, depth + 1));
                    break;

                case SynxValue.Str s when s.Value.Contains('\n'):
                    sb.Append(indent);
                    sb.Append(key);
                    sb.Append(" |\n");
                    foreach (var line in s.Value.Split('\n'))
                    {
                        sb.Append(indent);
                        sb.Append("  ");
                        sb.Append(line);
                        sb.Append('\n');
                    }
                    break;

                default:
                    sb.Append(indent);
                    sb.Append(key);
                    sb.Append(' ');
                    sb.Append(FormatPrimitive(val));
                    sb.Append('\n');
                    break;
            }
        }

        return sb.ToString();
    }

    internal static string FormatPrimitive(SynxValue value) => value switch
    {
        SynxValue.Str s => s.Value,
        SynxValue.Int i => i.Value.ToString(CultureInfo.InvariantCulture),
        SynxValue.Float f => FormatFloat(f.Value),
        SynxValue.Bool b => b.Value ? "true" : "false",
        SynxValue.Null => "null",
        SynxValue.Arr a => "[" + string.Join(", ", a.Items.Select(FormatPrimitive)) + "]",
        SynxValue.Obj => "[Object]",
        SynxValue.Secret => "[SECRET]",
        _ => ""
    };

    private static string FormatFloat(double f)
    {
        var s = f.ToString(CultureInfo.InvariantCulture);
        return s.Contains('.') ? s : s + ".0";
    }
}
