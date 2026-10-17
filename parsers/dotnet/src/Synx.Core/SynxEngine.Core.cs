using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace Synx;

/// <summary><c>!active</c> resolver — behavioral parity target: <c>synx-core::engine::resolve</c>.</summary>
public static partial class SynxEngine
{
    private const int MaxCalcExprLen = 4096;
    private const int MaxCalcResolvedLen = 64 * 1024;
    private const long MaxFileSize = 10 * 1024 * 1024;
    private const int DefaultMaxIncludeDepth = 16;
    private const int MaxResolveDepth = 512;
    /// <summary>Upper bound for one scratch string built from hostile templates, in UTF-8 bytes.</summary>
    private const int MaxEngineScratchString = 4 * 1024 * 1024;
    private const int MaxConstraintPatternBytes = 256;

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

    private static string GenerateUuid()
    {
        var hi = NextU64();
        var lo = NextU64();
        var timeHi = ((hi >> 16) & 0x0FFF) | 0x4000;
        var clkSeq = ((lo >> 48) & 0x3FFF) | 0x8000;
        return string.Format(CultureInfo.InvariantCulture, "{0:x8}-{1:x4}-{2:x4}-{3:x4}-{4:x12}",
            (uint)(hi >> 32), (ushort)(hi >> 16), (ushort)timeHi, (ushort)clkSeq, lo & 0x0000_FFFF_FFFF_FFFFUL);
    }

    /// <summary>Everything one resolution reads besides the object it is in.</summary>
    private sealed class Ctx
    {
        public required SynxValue Root { get; init; }
        public required SynxOptions Options { get; init; }
        public required Dictionary<string, Dictionary<string, SynxMeta>> Metadata { get; init; }
        public required Dictionary<string, SynxValue> Includes { get; init; }
        /// <summary>Root `:include` keys the pre-pass already resolved: key → (path, root).</summary>
        public required Dictionary<string, (string Path, SynxValue Root)> Preresolved { get; init; }
        public required List<string> RootSpecial { get; init; }
        /// <summary>One key buffer per depth: an object's keys are fixed during its pass.</summary>
        public readonly List<string[]> KeyBuffers = [];
    }

    public static void Resolve(SynxParseResult result, SynxOptions options)
    {
        if (result.Mode != SynxMode.Active)
            return;

        var metadata = result.Metadata;
        var useDirectives = result.Uses;

        // ── Load !use packages (before includes, so packages are available) ──
        var ownedWasm = options.WasmRuntime is null && useDirectives.Count > 0
            ? new SynxWasmRuntime()
            : null;
        var wasm = options.WasmRuntime ?? ownedWasm;
        var packagesMap = LoadPackages(useDirectives, options, wasm);

        if (ownedWasm != null && ownedWasm.MarkerNames.Count > 0)
        {
            options = CloneOptions(options);
            options.WasmRuntime = ownedWasm;
        }

        var includesMap = LoadIncludes(result.Includes, options);

        // ── `:include` / `:import` root keys double as aliases, so `{leaf:key}` works ──
        // The marker of the same key takes this result instead of resolving the file
        // again (each level of an include chain resolved it twice: 2^depth). It is kept
        // apart from the aliases, so `{x:include}` still sees only the real includes.
        var preresolved = new Dictionary<string, (string Path, SynxValue Root)>(StringComparer.Ordinal);
        if (result.Root is SynxValue.Obj rootForAliases && metadata.TryGetValue("", out var rootMetaForAliases)
            && options.IncludeDepth < (options.MaxIncludeDepth ?? DefaultMaxIncludeDepth))
        {
            foreach (var (key, value) in rootForAliases.Map)
            {
                if (!rootMetaForAliases.TryGetValue(key, out var meta)) continue;
                if (!meta.Markers.Contains("include") && !meta.Markers.Contains("import")) continue;
                if (value is not SynxValue.Str pathVal) continue;
                try
                {
                    var full = JailPath(options.BasePath ?? ".", pathVal.Value);
                    CheckFileSize(full);
                    var included = ReadAndResolve(full, options);
                    preresolved[key] = (pathVal.Value, included.Root);
                    includesMap.TryAdd(key, included.Root);
                }
                catch
                {
                    // Same as Rust: an unreadable include is no alias.
                }
            }
        }

        // ── Merge packages into root before resolution ──
        if (result.Root is SynxValue.Obj rootObj)
        {
            foreach (var (alias, pkgValue) in packagesMap)
                rootObj.Map.TryAdd(alias, pkgValue);
        }

        // ── Type and constraint registries ──
        // spec: built from the document as it stands, before inheritance: each object's
        // own declarations, then its children in document order. A declaration whose
        // object was later overwritten no longer counts.
        var typeRegistry = new Dictionary<string, string>(StringComparer.Ordinal);
        var constraintRegistry = new Dictionary<string, SynxConstraints>(StringComparer.Ordinal);
        CollectRegistries(result.Root, metadata, "", typeRegistry, constraintRegistry, 0);

        ApplyInheritance(result.Root, metadata);
        if (result.Root is SynxValue.Obj privateRoot)
        {
            List<string>? rm = null;
            foreach (var k in privateRoot.Map.Keys)
                if (k.StartsWith('_')) (rm ??= []).Add(k);
            if (rm != null)
                foreach (var k in rm) privateRoot.Map.Remove(k);
        }

        // safety: mask literal `:secret` values before any resolution, so a field that
        // interpolates `{that_key}` can never observe the plaintext.
        MaskSecretLiterals(result.Root, metadata, "");

        if (result.Root is SynxValue.Obj resolveRoot)
        {
            // Root keys a token lookup cannot find; resolution never adds or removes keys.
            var rootSpecial = new List<string>();
            foreach (var k in resolveRoot.Map.Keys)
                if (IsCalcSpecialKey(k)) rootSpecial.Add(k);
            var ctx = new Ctx
            {
                Root = result.Root,
                Options = options,
                Metadata = metadata,
                Includes = includesMap,
                Preresolved = preresolved,
                RootSpecial = rootSpecial,
            };
            ResolveValue(ctx, resolveRoot.Map, metadata.Count > 0 ? "" : null, 0);
        }

        // ── Validate field constraints, then types (global, by field name) ──
        ValidateFields(result.Root, constraintRegistry, typeRegistry);
    }

    /// <summary>
    /// One pass in document order: each key's subtree, then the key's own markers, then
    /// the next key — so a marker sees every key before it fully resolved, at any depth.
    /// After all keys, the <c>{…}</c> interpolation pass. <paramref name="path"/> is null
    /// when the document has no metadata at all.
    /// </summary>
    private static void ResolveValue(Ctx ctx, Dictionary<string, SynxValue> map, string? path, int depth)
    {
        if (depth >= MaxResolveDepth)
        {
            var all = map.Keys.ToList();
            foreach (var k in all)
                map[k] = new SynxValue.Str("NESTING_ERR: maximum object nesting depth exceeded");
            return;
        }

        var count = map.Count;
        if (count == 0) return;
        while (ctx.KeyBuffers.Count <= depth) ctx.KeyBuffers.Add([]);
        var keys = ctx.KeyBuffers[depth];
        if (keys.Length < count)
        {
            keys = new string[Math.Max(count, keys.Length * 2)];
            ctx.KeyBuffers[depth] = keys;
        }
        map.Keys.CopyTo(keys, 0);

        Dictionary<string, SynxMeta>? mm = null;
        if (path != null) ctx.Metadata.TryGetValue(path, out mm);

        var hasBrace = false;
        for (var i = 0; i < count; i++)
        {
            var key = keys[i];
            var child = map[key];
            switch (child)
            {
                case SynxValue.Obj o:
                    ResolveValue(ctx, o.Map, ChildPath(path, key), depth + 1);
                    break;
                case SynxValue.Arr a:
                    var listPath = ChildPath(path, key);
                    for (var idx = 0; idx < a.Items.Count; idx++)
                    {
                        if (a.Items[idx] is SynxValue.Obj io)
                            ResolveValue(ctx, io.Map, listPath == null ? null : ItemPath(listPath, idx), depth + 1);
                    }
                    break;
            }
            if (mm != null && mm.TryGetValue(key, out var meta))
            {
                ApplyMarkers(ctx, map, key, meta, path!);
            }
            if (!hasBrace && map[key] is SynxValue.Str s && s.Value.Contains('{'))
                hasBrace = true;
        }

        // Auto-{} interpolation on all string values.
        if (!hasBrace) return;
        for (var i = 0; i < count; i++)
        {
            var key = keys[i];
            if (map[key] is SynxValue.Str s && s.Value.Contains('{'))
            {
                var res = ResolveInterpolation(s.Value, ctx.Root, map, ctx.Includes);
                if (res != s.Value)
                    map[key] = new SynxValue.Str(res);
            }
        }
    }

    private static string? ChildPath(string? path, string key)
    {
        if (path == null) return null;
        var seg = SynxParserCore.KeySegment(key);
        return path.Length == 0 ? seg : string.Concat(path, ".", seg);
    }

    private static string ItemPath(string listPath, int idx) =>
        string.Concat(listPath, ".#", idx.ToString(CultureInfo.InvariantCulture));

    // ── Files ──

    private static bool IsSubPath(string rootDir, string candidate)
    {
        var r = Path.GetFullPath(rootDir).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
        var c = Path.GetFullPath(candidate);
        var cmp = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;
        if (string.Equals(r, c, cmp)) return true;
        return c.StartsWith(r + Path.DirectorySeparatorChar, cmp);
    }

    /// <summary>Resolve <paramref name="filePath"/> inside <paramref name="baseDir"/> or throw (Rust <c>jail_path</c>).</summary>
    private static string JailPath(string baseDir, string filePath)
    {
        if (filePath.Length > 0 && filePath[0] is '/' or '\\')
            throw new InvalidOperationException($"SECURITY: rooted paths are not allowed: '{filePath}'");
        if (Path.IsPathFullyQualified(filePath))
            throw new InvalidOperationException($"SECURITY: absolute paths are not allowed: '{filePath}'");
        var baseCanon = Path.GetFullPath(baseDir);
        var joined = Path.Combine(baseCanon, filePath);
        if (File.Exists(joined) || Directory.Exists(joined))
        {
            var full = Path.GetFullPath(joined);
            if (!IsSubPath(baseCanon, full))
                throw new InvalidOperationException($"SECURITY: path escapes base directory: '{filePath}'");
            return full;
        }
        // The file may not exist yet — at least verify no `..` escapes.
        if (joined.Contains("..", StringComparison.Ordinal))
            throw new InvalidOperationException($"SECURITY: path traversal detected: '{filePath}'");
        if (!IsSubPath(baseCanon, joined))
            throw new InvalidOperationException($"SECURITY: path escapes base directory: '{filePath}'");
        return joined;
    }

    private static void CheckFileSize(string path)
    {
        var fi = new FileInfo(path);
        if (fi.Exists && fi.Length > MaxFileSize)
            throw new InvalidOperationException($"SECURITY: file too large ({fi.Length} bytes, max {MaxFileSize})");
    }

    /// <summary>Read a file as text, with Rust's portable not-found / permission messages.</summary>
    private static string ReadText(string full, string shownPath)
    {
        try
        {
            return File.ReadAllText(full);
        }
        catch (Exception e) when (e is FileNotFoundException or DirectoryNotFoundException)
        {
            throw new InvalidOperationException($"file not found: {shownPath}");
        }
        catch (UnauthorizedAccessException)
        {
            throw new InvalidOperationException($"permission denied: {shownPath}");
        }
    }

    private static SynxParseResult ReadAndResolve(string full, SynxOptions options)
    {
        var parsed = SynxParserCore.Parse(File.ReadAllText(full));
        if (parsed.Mode == SynxMode.Active)
        {
            var child = CloneOptions(options);
            child.IncludeDepth = options.IncludeDepth + 1;
            child.BasePath = Path.GetDirectoryName(full) ?? ".";
            Resolve(parsed, child);
        }
        return parsed;
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
                map[inc.Alias] = ReadAndResolve(full, options).Root;
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
        PackagesPath = o.PackagesPath,
        WasmRuntime = o.WasmRuntime,
    };

    /// <summary>
    /// Load <c>!use</c> packages. WASM marker packages are loaded into the runtime;
    /// SYNX config packages are returned as values to merge into root.
    /// </summary>
    private static Dictionary<string, SynxValue> LoadPackages(
        List<SynxUseDirective> directives,
        SynxOptions options,
        SynxWasmRuntime? wasm)
    {
        var map = new Dictionary<string, SynxValue>(StringComparer.Ordinal);
        var pkgBase = options.PackagesPath ?? "./synx_packages";
        var baseDir = options.BasePath ?? ".";
        var pkgRoot = Path.Combine(baseDir, pkgBase);

        foreach (var ud in directives)
        {
            // safety: a package name or manifest entry must stay inside the packages
            // directory (`@x/../../etc` would read any file). A bad name skips the
            // package; a bad manifest path counts as absent (Rust `is_contained_relative`).
            if (!IsSafeRelative(ud.Package)) continue;
            var pkgDir = Path.Combine(pkgRoot, ud.Package);
            if (!Directory.Exists(pkgDir)) continue;

            if (wasm != null && IsMarkerPackage(pkgDir))
            {
                var wasmPath = ReadManifestWasm(pkgDir)
                    ?? Path.Combine(pkgDir, "src", "main.wasm");
                var caps = ReadManifestCapabilities(pkgDir);

                if (File.Exists(wasmPath))
                {
                    try
                    {
                        wasm.LoadModule(File.ReadAllBytes(wasmPath), caps);
                    }
                    catch (Exception e)
                    {
                        map[ud.Alias] = new SynxValue.Str($"WASM_ERR: {e.Message}");
                    }
                }
                continue;
            }

            var entry = ReadManifestMain(pkgDir)
                ?? Path.Combine(pkgDir, "src", "main.synx");

            if (!File.Exists(entry)) continue;
            try
            {
                CheckFileSize(entry);
                var parsed = SynxParserCore.Parse(File.ReadAllText(entry));
                if (parsed.Mode == SynxMode.Active)
                {
                    var child = CloneOptions(options);
                    child.IncludeDepth = options.IncludeDepth + 1;
                    child.BasePath = Path.GetDirectoryName(entry) ?? pkgDir;
                    Resolve(parsed, child);
                }
                map[ud.Alias] = parsed.Root;
            }
            catch { /* skip failed packages */ }
        }
        return map;
    }

    /// <summary>
    /// A relative path that stays inside its base: no leading <c>/</c> or <c>\</c>, no <c>:</c>
    /// anywhere, no <c>..</c> segment. Judged by text only (Rust <c>is_contained_relative</c>).
    /// </summary>
    private static bool IsSafeRelative(string p) =>
        p.Length > 0 && p[0] is not ('/' or '\\') && !p.Contains(':') && !Path.IsPathRooted(p)
        && Array.IndexOf(p.Split('/', '\\'), "..") < 0;

    private static bool IsMarkerPackage(string pkgDir)
    {
        var manifest = Path.Combine(pkgDir, "synx-pkg.synx");
        if (!File.Exists(manifest)) return false;
        foreach (var line in File.ReadLines(manifest))
        {
            var trimmed = line.Trim();
            if (trimmed == "type markers") return true;
            if (trimmed.StartsWith("main ", StringComparison.Ordinal) &&
                trimmed["main ".Length..].Trim().EndsWith(".wasm", StringComparison.Ordinal))
                return true;
        }
        return false;
    }

    private static string? ReadManifestMain(string pkgDir)
    {
        var manifest = Path.Combine(pkgDir, "synx-pkg.synx");
        if (!File.Exists(manifest)) return null;
        foreach (var line in File.ReadLines(manifest))
        {
            var trimmed = line.Trim();
            if (trimmed.StartsWith("main ", StringComparison.Ordinal))
            {
                var entry = trimmed["main ".Length..].Trim();
                if (IsSafeRelative(entry)) return Path.Combine(pkgDir, entry);
            }
            if (trimmed.StartsWith("entry ", StringComparison.Ordinal))
            {
                var entry = trimmed["entry ".Length..].Trim();
                if (IsSafeRelative(entry)) return Path.Combine(pkgDir, entry);
            }
        }
        return null;
    }

    private static string? ReadManifestWasm(string pkgDir)
    {
        var manifest = Path.Combine(pkgDir, "synx-pkg.synx");
        if (!File.Exists(manifest)) return null;
        foreach (var line in File.ReadLines(manifest))
        {
            var trimmed = line.Trim();
            if (trimmed.StartsWith("wasm ", StringComparison.Ordinal))
            {
                var path = trimmed["wasm ".Length..].Trim();
                if (IsSafeRelative(path)) return Path.Combine(pkgDir, path);
            }
            if (trimmed.StartsWith("main ", StringComparison.Ordinal))
            {
                var path = trimmed["main ".Length..].Trim();
                if (path.EndsWith(".wasm", StringComparison.Ordinal) && IsSafeRelative(path))
                    return Path.Combine(pkgDir, path);
            }
        }
        return null;
    }

    private static SynxWasmCapabilities ReadManifestCapabilities(string pkgDir)
    {
        var manifest = Path.Combine(pkgDir, "synx-pkg.synx");
        if (!File.Exists(manifest)) return new SynxWasmCapabilities();
        foreach (var line in File.ReadLines(manifest))
        {
            var trimmed = line.Trim();
            if (trimmed.StartsWith("permissions ", StringComparison.Ordinal))
                return SynxWasmCapabilities.FromManifestLine(trimmed["permissions ".Length..]);
        }
        return new SynxWasmCapabilities();
    }

    // ── :inherit ──

    private static void ApplyInheritance(SynxValue root, Dictionary<string, Dictionary<string, SynxMeta>> metadata)
    {
        if (!metadata.TryGetValue("", out var rootMeta) || root is not SynxValue.Obj rootObj)
            return;

        // Two surface syntaxes: `child:inherit:a:b` (parents in the chain) and
        // `child:inherit a` (the parser moves the value into the args).
        var inherits = new List<(string Child, List<string> Parents)>();
        foreach (var (key, meta) in rootMeta)
        {
            var idx = meta.Markers.IndexOf("inherit");
            if (idx < 0) continue;
            var parents = meta.Markers.Skip(idx + 1).ToList();
            if (parents.Count == 0 && meta.Args.Count > 0)
                parents = [.. meta.Args];
            if (parents.Count > 0)
                inherits.Add((key, parents));
        }

        var map = rootObj.Map;
        foreach (var (childKey, parents) in inherits)
        {
            // spec: only an object inherits; a list or a scalar child is left as written.
            if (!map.TryGetValue(childKey, out var childVal) || childVal is not SynxValue.Obj childObj)
                continue;

            // Parents merge left to right, the child overrides all. `source` remembers the
            // object each field came from (null = the child itself).
            var merged = new Dictionary<string, SynxValue>(StringComparer.Ordinal);
            var source = new Dictionary<string, string?>(StringComparer.Ordinal);
            foreach (var parentName in parents)
            {
                if (map.TryGetValue(parentName, out var pv) && pv is SynxValue.Obj po)
                {
                    foreach (var (k, v) in po.Map)
                    {
                        merged[k] = CloneValue(v);
                        source[k] = parentName;
                    }
                }
            }
            foreach (var (k, v) in childObj.Map)
            {
                merged[k] = CloneValue(v);
                source[k] = null;
            }

            // spec: a field keeps the declarations of the object its value came from; the
            // inherited subtree is a copy, and its nested declarations move with it.
            var top = new Dictionary<string, SynxMeta>(StringComparer.Ordinal);
            var nested = new List<(string Path, Dictionary<string, SynxMeta> Meta)>();
            var childSeg = SynxParserCore.KeySegment(childKey);
            foreach (var (k, src) in source)
            {
                var from = src ?? childKey;
                if (metadata.TryGetValue(SynxParserCore.KeySegment(from), out var fromMeta) && fromMeta.TryGetValue(k, out var m))
                    top[k] = CloneMeta(m);
                if (src != null)
                {
                    var prefix = SynxParserCore.KeySegment(src) + "." + SynxParserCore.KeySegment(k);
                    foreach (var (path, mm) in metadata)
                    {
                        if (!path.StartsWith(prefix, StringComparison.Ordinal)) continue;
                        var rest = path[prefix.Length..];
                        if (rest.Length > 0 && rest[0] != '.') continue;
                        nested.Add((childSeg + "." + SynxParserCore.KeySegment(k) + rest, CloneMetaMap(mm)));
                    }
                }
            }
            map[childKey] = new SynxValue.Obj(merged);
            if (top.Count == 0)
                metadata.Remove(childSeg);
            else
                metadata[childSeg] = top;
            foreach (var (path, mm) in nested)
                metadata[path] = mm;
        }
    }

    private static SynxMeta CloneMeta(SynxMeta m) => new()
    {
        Markers = [.. m.Markers],
        Args = [.. m.Args],
        TypeHint = m.TypeHint,
        Constraints = m.Constraints is { } c ? CloneConstraint(c) : null,
    };

    private static Dictionary<string, SynxMeta> CloneMetaMap(Dictionary<string, SynxMeta> mm)
    {
        var copy = new Dictionary<string, SynxMeta>(mm.Count, StringComparer.Ordinal);
        foreach (var (k, m) in mm) copy[k] = CloneMeta(m);
        return copy;
    }

    internal static SynxValue CloneValue(SynxValue v)
    {
        switch (v)
        {
            case SynxValue.Obj o:
                var map = new Dictionary<string, SynxValue>(o.Map.Count, StringComparer.Ordinal);
                foreach (var (k, x) in o.Map) map[k] = CloneValue(x);
                return new SynxValue.Obj(map);
            case SynxValue.Arr a:
                var items = new List<SynxValue>(a.Items.Count);
                foreach (var x in a.Items) items.Add(CloneValue(x));
                return new SynxValue.Arr(items);
            default:
                return v;
        }
    }

    // ── Registries and validation ──

    /// <summary>Fill the type and constraint registries from one object: its own declarations first, then each child in document order.</summary>
    private static void CollectRegistries(
        SynxValue value,
        Dictionary<string, Dictionary<string, SynxMeta>> metadata,
        string path,
        Dictionary<string, string> types,
        Dictionary<string, SynxConstraints> constraints,
        int depth)
    {
        if (depth > MaxResolveDepth || value is not SynxValue.Obj o || metadata.Count == 0) return;
        if (metadata.TryGetValue(path, out var mm))
        {
            foreach (var (key, meta) in mm)
            {
                if (meta.TypeHint is { } hint)
                    types.TryAdd(key, hint);
                if (meta.Constraints is { } c)
                {
                    if (constraints.TryGetValue(key, out var existing))
                        MergeConstraints(existing, c);
                    else
                        constraints[key] = CloneConstraint(c);
                }
            }
        }
        foreach (var (key, child) in o.Map)
        {
            switch (child)
            {
                case SynxValue.Obj:
                    CollectRegistries(child, metadata, ChildPath(path, key)!, types, constraints, depth + 1);
                    break;
                case SynxValue.Arr a:
                    var listPath = ChildPath(path, key)!;
                    for (var idx = 0; idx < a.Items.Count; idx++)
                    {
                        if (a.Items[idx] is SynxValue.Obj)
                            CollectRegistries(a.Items[idx], metadata, ItemPath(listPath, idx), types, constraints, depth + 1);
                    }
                    break;
            }
        }
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

    /// <summary>Rust <c>f64::max</c> / <c>f64::min</c>: a NaN operand yields the other.</summary>
    private static double RustMax(double a, double b) => double.IsNaN(a) ? b : double.IsNaN(b) ? a : Math.Max(a, b);
    private static double RustMin(double a, double b) => double.IsNaN(a) ? b : double.IsNaN(b) ? a : Math.Min(a, b);

    /// <summary>The same field declared twice: the stricter bounds win, the first type/pattern/enum stays.</summary>
    private static void MergeConstraints(SynxConstraints baseC, SynxConstraints incoming)
    {
        if (incoming.Required) baseC.Required = true;
        if (incoming.Readonly) baseC.Readonly = true;
        if (incoming.Min is { } imn)
            baseC.Min = baseC.Min is { } bmn ? RustMax(bmn, imn) : imn;
        if (incoming.Max is { } imx)
            baseC.Max = baseC.Max is { } bmx ? RustMin(bmx, imx) : imx;
        baseC.TypeName ??= incoming.TypeName;
        baseC.Pattern ??= incoming.Pattern;
        baseC.EnumValues ??= incoming.EnumValues?.ToList();
    }

    /// <summary>
    /// Constraints then type for every field of the tree, in one walk. A value the per-key
    /// pass already turned into an error is not measured against constraints again.
    /// </summary>
    private static void ValidateFields(SynxValue value, Dictionary<string, SynxConstraints> constraints, Dictionary<string, string> types)
    {
        if (constraints.Count == 0 && types.Count == 0) return;
        if (value is not SynxValue.Obj o) return;
        var keys = o.Map.Keys.ToArray();
        foreach (var key in keys)
        {
            var val = o.Map[key];
            if (constraints.TryGetValue(key, out var c)
                && !(val is SynxValue.Str es
                     && (es.Value.StartsWith("CONSTRAINT_ERR:", StringComparison.Ordinal)
                         || es.Value.StartsWith("TYPE_ERR:", StringComparison.Ordinal)))
                && ConstraintViolation(key, val, c) is { } err)
            {
                val = new SynxValue.Str(err);
                o.Map[key] = val;
            }
            if (types.TryGetValue(key, out var expected) && !ValueMatchesType(val, expected))
            {
                val = new SynxValue.Str($"TYPE_ERR: '{key}' expected {expected} but got {ValueTypeName(val)}");
                o.Map[key] = val;
            }
            switch (val)
            {
                case SynxValue.Obj:
                    ValidateFields(val, constraints, types);
                    break;
                case SynxValue.Arr arr:
                    foreach (var item in arr.Items)
                        if (item is SynxValue.Obj) ValidateFields(item, constraints, types);
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
        _ => "null",
    };

    private static void ValidateConstraints(Dictionary<string, SynxValue> map, string key, SynxConstraints c)
    {
        if (map.TryGetValue(key, out var v))
        {
            if (ConstraintViolation(key, v, c) is { } err)
                map[key] = new SynxValue.Str(err);
        }
        else if (c.Required)
        {
            map[key] = new SynxValue.Str($"CONSTRAINT_ERR: '{key}' is required");
        }
    }

    /// <summary>The error a value earns against its constraints, or null when it passes.</summary>
    private static string? ConstraintViolation(string key, SynxValue val, SynxConstraints c)
    {
        if (c.Required && (val is SynxValue.Null || (val is SynxValue.Str s0 && s0.Value.Length == 0)))
            return $"CONSTRAINT_ERR: '{key}' is required";

        // spec: an absent value (null) is checked against `enum` only, read as an empty
        // string; type, range and pattern apply to values that exist.
        if (val is SynxValue.Null)
        {
            if (c.EnumValues is { } nev && !nev.Contains(""))
                return $"CONSTRAINT_ERR: '{key}' must be one of [{string.Join("|", nev)}]";
            return null;
        }

        if (c.TypeName is { } tn)
        {
            var ok = tn switch
            {
                "int" => val is SynxValue.Int,
                "float" => val is SynxValue.Float or SynxValue.Int,
                "bool" => val is SynxValue.Bool,
                // A `:secret` is a masked string: `[type:string]:secret:env` must accept it.
                "string" => val is SynxValue.Str or SynxValue.Secret,
                _ => true,
            };
            if (!ok)
                return $"CONSTRAINT_ERR: '{key}' expected type '{tn}'";
        }

        if (c.EnumValues is { } ev)
        {
            var valStr = val switch
            {
                SynxValue.Str s => s.Value,
                SynxValue.Int n => n.Value.ToString(CultureInfo.InvariantCulture),
                SynxValue.Float f => SynxNum.Display(f.Value),
                SynxValue.Bool b => b.Value ? "true" : "false",
                _ => "",
            };
            if (!ev.Contains(valStr))
                return $"CONSTRAINT_ERR: '{key}' must be one of [{string.Join("|", ev)}]";
        }

        // min / max: numbers by value, strings by their count of Unicode scalar values.
        double? num = val switch
        {
            SynxValue.Int n => n.Value,
            SynxValue.Float f => f.Value,
            SynxValue.Str s when c.Min.HasValue || c.Max.HasValue => ScalarCount(s.Value),
            _ => null,
        };
        if (num is { } nn)
        {
            if (c.Min is { } mn && nn < mn)
                return $"CONSTRAINT_ERR: '{key}' value {SynxNum.Display(nn)} is below min {SynxNum.Display(mn)}";
            if (c.Max is { } mx && nn > mx)
                return $"CONSTRAINT_ERR: '{key}' value {SynxNum.Display(nn)} exceeds max {SynxNum.Display(mx)}";
        }

        // Longer, invalid and backtracking-prone patterns are skipped silently.
        if (c.Pattern is { } pat && val is SynxValue.Str ps
            && SynxNum.Utf8Len(pat) <= MaxConstraintPatternBytes
            && PatternRejects(pat, ps.Value))
            return $"CONSTRAINT_ERR: '{key}' does not match pattern /{pat}/";

        return null;
    }

    private static int ScalarCount(string s)
    {
        var n = s.Length;
        foreach (var c in s)
            if (char.IsLowSurrogate(c)) n--;
        return n;
    }

    [ThreadStatic]
    private static Dictionary<string, Regex?>? t_patterns;

    /// <summary>
    /// Whether <paramref name="value"/> fails <c>[pattern:…]</c> (unanchored, like Rust <c>is_match</c>).
    /// Invalid, refused (ReDoS-shaped) and timed-out patterns never reject.
    /// perf: compiled patterns are cached per thread, at most 256.
    /// </summary>
    internal static bool PatternRejects(string pattern, string value)
    {
        var cache = t_patterns ??= new Dictionary<string, Regex?>(StringComparer.Ordinal);
        if (!cache.TryGetValue(pattern, out var re))
        {
            if (cache.Count >= 256) cache.Clear();
            re = null;
            if (!HasNestedQuantifier(pattern))
            {
                try { re = new Regex(pattern, RegexOptions.None, TimeSpan.FromSeconds(1)); }
                catch (ArgumentException) { re = null; }
            }
            cache[pattern] = re;
        }
        if (re == null) return false;
        try { return !re.IsMatch(value); }
        catch (RegexMatchTimeoutException) { return false; }
    }

    /// <summary>
    /// Does the pattern repeat something that already repeats — the <c>(a+)+</c> shape behind
    /// exponential backtracking? Port of Rust <c>has_nested_quantifier</c>.
    /// </summary>
    internal static bool HasNestedQuantifier(string pattern)
    {
        var chars = new List<int>();
        foreach (var r in pattern.EnumerateRunes()) chars.Add(r.Value);
        var opens = new Stack<int>();
        var i = 0;
        while (i < chars.Count)
        {
            switch (chars[i])
            {
                case '\\':
                    i++;
                    break;
                case '[':
                    // A character class is skipped wholesale; quantifiers in it are literal.
                    while (i < chars.Count && chars[i] != ']')
                    {
                        if (chars[i] == '\\') i++;
                        i++;
                    }
                    break;
                case '(':
                    opens.Push(i);
                    break;
                case ')':
                    if (opens.Count > 0)
                    {
                        var start = opens.Pop();
                        var next = i + 1 < chars.Count ? chars[i + 1] : 0;
                        if (next is '*' or '+' or '{')
                        {
                            var body = new List<int>();
                            var j = start + 1;
                            while (j < i)
                            {
                                if (chars[j] == '\\') { j += 2; continue; }
                                body.Add(chars[j]);
                                j++;
                            }
                            for (var k = 0; k < body.Count; k++)
                            {
                                var b = body[k];
                                if (b is '*' or '+' || (b == '{' && k + 1 < body.Count && body[k + 1] is >= '0' and <= '9'))
                                    return true;
                            }
                        }
                    }
                    break;
            }
            i++;
        }
        return false;
    }

    // ── :secret pre-pass ──

    private static readonly HashSet<string> s_resolvingBeforeSecret = new(StringComparer.Ordinal)
    {
        "env", "include", "import", "watch", "ref", "alias", "calc", "map", "i18n", "default",
        "fallback", "once", "split", "join", "format", "replace",
    };

    /// <summary>
    /// Wrap every literal <c>:secret</c> value before resolution, so no later interpolation can
    /// read the plaintext. A secret that itself needs resolving (<c>:env:secret</c>, …) is left
    /// for its own marker to mask in place.
    /// </summary>
    private static void MaskSecretLiterals(SynxValue value, Dictionary<string, Dictionary<string, SynxMeta>> metadata, string path)
    {
        if (value is not SynxValue.Obj o || metadata.Count == 0) return;
        if (metadata.TryGetValue(path, out var mm))
        {
            foreach (var (key, meta) in mm)
            {
                if (!meta.Markers.Contains("secret") || meta.Markers.Any(s_resolvingBeforeSecret.Contains))
                    continue;
                if (o.Map.TryGetValue(key, out var v) && v is not (SynxValue.Secret or SynxValue.Obj or SynxValue.Arr))
                    o.Map[key] = new SynxValue.Secret(ValueToString(v));
            }
        }
        foreach (var (key, child) in o.Map)
        {
            switch (child)
            {
                case SynxValue.Obj:
                    MaskSecretLiterals(child, metadata, ChildPath(path, key)!);
                    break;
                case SynxValue.Arr arr:
                    var listPath = ChildPath(path, key)!;
                    for (var idx = 0; idx < arr.Items.Count; idx++)
                    {
                        if (arr.Items[idx] is SynxValue.Obj)
                            MaskSecretLiterals(arr.Items[idx], metadata, ItemPath(listPath, idx));
                    }
                    break;
            }
        }
    }

    // ── Values as text and numbers ──

    /// <summary>The value at <paramref name="pathStr"/> (direct root key first, then a dot path), not copied.</summary>
    private static SynxValue? DeepGet(SynxValue root, string pathStr)
    {
        if (root is SynxValue.Obj om && om.Map.TryGetValue(pathStr, out var direct))
            return direct;
        var current = root;
        var start = 0;
        while (true)
        {
            var dot = pathStr.IndexOf('.', start);
            var part = dot < 0 ? pathStr[start..] : pathStr[start..dot];
            if (current is not SynxValue.Obj mo || !mo.Map.TryGetValue(part, out var next))
                return null;
            current = next;
            if (dot < 0) return current;
            start = dot + 1;
        }
    }

    private static double? ValueAsNumber(SynxValue v) => v switch
    {
        SynxValue.Int n => n.Value,
        SynxValue.Float f => f.Value,
        _ => null,
    };

    /// <summary>Value as text (interpolation, :join, :calc operands, enum checks, :prompt): synx-core <c>value_to_string</c>.</summary>
    private static string ValueToString(SynxValue v) => v switch
    {
        SynxValue.Str s => s.Value,
        // safety: a `:secret` value must never reach a string sink as plaintext.
        SynxValue.Secret => "[SECRET]",
        SynxValue.Int n => n.Value.ToString(CultureInfo.InvariantCulture),
        SynxValue.Float f => SynxNum.FormatNumber(f.Value),
        SynxValue.Bool b => b.Value ? "true" : "false",
        SynxValue.Null => "null",
        _ => "",
    };

    /// <summary>Text from outside the document (<c>:env</c>, <c>:default</c>, <c>:split</c>) as a value: Rust <c>cast_primitive</c>.</summary>
    private static SynxValue CastPrimitive(string val)
    {
        if (val.Length >= 2)
        {
            var a = val[0];
            var b = val[^1];
            if ((a == '"' && b == '"') || (a == '\'' && b == '\''))
                return new SynxValue.Str(val[1..^1]);
        }
        switch (val)
        {
            case "true": return new SynxValue.Bool(true);
            case "false": return new SynxValue.Bool(false);
            case "null": return new SynxValue.Null();
        }
        if (SynxNum.TryParseI64(val, out var i)) return new SynxValue.Int(i);
        if (SynxNum.TryParseF64(val, out var d)) return new SynxValue.Float(d);
        return new SynxValue.Str(val);
    }

    /// <summary><c>(string)</c> or <c>[type:string]</c> keeps env/default text as text instead of casting it.</summary>
    private static bool WantsString(SynxMeta meta) =>
        meta.TypeHint == "string" || meta.Constraints?.TypeName == "string";

    /// <summary>A whole result below 2^63 is an int, anything else a float.</summary>
    private static SynxValue NumberValue(double d) =>
        d % 1 == 0 && Math.Abs(d) < long.MaxValue ? new SynxValue.Int((long)d) : new SynxValue.Float(d);

    // ── :format ──

    private static string ApplyFormatPattern(string pattern, SynxValue value)
    {
        switch (value)
        {
            case SynxValue.Int n:
                if (pattern.Contains('d') || pattern.Contains('i'))
                    return FormatIntPattern(pattern, n.Value);
                if (pattern.Contains('f') || pattern.Contains('e'))
                    return FormatFloatPattern(pattern, n.Value);
                return n.Value.ToString(CultureInfo.InvariantCulture);
            case SynxValue.Float f:
                if (pattern.Contains('f') || pattern.Contains('e'))
                    return FormatFloatPattern(pattern, f.Value);
                // spec: a whole float takes an integer pattern — `12.0` under `%05d` is `00012`.
                if ((pattern.Contains('d') || pattern.Contains('i')) && f.Value % 1 == 0 && Math.Abs(f.Value) < 9.0e15)
                    return FormatIntPattern(pattern, (long)f.Value);
                return SynxNum.FormatNumber(f.Value);
            case SynxValue.Str s:
                return s.Value;
            default:
                return ValueToString(value);
        }
    }

    private static string FormatIntPattern(string pattern, long n)
    {
        // Guardrail: a user-controlled width can be enormous.
        const ulong maxWidth = 4096;
        var text = n.ToString(CultureInfo.InvariantCulture);
        if (pattern.StartsWith('%'))
        {
            var s = pattern.AsSpan(1);
            if (s.Length > 0 && s[^1] is 'd' or 'i')
            {
                var inner = s[..^1];
                if (inner.Length > 0 && inner[0] == '0' && SynxNum.TryParseU64(inner[1..], ulong.MaxValue, out var w))
                    return text.PadLeft((int)Math.Min(w, maxWidth), '0');
                if (SynxNum.TryParseU64(inner, ulong.MaxValue, out var w2))
                    return text.PadLeft((int)Math.Min(w2, maxWidth));
            }
        }
        return text;
    }

    private static string FormatFloatPattern(string pattern, double f)
    {
        const ulong maxPrec = 1024;
        if (pattern.StartsWith('%'))
        {
            var s = pattern.AsSpan(1);
            // %e — exponent form in the JavaScript `toExponential()` shape: 1.23456789e+5.
            if (s.SequenceEqual("e"))
            {
                if (f == 0) return "0e+0";
                var raw = SynxNum.LowerExpShortest(f);
                var epos = raw.LastIndexOf('e');
                if (epos >= 0)
                {
                    var mantissa = raw[..epos];
                    var expPart = raw[(epos + 1)..];
                    var sign = '+';
                    if (expPart.StartsWith('-')) { sign = '-'; expPart = expPart[1..]; }
                    else if (expPart.StartsWith('+')) expPart = expPart[1..];
                    if (mantissa.EndsWith(".0", StringComparison.Ordinal)) mantissa = mantissa[..^2];
                    return $"{mantissa}e{sign}{expPart}";
                }
                return raw;
            }
            if (s.Length > 0 && s[^1] is 'f' or 'e')
            {
                var inner = s[..^1];
                if (inner.Length > 0 && inner[0] == '.' && SynxNum.TryParseU64(inner[1..], ulong.MaxValue, out var prec))
                {
                    var p = (int)Math.Min(prec, maxPrec);
                    return s[^1] == 'e' ? SynxNum.ExpFixed(f, p) : SynxNum.Fixed(f, p);
                }
            }
        }
        return SynxNum.Display(f);
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

    private static bool AllowSpam(string bucketKey, ulong maxCalls, ulong windowSec)
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
            if ((ulong)calls.Count >= maxCalls) return false;
            calls.Add(now);
            return true;
        }
    }

    /// <summary>The integer at the start of <paramref name="s"/> (after whitespace, optional sign), or 0 — JavaScript's <c>parseInt(s, 10) || 0</c>.</summary>
    private static long LeadingInt(string s)
    {
        var t = s.AsSpan().TrimStart();
        var i = 0;
        var neg = false;
        if (t.Length > 0 && t[0] is '-' or '+')
        {
            neg = t[0] == '-';
            i = 1;
        }
        long n = 0;
        for (; i < t.Length && char.IsAsciiDigit(t[i]); i++)
        {
            var d = t[i] - '0';
            n = n > (long.MaxValue - d) / 10 ? long.MaxValue : n * 10 + d;
        }
        return neg ? -n : n;
    }

    /// <summary>Compare two versions: each dot-separated part is its leading integer, positions never shift.</summary>
    private static bool CompareVersions(string current, string op, string required)
    {
        var cv = current.Split('.').Select(LeadingInt).ToList();
        var rv = required.Split('.').Select(LeadingInt).ToList();
        var len = Math.Max(cv.Count, rv.Count);
        var ord = 0;
        for (var i = 0; i < len; i++)
        {
            var a = i < cv.Count ? cv[i] : 0;
            var b = i < rv.Count ? rv[i] : 0;
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

    /// <summary>Rust <c>str::lines</c>: split at <c>\n</c>, drop one trailing <c>\r</c> per line, no final empty line.</summary>
    private static List<string> RustLines(string content)
    {
        var lines = new List<string>();
        var start = 0;
        while (start < content.Length)
        {
            var nl = content.IndexOf('\n', start);
            var end = nl < 0 ? content.Length : nl;
            var line = content[start..end];
            if (line.EndsWith('\r')) line = line[..^1];
            lines.Add(line);
            if (nl < 0) break;
            start = nl + 1;
        }
        return lines;
    }

    private static string? ReadLock(string lockPath, string key)
    {
        string content;
        try { content = File.ReadAllText(lockPath); }
        catch { return null; }
        foreach (var line in RustLines(content))
        {
            if (line.StartsWith(key, StringComparison.Ordinal) && line.Length > key.Length && line[key.Length] == ' ')
                return line[key.Length..].TrimStart();
        }
        return null;
    }

    private static void WriteLock(string lockPath, string key, string val)
    {
        string content;
        try { content = File.ReadAllText(lockPath); }
        catch { content = ""; }
        var lines = RustLines(content);
        var nl = key + " " + val;
        var found = false;
        for (var i = 0; i < lines.Count; i++)
        {
            if (lines[i].StartsWith(key, StringComparison.Ordinal) && lines[i].Length > key.Length && lines[i][key.Length] == ' ')
            {
                lines[i] = nl;
                found = true;
                break;
            }
        }
        if (!found) lines.Add(nl);
        try { File.WriteAllText(lockPath, string.Join("\n", lines) + "\n"); }
        catch { /* as Rust: a lock that cannot be written is not an error */ }
    }

    /// <summary>A value from file content by key path: JSON dot path, or a SYNX document's key path.</summary>
    private static SynxValue? ExtractFromContent(string content, string keyPath, string ext)
    {
        var parts = keyPath.Split('.');
        if (ext == "json")
        {
            try
            {
                using var doc = JsonDocument.Parse(content);
                var current = doc.RootElement;
                foreach (var part in parts)
                {
                    if (current.ValueKind != JsonValueKind.Object || !current.TryGetProperty(part, out var next))
                        return null;
                    current = next;
                }
                return SynxFormat.JsonElementToSynxValue(current);
            }
            catch (JsonException)
            {
                return null;
            }
        }
        SynxValue cur = SynxParserCore.Parse(content).Root;
        foreach (var part in parts)
        {
            if (cur is not SynxValue.Obj o || !o.Map.TryGetValue(part, out var next))
                return null;
            cur = next;
        }
        return cur;
    }

    /// <summary>A value as SYNX text for <c>:prompt</c>.</summary>
    private static string StringifyValue(SynxValue value, int indent)
    {
        var sp = new string(' ', indent);
        switch (value)
        {
            case SynxValue.Obj o:
            {
                var keys = o.Map.Keys.ToList();
                keys.Sort(CodePointComparer.Instance);
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
            case SynxValue.Arr a:
            {
                // A list prompts as its items, as a list inside an object does.
                var sb = new StringBuilder();
                foreach (var item in a.Items)
                    sb.Append(sp).Append("  - ").Append(ValueToString(item)).Append('\n');
                return sb.ToString();
            }
            default:
                return sp + ValueToString(value) + "\n";
        }
    }

    // ── Interpolation ──

    /// <summary>Rust <c>char::is_alphanumeric</c> on whole scalars, or <c>_</c> / <c>.</c>.</summary>
    private static bool IsRefName(string s)
    {
        foreach (var r in s.EnumerateRunes())
        {
            if (r.Value is '_' or '.') continue;
            var cat = Rune.GetUnicodeCategory(r);
            if (Rune.IsLetterOrDigit(r) || cat is UnicodeCategory.LetterNumber or UnicodeCategory.OtherNumber)
                continue;
            return false;
        }
        return true;
    }

    /// <summary>Append at most <paramref name="room"/> UTF-8 bytes of <paramref name="s"/>, cut at a character boundary.</summary>
    private static void AppendCapped(StringBuilder sb, ref int bytes, string s, int room)
    {
        if (room <= 0) return;
        var i = 0;
        var used = 0;
        while (i < s.Length)
        {
            var pair = char.IsHighSurrogate(s[i]) && i + 1 < s.Length && char.IsLowSurrogate(s[i + 1]);
            var b = pair ? 4 : SynxNum.Utf8Len(s[i]);
            if (used + b > room) break;
            used += b;
            i += pair ? 2 : 1;
        }
        sb.Append(s, 0, i);
        bytes += used;
    }

    /// <summary>Resolve <c>{key}</c>, <c>{key.nested}</c>, <c>{key:alias}</c>, <c>{key:include}</c> in a string.</summary>
    private static string ResolveInterpolation(string tpl, SynxValue root,
        Dictionary<string, SynxValue> localMap, Dictionary<string, SynxValue> includes)
    {
        var sb = new StringBuilder(tpl.Length);
        var bytes = 0;
        var i = 0;
        while (i < tpl.Length)
        {
            if (bytes >= MaxEngineScratchString) break;
            if (tpl[i] == '{')
            {
                var close = tpl.IndexOf('}', i + 1);
                if (close > i)
                {
                    var inner = tpl.Substring(i + 1, close - i - 1);
                    var colon = inner.IndexOf(':');
                    string? refName = null;
                    SynxValue? resolved = null;
                    if (colon >= 0)
                    {
                        var name = inner[..colon];
                        if (IsRefName(name))
                        {
                            refName = name;
                            var scope = inner[(colon + 1)..];
                            if (scope == "include")
                            {
                                if (includes.Count == 1)
                                    resolved = DeepGet(includes.Values.First(), name);
                            }
                            else if (includes.TryGetValue(scope, out var inc))
                                resolved = DeepGet(inc, name);
                        }
                    }
                    else if (IsRefName(inner))
                    {
                        refName = inner;
                        resolved = DeepGet(root, inner) ?? (localMap.TryGetValue(inner, out var lv) ? lv : null);
                    }
                    if (refName != null)
                    {
                        if (resolved != null)
                            AppendCapped(sb, ref bytes, ValueToString(resolved), MaxEngineScratchString - bytes);
                        else
                        {
                            sb.Append('{');
                            bytes++;
                            AppendCapped(sb, ref bytes, inner, MaxEngineScratchString - (bytes + 1));
                            if (bytes < MaxEngineScratchString)
                            {
                                sb.Append('}');
                                bytes++;
                            }
                        }
                        i = close + 1;
                        continue;
                    }
                }
            }
            // One whole character of literal text.
            if (char.IsHighSurrogate(tpl[i]) && i + 1 < tpl.Length && char.IsLowSurrogate(tpl[i + 1]))
            {
                sb.Append(tpl, i, 2);
                bytes += 4;
                i += 2;
            }
            else
            {
                sb.Append(tpl[i]);
                bytes += SynxNum.Utf8Len(tpl[i]);
                i++;
            }
        }
        return sb.ToString();
    }

    private static string PluralCategory(string lang, long n)
    {
        var absN = n == long.MinValue ? (ulong)long.MaxValue + 1 : (ulong)Math.Abs(n);
        var n10 = absN % 10;
        var n100 = absN % 100;
        return lang switch
        {
            "ru" or "uk" or "be" or "pl" => n10 == 1 && n100 != 11 ? "one"
                : n10 is >= 2 and <= 4 && n100 is not (>= 12 and <= 14) ? "few"
                : "many",
            "cs" or "sk" => absN == 1 ? "one" : absN is >= 2 and <= 4 ? "few" : "other",
            "ar" => absN == 0 ? "zero" : absN == 1 ? "one" : absN == 2 ? "two"
                : n100 is >= 3 and <= 10 ? "few" : n100 is >= 11 and <= 99 ? "many" : "other",
            "fr" or "pt" => absN <= 1 ? "one" : "other",
            "ja" or "zh" or "ko" or "vi" or "th" => "other",
            _ => absN == 1 ? "one" : "other",
        };
    }

    private static SynxValue WeightedPick(List<SynxValue> items, List<double> weights)
    {
        var w = new List<double>(weights);
        if (w.Count < items.Count)
        {
            var assigned = w.Sum();
            // Room left under 100 is shared equally; past 100, unassigned items get the
            // average assigned weight so they stay visible.
            var perItem = assigned < 100.0
                ? (100.0 - assigned) / (items.Count - w.Count)
                : assigned / w.Count;
            while (w.Count < items.Count) w.Add(perItem);
        }
        var total = w.Sum();
        if (total <= 0) return CloneValue(items[RandomInt(items.Count)]);
        var rv = RandomF64_01();
        double cum = 0;
        for (var i = 0; i < items.Count; i++)
        {
            cum += w[i] / total;
            if (rv <= cum) return CloneValue(items[i]);
        }
        return items.Count > 0 ? CloneValue(items[^1]) : new SynxValue.Null();
    }

    // ── :calc ──

    private static bool IsWordChar(char c) => char.IsAsciiLetterOrDigit(c) || c == '_';

    /// <summary>
    /// A key whole-word substitution can reach in ways a token lookup cannot: one with a
    /// non-word character (<c>max-hp</c>, <c>a.b</c>, <c>цена</c>), or one a substituted number
    /// can itself spell (<c>2</c>, <c>NaN</c>, <c>inf</c>).
    /// </summary>
    private static bool IsCalcSpecialKey(string k)
    {
        var allWord = true;
        var allDigit = true;
        foreach (var c in k)
        {
            if (!IsWordChar(c)) allWord = false;
            if (!char.IsAsciiDigit(c)) allDigit = false;
        }
        return !allWord || allDigit || k == "NaN" || k == "inf";
    }

    /// <summary>
    /// Substitute numeric names into a <c>:calc</c> expression: a key of the current object
    /// first (it shadows the root), then a root key. Null when the result grows past
    /// <see cref="MaxCalcResolvedLen"/>. perf: one pass over the words with a lookup each;
    /// the sequential form survives only when a special key with a numeric value is in play.
    /// </summary>
    private static string? SubstituteCalcVars(string expr, Dictionary<string, SynxValue> local, string selfKey,
        Dictionary<string, SynxValue>? root, List<string> rootSpecial)
    {
        var localIsRoot = ReferenceEquals(root, local);

        var special = false;
        if (root != null)
        {
            foreach (var k in rootSpecial)
            {
                if (root.TryGetValue(k, out var v) && ValueAsNumber(v) != null) { special = true; break; }
            }
        }
        if (!special && !localIsRoot)
        {
            foreach (var (k, v) in local)
            {
                if (k != selfKey && ValueAsNumber(v) != null && IsCalcSpecialKey(k)) { special = true; break; }
            }
        }

        if (special)
        {
            // Exact sequential whole-word replacement, object before root.
            var resolved = expr;
            var maps = localIsRoot || root == null ? new[] { local } : new[] { local, root };
            foreach (var m in maps)
            {
                foreach (var (rk, rv) in m)
                {
                    if (ReferenceEquals(m, local) && rk == selfKey) continue;
                    if (ValueAsNumber(rv) is not { } n) continue;
                    // An empty name matches everywhere: the expression only grows past the cap.
                    if (rk.Length == 0) return null;
                    resolved = ReplaceWord(resolved, rk, SynxNum.FormatNumber(n));
                    if (SynxNum.Utf8Len(resolved) > MaxCalcResolvedLen) return null;
                }
            }
            return resolved;
        }

        var sb = new StringBuilder(expr.Length + 16);
        var bytes = 0;
        var i = 0;
        while (i < expr.Length)
        {
            if (IsWordChar(expr[i]))
            {
                var start = i;
                while (i < expr.Length && IsWordChar(expr[i])) i++;
                var token = expr[start..i];
                double? n = null;
                if (token != selfKey && local.TryGetValue(token, out var lv)) n = ValueAsNumber(lv);
                if (n == null && root != null && root.TryGetValue(token, out var rv)) n = ValueAsNumber(rv);
                var text = n is { } num ? SynxNum.FormatNumber(num) : token;
                sb.Append(text);
                bytes += text.Length;
            }
            else
            {
                if (char.IsHighSurrogate(expr[i]) && i + 1 < expr.Length && char.IsLowSurrogate(expr[i + 1]))
                {
                    sb.Append(expr, i, 2);
                    bytes += 4;
                    i += 2;
                }
                else
                {
                    sb.Append(expr[i]);
                    bytes += SynxNum.Utf8Len(expr[i]);
                    i++;
                }
            }
            if (bytes > MaxCalcResolvedLen) return null;
        }
        return sb.ToString();
    }

    /// <summary>Replace whole-word occurrences of <paramref name="word"/> (word = ASCII alphanumerics and <c>_</c>).</summary>
    private static string ReplaceWord(string haystack, string word, string replacement)
    {
        if (word.Length > haystack.Length) return haystack;
        var sb = new StringBuilder(haystack.Length);
        var i = 0;
        var last = haystack.Length - word.Length;
        while (i <= last)
        {
            if (string.CompareOrdinal(haystack, i, word, 0, word.Length) == 0)
            {
                var beforeOk = i == 0 || !IsWordChar(haystack[i - 1]);
                var afterOk = i + word.Length >= haystack.Length || !IsWordChar(haystack[i + word.Length]);
                if (beforeOk && afterOk)
                {
                    sb.Append(replacement);
                    i += word.Length;
                    continue;
                }
            }
            sb.Append(haystack[i]);
            i++;
        }
        if (i < haystack.Length) sb.Append(haystack, i, haystack.Length - i);
        return sb.ToString();
    }

    /// <summary>Substitute dot-path references (<c>base.hp</c>, <c>server.port</c>) with root values. Null when too long.</summary>
    private static string? ResolveCalcDots(string resolved, SynxValue root)
    {
        var sb = new StringBuilder(resolved.Length);
        var bytes = 0;
        var i = 0;
        var len = resolved.Length;
        while (i < len)
        {
            if (IsWordChar(resolved[i]))
            {
                var start = i;
                var hasDot = false;
                while (i < len && (IsWordChar(resolved[i]) || resolved[i] == '.'))
                {
                    if (resolved[i] == '.') hasDot = true;
                    i++;
                }
                var token = resolved[start..i];
                var text = token;
                if (hasDot && DeepGet(root, token) is { } dv && ValueAsNumber(dv) is { } n)
                    text = SynxNum.FormatNumber(n);
                sb.Append(text);
                bytes += text.Length;
            }
            else if (char.IsHighSurrogate(resolved[i]) && i + 1 < len && char.IsLowSurrogate(resolved[i + 1]))
            {
                sb.Append(resolved, i, 2);
                bytes += 4;
                i += 2;
            }
            else
            {
                sb.Append(resolved[i]);
                bytes += SynxNum.Utf8Len(resolved[i]);
                i++;
            }
            if (bytes > MaxCalcResolvedLen) return null;
        }
        return sb.ToString();
    }
}
