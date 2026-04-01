namespace Synx;

/// <summary>Constraint list from <c>key[min:3, required, …]</c>.</summary>
public sealed class SynxConstraints
{
    public double? Min { get; set; }
    public double? Max { get; set; }
    public string? TypeName { get; set; }
    public bool Required { get; set; }
    public bool Readonly { get; set; }
    public string? Pattern { get; set; }
    public List<string>? EnumValues { get; set; }
}

/// <summary>Per-key metadata in <c>!active</c> mode.</summary>
public sealed class SynxMeta
{
    public List<string> Markers { get; set; } = [];
    public List<string> Args { get; set; } = [];
    public string? TypeHint { get; set; }
    public SynxConstraints? Constraints { get; set; }
}

/// <summary>Options for <see cref="SynxEngine.Resolve"/> (env, paths, locale).</summary>
public sealed class SynxOptions
{
    public Dictionary<string, string>? Env { get; set; }
    public string? Region { get; set; }
    public string? Lang { get; set; }
    public string? BasePath { get; set; }
    public int? MaxIncludeDepth { get; set; }
    public int IncludeDepth { get; set; }
}

public sealed class SynxIncludeDirective
{
    public required string Path { get; init; }
    public required string Alias { get; init; }
}
