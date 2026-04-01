namespace Synx;

/// <summary>
/// Structural diff between two parsed SYNX objects.
/// Matches Rust <c>synx_core::diff</c>.
/// </summary>
public sealed class SynxDiffResult
{
    public Dictionary<string, SynxValue> Added { get; } = new(StringComparer.Ordinal);
    public Dictionary<string, SynxValue> Removed { get; } = new(StringComparer.Ordinal);
    public Dictionary<string, SynxDiffChange> Changed { get; } = new(StringComparer.Ordinal);
    public List<string> Unchanged { get; } = [];

    /// <summary>Convert to a <see cref="SynxValue"/> tree (same shape as Rust <c>diff_to_value</c>).</summary>
    public SynxValue ToValue()
    {
        var root = new Dictionary<string, SynxValue>(StringComparer.Ordinal)
        {
            ["added"] = new SynxValue.Obj(Added),
            ["removed"] = new SynxValue.Obj(Removed),
            ["changed"] = new SynxValue.Obj(
                Changed.ToDictionary(
                    kv => kv.Key,
                    kv => (SynxValue)new SynxValue.Obj(new Dictionary<string, SynxValue>(StringComparer.Ordinal)
                    {
                        ["from"] = kv.Value.From,
                        ["to"] = kv.Value.To,
                    }),
                    StringComparer.Ordinal)),
            ["unchanged"] = new SynxValue.Arr(Unchanged.Select(s => (SynxValue)new SynxValue.Str(s)).ToList()),
        };
        return new SynxValue.Obj(root);
    }
}

public sealed class SynxDiffChange
{
    public required SynxValue From { get; init; }
    public required SynxValue To { get; init; }
}

internal static class SynxDiff
{
    internal static SynxDiffResult Diff(
        Dictionary<string, SynxValue> a,
        Dictionary<string, SynxValue> b)
    {
        var result = new SynxDiffResult();

        foreach (var (key, aVal) in a)
        {
            if (b.TryGetValue(key, out var bVal))
            {
                if (DeepEqual(aVal, bVal))
                    result.Unchanged.Add(key);
                else
                    result.Changed[key] = new SynxDiffChange { From = aVal, To = bVal };
            }
            else
            {
                result.Removed[key] = aVal;
            }
        }

        foreach (var (key, bVal) in b)
        {
            if (!a.ContainsKey(key))
                result.Added[key] = bVal;
        }

        result.Unchanged.Sort(StringComparer.Ordinal);
        return result;
    }

    private static bool DeepEqual(SynxValue a, SynxValue b) => (a, b) switch
    {
        (SynxValue.Null, SynxValue.Null) => true,
        (SynxValue.Bool x, SynxValue.Bool y) => x.Value == y.Value,
        (SynxValue.Int x, SynxValue.Int y) => x.Value == y.Value,
        (SynxValue.Float x, SynxValue.Float y) => x.Value == y.Value,
        (SynxValue.Str x, SynxValue.Str y) => x.Value == y.Value,
        (SynxValue.Secret x, SynxValue.Secret y) => x.Value == y.Value,
        (SynxValue.Arr x, SynxValue.Arr y) =>
            x.Items.Count == y.Items.Count &&
            x.Items.Zip(y.Items).All(p => DeepEqual(p.First, p.Second)),
        (SynxValue.Obj x, SynxValue.Obj y) =>
            x.Map.Count == y.Map.Count &&
            x.Map.All(kv => y.Map.TryGetValue(kv.Key, out var yv) && DeepEqual(kv.Value, yv)),
        _ => false,
    };
}
