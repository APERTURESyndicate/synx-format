using System.Text;

namespace Synx;

/// <summary>
/// Canonical SYNX formatter — sorts keys, normalizes indentation, strips comments.
/// Matches Rust <c>fmt_canonical</c> byte for byte.
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
        text = SynxParserCore.ClampSynxText(text, out _);
        var lines = RustLines(text);
        var directives = new List<string>();
        var bodyStart = 0;

        for (var i = 0; i < lines.Count; i++)
        {
            var t = lines[i].Trim();
            if (Directives.Contains(t))
            {
                directives.Add(t);
                bodyStart = i + 1;
            }
            else if (t.Length == 0 || t.StartsWith('#') || t.StartsWith("//", StringComparison.Ordinal))
            {
                bodyStart = i + 1;
            }
            else
            {
                break;
            }
        }

        var (nodes, _) = FmtParse(lines, bodyStart, 0, 0);
        FmtSort(nodes, 0);

        var sb = new StringBuilder(Math.Max(64, text.Length));
        if (directives.Count > 0)
        {
            sb.AppendJoin('\n', directives);
            sb.Append("\n\n");
        }
        FmtEmit(nodes, 0, sb);

        var end = sb.Length;
        while (end > 0 && SynxParserCore.IsWs(sb[end - 1])) end--;
        sb.Length = end;
        return sb.Append('\n').ToString();
    }

    /// <summary>Rust <c>str::lines</c>: split at <c>\n</c>, drop a trailing <c>\r</c>, no final empty line.</summary>
    private static List<string> RustLines(string text)
    {
        var lines = new List<string>();
        var start = 0;
        while (start < text.Length)
        {
            var nl = text.IndexOf('\n', start);
            var end = nl < 0 ? text.Length : nl;
            var e = end > start && text[end - 1] == '\r' ? end - 1 : end;
            lines.Add(text[start..e]);
            if (nl < 0) break;
            start = nl + 1;
        }
        return lines;
    }

    private sealed class FmtNode
    {
        public string Header = "";
        public string SortKey = "";
        public readonly List<FmtNode> Children = [];
        public readonly List<string> ListItems = [];
        public bool IsMultiline;
    }

    /// <summary>Indentation in UTF-8 bytes of leading whitespace, as in the parser.</summary>
    private static int Indent(string line)
    {
        var n = 0;
        foreach (var c in line)
        {
            if (!SynxParserCore.IsWs(c)) break;
            n += SynxNum.Utf8Len(c);
        }
        return n;
    }

    /// <summary>Length in UTF-16 units of at most <paramref name="limit"/> UTF-8 bytes of leading whitespace, by whole characters.</summary>
    private static int WsPrefixLen(string s, int limit)
    {
        var off = 0;
        var i = 0;
        while (i < s.Length && off < limit && SynxParserCore.IsWs(s[i]))
        {
            off += SynxNum.Utf8Len(s[i]);
            i++;
        }
        return i;
    }

    private static (List<FmtNode> nodes, int next) FmtParse(List<string> lines, int start, int baseIndent, int depth)
    {
        if (depth > MaxDepth)
        {
            // Past the cap the subtree is dropped, but its lines are consumed: everything
            // up to the next non-blank line shallower than `baseIndent` (Rust `fmt_parse`).
            var j = start;
            while (j < lines.Count && (lines[j].Trim().Length == 0 || Indent(lines[j]) >= baseIndent))
                j++;
            return ([], j);
        }
        var nodes = new List<FmtNode>();
        var i = start;
        while (i < lines.Count)
        {
            var raw = lines[i];
            var t = raw.Trim();
            if (t.Length == 0) { i++; continue; }
            var ind = Indent(raw);
            if (ind < baseIndent) break;
            if (ind > baseIndent) { i++; continue; }
            if (t.StartsWith("- ", StringComparison.Ordinal) || t.StartsWith('#') || t.StartsWith("//", StringComparison.Ordinal)) { i++; continue; }

            // spec: `|+` opens a block too; its body is text with its relative indent kept,
            // never child keys to sort.
            var preserve = t.EndsWith(" |+", StringComparison.Ordinal) || t == "|+";
            var node = new FmtNode
            {
                Header = t,
                SortKey = SortKey(t),
                IsMultiline = preserve || t.EndsWith(" |", StringComparison.Ordinal) || t == "|",
            };
            var blockBase = -1;
            i++;

            while (i < lines.Count)
            {
                var cr = lines[i];
                var ct = cr.Trim();
                if (ct.Length == 0) { i++; continue; }
                var ci = Indent(cr);
                if (ci <= baseIndent) break;

                if (preserve)
                {
                    if (blockBase < 0) blockBase = ci;
                    var bodyEnd = cr.Length;
                    while (bodyEnd > 0 && SynxParserCore.IsWs(cr[bodyEnd - 1])) bodyEnd--;
                    var body = cr[..bodyEnd];
                    node.ListItems.Add(body[WsPrefixLen(body, Math.Min(blockBase, ci))..]);
                    i++;
                }
                else if (node.IsMultiline || ct.StartsWith("- ", StringComparison.Ordinal))
                {
                    node.ListItems.Add(ct);
                    i++;
                }
                else if (ct.StartsWith('#') || ct.StartsWith("//", StringComparison.Ordinal))
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

    /// <summary>The key of a header, lowercased: up to the first whitespace, <c>[</c>, <c>:</c> or <c>(</c>.</summary>
    private static string SortKey(string header)
    {
        var end = 0;
        while (end < header.Length && !SynxParserCore.IsWs(header[end]) && header[end] is not ('[' or ':' or '('))
            end++;
        return header[..end].ToLowerInvariant();
    }

    private static void FmtSort(List<FmtNode> nodes, int depth)
    {
        if (depth > MaxDepth) return;
        // Stable, by code point: equal keys (a repeated key) keep document order, so the last still wins.
        if (nodes.Count > 1)
        {
            var sorted = nodes.Select((n, i) => (n, i)).ToArray();
            Array.Sort(sorted, (a, b) =>
            {
                var c = SynxEngine.CompareCodePoints(a.n.SortKey, b.n.SortKey);
                return c != 0 ? c : a.i.CompareTo(b.i);
            });
            for (var i = 0; i < sorted.Length; i++) nodes[i] = sorted[i].n;
        }
        foreach (var n in nodes) FmtSort(n.Children, depth + 1);
    }

    private static void FmtEmit(List<FmtNode> nodes, int indent, StringBuilder sb)
    {
        foreach (var n in nodes)
        {
            sb.Append(' ', indent).Append(n.Header).Append('\n');
            if (n.Children.Count > 0)
                FmtEmit(n.Children, indent + 2, sb);
            foreach (var li in n.ListItems)
                sb.Append(' ', indent + 2).Append(li).Append('\n');
            if (indent == 0 && (n.Children.Count > 0 || n.ListItems.Count > 0))
                sb.Append('\n');
        }
    }
}
