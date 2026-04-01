using System.Text;

namespace Synx;

/// <summary>
/// Canonical SYNX formatter — sorts keys, normalizes indentation, strips comments.
/// Matches Rust <c>fmt_canonical</c>.
/// </summary>
internal static class SynxFormatter
{
    private const int MaxDepth = 128;

    private static readonly HashSet<string> Directives = new(StringComparer.Ordinal)
    {
        "!active", "!lock", "!tool", "!schema", "!llm", "#!mode:active"
    };

    internal static string Format(string text)
    {
        var lines = text.Split('\n');
        var directives = new List<string>();
        int bodyStart = 0;

        for (int i = 0; i < lines.Length; i++)
        {
            var t = lines[i].Trim();
            if (Directives.Contains(t))
            {
                directives.Add(t);
                bodyStart = i + 1;
            }
            else if (t.Length == 0 || t.StartsWith('#') || t.StartsWith("//"))
            {
                bodyStart = i + 1;
            }
            else
            {
                break;
            }
        }

        var (nodes, _) = FmtParse(lines, bodyStart, 0, 0);
        FmtSort(nodes);

        var sb = new StringBuilder(Math.Max(64, text.Length));
        if (directives.Count > 0)
        {
            sb.AppendJoin('\n', directives);
            sb.Append("\n\n");
        }
        FmtEmit(nodes, 0, sb);

        var result = sb.ToString().TrimEnd();
        return result + "\n";
    }

    private sealed class FmtNode
    {
        public string Header = "";
        public List<FmtNode> Children = [];
        public List<string> ListItems = [];
        public bool IsMultiline;
    }

    private static int Indent(string line) => line.Length - line.TrimStart().Length;

    private static (List<FmtNode> nodes, int next) FmtParse(string[] lines, int start, int baseIndent, int depth)
    {
        if (depth > MaxDepth) return ([], start);
        var nodes = new List<FmtNode>();
        int i = start;
        while (i < lines.Length)
        {
            var raw = lines[i];
            var t = raw.Trim();
            if (t.Length == 0) { i++; continue; }
            var ind = Indent(raw);
            if (ind < baseIndent) break;
            if (ind > baseIndent) { i++; continue; }
            if (t.StartsWith("- ") || t.StartsWith('#') || t.StartsWith("//")) { i++; continue; }

            var isMultiline = t.EndsWith(" |") || t == "|";
            var node = new FmtNode { Header = t, IsMultiline = isMultiline };
            i++;

            while (i < lines.Length)
            {
                var cr = lines[i];
                var ct = cr.Trim();
                if (ct.Length == 0) { i++; continue; }
                var ci = Indent(cr);
                if (ci <= baseIndent) break;

                if (node.IsMultiline || ct.StartsWith("- "))
                {
                    node.ListItems.Add(ct);
                    i++;
                }
                else if (ct.StartsWith('#') || ct.StartsWith("//"))
                {
                    i++;
                }
                else
                {
                    var (subs, ni) = FmtParse(lines, i, ci, depth + 1);
                    node.Children.AddRange(subs);
                    i = ni;
                }
            }
            nodes.Add(node);
        }
        return (nodes, i);
    }

    private static void FmtSort(List<FmtNode> nodes)
    {
        nodes.Sort((a, b) =>
        {
            var ka = ExtractKey(a.Header);
            var kb = ExtractKey(b.Header);
            return string.Compare(ka, kb, StringComparison.OrdinalIgnoreCase);
        });
        foreach (var n in nodes) FmtSort(n.Children);
    }

    private static string ExtractKey(string header)
    {
        foreach (var c in header)
        {
            if (char.IsWhiteSpace(c) || c == '[' || c == ':' || c == '(')
                return header[..header.IndexOf(c)];
        }
        return header;
    }

    private static void FmtEmit(List<FmtNode> nodes, int indent, StringBuilder sb)
    {
        var sp = new string(' ', indent);
        var itemSp = new string(' ', indent + 2);
        foreach (var n in nodes)
        {
            sb.Append(sp);
            sb.Append(n.Header);
            sb.Append('\n');
            if (n.Children.Count > 0)
                FmtEmit(n.Children, indent + 2, sb);
            foreach (var li in n.ListItems)
            {
                sb.Append(itemSp);
                sb.Append(li);
                sb.Append('\n');
            }
            if (indent == 0 && (n.Children.Count > 0 || n.ListItems.Count > 0))
                sb.Append('\n');
        }
    }
}
