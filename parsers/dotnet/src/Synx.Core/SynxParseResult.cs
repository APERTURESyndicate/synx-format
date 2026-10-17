namespace Synx;

public enum SynxMode
{
    Static,
    Active,
}

/// <summary>Parse + metadata; call <see cref="SynxEngine.Resolve"/> when <see cref="Mode"/> is <see cref="SynxMode.Active"/>.</summary>
public sealed class SynxParseResult
{
    public SynxValue Root { get; set; } = new SynxValue.Obj(new Dictionary<string, SynxValue>(StringComparer.Ordinal));
    public SynxMode Mode { get; set; } = SynxMode.Static;
    public bool Locked { get; set; }
    public bool Tool { get; set; }
    public bool Schema { get; set; }
    public bool Llm { get; set; }

    /// <summary>
    /// Dot-path → key → meta (<c>""</c> is root), aligned with Rust <c>ParseResult::metadata</c>:
    /// a list item is <c>list.#0</c>, and <c>.</c> / <c>\</c> inside a key are escaped (<c>a\.b</c>).
    /// </summary>
    public Dictionary<string, Dictionary<string, SynxMeta>> Metadata { get; set; } = new(StringComparer.Ordinal);

    public List<SynxIncludeDirective> Includes { get; set; } = [];

    /// <summary><c>!use @scope/name [as alias]</c> directives.</summary>
    public List<SynxUseDirective> Uses { get; set; } = [];

    /// <summary>Input was cut by a resource limit (16 MiB, 2 000 000 lines, a 1 MiB multiline body).</summary>
    public bool Truncated { get; set; }
}
