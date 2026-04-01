namespace Synx;

/// <summary>SYNX value tree (subset of synx-core <c>Value</c>).</summary>
public abstract record SynxValue
{
    public sealed record Null() : SynxValue;
    public sealed record Bool(bool Value) : SynxValue;
    public sealed record Int(long Value) : SynxValue;
    public sealed record Float(double Value) : SynxValue;
    public sealed record Str(string Value) : SynxValue;
    /// <summary>Resolved secret — JSON emission matches string escaping (same as Rust).</summary>
    public sealed record Secret(string Value) : SynxValue;
    public sealed record Arr(List<SynxValue> Items) : SynxValue;
    public sealed record Obj(Dictionary<string, SynxValue> Map) : SynxValue;
}
