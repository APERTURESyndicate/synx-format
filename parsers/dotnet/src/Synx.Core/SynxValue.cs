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

    // ── Accessor helpers (mirror Rust Value::as_*) ──────────────

    /// <summary>Returns the string if this is <see cref="Str"/> or <see cref="Secret"/>, otherwise <c>null</c>.</summary>
    public string? AsString() => this switch
    {
        Str s => s.Value,
        Secret s => s.Value,
        _ => null
    };

    /// <summary>Returns the integer if this is <see cref="Int"/>, otherwise <c>null</c>.</summary>
    public long? AsInt() => this is Int i ? i.Value : null;

    /// <summary>Returns the float if this is <see cref="Float"/>, otherwise <c>null</c>.</summary>
    public double? AsFloat() => this switch
    {
        Float f => f.Value,
        Int i => i.Value,
        _ => null
    };

    /// <summary>Returns the boolean if this is <see cref="Bool"/>, otherwise <c>null</c>.</summary>
    public bool? AsBool() => this is Bool b ? b.Value : null;

    /// <summary>Returns the list if this is <see cref="Arr"/>, otherwise <c>null</c>.</summary>
    public List<SynxValue>? AsArray() => this is Arr a ? a.Items : null;

    /// <summary>Returns the map if this is <see cref="Obj"/>, otherwise <c>null</c>.</summary>
    public Dictionary<string, SynxValue>? AsObject() => this is Obj o ? o.Map : null;

    /// <summary>Returns <c>true</c> if this is <see cref="Null"/>.</summary>
    public bool IsNull() => this is Null;

    /// <summary>Indexer for object access: <c>value["key"]</c>. Returns <see cref="Null"/> if not an object or key missing.</summary>
    public SynxValue this[string key] => this is Obj o && o.Map.TryGetValue(key, out var v) ? v : new Null();

    /// <summary>Indexer for array access: <c>value[0]</c>. Returns <see cref="Null"/> if not an array or index out of range.</summary>
    public SynxValue this[int index] => this is Arr a && index >= 0 && index < a.Items.Count ? a.Items[index] : new Null();

    /// <summary>Returns the value as a display string (unwraps primitives, returns type name for composites).</summary>
    public override string ToString() => this switch
    {
        Null => "null",
        Bool b => b.Value ? "true" : "false",
        Int i => i.Value.ToString(),
        Float f => f.Value.ToString(System.Globalization.CultureInfo.InvariantCulture),
        Str s => s.Value,
        Secret _ => "[SECRET]",
        Arr a => $"[{a.Items.Count} items]",
        Obj o => $"{{{o.Map.Count} keys}}",
        _ => base.ToString() ?? ""
    };
}
