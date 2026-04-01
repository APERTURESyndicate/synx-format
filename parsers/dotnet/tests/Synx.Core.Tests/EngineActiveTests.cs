using Synx;
using Xunit;

namespace Synx.Tests;

public class EngineActiveTests
{
    [Fact]
    public void Calc_resolves_sibling_numbers()
    {
        var text = "!active\nprice 100\ntax:calc price * 0.2";
        var map = SynxFormat.ParseActive(text);
        Assert.Equal(new SynxValue.Int(100), map["price"]);
        Assert.Equal(new SynxValue.Int(20), map["tax"]);
    }

    [Fact]
    public void Ref_copies_value()
    {
        var text = "!active\nbase_rate 50\nquick_rate:ref base_rate";
        var map = SynxFormat.ParseActive(text);
        Assert.Equal(new SynxValue.Int(50), map["quick_rate"]);
    }

    [Fact]
    public void Ref_calc_shorthand()
    {
        var text = "!active\nbase_rate 50\ndouble_rate:ref:calc:*2 base_rate";
        var map = SynxFormat.ParseActive(text);
        Assert.Equal(new SynxValue.Int(100), map["double_rate"]);
    }

    [Fact]
    public void Metadata_constraints_recorded_on_parse()
    {
        var text = "!active\nname[min:3, max:30, required] Wario";
        var r = SynxFormat.ParseFull(text);
        Assert.True(r.Metadata.TryGetValue("", out var mm));
        Assert.True(mm.ContainsKey("name"));
        Assert.True(mm["name"].Constraints?.Required);
        Assert.Equal(3, mm["name"].Constraints?.Min);
    }

    [Fact]
    public void Interpolation_fills_from_root()
    {
        var text = "!active\nname Wario\ngreeting Hello, {name}!";
        var map = SynxFormat.ParseActive(text);
        Assert.Equal(new SynxValue.Str("Hello, Wario!"), map["greeting"]);
    }
}
