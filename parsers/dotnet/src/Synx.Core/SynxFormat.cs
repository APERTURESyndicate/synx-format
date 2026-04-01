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
}
