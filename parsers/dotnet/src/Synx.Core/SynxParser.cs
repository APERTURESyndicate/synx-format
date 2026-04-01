using System.Globalization;
using System.IO;
using System.Text;

namespace Synx;

internal static class SynxParserCore
{
    internal const int MaxParseNestingDepth = 2048;

    internal static SynxParseResult Parse(string text)
    {
        var lines = SplitLines(text);
        var root = new Dictionary<string, SynxValue>(StringComparer.Ordinal);
        var stack = new List<(int Indent, StackEntry Entry)> { (-1, new StackEntry.Root()) };
        var metadata = new Dictionary<string, Dictionary<string, SynxMeta>>(StringComparer.Ordinal);
        var includes = new List<SynxIncludeDirective>();
        var mode = SynxMode.Static;
        var locked = false;
        var tool = false;
        var schema = false;

        BlockState? block = null;
        ListState? list = null;
        var inBlockComment = false;

        for (var i = 0; i < lines.Count; i++)
        {
            var raw = lines[i];
            var trimmed = raw.Trim();

            if (trimmed == "!active")
            {
                mode = SynxMode.Active;
                continue;
            }
            if (trimmed == "!lock")
            {
                locked = true;
                continue;
            }
            if (trimmed == "!tool")
            {
                tool = true;
                continue;
            }
            if (trimmed == "!schema")
            {
                schema = true;
                continue;
            }
            if (trimmed.StartsWith("!include ", StringComparison.Ordinal))
            {
                var rest = trimmed["!include ".Length..].Trim();
                var sp = rest.Split([' ', '\t'], 2, StringSplitOptions.RemoveEmptyEntries);
                var incPath = sp[0];
                var alias = sp.Length > 1
                    ? sp[1].Trim()
                    : DeriveIncludeAlias(incPath);
                includes.Add(new SynxIncludeDirective { Path = incPath, Alias = alias });
                continue;
            }
            if (trimmed.StartsWith("#!mode:", StringComparison.Ordinal))
            {
                var declared = trimmed.Split(':', 2)[1].Trim();
                mode = declared == "active" ? SynxMode.Active : SynxMode.Static;
                continue;
            }

            if (trimmed == "###")
            {
                inBlockComment = !inBlockComment;
                continue;
            }
            if (inBlockComment)
                continue;

            if (trimmed.Length == 0 || trimmed[0] == '#' || trimmed.StartsWith("//", StringComparison.Ordinal))
                continue;

            var indent = raw.Length - raw.TrimStart().Length;

            if (block is { } blk)
            {
                if (indent > blk.Indent)
                {
                    if (blk.Content.Length > 0)
                        blk.Content.Append('\n');
                    blk.Content.Append(trimmed);
                    continue;
                }

                var content = blk.Content.ToString();
                var blkKey = blk.Key;
                var blkStackIdx = blk.StackIdx;
                block = null;
                InsertValue(root, stack, blkStackIdx, blkKey, new SynxValue.Str(content));
            }

            if (trimmed.StartsWith("- ", StringComparison.Ordinal))
            {
                if (list is { } lst0 && indent > lst0.Indent)
                {
                    var valStr = StripComment(trimmed.AsSpan(2).Trim());
                    lst0.Items.Add(Cast(valStr.ToString()));
                    continue;
                }
            }
            else if (list is { } lst1 && indent <= lst1.Indent)
            {
                var arr = new SynxValue.Arr(lst1.Items);
                InsertValue(root, stack, lst1.StackIdx, lst1.Key, arr);
                list = null;
            }

            var parsed = ParseLine(trimmed);
            if (parsed is null)
                continue;

            while (stack.Count > 1 && stack[^1].Indent >= indent)
                stack.RemoveAt(stack.Count - 1);

            var parentIdx = stack.Count - 1;

            if (mode == SynxMode.Active && (parsed.Markers.Count > 0 || parsed.Constraints != null ||
                                            parsed.TypeHint != null))
            {
                var pathPrefix = BuildPath(stack);
                if (!metadata.TryGetValue(pathPrefix, out var mm))
                {
                    mm = new Dictionary<string, SynxMeta>(StringComparer.Ordinal);
                    metadata[pathPrefix] = mm;
                }
                mm[parsed.Key] = new SynxMeta
                {
                    Markers = [.. parsed.Markers],
                    Args = [.. parsed.MarkerArgs],
                    TypeHint = parsed.TypeHint,
                    Constraints = CloneConstraints(parsed.Constraints),
                };
            }

            var isBlock = parsed.Value == "|";
            var isListMarker = parsed.Markers.Any(m =>
                m is "random" or "unique" or "geo" or "join");

            if (isBlock)
            {
                InsertValue(root, stack, parentIdx, parsed.Key, new SynxValue.Str(""));
                block = new BlockState
                {
                    Indent = indent,
                    Key = parsed.Key,
                    Content = new StringBuilder(),
                    StackIdx = parentIdx,
                };
            }
            else if (isListMarker && parsed.Value.Length == 0)
            {
                list = new ListState
                {
                    Indent = indent,
                    Key = parsed.Key,
                    Items = [],
                    StackIdx = parentIdx,
                };
            }
            else if (parsed.Value.Length == 0)
            {
                var peek = PeekNextNonEmptyTrimmed(lines, i + 1);
                if (peek != null && peek.StartsWith("- ", StringComparison.Ordinal))
                {
                    list = new ListState
                    {
                        Indent = indent,
                        Key = parsed.Key,
                        Items = [],
                        StackIdx = parentIdx,
                    };
                    continue;
                }

                InsertValue(root, stack, parentIdx, parsed.Key, new SynxValue.Obj(new Dictionary<string, SynxValue>(StringComparer.Ordinal)));
                if (stack.Count < MaxParseNestingDepth)
                    stack.Add((indent, new StackEntry.Key(parsed.Key)));
            }
            else
            {
                var value = parsed.TypeHint is { } hint
                    ? CastTyped(parsed.Value, hint)
                    : Cast(parsed.Value);
                InsertValue(root, stack, parentIdx, parsed.Key, value);
            }
        }

        if (block is { } blkEnd)
        {
            InsertValue(root, stack, blkEnd.StackIdx, blkEnd.Key, new SynxValue.Str(blkEnd.Content.ToString()));
        }

        if (list is { } lstEnd)
        {
            InsertValue(root, stack, lstEnd.StackIdx, lstEnd.Key, new SynxValue.Arr(lstEnd.Items));
        }

        return new SynxParseResult
        {
            Root = new SynxValue.Obj(root),
            Mode = mode,
            Locked = locked,
            Tool = tool,
            Schema = schema,
            Metadata = metadata,
            Includes = includes,
        };
    }

    private static string DeriveIncludeAlias(string path)
    {
        var name = path.Replace('\\', '/').Split('/')[^1];
        return name.EndsWith(".synx", StringComparison.OrdinalIgnoreCase) ||
               name.EndsWith(".SYNX", StringComparison.Ordinal)
            ? name[..^5]
            : name;
    }

    private static SynxConstraints? CloneConstraints(SynxConstraints? c)
    {
        if (c == null) return null;
        return new SynxConstraints
        {
            Min = c.Min,
            Max = c.Max,
            TypeName = c.TypeName,
            Required = c.Required,
            Readonly = c.Readonly,
            Pattern = c.Pattern,
            EnumValues = c.EnumValues?.ToList(),
        };
    }

    private static string BuildPath(List<(int Indent, StackEntry Entry)> stack)
    {
        var parts = new List<string>();
        foreach (var (_, e) in stack.Skip(1))
        {
            if (e is StackEntry.Key k)
                parts.Add(k.Name);
        }
        return string.Join(".", parts);
    }

    private static List<string> SplitLines(string text)
    {
        var lines = new List<string>();
        var start = 0;
        for (var i = 0; i <= text.Length; i++)
        {
            if (i == text.Length || text[i] == '\n')
            {
                var line = text[start..i];
                if (line.Length > 0 && line[^1] == '\r')
                    line = line[..^1];
                lines.Add(line);
                start = i + 1;
            }
        }
        return lines;
    }

    private static string? PeekNextNonEmptyTrimmed(List<string> lines, int from)
    {
        for (var j = from; j < lines.Count; j++)
        {
            var pt = lines[j].Trim();
            if (pt.Length > 0)
                return pt;
        }
        return null;
    }

    private abstract record StackEntry
    {
        public sealed record Root : StackEntry;
        public sealed record Key(string Name) : StackEntry;
    }

    private sealed class BlockState
    {
        public required int Indent { get; init; }
        public required string Key { get; init; }
        public required StringBuilder Content { get; init; }
        public required int StackIdx { get; init; }
    }

    private sealed class ListState
    {
        public required int Indent { get; init; }
        public required string Key { get; init; }
        public required List<SynxValue> Items { get; init; }
        public required int StackIdx { get; init; }
    }

    private sealed class ParsedLine
    {
        public required string Key { get; init; }
        public string? TypeHint { get; init; }
        public required string Value { get; init; }
        public required List<string> Markers { get; init; }
        public required List<string> MarkerArgs { get; init; }
        public SynxConstraints? Constraints { get; init; }
    }

    private static ParsedLine? ParseLine(string trimmed)
    {
        if (trimmed.Length == 0
            || trimmed[0] == '#'
            || trimmed.StartsWith("//", StringComparison.Ordinal)
            || trimmed.StartsWith("- ", StringComparison.Ordinal))
            return null;

        var bytes = Encoding.UTF8.GetBytes(trimmed);
        var len = bytes.Length;
        var first = bytes[0];
        if (first == (byte)'[' || first == (byte)':' || first == (byte)'-' ||
            first == (byte)'#' || first == (byte)'/' || first == (byte)'(')
            return null;

        var pos = 0;
        while (pos < len && bytes[pos] is not (byte)' ' and not (byte)'\t' and not (byte)'[' and not (byte)':' and not (byte)'(')
            pos++;
        var key = trimmed[..pos];

        string? typeHint = null;
        if (pos < len && bytes[pos] == (byte)'(')
        {
            var start = pos + 1;
            var closeRel = trimmed.AsSpan(start).IndexOf(')');
            if (closeRel >= 0)
            {
                typeHint = trimmed.Substring(start, closeRel);
                pos = start + closeRel + 1;
            }
        }

        SynxConstraints? constraints = null;
        if (pos < len && bytes[pos] == (byte)'[')
        {
            var closeRel = trimmed.AsSpan(pos).IndexOf(']');
            if (closeRel >= 0)
            {
                var inner = trimmed.Substring(pos + 1, closeRel - 1);
                constraints = ParseConstraintInner(inner);
                pos += closeRel + 1;
            }
        }

        var markers = new List<string>();
        var markerArgs = new List<string>();
        if (pos < len && bytes[pos] == (byte)':')
        {
            var markerStart = pos + 1;
            var markerEnd = markerStart;
            while (markerEnd < len && bytes[markerEnd] is not ((byte)' ' or (byte)'\t'))
                markerEnd++;
            var chain = trimmed[markerStart..markerEnd];
            markers.AddRange(chain.Split(':', StringSplitOptions.RemoveEmptyEntries));
            pos = markerEnd;
        }

        while (pos < len && bytes[pos] is (byte)' ' or (byte)'\t')
            pos++;

        var rawValue = pos < len ? StripComment(trimmed.AsSpan(pos)) : ReadOnlySpan<char>.Empty;
        var rawValueStr = rawValue.ToString();

        if (markers.Contains("random") && rawValueStr.Length > 0)
        {
            var parts = rawValueStr.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);
            var nums = parts.Where(p => double.TryParse(p, CultureInfo.InvariantCulture, out _)).ToList();
            if (nums.Count > 0)
            {
                markerArgs.AddRange(nums);
                rawValueStr = "";
            }
        }

        return new ParsedLine
        {
            Key = key,
            TypeHint = typeHint,
            Value = rawValueStr,
            Markers = markers,
            MarkerArgs = markerArgs,
            Constraints = constraints,
        };
    }

    private static SynxConstraints? ParseConstraintInner(string raw)
    {
        var c = new SynxConstraints();
        var any = false;
        foreach (var part in raw.Split(',').Select(s => s.Trim()).Where(s => s.Length > 0))
        {
            if (part == "required")
            {
                c.Required = true;
                any = true;
            }
            else if (part == "readonly")
            {
                c.Readonly = true;
                any = true;
            }
            else
            {
                var colon = part.IndexOf(':');
                if (colon < 0) continue;
                var k = part[..colon].Trim();
                var v = part[(colon + 1)..].Trim();
                any = true;
                switch (k)
                {
                    case "min":
                        if (double.TryParse(v, CultureInfo.InvariantCulture, out var mn))
                            c.Min = mn;
                        break;
                    case "max":
                        if (double.TryParse(v, CultureInfo.InvariantCulture, out var mx))
                            c.Max = mx;
                        break;
                    case "type":
                        c.TypeName = v;
                        break;
                    case "pattern":
                        c.Pattern = v;
                        break;
                    case "enum":
                        c.EnumValues = v.Split('|').Select(s => s.Trim()).ToList();
                        break;
                }
            }
        }
        return any ? c : null;
    }

    private static string StripComment(ReadOnlySpan<char> val)
    {
        var s = val.ToString();
        var idx = s.IndexOf(" //", StringComparison.Ordinal);
        if (idx >= 0)
            s = s[..idx];
        idx = s.IndexOf(" #", StringComparison.Ordinal);
        if (idx >= 0)
            s = s[..idx];
        return s.TrimEnd();
    }

    private static SynxValue Cast(string val)
    {
        if (val.Length >= 2)
        {
            var a = val[0];
            var b = val[^1];
            if ((a == '"' && b == '"') || (a == '\'' && b == '\''))
                return new SynxValue.Str(val[1..^1]);
        }

        if (val == "true") return new SynxValue.Bool(true);
        if (val == "false") return new SynxValue.Bool(false);
        if (val == "null") return new SynxValue.Null();

        if (val.Length == 0)
            return new SynxValue.Str("");

        var bytes = Encoding.UTF8.GetBytes(val);
        var blen = bytes.Length;
        var start = 0;
        if (bytes[0] == (byte)'-')
        {
            if (blen == 1)
                return new SynxValue.Str(val);
            start = 1;
        }

        if (bytes[start] >= (byte)'0' && bytes[start] <= (byte)'9')
        {
            int? dotPos = null;
            var allNumeric = true;
            for (var j = start; j < blen; j++)
            {
                if (bytes[j] == (byte)'.')
                {
                    if (dotPos.HasValue)
                    {
                        allNumeric = false;
                        break;
                    }
                    dotPos = j;
                }
                else if (bytes[j] < (byte)'0' || bytes[j] > (byte)'9')
                {
                    allNumeric = false;
                    break;
                }
            }

            if (allNumeric)
            {
                if (dotPos is { } dp)
                {
                    if (dp > start && dp < blen - 1 && double.TryParse(val, CultureInfo.InvariantCulture, out var f))
                        return new SynxValue.Float(f);
                }
                else if (long.TryParse(val, CultureInfo.InvariantCulture, out var n))
                    return new SynxValue.Int(n);
            }
        }

        return new SynxValue.Str(val);
    }

    private static readonly Random s_rng = new(42);

    private static SynxValue CastTyped(string val, string hint)
    {
        return hint switch
        {
            "int" => new SynxValue.Int(long.TryParse(val, CultureInfo.InvariantCulture, out var i) ? i : 0),
            "float" => new SynxValue.Float(double.TryParse(val, CultureInfo.InvariantCulture, out var f) ? f : 0),
            "bool" => new SynxValue.Bool(val.Trim() == "true"),
            "string" => new SynxValue.Str(val),
            "random" or "random:int" => new SynxValue.Int(s_rng.NextInt64()),
            "random:float" => new SynxValue.Float(s_rng.NextDouble()),
            "random:bool" => new SynxValue.Bool(s_rng.Next(2) == 1),
            _ => Cast(val),
        };
    }

    private static void InsertValue(
        Dictionary<string, SynxValue> root,
        List<(int Indent, StackEntry Entry)> stack,
        int parentIdx,
        string key,
        SynxValue value)
    {
        if (NavigateToParent(root, stack, parentIdx) is not { } target)
            return;
        target[key] = value;
    }

    private static Dictionary<string, SynxValue>? NavigateToParent(
        Dictionary<string, SynxValue> root,
        List<(int Indent, StackEntry Entry)> stack,
        int targetIdx)
    {
        if (targetIdx == 0)
            return root;

        var path = new List<string>();
        foreach (var (_, entry) in stack.Skip(1).Take(targetIdx))
        {
            if (entry is StackEntry.Key k)
                path.Add(k.Name);
        }

        var cur = root;
        foreach (var seg in path)
        {
            if (!cur.TryGetValue(seg, out var child) || child is not SynxValue.Obj o)
                return null;
            cur = o.Map;
        }
        return cur;
    }

    internal static Dictionary<string, SynxValue> ReshapeToolOutput(SynxValue root, bool schema)
    {
        if (root is not SynxValue.Obj mapObj)
            return new Dictionary<string, SynxValue>(StringComparer.Ordinal);

        var map = mapObj.Map;
        if (schema)
        {
            var tools = new List<SynxValue>();
            foreach (var key in map.Keys.OrderBy(k => k, StringComparer.Ordinal))
            {
                var val = map[key];
                var def = new Dictionary<string, SynxValue>(StringComparer.Ordinal)
                {
                    ["name"] = new SynxValue.Str(key),
                    ["params"] = val,
                };
                tools.Add(new SynxValue.Obj(def));
            }
            var outer = new Dictionary<string, SynxValue>(StringComparer.Ordinal)
            {
                ["tools"] = new SynxValue.Arr(tools),
            };
            return outer;
        }

        if (map.Count == 0)
        {
            return new Dictionary<string, SynxValue>(StringComparer.Ordinal)
            {
                ["tool"] = new SynxValue.Null(),
                ["params"] = new SynxValue.Obj(new Dictionary<string, SynxValue>(StringComparer.Ordinal)),
            };
        }

        var toolKey = map.Keys.OrderBy(k => k, StringComparer.Ordinal).First();
        var toolValue = map[toolKey];
        var parameters = toolValue switch
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
