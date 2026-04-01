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

    /// <summary>Dot-path → key → meta (<c>""</c> is root), aligned with Rust <c>ParseResult::metadata</c>.</summary>
    public Dictionary<string, Dictionary<string, SynxMeta>> Metadata { get; set; } = new(StringComparer.Ordinal);

    public List<SynxIncludeDirective> Includes { get; set; } = [];
}
