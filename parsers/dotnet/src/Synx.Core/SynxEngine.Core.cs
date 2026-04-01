using System.Collections.Concurrent;
using System.Globalization;
using System.Text;

namespace Synx;

/// <summary><c>!active</c> resolver — behavioral parity target: <c>synx-core::engine::resolve</c>.</summary>
public static partial class SynxEngine
{
    private const int MaxCalcExprLen = 4096;
    private const int MaxCalcResolvedLen = 64 * 1024;
    private const long MaxFileSize = 10 * 1024 * 1024;
    private const int DefaultMaxIncludeDepth = 16;
    private const int MaxResolveDepth = 512;

    private static readonly object SpamLock = new();
    private static readonly Dictionary<string, List<DateTime>> SpamBuckets = new(StringComparer.Ordinal);

    [ThreadStatic]
    private static ulong _rngState;

    private static ulong NextU64()
    {
        if (_rngState == 0)
            _rngState = (ulong)DateTime.UtcNow.Ticks ^ (ulong)Environment.TickCount64;
        var x = _rngState;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        if (x == 0) x = 0xcafe_dead_beef_1234UL;
        _rngState = x;
        return x;
    }

    private static int RandomInt(int bound) => bound <= 0 ? 0 : (int)(NextU64() % (ulong)bound);

    private static double RandomF64_01() => (NextU64() % 10_000) / 10_000.0;

    private static long RandomI63() => (long)(NextU64() % 2_147_483_647UL);

    private static bool RandomBool() => (NextU64() & 1) == 0;

    private static string GenerateUuid()
    {
        var hi = NextU64();
        var lo = NextU64();
        var timeHi = ((hi >> 16) & 0x0FFF) | 0x4000;
        var clkSeq = ((lo >> 48) & 0x3FFF) | 0x8000;
        return string.Format(CultureInfo.InvariantCulture, "{0:x8}-{1:x4}-{2:x4}-{3:x4}-{4:x12}",
            (uint)(hi >> 32), (ushort)(hi >> 16), (ushort)timeHi, (ushort)clkSeq, lo & 0x0000_FFFF_FFFF_FFFFUL);
    }

    public static void Resolve(SynxParseResult result, SynxOptions options)
    {
        if (result.Mode != SynxMode.Active)
            return;

        var metadata = result.Metadata;
        var includesDirectives = result.Includes;

        var includesMap = LoadIncludes(includesDirectives, options);
        ApplyInheritance(result.Root, metadata);
        StripPrivateKeys(result.Root);

        var typeRegistry = BuildTypeRegistry(metadata);
        var constraintRegistry = BuildConstraintRegistry(metadata);

        ResolveValue(result.Root, result.Root, options, metadata, "", includesMap, 0);

        ValidateFieldConstraints(result.Root, constraintRegistry);
        ValidateFieldTypes(result.Root, typeRegistry, "");
    }

    private static void StripPrivateKeys(SynxValue root)
    {
        if (root is not SynxValue.Obj o) return;
        var rm = o.Map.Keys.Where(k => k.StartsWith("_", StringComparison.Ordinal)).ToList();
        foreach (var k in rm)
            o.Map.Remove(k);
    }

    private static bool IsSubPath(string rootDir, string candidate)
    {
        var r = Path.GetFullPath(rootDir).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
        var c = Path.GetFullPath(candidate);
        var cmp = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;
        if (string.Equals(r, c, cmp)) return true;
        var prefix = r + Path.DirectorySeparatorChar;
        return c.StartsWith(prefix, cmp);
    }

    private static string JailPath(string baseDir, string filePath)
    {
        if (Path.IsPathRooted(filePath))
            throw new InvalidOperationException($"SECURITY: absolute paths are not allowed: '{filePath}'");
        var baseCanon = Path.GetFullPath(baseDir);
        var full = Path.GetFullPath(Path.Combine(baseCanon, filePath));
        if (!IsSubPath(baseCanon, full) && !string.Equals(Path.GetFullPath(baseCanon), full,
                OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal))
            throw new InvalidOperationException($"SECURITY: path escapes base directory: '{filePath}'");
        return full;
    }

    private static void CheckFileSize(string path)
    {
        var fi = new FileInfo(path);
        if (fi.Exists && fi.Length > MaxFileSize)
            throw new InvalidOperationException($"SECURITY: file too large ({fi.Length} bytes, max {MaxFileSize})");
    }

    private static bool SafeFileExists(string baseDir, string rel)
    {
        try
        {
            return File.Exists(JailPath(baseDir, rel));
        }
        catch
        {
            return false;
        }
    }

    private static Dictionary<string, SynxValue> LoadIncludes(List<SynxIncludeDirective> directives, SynxOptions options)
    {
        var map = new Dictionary<string, SynxValue>(StringComparer.Ordinal);
        var baseDir = options.BasePath ?? ".";
        var maxDepth = options.MaxIncludeDepth ?? DefaultMaxIncludeDepth;
        if (options.IncludeDepth >= maxDepth) return map;

        foreach (var inc in directives)
        {
            try
            {
                var full = JailPath(baseDir, inc.Path);
                CheckFileSize(full);
                var text = File.ReadAllText(full);
                var parsed = SynxParserCore.Parse(text);
                if (parsed.Mode == SynxMode.Active)
                {
                    var child = CloneOptions(options);
                    child.IncludeDepth = options.IncludeDepth + 1;
                    child.BasePath = Path.GetDirectoryName(full) ?? ".";
                    Resolve(parsed, child);
                }
                map[inc.Alias] = parsed.Root;
            }
            catch
            {
                /* same as Rust: skip failed includes */
            }
        }
        return map;
    }

    private static SynxOptions CloneOptions(SynxOptions o) => new()
    {
        Env = o.Env != null ? new Dictionary<string, string>(o.Env, StringComparer.Ordinal) : null,
        Region = o.Region,
        Lang = o.Lang,
        BasePath = o.BasePath,
        MaxIncludeDepth = o.MaxIncludeDepth,
        IncludeDepth = o.IncludeDepth,
    };

    private static void ApplyInheritance(SynxValue root, Dictionary<string, Dictionary<string, SynxMeta>> metadata)
    {
        if (!metadata.TryGetValue("", out var rootMeta) || root is not SynxValue.Obj rootObj)
            return;

        var inherits = new List<(string Child, List<string> Parents)>();
        foreach (var (key, meta) in rootMeta)
        {
            var idx = meta.Markers.IndexOf("inherit");
            if (idx < 0) continue;
            var parents = meta.Markers.Skip(idx + 1).ToList();
            if (parents.Count > 0)
                inherits.Add((key, parents));
        }

        foreach (var (childKey, parents) in inherits)
        {
            var merged = new Dictionary<string, SynxValue>(StringComparer.Ordinal);
            foreach (var pn in parents)
            {
                if (rootObj.Map.TryGetValue(pn, out var pv) && pv is SynxValue.Obj po)
                {
                    foreach (var (k, v) in po.Map)
                        merged[k] = CloneValue(v);
                }
            }
            if (rootObj.Map.TryGetValue(childKey, out var cv) && cv is SynxValue.Obj co)
            {
                foreach (var (k, v) in co.Map)
                    merged[k] = CloneValue(v);
            }
            rootObj.Map[childKey] = new SynxValue.Obj(merged);
        }
    }

    private static SynxValue CloneValue(SynxValue v) => v switch
    {
        SynxValue.Obj o => new SynxValue.Obj(o.Map.ToDictionary(x => x.Key, x => CloneValue(x.Value), StringComparer.Ordinal)),
        SynxValue.Arr a => new SynxValue.Arr(a.Items.Select(CloneValue).ToList()),
        _ => v,
    };

    private static Dictionary<string, string> BuildTypeRegistry(Dictionary<string, Dictionary<string, SynxMeta>> metadata)
    {
        var reg = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var mm in metadata.Values)
        {
            foreach (var (key, meta) in mm)
            {
                if (meta.TypeHint is { } th && !reg.ContainsKey(key))
                    reg[key] = th;
            }
        }
        return reg;
    }

    private static Dictionary<string, SynxConstraints> BuildConstraintRegistry(
        Dictionary<string, Dictionary<string, SynxMeta>> metadata)
    {
        var reg = new Dictionary<string, SynxConstraints>(StringComparer.Ordinal);
        foreach (var mm in metadata.Values)
        {
            foreach (var (key, meta) in mm)
            {
                if (meta.Constraints is not { } c) continue;
                if (!reg.TryGetValue(key, out var existing))
                    reg[key] = CloneConstraint(c);
                else
                    MergeConstraints(existing, c);
            }
        }
        return reg;
    }

    private static SynxConstraints CloneConstraint(SynxConstraints c) => new()
    {
        Min = c.Min,
        Max = c.Max,
        TypeName = c.TypeName,
        Required = c.Required,
        Readonly = c.Readonly,
        Pattern = c.Pattern,
        EnumValues = c.EnumValues?.ToList(),
    };

    private static void MergeConstraints(SynxConstraints baseC, SynxConstraints incoming)
    {
        if (incoming.Required) baseC.Required = true;
        if (incoming.Readonly) baseC.Readonly = true;
        if (incoming.Min.HasValue)
            baseC.Min = baseC.Min.HasValue ? Math.Max(baseC.Min.Value, incoming.Min.Value) : incoming.Min;
        if (incoming.Max.HasValue)
            baseC.Max = baseC.Max.HasValue ? Math.Min(baseC.Max.Value, incoming.Max.Value) : incoming.Max;
        baseC.TypeName ??= incoming.TypeName;
        baseC.Pattern ??= incoming.Pattern;
        baseC.EnumValues ??= incoming.EnumValues?.ToList();
    }

    private static void ResolveValue(
        SynxValue value,
        SynxValue rootPtr,
        SynxOptions options,
        Dictionary<string, Dictionary<string, SynxMeta>> metadata,
        string path,
        Dictionary<string, SynxValue> includes,
        int depth)
    {
        if (depth >= MaxResolveDepth)
        {
            if (value is SynxValue.Obj o)
            {
                foreach (var k in o.Map.Keys.ToList())
                    o.Map[k] = new SynxValue.Str("NESTING_ERR: maximum object nesting depth exceeded");
            }
            return;
        }

        metadata.TryGetValue(path, out var metaMap);

        if (value is SynxValue.Obj mapObj)
        {
            var keys = mapObj.Map.Keys.ToList();

            foreach (var key in keys)
            {
                if (!mapObj.Map.TryGetValue(key, out var child)) continue;
                var childPath = path.Length == 0 ? key : $"{path}.{key}";
                switch (child)
                {
                    case SynxValue.Obj:
                        ResolveValue(child, rootPtr, options, metadata, childPath, includes, depth + 1);
                        break;
                    case SynxValue.Arr arr:
                        foreach (var item in arr.Items.Where(i => i is SynxValue.Obj))
                            ResolveValue(item, rootPtr, options, metadata, childPath, includes, depth + 1);
                        break;
                }
            }

            if (metaMap != null)
            {
                foreach (var key in keys)
                {
                    if (!metaMap.TryGetValue(key, out var meta)) continue;
                    ApplyMarkers(mapObj.Map, key, meta, rootPtr, options, path, metadata, includes);
                }
            }

            foreach (var key in mapObj.Map.Keys.ToList())
            {
                if (mapObj.Map[key] is SynxValue.Str s && s.Value.Contains('{'))
                {
                    var res = ResolveInterpolation(s.Value, rootPtr, mapObj.Map, includes);
                    if (res != s.Value)
                        mapObj.Map[key] = new SynxValue.Str(res);
                }
            }
        }
    }

    private static void ValidateFieldConstraints(SynxValue value, Dictionary<string, SynxConstraints> registry)
    {
        if (value is not SynxValue.Obj o) return;
        foreach (var key in o.Map.Keys.ToList())
        {
            if (registry.TryGetValue(key, out var c))
                ValidateConstraintsOnMap(o.Map, key, c);
            var child = o.Map[key];
            switch (child)
            {
                case SynxValue.Obj:
                    ValidateFieldConstraints(child, registry);
                    break;
                case SynxValue.Arr arr:
                    foreach (var item in arr.Items.Where(i => i is SynxValue.Obj))
                        ValidateFieldConstraints(item, registry);
                    break;
            }
        }
    }

    private static void ValidateFieldTypes(SynxValue value, Dictionary<string, string> registry, string path)
    {
        if (value is not SynxValue.Obj o) return;
        foreach (var key in o.Map.Keys.ToList())
        {
            if (registry.TryGetValue(key, out var expected))
            {
                if (o.Map.TryGetValue(key, out var val) && !ValueMatchesType(val, expected))
                {
                    o.Map[key] = new SynxValue.Str(
                        $"TYPE_ERR: '{key}' expected {expected} but got {ValueTypeName(val)}");
                }
            }
            if (!o.Map.TryGetValue(key, out var child)) continue;
            switch (child)
            {
                case SynxValue.Obj:
                    ValidateFieldTypes(child, registry, path.Length == 0 ? key : $"{path}.{key}");
                    break;
                case SynxValue.Arr arr:
                    foreach (var item in arr.Items.Where(i => i is SynxValue.Obj))
                        ValidateFieldTypes(item, registry, path);
                    break;
            }
        }
    }

    private static bool ValueMatchesType(SynxValue v, string expected) => expected switch
    {
        "int" => v is SynxValue.Int,
        "float" => v is SynxValue.Float or SynxValue.Int,
        "bool" => v is SynxValue.Bool,
        "string" => v is SynxValue.Str or SynxValue.Secret,
        "array" => v is SynxValue.Arr,
        "object" => v is SynxValue.Obj,
        _ => true,
    };

    private static string ValueTypeName(SynxValue v) => v switch
    {
        SynxValue.Int => "int",
        SynxValue.Float => "float",
        SynxValue.Bool => "bool",
        SynxValue.Str => "string",
        SynxValue.Secret => "secret",
        SynxValue.Arr => "array",
        SynxValue.Obj => "object",
        SynxValue.Null => "null",
        _ => "unknown",
    };

    private static SynxValue? DeepGet(SynxValue root, string pathStr)
    {
        if (root is SynxValue.Obj om && om.Map.TryGetValue(pathStr, out var direct))
            return CloneValue(direct);
        var parts = pathStr.Split('.');
        var current = root;
        foreach (var part in parts)
        {
            if (current is not SynxValue.Obj mo || !mo.Map.TryGetValue(part, out var next))
                return null;
            current = next;
        }
        return CloneValue(current);
    }

    private static string? ValueAsNumberString(SynxValue v) => v switch
    {
        SynxValue.Int n => FormatNumber(n.Value),
        SynxValue.Float f => FormatNumber(f.Value),
        _ => null,
    };

    private static double? ValueAsNumber(SynxValue v) => v switch
    {
        SynxValue.Int n => n.Value,
        SynxValue.Float f => f.Value,
        _ => null,
    };

    private static string ValueToString(SynxValue v) => v switch
    {
        SynxValue.Str s => s.Value,
        SynxValue.Secret s => s.Value,
        SynxValue.Int n => n.Value.ToString(CultureInfo.InvariantCulture),
        SynxValue.Float f => FormatNumber(f.Value),
        SynxValue.Bool b => b.Value ? "true" : "false",
        SynxValue.Null => "null",
        _ => "",
    };

    private static string FormatNumber(double n)
    {
        if (n % 1 == 0 && Math.Abs(n) < long.MaxValue)
            return ((long)n).ToString(CultureInfo.InvariantCulture);
        return n.ToString(CultureInfo.InvariantCulture);
    }

    private static SynxValue CastPrimitive(string val)
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
        if (long.TryParse(val, CultureInfo.InvariantCulture, out var i)) return new SynxValue.Int(i);
        if (double.TryParse(val, CultureInfo.InvariantCulture, out var d)) return new SynxValue.Float(d);
        return new SynxValue.Str(val);
    }

    private static string ApplyFormatPattern(string pattern, SynxValue value) => value switch
    {
        SynxValue.Int n => pattern.Contains('d', StringComparison.Ordinal) || pattern.Contains('i', StringComparison.Ordinal)
            ? FormatIntPattern(pattern, n.Value)
            : pattern.Contains('f', StringComparison.Ordinal) || pattern.Contains('e', StringComparison.Ordinal)
                ? FormatFloatPattern(pattern, n.Value)
                : n.Value.ToString(CultureInfo.InvariantCulture),
        SynxValue.Float f => pattern.Contains('f', StringComparison.Ordinal) || pattern.Contains('e', StringComparison.Ordinal)
            ? FormatFloatPattern(pattern, f.Value)
            : FormatNumber(f.Value),
        SynxValue.Str s => s.Value,
        _ => ValueToString(value),
    };

    private static string FormatIntPattern(string pattern, long n)
    {
        if (pattern.StartsWith("%", StringComparison.Ordinal))
        {
            var s = pattern[1..];
            if (s.Length > 0 && s[0] == '0')
            {
                var w = s[1..].TrimEnd('d', 'i');
                if (int.TryParse(w, CultureInfo.InvariantCulture, out var width))
                    return n.ToString($"D{width}", CultureInfo.InvariantCulture);
            }
            s = s.TrimEnd('d', 'i');
            if (int.TryParse(s, CultureInfo.InvariantCulture, out var width2))
                return n.ToString($"D{width2}", CultureInfo.InvariantCulture).PadLeft(width2);
        }
        return n.ToString(CultureInfo.InvariantCulture);
    }

    private static string FormatFloatPattern(string pattern, double f)
    {
        if (pattern.StartsWith("%", StringComparison.Ordinal))
        {
            var inner = pattern[1..];
            if (inner.StartsWith(".", StringComparison.Ordinal))
            {
                var precPart = inner[1..].TrimEnd('f', 'e');
                if (int.TryParse(precPart, CultureInfo.InvariantCulture, out var prec))
                    return f.ToString($"F{prec}", CultureInfo.InvariantCulture);
            }
        }
        return f.ToString(CultureInfo.InvariantCulture);
    }

    private static string DelimiterFromKeyword(string kw) => kw switch
    {
        "space" => " ",
        "pipe" => "|",
        "dash" => "-",
        "dot" => ".",
        "semi" => ";",
        "tab" => "\t",
        "slash" => "/",
        _ => kw,
    };

    private static bool AllowSpam(string bucketKey, int maxCalls, ulong windowSec)
    {
        var window = TimeSpan.FromSeconds(Math.Max(1, (double)windowSec));
        lock (SpamLock)
        {
            if (!SpamBuckets.TryGetValue(bucketKey, out var calls))
            {
                calls = [];
                SpamBuckets[bucketKey] = calls;
            }
            var now = DateTime.UtcNow;
            calls.RemoveAll(ts => now - ts > window);
            if (calls.Count >= maxCalls) return false;
            calls.Add(now);
            return true;
        }
    }

    private static bool CompareVersions(string current, string op, string required)
    {
        static List<ulong> P(string s) =>
            s.Split('.').Select(p => ulong.TryParse(p, CultureInfo.InvariantCulture, out var x) ? x : 0UL).ToList();
        var cv = P(current);
        var rv = P(required);
        var len = Math.Max(cv.Count, rv.Count);
        var ord = 0;
        for (var i = 0; i < len; i++)
        {
            var a = i < cv.Count ? cv[i] : 0UL;
            var b = i < rv.Count ? rv[i] : 0UL;
            if (a == b) continue;
            ord = a.CompareTo(b);
            break;
        }
        return op switch
        {
            ">=" => ord >= 0,
            "<=" => ord <= 0,
            ">" => ord > 0,
            "<" => ord < 0,
            "==" or "=" => ord == 0,
            "!=" => ord != 0,
            _ => false,
        };
    }

    private static string? ReadLock(string lockPath, string key)
    {
        if (!File.Exists(lockPath)) return null;
        foreach (var line in File.ReadAllLines(lockPath))
        {
            if (line.StartsWith(key + " ", StringComparison.Ordinal))
                return line[(key.Length + 1)..].Trim();
        }
        return null;
    }

    private static void WriteLock(string lockPath, string key, string val)
    {
        var lines = File.Exists(lockPath) ? File.ReadAllLines(lockPath).ToList() : [];
        var nl = key + " " + val;
        var found = false;
        for (var i = 0; i < lines.Count; i++)
        {
            if (lines[i].StartsWith(key + " ", StringComparison.Ordinal))
            {
                lines[i] = nl;
                found = true;
                break;
            }
        }
        if (!found) lines.Add(nl);
        File.WriteAllText(lockPath, string.Join("\n", lines) + "\n");
    }

    private static SynxValue? ExtractFromContent(string content, string keyPath, string ext)
    {
        if (ext.Equals("json", StringComparison.OrdinalIgnoreCase))
        {
            var search = "\"" + keyPath + "\"";
            var pos = content.IndexOf(search, StringComparison.Ordinal);
            if (pos < 0) return null;
            var after = content.AsSpan(pos + search.Length).TrimStart();
            if (!after.StartsWith(":")) return null;
            var rest = after[1..].TrimStart();
            var valS = rest.ToString().TrimEnd().TrimEnd(',').TrimEnd('}').Trim().Trim('"');
            return CastPrimitive(valS);
        }
        foreach (var line in content.Split('\n'))
        {
            var trimmed = line.TrimStart();
            if (trimmed.StartsWith(keyPath, StringComparison.Ordinal) && trimmed.Length > keyPath.Length &&
                char.IsWhiteSpace(trimmed[keyPath.Length]))
                return CastPrimitive(trimmed[(keyPath.Length + 1)..].Trim());
        }
        return null;
    }

    private static string StringifyValue(SynxValue value, int indent)
    {
        var sp = new string(' ', indent);
        if (value is not SynxValue.Obj o)
            return sp + ValueToString(value) + "\n";
        var keys = o.Map.Keys.OrderBy(k => k, StringComparer.Ordinal).ToList();
        var sb = new StringBuilder();
        foreach (var key in keys)
        {
            var val = o.Map[key];
            switch (val)
            {
                case SynxValue.Obj:
                    sb.Append(sp).Append(key).Append('\n');
                    sb.Append(StringifyValue(val, indent + 2));
                    break;
                case SynxValue.Arr arr:
                    sb.Append(sp).Append(key).Append('\n');
                    foreach (var item in arr.Items)
                        sb.Append(sp).Append("  - ").Append(ValueToString(item)).Append('\n');
                    break;
                default:
                    sb.Append(sp).Append(key).Append(' ').Append(ValueToString(val)).Append('\n');
                    break;
            }
        }
        return sb.ToString();
    }

    private static string ResolveInterpolation(string tpl, SynxValue root,
        Dictionary<string, SynxValue> localMap, Dictionary<string, SynxValue> includes)
    {
        var sb = new StringBuilder(tpl.Length);
        var i = 0;
        while (i < tpl.Length)
        {
            if (tpl[i] == '{')
            {
                var close = tpl.IndexOf('}', i + 1);
                if (close > i)
                {
                    var inner = tpl.AsSpan(i + 1, close - i - 1);
                    var innerStr = inner.ToString();
                    var colon = innerStr.IndexOf(':');
                    if (colon >= 0)
                    {
                        var refName = innerStr[..colon];
                        var scope = innerStr[(colon + 1)..];
                        if (refName.All(c => char.IsLetterOrDigit(c) || c == '_' || c == '.'))
                        {
                            SynxValue? resolved = null;
                            if (scope == "include" && includes.Count == 1)
                            {
                                var first = includes.Values.First();
                                resolved = DeepGet(first, refName);
                            }
                            else if (includes.TryGetValue(scope, out var inc))
                                resolved = DeepGet(inc, refName);
                            if (resolved != null)
                            {
                                sb.Append(ValueToString(resolved));
                                i = close + 1;
                                continue;
                            }
                        }
                    }
                    else if (innerStr.All(c => char.IsLetterOrDigit(c) || c == '_' || c == '.'))
                    {
                        var resolved = DeepGet(root, innerStr) ?? (localMap.TryGetValue(innerStr, out var lv) ? lv : null);
                        if (resolved != null)
                        {
                            sb.Append(ValueToString(resolved));
                            i = close + 1;
                            continue;
                        }
                    }
                }
            }
            sb.Append(tpl[i]);
            i++;
        }
        return sb.ToString();
    }

    private static string PluralCategory(string lang, long n)
    {
        var absN = (ulong)Math.Abs(n);
        var n10 = absN % 10;
        var n100 = absN % 100;
        return lang switch
        {
            "ru" or "uk" or "be" => n10 == 1 && n100 != 11 ? "one"
                : n10 is >= 2 and <= 4 && n100 is not (>= 12 and <= 14) ? "few"
                : "many",
            "pl" => n10 == 1 && n100 != 11 ? "one"
                : n10 is >= 2 and <= 4 && n100 is not (>= 12 and <= 14) ? "few"
                : "many",
            "cs" or "sk" => absN == 1 ? "one" : n10 is >= 2 and <= 4 ? "few" : "other",
            "ar" => absN == 0 ? "zero" : absN == 1 ? "one" : absN == 2 ? "two"
                : n100 is >= 3 and <= 10 ? "few" : n100 is >= 11 and <= 99 ? "many" : "other",
            "fr" or "pt" => absN <= 1 ? "one" : "other",
            "ja" or "zh" or "ko" or "vi" or "th" => "other",
            _ => absN == 1 ? "one" : "other",
        };
    }

    private static SynxValue WeightedPick(SynxValue.Arr arr, List<double> weights)
    {
        var items = arr.Items;
        var w = new List<double>(weights);
        while (w.Count < items.Count)
        {
            var assigned = w.Sum();
            var per = assigned < 100.0 ? (100.0 - assigned) / (items.Count - w.Count) : assigned / items.Count;
            w.Add(per);
        }
        var total = w.Sum();
        if (total <= 0) return items[RandomInt(items.Count)];
        var rv = RandomF64_01();
        double cum = 0;
        for (var i = 0; i < items.Count; i++)
        {
            cum += w[i] / total;
            if (rv <= cum) return CloneValue(items[i]);
        }
        return items.Count > 0 ? CloneValue(items[^1]) : new SynxValue.Null();
    }

    private static string ReplaceWord(string haystack, string word, string replacement)
    {
        var wb = Encoding.UTF8.GetBytes(word);
        var hb = Encoding.UTF8.GetBytes(haystack);
        if (wb.Length > hb.Length) return haystack;
        var sb = new StringBuilder(haystack.Length);
        var i = 0;
        while (i <= hb.Length - wb.Length)
        {
            var match = true;
            for (var j = 0; j < wb.Length; j++)
            {
                if (hb[i + j] != wb[j])
                {
                    match = false;
                    break;
                }
            }
            if (match)
            {
                var beforeOk = i == 0 || !IsWordByte(hb[i - 1]);
                var afterOk = i + wb.Length >= hb.Length || !IsWordByte(hb[i + wb.Length]);
                if (beforeOk && afterOk)
                {
                    sb.Append(replacement);
                    i += wb.Length;
                    continue;
                }
            }
            sb.Append((char)hb[i]);
            i++;
        }
        while (i < hb.Length)
        {
            sb.Append((char)hb[i]);
            i++;
        }
        return sb.ToString();
    }

    private static bool IsWordByte(byte b) => char.IsAsciiLetterOrDigit((char)b) || b == (byte)'_';

    private static bool IsWordCharByte(byte b) => char.IsAsciiLetterOrDigit((char)b) || b == (byte)'_';

    private static void ValidateConstraintsOnMap(Dictionary<string, SynxValue> map, string key, SynxConstraints c)
    {
        if (!map.TryGetValue(key, out var val))
        {
            if (c.Required)
                map[key] = new SynxValue.Str($"CONSTRAINT_ERR: '{key}' is required");
            return;
        }

        if (c.Required)
        {
            var empty = val is SynxValue.Null
                        || (val is SynxValue.Str s && s.Value.Length == 0);
            if (empty)
            {
                map[key] = new SynxValue.Str($"CONSTRAINT_ERR: '{key}' is required");
                return;
            }
        }

        if (c.TypeName is { } tn)
        {
            var ok = tn switch
            {
                "int" => val is SynxValue.Int,
                "float" => val is SynxValue.Float or SynxValue.Int,
                "bool" => val is SynxValue.Bool,
                "string" => val is SynxValue.Str,
                _ => true,
            };
            if (!ok)
            {
                map[key] = new SynxValue.Str($"CONSTRAINT_ERR: '{key}' expected type '{tn}'");
                return;
            }
        }

        if (c.EnumValues is { } ev)
        {
            var valStr = val switch
            {
                SynxValue.Str s => s.Value,
                SynxValue.Int n => n.Value.ToString(CultureInfo.InvariantCulture),
                SynxValue.Float f => f.Value.ToString(CultureInfo.InvariantCulture),
                SynxValue.Bool b => b.Value ? "true" : "false",
                _ => "",
            };
            if (!ev.Contains(valStr))
            {
                map[key] = new SynxValue.Str(
                    $"CONSTRAINT_ERR: '{key}' must be one of [{string.Join("|", ev)}]");
                return;
            }
        }

        double? num = val switch
        {
            SynxValue.Int n => n.Value,
            SynxValue.Float f => f.Value,
            SynxValue.Str s when c.Min.HasValue || c.Max.HasValue => s.Value.Length,
            _ => null,
        };
        if (num is { } nn)
        {
            if (c.Min is { } mn && nn < mn)
                map[key] = new SynxValue.Str($"CONSTRAINT_ERR: '{key}' value {nn} is below min {mn}");
            else if (c.Max is { } mx && nn > mx)
                map[key] = new SynxValue.Str($"CONSTRAINT_ERR: '{key}' value {nn} exceeds max {mx}");
        }
    }
}
