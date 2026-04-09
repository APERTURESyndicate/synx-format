using System.Text;
using System.Text.Json;
using Wasmtime;

namespace Synx;

/// <summary>
/// WASM marker module loaded from a <c>.wasm</c> file.
/// Mirrors Rust <c>synx_core::wasm::WasmMarkerModule</c>.
/// </summary>
internal sealed class SynxWasmModule : IDisposable
{
    private const ulong MaxFuel = 10_000_000;
    private const int MaxIoSize = 256 * 1024;
    private const int MaxModuleSize = 2 * 1024 * 1024;

    private readonly Engine _engine;
    private readonly Module _module;

    public List<string> Markers { get; }
    public SynxWasmCapabilities Capabilities { get; }

    private SynxWasmModule(Engine engine, Module module, List<string> markers, SynxWasmCapabilities capabilities)
    {
        _engine = engine;
        _module = module;
        Markers = markers;
        Capabilities = capabilities;
    }

    public static SynxWasmModule FromBytes(byte[] wasmBytes, SynxWasmCapabilities? capabilities = null)
    {
        if (wasmBytes.Length > MaxModuleSize)
            throw new InvalidOperationException(
                $"WASM module too large ({wasmBytes.Length} bytes, max {MaxModuleSize})");

        var caps = capabilities ?? new SynxWasmCapabilities();

        var config = new Config();
        config.WithFuelConsumption(true);
        var engine = new Engine(config);
        var module = Module.FromBytes(engine, "synx_marker", wasmBytes);

        var markers = DiscoverMarkers(engine, module);
        return new SynxWasmModule(engine, module, markers, caps);
    }

    private static List<string> DiscoverMarkers(Engine engine, Module module)
    {
        using var store = new Store(engine);
        store.Fuel = MaxFuel;

        var linker = new Linker(engine);
        var instance = linker.Instantiate(store, module);

        var synxMarkers = instance.GetFunction("synx_markers")
            ?? throw new InvalidOperationException("WASM module missing synx_markers export");

        var packed = (long)(synxMarkers.Invoke() ?? throw new InvalidOperationException("synx_markers() returned null"));
        var (ptr, len) = UnpackPtrLen(packed);

        if (len == 0 || len > MaxIoSize)
            throw new InvalidOperationException("synx_markers() returned invalid length");

        var memory = instance.GetMemory("memory")
            ?? throw new InvalidOperationException("WASM module missing memory export");

        var memLen = (int)memory.GetLength();
        var data = memory.GetSpan(0, memLen);
        if (ptr + len > data.Length)
            throw new InvalidOperationException("synx_markers() returned out-of-bounds pointer");

        var jsonBytes = data.Slice(ptr, len);
        var names = JsonSerializer.Deserialize<List<string>>(jsonBytes)
            ?? throw new InvalidOperationException("synx_markers() returned invalid JSON");

        if (names.Count == 0)
            throw new InvalidOperationException("synx_markers() returned empty list");

        return names;
    }

    public SynxValue Apply(string marker, SynxValue value, IReadOnlyList<string> args)
    {
        var fuel = Capabilities.FuelLimit ?? MaxFuel;
        using var store = new Store(_engine);
        store.Fuel = fuel;

        var linker = new Linker(_engine);
        var instance = linker.Instantiate(store, _module);

        var memory = instance.GetMemory("memory")
            ?? throw new InvalidOperationException("WASM module missing memory export");

        var synxAlloc = instance.GetFunction("synx_alloc")
            ?? throw new InvalidOperationException("WASM module missing synx_alloc export");

        var synxApply = instance.GetFunction("synx_apply")
            ?? throw new InvalidOperationException("WASM module missing synx_apply export");

        // Build input JSON
        var input = new Dictionary<string, object?>
        {
            ["marker"] = marker,
            ["value"] = ValueToJsonElement(value),
            ["args"] = args.ToList(),
        };
        var inputBytes = Encoding.UTF8.GetBytes(JsonSerializer.Serialize(input));

        if (inputBytes.Length > MaxIoSize)
            throw new InvalidOperationException("marker input too large");

        // Allocate and write
        var inPtr = (int)(synxAlloc.Invoke(inputBytes.Length)
            ?? throw new InvalidOperationException("synx_alloc returned null"));

        memory.WriteString(inPtr, Encoding.UTF8.GetString(inputBytes));

        // Call synx_apply
        var packed = (long)(synxApply.Invoke(inPtr, inputBytes.Length)
            ?? throw new InvalidOperationException("synx_apply returned null"));

        var (outPtr, outLen) = UnpackPtrLen(packed);
        if (outLen == 0 || outLen > MaxIoSize)
            throw new InvalidOperationException("synx_apply returned invalid length");

        var memLen = (int)memory.GetLength();
        var data = memory.GetSpan(0, memLen);
        if (outPtr + outLen > data.Length)
            throw new InvalidOperationException("synx_apply returned out-of-bounds pointer");

        var outBytes = data.Slice(outPtr, outLen);
        using var doc = JsonDocument.Parse(outBytes.ToArray());
        var root = doc.RootElement;

        if (root.TryGetProperty("error", out var errEl) && errEl.ValueKind == JsonValueKind.String)
            throw new InvalidOperationException($"WASM marker '{marker}' error: {errEl.GetString()}");

        if (!root.TryGetProperty("value", out var valEl))
            throw new InvalidOperationException("synx_apply output missing 'value' field");

        return JsonElementToSynxValue(valEl);
    }

    private static (int ptr, int len) UnpackPtrLen(long packed)
    {
        var ptr = (int)((packed >> 32) & 0xFFFF_FFFF);
        var len = (int)(packed & 0xFFFF_FFFF);
        return (ptr, len);
    }

    private static object? ValueToJsonElement(SynxValue value) => value switch
    {
        SynxValue.Null => null,
        SynxValue.Bool b => b.Value,
        SynxValue.Int i => i.Value,
        SynxValue.Float f => f.Value,
        SynxValue.Str s => s.Value,
        SynxValue.Secret s => s.Value,
        SynxValue.Arr a => a.Items.Select(ValueToJsonElement).ToList(),
        SynxValue.Obj o => o.Map.ToDictionary(kv => kv.Key, kv => ValueToJsonElement(kv.Value)),
        _ => null,
    };

    private static SynxValue JsonElementToSynxValue(JsonElement el) => el.ValueKind switch
    {
        JsonValueKind.Null or JsonValueKind.Undefined => new SynxValue.Null(),
        JsonValueKind.True => new SynxValue.Bool(true),
        JsonValueKind.False => new SynxValue.Bool(false),
        JsonValueKind.String => new SynxValue.Str(el.GetString() ?? ""),
        JsonValueKind.Number when el.TryGetInt64(out var l) => new SynxValue.Int(l),
        JsonValueKind.Number => new SynxValue.Float(el.GetDouble()),
        JsonValueKind.Array => new SynxValue.Arr(el.EnumerateArray().Select(JsonElementToSynxValue).ToList()),
        JsonValueKind.Object => new SynxValue.Obj(
            el.EnumerateObject().ToDictionary(
                p => p.Name,
                p => JsonElementToSynxValue(p.Value),
                StringComparer.Ordinal)),
        _ => new SynxValue.Null(),
    };

    public void Dispose()
    {
        _module.Dispose();
        _engine.Dispose();
    }
}

/// <summary>Capability flags for WASM marker modules.</summary>
public sealed class SynxWasmCapabilities
{
    public bool EnvRead { get; set; }
    public bool FsRead { get; set; }
    public bool Network { get; set; }
    public ulong? FuelLimit { get; set; }

    public static SynxWasmCapabilities FromManifestLine(string line)
    {
        var caps = new SynxWasmCapabilities();
        foreach (var part in line.Split(','))
        {
            switch (part.Trim())
            {
                case "env_read": caps.EnvRead = true; break;
                case "fs_read": caps.FsRead = true; break;
                case "network": caps.Network = true; break;
            }
        }
        return caps;
    }
}

/// <summary>
/// WASM marker runtime — manages loaded modules and dispatches marker calls.
/// Mirrors Rust <c>synx_core::wasm::WasmMarkerRuntime</c>.
/// </summary>
public sealed class SynxWasmRuntime : IDisposable
{
    private static readonly HashSet<string> BuiltinMarkers = new(StringComparer.Ordinal)
    {
        "spam", "include", "import", "env", "random", "ref", "i18n", "calc",
        "alias", "secret", "unique", "geo", "template", "split", "join",
        "default", "clamp", "round", "map", "format", "fallback", "once",
        "version", "watch", "prompt", "vision", "audio",
    };

    private readonly List<SynxWasmModule> _modules = [];
    private readonly Dictionary<string, int> _dispatch = new(StringComparer.Ordinal);

    public IReadOnlyList<string> LoadModule(byte[] wasmBytes, SynxWasmCapabilities? capabilities = null)
    {
        var module = SynxWasmModule.FromBytes(wasmBytes, capabilities);
        var idx = _modules.Count;
        var names = new List<string>();

        foreach (var name in module.Markers)
        {
            if (_dispatch.ContainsKey(name))
                throw new InvalidOperationException($"marker '{name}' already registered by another module");
            _dispatch[name] = idx;
            names.Add(name);
        }

        _modules.Add(module);
        return names;
    }

    public bool HasMarker(string name) => _dispatch.ContainsKey(name);

    public bool IsBuiltinMarker(string name) => BuiltinMarkers.Contains(name);

    public SynxValue ApplyMarker(string marker, SynxValue value, IReadOnlyList<string> args)
    {
        if (!_dispatch.TryGetValue(marker, out var idx))
            throw new InvalidOperationException($"no WASM module provides marker '{marker}'");
        return _modules[idx].Apply(marker, value, args);
    }

    public IReadOnlyList<string> MarkerNames => _dispatch.Keys.ToList();

    public void Dispose()
    {
        foreach (var m in _modules) m.Dispose();
        _modules.Clear();
        _dispatch.Clear();
    }
}
