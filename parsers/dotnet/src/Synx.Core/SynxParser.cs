using System.Globalization;
using System.Text;

namespace Synx;

internal static class SynxParserCore
{
    // Resource limits — the same caps as synx-core `parser.rs`, so the same input is
    // accepted, truncated or rejected identically across implementations.
    internal const int MaxParseNestingDepth = 128;
    internal const int MaxSynxInputBytes = 16 * 1024 * 1024;
    private const int MaxLineStarts = 2_000_000;
    private const int MaxMultilineBlockBytes = 1024 * 1024;
    private const int MaxListItems = 1_048_576;
    private const int MaxIncludeDirectives = 4096;
    private const int MaxConstraintEnumParts = 4096;
    private const int MaxMarkerChainSegments = 512;

    /// <summary>
    /// Drop a leading U+FEFF (SYNX §2.1) and cut the text to 16 MiB of UTF-8 without
    /// splitting a character. <c>string.Trim</c> does not treat U+FEFF as whitespace,
    /// so a BOM would otherwise stay glued to the first key or hide <c>!active</c>.
    /// </summary>
    internal static string ClampSynxText(string text, out bool truncated)
    {
        truncated = false;
        // A dropped BOM is not truncation: synx-core measures the original length after it.
        if (text.Length > 0 && text[0] == (char)0xFEFF)
            text = text[1..];
        if (text.Length <= MaxSynxInputBytes / 3 || SynxNum.Utf8Len(text) <= MaxSynxInputBytes)
            return text;
        truncated = true;
        var bytes = 0;
        var i = 0;
        while (i < text.Length)
        {
            var n = char.IsHighSurrogate(text[i]) && i + 1 < text.Length && char.IsLowSurrogate(text[i + 1]) ? 2 : 1;
            var b = n == 2 ? 4 : SynxNum.Utf8Len(text[i]);
            if (bytes + b > MaxSynxInputBytes) break;
            bytes += b;
            i += n;
        }
        return text[..i];
    }

    /// <summary>SYNX whitespace: Unicode White_Space, the set Rust <c>char::is_whitespace</c> accepts.</summary>
    internal static bool IsWs(char c) => c <= ' ' ? c == ' ' || (c >= '\t' && c <= '\r') : c >= '\u0085' && char.IsWhiteSpace(c);

    private static bool IsReservedJsKey(string key) => key is "__proto__" or "constructor" or "prototype";

    internal static SynxParseResult Parse(string text)
    {
        text = ClampSynxText(text, out var truncated);
        // Keep at most MaxLineStarts lines: cut before the newline that would start one more.
        {
            var seen = 0;
            var scan = 0;
            while (true)
            {
                var nl = text.IndexOf('\n', scan);
                if (nl < 0) break;
                if (seen >= MaxLineStarts - 1)
                {
                    text = text[..nl];
                    truncated = true;
                    break;
                }
                seen++;
                scan = nl + 1;
            }
        }

        var root = new Dictionary<string, SynxValue>(StringComparer.Ordinal);
        var stack = new List<Frame>(16) { new(-1, FrameKind.Root, "", 0, 0, root) { Path = "" } };
        var metadata = new Dictionary<string, Dictionary<string, SynxMeta>>(StringComparer.Ordinal);
        var includes = new List<SynxIncludeDirective>();
        var uses = new List<SynxUseDirective>();
        var mode = SynxMode.Static;
        var locked = false;
        var tool = false;
        var schema = false;
        var llm = false;

        BlockState? block = null;
        // Open lists, outermost first.
        // spec: a list inside a list item ends at the next item of the outer list (§8.5).
        // One variable held "the" open list, so a nested `tags` replaced its parent:
        // `- name b` after it matched no list and was dropped without a word, and the keys
        // under it landed in the item before.
        var lists = new List<ListState>();
        var inBlockComment = false;
        // Indent of a rejected key whose block is being dropped (-1: none).
        var droppedBelow = -1;

        var len = text.Length;
        var pos = 0;
        while (pos <= len)
        {
            // perf: lines are walked as offsets; a line is copied only where it is stored.
            var start = pos;
            var nlIdx = text.IndexOf('\n', start);
            var nl = nlIdx < 0 ? len : nlIdx;
            pos = nl + 1;
            var end = nl > start && text[nl - 1] == '\r' ? nl - 1 : nl;

            var ts = start;
            while (ts < end && IsWs(text[ts])) ts++;
            // A blank line is never structural and never body (SYNX §8.4.1).
            if (ts == end)
                continue;
            var te = end;
            while (te > ts && IsWs(text[te - 1])) te--;

            // Indentation is the UTF-8 byte length of the leading whitespace (SYNX §4).
            var indent = 0;
            for (var k = start; k < ts; k++) indent += SynxNum.Utf8Len(text[k]);

            // safety: the lines under a rejected key go with it; skipping only the key line
            // handed its block to whatever was open above it (B-076, as synx-core).
            if (droppedBelow >= 0)
            {
                if (indent > droppedBelow)
                    continue;
                droppedBelow = -1;
            }

            // Block continuation runs before every line-class rule below: a `|` / `|+`
            // body is raw text, so `#`, `//`, `###` and `!…` are ordinary content there.
            if (block is { } blk)
            {
                if (indent > blk.Indent)
                {
                    if (blk.Bytes < MaxMultilineBlockBytes)
                    {
                        if (blk.Content.Length > 0)
                        {
                            blk.Content.Append('\n');
                            blk.Bytes++;
                        }
                        var room = MaxMultilineBlockBytes - blk.Bytes;
                        if (room > 0)
                        {
                            int sliceStart, sliceEnd;
                            if (blk.PreserveIndent)
                            {
                                // `|+` (SYNX 3.7): lock the strip prefix to the indent of the first
                                // continuation line and cut that many bytes of whitespace — by whole
                                // characters — from each line. Spec §8.4.1.
                                if (blk.BaseIndent < 0) blk.BaseIndent = indent;
                                var strip = Math.Min(blk.BaseIndent, indent);
                                var rawEnd = end;
                                while (rawEnd > start && IsWs(text[rawEnd - 1])) rawEnd--;
                                var cut = start;
                                var off = 0;
                                while (cut < rawEnd && off < strip && IsWs(text[cut]))
                                {
                                    off += SynxNum.Utf8Len(text[cut]);
                                    cut++;
                                }
                                sliceStart = cut;
                                sliceEnd = rawEnd;
                            }
                            else
                            {
                                sliceStart = ts;
                                sliceEnd = te;
                            }
                            var take = sliceStart;
                            var used = 0;
                            while (take < sliceEnd)
                            {
                                var pair = char.IsHighSurrogate(text[take]) && take + 1 < sliceEnd && char.IsLowSurrogate(text[take + 1]);
                                var b = pair ? 4 : SynxNum.Utf8Len(text[take]);
                                if (used + b > room) break;
                                used += b;
                                take += pair ? 2 : 1;
                            }
                            blk.Content.Append(text, sliceStart, take - sliceStart);
                            blk.Bytes += used;
                            if (take < sliceEnd) truncated = true;
                        }
                    }
                    else
                    {
                        truncated = true;
                    }
                    continue;
                }

                block = null;
                Insert(stack, blk.StackIdx, blk.Key, new SynxValue.Str(blk.Content.ToString()));
            }

            var trimmed = text.AsSpan(ts, te - ts);

            if (trimmed.SequenceEqual("###"))
            {
                inBlockComment = !inBlockComment;
                continue;
            }
            if (inBlockComment)
                continue;

            // Directive lines (§6). Every form starts with `!` or `#!mode:`; unknown `!…`
            // lines fall through to the generic line handling.
            if (trimmed[0] == '!' || trimmed.StartsWith("#!mode:"))
            {
                if (trimmed.SequenceEqual("!active")) { mode = SynxMode.Active; continue; }
                if (trimmed.SequenceEqual("!lock")) { locked = true; continue; }
                if (trimmed.SequenceEqual("!tool")) { tool = true; continue; }
                if (trimmed.SequenceEqual("!schema")) { schema = true; continue; }
                if (trimmed.SequenceEqual("!llm")) { llm = true; continue; }
                if (trimmed.StartsWith("!include "))
                {
                    if (includes.Count < MaxIncludeDirectives)
                    {
                        var rest = TrimWs(trimmed[9..]);
                        var ws = 0;
                        while (ws < rest.Length && !IsWs(rest[ws])) ws++;
                        var incPath = rest[..ws].ToString();
                        var alias = ws < rest.Length ? TrimWs(rest[(ws + 1)..]).ToString() : DeriveIncludeAlias(incPath);
                        includes.Add(new SynxIncludeDirective { Path = incPath, Alias = alias });
                    }
                    continue;
                }
                if (trimmed.StartsWith("!use "))
                {
                    var rest = TrimWs(trimmed[5..]).ToString();
                    if (rest.StartsWith('@'))
                    {
                        // Parse: !use @scope/name [as alias]
                        var asIdx = rest.IndexOf(" as ", StringComparison.Ordinal);
                        var package = (asIdx < 0 ? rest : rest[..asIdx]).Trim();
                        var alias = asIdx >= 0 ? rest[(asIdx + 4)..].Trim() : package[(package.LastIndexOf('/') + 1)..];
                        if (package.Length > 0)
                            uses.Add(new SynxUseDirective { Package = package, Alias = alias });
                    }
                    continue;
                }
                if (trimmed.StartsWith("#!mode:"))
                {
                    var declared = TrimWs(trimmed[7..]);
                    mode = declared.SequenceEqual("active") ? SynxMode.Active : SynxMode.Static;
                    continue;
                }
            }

            if (trimmed[0] == '#' || trimmed.StartsWith("//"))
                continue;

            if (trimmed.StartsWith("- "))
            {
                // The list this item belongs to: the innermost open one whose key sits to the
                // left of the dash. Lists nested deeper than the dash end here — but only when
                // a list does take the line; a dash no list accepts is skipped and closes
                // nothing, as it always was.
                var accepting = lists.Count;
                while (accepting > 0 && indent < lists[accepting - 1].Indent) accepting--;
                if (accepting > 0 && indent > lists[accepting - 1].Indent)
                    lists.RemoveRange(accepting, lists.Count - accepting);
                if (lists.Count > 0 && indent > lists[^1].Indent)
                {
                    var lst = lists[^1];
                    // Pop every frame of the previous item — its own frame and any object
                    // nested inside it; popping only item frames stopped at a nested object,
                    // so the next item's keys were looked up through it and dropped.
                    while (stack.Count > 1 && stack[^1].Indent >= indent)
                        stack.RemoveAt(stack.Count - 1);

                    var valStr = StripComment(TrimWs(trimmed[2..]));

                    // The item is an object when the next line of content is deeper and is
                    // not another item.
                    var nested = false;
                    if (NextContentLine(text, pos) is var (ps, pe) && ps >= 0)
                    {
                        var pts = ps;
                        var pi = 0;
                        while (pts < pe && IsWs(text[pts])) { pi += SynxNum.Utf8Len(text[pts]); pts++; }
                        if (pi > indent && !text.AsSpan(pts, pe - pts).StartsWith("- "))
                            nested = true;
                    }

                    var listParent = stack[lst.StackIdx].Map;
                    if (!listParent.TryGetValue(lst.Key, out var existing))
                    {
                        existing = new SynxValue.Arr([]);
                        listParent[lst.Key] = existing;
                    }
                    // A key re-declared under the open list as something else takes the items
                    // no more: they are dropped, never written into a detached array.
                    if (existing is not SynxValue.Arr items)
                        continue;
                    if (items.Items.Count >= MaxListItems)
                    {
                        // The cap dropped an item: say so, like every other cap.
                        truncated = true;
                        continue;
                    }
                    if (nested)
                    {
                        var itemObj = new Dictionary<string, SynxValue>(StringComparer.Ordinal);
                        string? headKey = null;
                        SynxMeta? headMeta = null;
                        if (ParseLine(valStr, out var itemLine))
                        {
                            SynxValue itemVal = itemLine.TypeHint is { } ih
                                ? CastTyped(itemLine.Value, ih)
                                : itemLine.Value.Length > 0
                                    ? Cast(itemLine.Value)
                                    : new SynxValue.Obj(new Dictionary<string, SynxValue>(StringComparer.Ordinal));
                            if (!IsReservedJsKey(itemLine.Key))
                            {
                                itemObj[itemLine.Key] = itemVal;
                                if (mode == SynxMode.Active
                                    && (itemLine.Markers.Count > 0 || itemLine.Constraints != null || itemLine.TypeHint != null))
                                {
                                    headKey = itemLine.Key;
                                    headMeta = new SynxMeta
                                    {
                                        Markers = ReferenceEquals(itemLine.Markers, s_noMarkers) ? [] : itemLine.Markers,
                                        Args = ReferenceEquals(itemLine.MarkerArgs, s_noMarkers) ? [] : itemLine.MarkerArgs,
                                        TypeHint = itemLine.TypeHint,
                                        Constraints = itemLine.Constraints,
                                    };
                                }
                            }
                        }
                        else
                        {
                            itemObj["_value"] = Cast(valStr);
                        }
                        var itemIdx = items.Items.Count;
                        items.Items.Add(new SynxValue.Obj(itemObj));
                        if (stack.Count < MaxParseNestingDepth)
                        {
                            stack.Add(new Frame(indent, FrameKind.ListItem, lst.Key, itemIdx, lst.StackIdx, itemObj));
                            // spec: the key on the `- ` line is a key line like the ones under
                            // it. Its markers and constraints were read and thrown away:
                            // `- token:secret abc` stayed in the clear and `- host:env H` kept
                            // the variable's name.
                            if (headMeta != null)
                            {
                                var itemPath = PathOf(stack, stack.Count - 1);
                                if (!metadata.TryGetValue(itemPath, out var itemMeta))
                                {
                                    itemMeta = new Dictionary<string, SynxMeta>(StringComparer.Ordinal);
                                    metadata[itemPath] = itemMeta;
                                }
                                itemMeta[headKey!] = headMeta;
                            }
                        }
                    }
                    else
                    {
                        items.Items.Add(Cast(valStr));
                    }
                    continue;
                }
            }
            else
            {
                // A non-item line closes every list it is at or below the indent of.
                var closed = false;
                while (lists.Count > 0 && indent <= lists[^1].Indent)
                {
                    lists.RemoveAt(lists.Count - 1);
                    closed = true;
                }
                if (closed)
                {
                    while (stack.Count > 1 && stack[^1].Kind == FrameKind.ListItem && stack[^1].Indent >= indent)
                        stack.RemoveAt(stack.Count - 1);
                }
            }

            if (!ParseLine(trimmed, out var parsed))
                continue;

            // Reject prototype-polluting keys so JSON consumers (notably JS) are never
            // exposed to __proto__ / constructor / prototype injection.
            if (IsReservedJsKey(parsed.Key))
            {
                droppedBelow = indent;
                continue;
            }

            while (stack.Count > 1 && stack[^1].Indent >= indent)
                stack.RemoveAt(stack.Count - 1);

            var parentIdx = stack.Count - 1;
            var parentMap = stack[parentIdx].Map;

            if (mode == SynxMode.Active && metadata.Count > 0
                && parentMap.TryGetValue(parsed.Key, out var replaced) && replaced is SynxValue.Obj or SynxValue.Arr)
            {
                // A key that replaces an object or a list replaces its declarations too:
                // metadata is filed by path, so the old subtree's markers and constraints
                // used to apply to whatever took its place.
                var parentPath = PathOf(stack, parentIdx);
                var sb = new StringBuilder(parentPath);
                if (sb.Length > 0) sb.Append('.');
                AppendKeySegment(sb, parsed.Key);
                var prefix = sb.ToString();
                List<string>? drop = null;
                foreach (var p in metadata.Keys)
                {
                    if (p == prefix || (p.Length > prefix.Length && p[prefix.Length] == '.' && p.StartsWith(prefix, StringComparison.Ordinal)))
                        (drop ??= []).Add(p);
                }
                if (drop != null)
                    foreach (var p in drop) metadata.Remove(p);
            }

            if (mode == SynxMode.Active && (parsed.Markers.Count > 0 || parsed.Constraints != null || parsed.TypeHint != null))
            {
                var path = PathOf(stack, parentIdx);
                if (!metadata.TryGetValue(path, out var mm))
                {
                    mm = new Dictionary<string, SynxMeta>(StringComparer.Ordinal);
                    metadata[path] = mm;
                }
                mm[parsed.Key] = new SynxMeta
                {
                    Markers = ReferenceEquals(parsed.Markers, s_noMarkers) ? [] : parsed.Markers,
                    Args = ReferenceEquals(parsed.MarkerArgs, s_noMarkers) ? [] : parsed.MarkerArgs,
                    TypeHint = parsed.TypeHint,
                    Constraints = parsed.Constraints,
                };
            }

            // `|` (3.6) trims each continuation line; `|+` (3.7) keeps the indent relative
            // to the first one. Spec §8.4.1.
            var isBlock = parsed.Value.SequenceEqual("|") || parsed.Value.SequenceEqual("|+");
            var isListMarker = false;
            foreach (var m in parsed.Markers)
            {
                if (m is "random" or "unique" or "geo" or "join") { isListMarker = true; break; }
            }

            if (isBlock)
            {
                parentMap[parsed.Key] = new SynxValue.Str("");
                block = new BlockState
                {
                    Indent = indent,
                    Key = parsed.Key,
                    Content = new StringBuilder(),
                    StackIdx = parentIdx,
                    PreserveIndent = parsed.Value.SequenceEqual("|+"),
                };
            }
            else if (isListMarker && parsed.Value.Length == 0)
            {
                // Insert an empty array now so the key exists even if the list stays empty.
                parentMap[parsed.Key] = new SynxValue.Arr([]);
                lists.Add(new ListState { Indent = indent, Key = parsed.Key, StackIdx = parentIdx });
            }
            else if (parsed.Value.Length == 0)
            {
                if (NextContentLine(text, pos) is var (ps, pe) && ps >= 0 && TrimWs(text.AsSpan(ps, pe - ps)).StartsWith("- "))
                {
                    parentMap[parsed.Key] = new SynxValue.Arr([]);
                    lists.Add(new ListState { Indent = indent, Key = parsed.Key, StackIdx = parentIdx });
                    continue;
                }

                var obj = new Dictionary<string, SynxValue>(StringComparer.Ordinal);
                parentMap[parsed.Key] = new SynxValue.Obj(obj);
                // Past the cap the object is still inserted, but nesting stops increasing.
                if (stack.Count < MaxParseNestingDepth)
                    stack.Add(new Frame(indent, FrameKind.Key, parsed.Key, 0, parentIdx, obj));
            }
            else
            {
                parentMap[parsed.Key] = parsed.TypeHint is { } hint
                    ? CastTyped(parsed.Value, hint)
                    : Cast(parsed.Value);
            }
        }

        if (block is { } blkEnd)
            Insert(stack, blkEnd.StackIdx, blkEnd.Key, new SynxValue.Str(blkEnd.Content.ToString()));

        return new SynxParseResult
        {
            Root = new SynxValue.Obj(root),
            Mode = mode,
            Locked = locked,
            Tool = tool,
            Schema = schema,
            Llm = llm,
            Metadata = metadata,
            Includes = includes,
            Uses = uses,
            Truncated = truncated,
        };
    }

    private static void Insert(List<Frame> stack, int idx, string key, SynxValue value)
    {
        if (idx < stack.Count)
            stack[idx].Map[key] = value;
    }

    /// <summary>
    /// The first line at or after <paramref name="from"/> that is content — not blank and not
    /// a comment — CR excluded; (-1, -1) when none.
    /// spec: a comment line is ignored (§5), so it cannot stand between a key and its list or
    /// between a `- ` line and the keys of its item. Only blank lines were skipped here:
    /// `items` / `# note` / `- a` read as an empty object and lost every item, and a comment
    /// after `- name a` turned the item into the text `name a` and sent its keys to the parent.
    /// </summary>
    private static (int Start, int End) NextContentLine(string text, int from)
    {
        var p = from;
        var inComment = false;
        while (p <= text.Length)
        {
            var nlIdx = p < text.Length ? text.IndexOf('\n', p) : -1;
            var nl = nlIdx < 0 ? text.Length : nlIdx;
            var end = nl > p && text[nl - 1] == '\r' ? nl - 1 : nl;
            var t = TrimWs(text.AsSpan(p, end - p));
            if (t.SequenceEqual("###"))
                inComment = !inComment;
            else if (!inComment && !t.IsEmpty && t[0] != '#' && !t.StartsWith("//"))
                return (p, end);
            p = nl + 1;
        }
        return (-1, -1);
    }

    internal static ReadOnlySpan<char> TrimWs(ReadOnlySpan<char> s)
    {
        var a = 0;
        var b = s.Length;
        while (a < b && IsWs(s[a])) a++;
        while (b > a && IsWs(s[b - 1])) b--;
        return s[a..b];
    }

    private static string DeriveIncludeAlias(string path)
    {
        var name = path[(path.LastIndexOfAny(['/', '\\']) + 1)..];
        if (name.EndsWith(".synx", StringComparison.Ordinal) || name.EndsWith(".SYNX", StringComparison.Ordinal))
            return name[..^5];
        return name;
    }

    // ── Metadata paths ──

    /// <summary>
    /// The metadata path of frame <paramref name="idx"/>: object keys, and for a list item
    /// the list's key followed by <c>#&lt;index&gt;</c>. Follows the frame chain, so an
    /// unrelated object opened between a list key and its item stays out of it.
    /// </summary>
    private static string PathOf(List<Frame> stack, int idx)
    {
        var f = stack[idx];
        if (f.Path != null) return f.Path;
        var parentPath = PathOf(stack, f.Kind == FrameKind.ListItem ? f.Parent : idx - 1);
        var sb = new StringBuilder(parentPath.Length + f.Key.Length + 8);
        sb.Append(parentPath);
        if (sb.Length > 0) sb.Append('.');
        AppendKeySegment(sb, f.Key);
        if (f.Kind == FrameKind.ListItem)
            sb.Append(".#").Append(f.ItemIdx.ToString(CultureInfo.InvariantCulture));
        f.Path = sb.ToString();
        return f.Path;
    }

    /// <summary>
    /// Append <paramref name="key"/> as one path segment: <c>.</c> and <c>\</c> are escaped
    /// with <c>\</c>, so the object <c>a.b</c> and <c>b</c> inside <c>a</c> never share a path.
    /// </summary>
    internal static void AppendKeySegment(StringBuilder sb, string key)
    {
        if (key.AsSpan().IndexOfAny('.', '\\') < 0)
        {
            sb.Append(key);
            return;
        }
        foreach (var c in key)
        {
            if (c is '.' or '\\') sb.Append('\\');
            sb.Append(c);
        }
    }

    internal static string KeySegment(string key)
    {
        if (key.AsSpan().IndexOfAny('.', '\\') < 0) return key;
        var sb = new StringBuilder(key.Length + 2);
        AppendKeySegment(sb, key);
        return sb.ToString();
    }

    /// <summary>The unescaped segments of a metadata path: <c>a\.b.c</c> is <c>["a.b", "c"]</c>.</summary>
    internal static List<string> MetaPathSegments(string path)
    {
        var outList = new List<string>();
        if (path.Length == 0) return outList;
        var seg = new StringBuilder();
        for (var i = 0; i < path.Length; i++)
        {
            var c = path[i];
            if (c == '\\')
            {
                if (i + 1 < path.Length) seg.Append(path[++i]);
            }
            else if (c == '.')
            {
                outList.Add(seg.ToString());
                seg.Clear();
            }
            else seg.Append(c);
        }
        outList.Add(seg.ToString());
        return outList;
    }

    private enum FrameKind { Root, Key, ListItem }

    /// <summary>
    /// One open object. <see cref="Map"/> is the object itself: a frame lives only while
    /// its object is attached (anything that could replace it pops the frame first), so
    /// holding it equals navigating to it from the root.
    /// </summary>
    private sealed class Frame(int indent, FrameKind kind, string key, int itemIdx, int parent, Dictionary<string, SynxValue> map)
    {
        public readonly int Indent = indent;
        public readonly FrameKind Kind = kind;
        public readonly string Key = key;
        public readonly int ItemIdx = itemIdx;
        /// <summary>For a list item, the frame holding the list (not necessarily the one below).</summary>
        public readonly int Parent = parent;
        public readonly Dictionary<string, SynxValue> Map = map;
        public string? Path;
    }

    private sealed class BlockState
    {
        public required int Indent { get; init; }
        public required string Key { get; init; }
        public required StringBuilder Content { get; init; }
        public required int StackIdx { get; init; }
        /// <summary>SYNX 3.7 <c>|+</c>: keep indent relative to the first continuation line.</summary>
        public bool PreserveIndent { get; init; }
        /// <summary>Locked on the first content line; -1 = unlocked.</summary>
        public int BaseIndent { get; set; } = -1;
        /// <summary>UTF-8 bytes in <see cref="Content"/> (the block cap is in bytes).</summary>
        public int Bytes { get; set; }
    }

    private sealed class ListState
    {
        public required int Indent { get; init; }
        public required string Key { get; init; }
        public required int StackIdx { get; init; }
    }

    /// <summary>One key line. perf: the value stays a slice of the line; it is copied only by the cast that stores it.</summary>
    private ref struct ParsedLine
    {
        public string Key;
        public string? TypeHint;
        public ReadOnlySpan<char> Value;
        public List<string> Markers;
        public List<string> MarkerArgs;
        public SynxConstraints? Constraints;
    }

    private static readonly List<string> s_noMarkers = [];

    private static bool ParseLine(ReadOnlySpan<char> trimmed, out ParsedLine parsed)
    {
        parsed = default;
        if (trimmed.IsEmpty) return false;
        var first = trimmed[0];
        if (first is '[' or ':' or '-' or '#' or '/' or '(')
            return false;

        // Offsets are UTF-16 indexes: every delimiter is ASCII, so slicing at them never
        // splits a character.
        var len = trimmed.Length;
        var pos = 0;
        while (pos < len && trimmed[pos] is not (' ' or '\t' or '[' or ':' or '('))
            pos++;
        var key = trimmed[..pos].ToString();

        string? typeHint = null;
        if (pos < len && trimmed[pos] == '(')
        {
            var start = pos + 1;
            var closeRel = trimmed[start..].IndexOf(')');
            if (closeRel >= 0)
            {
                typeHint = trimmed.Slice(start, closeRel).ToString();
                pos = start + closeRel + 1;
            }
            else
            {
                pos++;
            }
        }

        SynxConstraints? constraints = null;
        if (pos < len && trimmed[pos] == '[')
        {
            // Balanced-bracket scan so patterns like `^[a-z]+$` keep their own `]`.
            var cstart = pos + 1;
            var depth = 1;
            var scan = cstart;
            while (scan < len && depth > 0)
            {
                if (trimmed[scan] == '[') depth++;
                else if (trimmed[scan] == ']' && --depth == 0) break;
                scan++;
            }
            if (depth == 0)
            {
                constraints = ParseConstraints(trimmed[cstart..scan]);
                pos = scan + 1;
            }
            else
            {
                // Unbalanced: fall back to the first `]`, if any.
                var rel = trimmed[cstart..].IndexOf(']');
                if (rel >= 0)
                {
                    constraints = ParseConstraints(trimmed.Slice(cstart, rel));
                    pos = cstart + rel + 1;
                }
                else
                {
                    constraints = ParseConstraints(trimmed[cstart..]);
                    pos = len;
                }
            }
        }

        var markers = s_noMarkers;
        if (pos < len && trimmed[pos] == ':')
        {
            var markerStart = pos + 1;
            var markerEnd = markerStart;
            while (markerEnd < len && trimmed[markerEnd] is not (' ' or '\t'))
                markerEnd++;
            // Empty segments are kept (`a::b` is three), as in synx-core.
            markers = [];
            var chain = trimmed[markerStart..markerEnd];
            while (markers.Count < MaxMarkerChainSegments)
            {
                var c = chain.IndexOf(':');
                if (c < 0)
                {
                    markers.Add(chain.ToString());
                    break;
                }
                markers.Add(chain[..c].ToString());
                chain = chain[(c + 1)..];
            }
            pos = markerEnd;
        }

        while (pos < len && trimmed[pos] is ' ' or '\t')
            pos++;

        var rawValue = pos < len ? StripComment(trimmed[pos..]) : ReadOnlySpan<char>.Empty;
        var args = s_noMarkers;

        if (markers.Count > 0 && !rawValue.IsEmpty)
        {
            // :random — weight percentages ride in the value.
            if (markers.Contains("random"))
            {
                List<string>? nums = null;
                var rest = rawValue;
                while (true)
                {
                    rest = TrimWs(rest);
                    if (rest.IsEmpty) break;
                    var w = 0;
                    while (w < rest.Length && !IsWs(rest[w])) w++;
                    // spec: a weight is `digits[.digits]`. Anything a float parser reads
                    // counted, so `a:random inf`, `-5` or `nan` lost its value to a weight list.
                    if (IsWeight(rest[..w]))
                        (nums ??= []).Add(rest[..w].ToString());
                    rest = rest[w..];
                }
                if (nums != null)
                {
                    args = nums;
                    rawValue = ReadOnlySpan<char>.Empty;
                }
            }
            // :inherit — a value names the parent key; it opens a group, not a leaf.
            if (!rawValue.IsEmpty && markers.Contains("inherit"))
            {
                args = [TrimWs(rawValue).ToString()];
                rawValue = ReadOnlySpan<char>.Empty;
            }
        }

        parsed = new ParsedLine
        {
            Key = key,
            TypeHint = typeHint,
            Value = rawValue,
            Markers = markers,
            MarkerArgs = args,
            Constraints = constraints,
        };
        return true;
    }

    /// <summary>The body of a <c>[…]</c> block; present even when nothing in it is recognised.</summary>
    internal static SynxConstraints ParseConstraints(ReadOnlySpan<char> raw)
    {
        var c = new SynxConstraints();
        while (true)
        {
            var comma = raw.IndexOf(',');
            var part = TrimWs(comma < 0 ? raw : raw[..comma]);
            raw = comma < 0 ? ReadOnlySpan<char>.Empty : raw[(comma + 1)..];
            if (part.IsEmpty)
            {
                if (comma < 0) break;
                continue;
            }
            if (part.SequenceEqual("required"))
                c.Required = true;
            else if (part.SequenceEqual("readonly"))
                c.Readonly = true;
            else
            {
                var colon = part.IndexOf(':');
                if (colon < 0) continue;
                var k = TrimWs(part[..colon]);
                var v = TrimWs(part[(colon + 1)..]);
                switch (k)
                {
                    // A bound is a finite number: `inf` and `nan` bound nothing sensible
                    // (`[max:-inf]` rejected every value).
                    case "min":
                        c.Min = SynxNum.TryParseF64(v, out var mn) && double.IsFinite(mn) ? mn : null;
                        break;
                    case "max":
                        c.Max = SynxNum.TryParseF64(v, out var mx) && double.IsFinite(mx) ? mx : null;
                        break;
                    case "type":
                        c.TypeName = v.ToString();
                        break;
                    case "pattern":
                        c.Pattern = v.ToString();
                        break;
                    case "enum":
                        var vals = new List<string>();
                        while (vals.Count < MaxConstraintEnumParts)
                        {
                            var bar = v.IndexOf('|');
                            vals.Add((bar < 0 ? v : v[..bar]).ToString());
                            if (bar < 0) break;
                            v = v[(bar + 1)..];
                        }
                        c.EnumValues = vals;
                        break;
                }
            }
            if (comma < 0) break;
        }
        return c;
    }

    /// <summary><c>digits[.digits]</c> — a <c>:random</c> weight.</summary>
    private static bool IsWeight(ReadOnlySpan<char> s)
    {
        var dot = s.IndexOf('.');
        var whole = dot < 0 ? s : s[..dot];
        if (!IsDigits(whole)) return false;
        return dot < 0 || IsDigits(s[(dot + 1)..]);

        static bool IsDigits(ReadOnlySpan<char> t)
        {
            if (t.IsEmpty) return false;
            foreach (var ch in t)
                if (!char.IsAsciiDigit(ch)) return false;
            return true;
        }
    }

    /// <summary>Cut an inline <c> //</c> or <c> #</c> comment and trailing whitespace off a value.</summary>
    private static ReadOnlySpan<char> StripComment(ReadOnlySpan<char> val)
    {
        if (val.IndexOf(' ') >= 0)
        {
            var idx = val.IndexOf(" //");
            if (idx >= 0) val = val[..idx];
            idx = val.IndexOf(" #");
            if (idx >= 0) val = val[..idx];
        }
        var e = val.Length;
        while (e > 0 && IsWs(val[e - 1])) e--;
        return val[..e];
    }

    /// <summary>SYNX §8.3 automatic casting. perf: only a string result is copied.</summary>
    internal static SynxValue Cast(ReadOnlySpan<char> val)
    {
        if (val.Length >= 2)
        {
            var a = val[0];
            var b = val[^1];
            if ((a == '"' && b == '"') || (a == '\'' && b == '\''))
                return new SynxValue.Str(val[1..^1].ToString());
        }

        if (val.IsEmpty) return new SynxValue.Str("");
        if (val.SequenceEqual("true")) return new SynxValue.Bool(true);
        if (val.SequenceEqual("false")) return new SynxValue.Bool(false);
        if (val.SequenceEqual("null")) return new SynxValue.Null();

        var start = 0;
        if (val[0] == '-')
        {
            if (val.Length == 1) return new SynxValue.Str("-");
            start = 1;
        }

        if (char.IsAsciiDigit(val[start]))
        {
            var dotPos = -1;
            var allNumeric = true;
            for (var j = start; j < val.Length; j++)
            {
                var ch = val[j];
                if (ch == '.')
                {
                    if (dotPos >= 0) { allNumeric = false; break; }
                    dotPos = j;
                }
                else if (!char.IsAsciiDigit(ch))
                {
                    allNumeric = false;
                    break;
                }
            }

            if (allNumeric)
            {
                if (dotPos >= 0)
                {
                    if (dotPos > start && dotPos < val.Length - 1)
                        return new SynxValue.Float(double.Parse(val, NumberStyles.Float, CultureInfo.InvariantCulture));
                }
                else if (SynxNum.TryParseI64(val, out var n))
                    return new SynxValue.Int(n);
            }
        }

        return new SynxValue.Str(val.ToString());
    }

    /// <summary>SYNX §8.3 typed casting.</summary>
    private static SynxValue CastTyped(ReadOnlySpan<char> val, string hint) => hint switch
    {
        "int" => new SynxValue.Int(SynxNum.TryParseI64(val, out var i) ? i : 0),
        "float" => new SynxValue.Float(SynxNum.TryParseF64(val, out var f) ? f : 0),
        "bool" => new SynxValue.Bool(TrimWs(val).SequenceEqual("true")),
        "string" => new SynxValue.Str(val.ToString()),
        // Full signed 64-bit range, as Rust `rng::random_i64()`.
        // safety: one `new Random(42)` was shared by every thread. A seeded Random is not
        // thread-safe: under parallel parsing its state broke for good and every later
        // `(random)` returned long.MinValue; and each process started with the same values.
        "random" or "random:int" => new SynxValue.Int(Random.Shared.NextInt64(long.MinValue, long.MaxValue)),
        "random:float" => new SynxValue.Float(Random.Shared.NextDouble()),
        "random:bool" => new SynxValue.Bool(Random.Shared.Next(2) == 1),
        _ => Cast(val),
    };

    internal static Dictionary<string, SynxValue> ReshapeToolOutput(SynxValue root, bool schema)
    {
        if (root is not SynxValue.Obj mapObj)
            return new Dictionary<string, SynxValue>(StringComparer.Ordinal);

        var map = mapObj.Map;
        var keys = map.Keys.ToList();
        keys.Sort(CodePointComparer.Instance);
        if (schema)
        {
            var tools = new List<SynxValue>();
            foreach (var key in keys)
            {
                tools.Add(new SynxValue.Obj(new Dictionary<string, SynxValue>(StringComparer.Ordinal)
                {
                    ["name"] = new SynxValue.Str(key),
                    ["params"] = map[key],
                }));
            }
            return new Dictionary<string, SynxValue>(StringComparer.Ordinal)
            {
                ["tools"] = new SynxValue.Arr(tools),
            };
        }

        if (map.Count == 0)
        {
            return new Dictionary<string, SynxValue>(StringComparer.Ordinal)
            {
                ["tool"] = new SynxValue.Null(),
                ["params"] = new SynxValue.Obj(new Dictionary<string, SynxValue>(StringComparer.Ordinal)),
            };
        }

        var toolKey = keys[0];
        var parameters = map[toolKey] switch
        {
            SynxValue.Obj o => new SynxValue.Obj(new Dictionary<string, SynxValue>(o.Map, StringComparer.Ordinal)),
            _ => new SynxValue.Obj(new Dictionary<string, SynxValue>(StringComparer.Ordinal)),
        };

        return new Dictionary<string, SynxValue>(StringComparer.Ordinal)
        {
            ["tool"] = new SynxValue.Str(toolKey),
            ["params"] = parameters,
        };
    }
}
