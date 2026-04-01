using System.Text.Json;

namespace Synx;

/// <summary>Entry points aligned with Rust <c>synx_core::Synx</c>.</summary>
public static class SynxFormat
{
    /// <summary>Parse SYNX text into a root object map (static structure; no <c>!active</c> engine).</summary>
    public static Dictionary<string, SynxValue> Parse(string text)
    {
        var r = SynxParserCore.Parse(text);
        return r.Root is SynxValue.Obj o
            ? o.Map
            : new Dictionary<string, SynxValue>(StringComparer.Ordinal);
    }

    /// <summary>Parse and run <c>!active</c> resolution (markers, constraints, includes).</summary>
    public static Dictionary<string, SynxValue> ParseActive(string text, SynxOptions? options = null)
    {
        var r = SynxParserCore.Parse(text);
        SynxEngine.Resolve(r, options ?? new SynxOptions());
        return r.Root is SynxValue.Obj o
            ? o.Map
            : new Dictionary<string, SynxValue>(StringComparer.Ordinal);
    }

    /// <summary>Full parse result (mode, tool flags) without running the resolver.</summary>
    public static SynxParseResult ParseFull(string text) => SynxParserCore.Parse(text);

    /// <summary>Parse then resolve when <see cref="SynxParseResult.Mode"/> is <see cref="SynxMode.Active"/>.</summary>
    public static SynxParseResult ParseFullActive(string text, SynxOptions? options = null)
    {
        var r = SynxParserCore.Parse(text);
        SynxEngine.Resolve(r, options ?? new SynxOptions());
        return r;
    }

    /// <summary>
    /// Tool call reshape: same as <c>Synx::parse_tool</c> when the document is not <c>!active</c>
    /// (no marker resolution in this preview).
    /// </summary>
    public static Dictionary<string, SynxValue> ParseTool(string text)
    {
        var r = SynxParserCore.Parse(text);
        return SynxParserCore.ReshapeToolOutput(r.Root, r.Schema);
    }

    /// <summary>Canonical JSON for a value tree (<c>synx_core::to_json</c>).</summary>
    public static string ToJson(SynxValue value) => SynxJson.ToJson(value);

    /// <summary>Canonical JSON for a root map.</summary>
    public static string ToJson(Dictionary<string, SynxValue> map) => SynxJson.ToJson(map);

    // ── Format ──

    /// <summary>Canonical SYNX reformatter — sorts keys, normalizes indentation, strips comments.</summary>
    public static string Format(string text) => SynxFormatter.Format(text);

    // ── Diff ──

    /// <summary>Structural diff between two parsed SYNX objects.</summary>
    public static SynxDiffResult Diff(Dictionary<string, SynxValue> a, Dictionary<string, SynxValue> b)
        => SynxDiff.Diff(a, b);

    /// <summary>Structural diff between two SYNX text documents (parse then diff).</summary>
    public static SynxDiffResult Diff(string textA, string textB)
        => SynxDiff.Diff(Parse(textA), Parse(textB));

    /// <summary>Diff as JSON string (matches Rust <c>synx_core::diff_to_value</c> → JSON).</summary>
    public static string DiffJson(string textA, string textB)
        => ToJson(Diff(textA, textB).ToValue());

    // ── Compile / Decompile (.synxb) ──

    /// <summary>Compile SYNX text to binary <c>.synxb</c> format.</summary>
    public static byte[] Compile(string text, bool resolved = false)
    {
        var result = SynxParserCore.Parse(text);
        if (resolved && result.Mode == SynxMode.Active)
            SynxEngine.Resolve(result, new SynxOptions());
        return SynxBinary.Compile(result, resolved);
    }

    /// <summary>Decompile a <c>.synxb</c> binary back to SYNX text.</summary>
    public static string Decompile(byte[] data)
    {
        var result = SynxBinary.Decompile(data);
        var sb = new System.Text.StringBuilder();
        if (result.Tool) sb.AppendLine("!tool");
        if (result.Schema) sb.AppendLine("!schema");
        if (result.Llm) sb.AppendLine("!llm");
        if (result.Mode == SynxMode.Active) sb.AppendLine("!active");
        if (result.Locked) sb.AppendLine("!lock");
        if (sb.Length > 0) sb.AppendLine();
        sb.Append(Stringify(result.Root));
        return sb.ToString();
    }

    /// <summary>Check whether a byte array starts with the <c>.synxb</c> magic header.</summary>
    public static bool IsSynxb(byte[] data) => SynxBinary.IsSynxb(data);

    /// <summary>
    /// Parse SYNX text (static, no <c>!active</c> engine) and deserialize directly into
    /// <typeparamref name="T"/> using <c>System.Text.Json</c>.
    /// </summary>
    /// <remarks>
    /// Equivalent to <c>JsonSerializer.Deserialize&lt;T&gt;(SynxFormat.ToJson(SynxFormat.Parse(text)))</c>
    /// but expressed as a single call. Pass <paramref name="jsonOptions"/> to control property
    /// name casing, converters, etc.
    /// </remarks>
    public static T? Deserialize<T>(string text, JsonSerializerOptions? jsonOptions = null)
    {
        var json = ToJson(Parse(text));
        return JsonSerializer.Deserialize<T>(json, jsonOptions);
    }

    /// <summary>
    /// Parse and resolve SYNX text with the <c>!active</c> engine (markers, constraints,
    /// environment variables, includes) then deserialize into <typeparamref name="T"/>.
    /// </summary>
    public static T? DeserializeActive<T>(string text, SynxOptions? synxOptions = null, JsonSerializerOptions? jsonOptions = null)
    {
        var json = ToJson(ParseActive(text, synxOptions));
        return JsonSerializer.Deserialize<T>(json, jsonOptions);
    }

    // ── Non-generic Deserialize (runtime Type) ──

    /// <summary>
    /// Parse SYNX text and deserialize into <paramref name="type"/> at runtime.
    /// Equivalent to <c>JsonSerializer.Deserialize(json, type)</c>.
    /// </summary>
    public static object? Deserialize(string text, Type type, JsonSerializerOptions? jsonOptions = null)
    {
        var json = ToJson(Parse(text));
        return JsonSerializer.Deserialize(json, type, jsonOptions);
    }

    /// <summary>
    /// Parse + resolve SYNX text with the <c>!active</c> engine, then deserialize into
    /// <paramref name="type"/> at runtime.
    /// </summary>
    public static object? DeserializeActive(string text, Type type, SynxOptions? synxOptions = null, JsonSerializerOptions? jsonOptions = null)
    {
        var json = ToJson(ParseActive(text, synxOptions));
        return JsonSerializer.Deserialize(json, type, jsonOptions);
    }

    // ── Async variants (stream-based I/O) ──

    /// <summary>
    /// Read SYNX text from a <see cref="Stream"/> and deserialize into <typeparamref name="T"/>.
    /// The SYNX equivalent of <c>JsonSerializer.DeserializeAsync&lt;T&gt;(stream)</c>.
    /// </summary>
    public static async Task<T?> DeserializeAsync<T>(Stream stream, JsonSerializerOptions? jsonOptions = null, CancellationToken ct = default)
    {
        using var reader = new StreamReader(stream, leaveOpen: true);
        var text = await reader.ReadToEndAsync(ct);
        return Deserialize<T>(text, jsonOptions);
    }

    /// <summary>
    /// Read SYNX text from a <see cref="Stream"/>, resolve with <c>!active</c> engine,
    /// and deserialize into <typeparamref name="T"/>.
    /// </summary>
    public static async Task<T?> DeserializeActiveAsync<T>(Stream stream, SynxOptions? synxOptions = null, JsonSerializerOptions? jsonOptions = null, CancellationToken ct = default)
    {
        using var reader = new StreamReader(stream, leaveOpen: true);
        var text = await reader.ReadToEndAsync(ct);
        return DeserializeActive<T>(text, synxOptions, jsonOptions);
    }

    /// <summary>
    /// Serialize <paramref name="obj"/> to SYNX text and write to a <see cref="Stream"/>.
    /// The SYNX equivalent of <c>JsonSerializer.SerializeAsync&lt;T&gt;(stream, obj)</c>.
    /// </summary>
    public static async Task SerializeAsync<T>(Stream stream, T obj, JsonSerializerOptions? jsonOptions = null, CancellationToken ct = default)
    {
        var synx = Serialize(obj, jsonOptions);
        using var writer = new StreamWriter(stream, leaveOpen: true);
        await writer.WriteAsync(synx.AsMemory(), ct);
        await writer.FlushAsync(ct);
    }

    // ── Stringify: object → SYNX text (analogous to Rust Synx::stringify) ──

    /// <summary>Serialize a <see cref="SynxValue"/> tree back to canonical SYNX text.</summary>
    public static string Stringify(SynxValue value) => SynxStringify.Serialize(value, 0);

    /// <summary>Serialize a root map back to canonical SYNX text.</summary>
    public static string Stringify(Dictionary<string, SynxValue> map)
        => SynxStringify.Serialize(new SynxValue.Obj(map), 0);

    /// <summary>
    /// Serialize any object to SYNX text via <c>System.Text.Json</c> round-trip.
    /// The SYNX equivalent of <c>JsonSerializer.Serialize&lt;T&gt;(obj)</c>.
    /// </summary>
    public static string Serialize<T>(T obj, JsonSerializerOptions? jsonOptions = null)
    {
        jsonOptions ??= new JsonSerializerOptions { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };
        var json = JsonSerializer.Serialize(obj, jsonOptions);
        var map = Parse(JsonToSynx(json));
        return Stringify(map);
    }

    // ── File I/O helpers ──

    /// <summary>Load and deserialize a <c>.synx</c> file into <typeparamref name="T"/>.</summary>
    public static async Task<T?> LoadFileAsync<T>(string filePath, JsonSerializerOptions? jsonOptions = null, CancellationToken ct = default)
    {
        var text = await File.ReadAllTextAsync(filePath, ct);
        return Deserialize<T>(text, jsonOptions);
    }

    /// <summary>Load, resolve with <c>!active</c>, and deserialize a <c>.synx</c> file.</summary>
    public static async Task<T?> LoadFileActiveAsync<T>(string filePath, SynxOptions? synxOptions = null, JsonSerializerOptions? jsonOptions = null, CancellationToken ct = default)
    {
        var text = await File.ReadAllTextAsync(filePath, ct);
        return DeserializeActive<T>(text, synxOptions, jsonOptions);
    }

    /// <summary>Serialize <paramref name="obj"/> and save to a <c>.synx</c> file.</summary>
    public static async Task SaveFileAsync<T>(string filePath, T obj, JsonSerializerOptions? jsonOptions = null, CancellationToken ct = default)
    {
        var synx = Serialize(obj, jsonOptions);
        await File.WriteAllTextAsync(filePath, synx, ct);
    }

    /// <summary>Load and deserialize a <c>.synx</c> file synchronously.</summary>
    public static T? LoadFile<T>(string filePath, JsonSerializerOptions? jsonOptions = null)
        => Deserialize<T>(File.ReadAllText(filePath), jsonOptions);

    /// <summary>Serialize and save to a <c>.synx</c> file synchronously.</summary>
    public static void SaveFile<T>(string filePath, T obj, JsonSerializerOptions? jsonOptions = null)
        => File.WriteAllText(filePath, Serialize(obj, jsonOptions));

    // ── JSON ↔ SYNX conversion ──

    /// <summary>Convert a JSON string to SYNX text.</summary>
    public static string FromJson(string json)
    {
        using var doc = JsonDocument.Parse(json);
        var root = JsonElementToSynxValue(doc.RootElement);
        return Stringify(root);
    }


    internal static string JsonToSynx(string json)
    {
        using var doc = JsonDocument.Parse(json);
        var root = JsonElementToSynxValue(doc.RootElement);
        return Stringify(root);
    }

    internal static SynxValue JsonElementToSynxValue(JsonElement el)
    {
        return el.ValueKind switch
        {
            JsonValueKind.Object => new SynxValue.Obj(
                el.EnumerateObject().ToDictionary(
                    p => p.Name,
                    p => JsonElementToSynxValue(p.Value),
                    StringComparer.Ordinal)),
            JsonValueKind.Array => new SynxValue.Arr(
                el.EnumerateArray().Select(JsonElementToSynxValue).ToList()),
            JsonValueKind.String => new SynxValue.Str(el.GetString()!),
            JsonValueKind.Number when el.TryGetInt64(out var l) => new SynxValue.Int(l),
            JsonValueKind.Number => new SynxValue.Float(el.GetDouble()),
            JsonValueKind.True => new SynxValue.Bool(true),
            JsonValueKind.False => new SynxValue.Bool(false),
            _ => new SynxValue.Null()
        };
    }
}
