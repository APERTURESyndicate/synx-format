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

    [Fact]
    public void Stringify_roundtrip_preserves_structure()
    {
        var text = "app MyApp\nserver\n  host localhost\n  port 8080\ntags\n  - alpha\n  - beta\n";
        var map = SynxFormat.Parse(text);
        var synx = SynxFormat.Stringify(map);
        var map2 = SynxFormat.Parse(synx);
        Assert.Equal(SynxFormat.ToJson(map), SynxFormat.ToJson(map2));
    }

    [Fact]
    public void Stringify_nested_object()
    {
        var map = new Dictionary<string, SynxValue>
        {
            ["name"] = new SynxValue.Str("Test"),
            ["count"] = new SynxValue.Int(42),
            ["active"] = new SynxValue.Bool(true),
        };
        var synx = SynxFormat.Stringify(map);
        Assert.Contains("active true", synx);
        Assert.Contains("count 42", synx);
        Assert.Contains("name Test", synx);
    }

    [Fact]
    public void Serialize_generic_object()
    {
        var obj = new { RetryCount = 3, RetryDelayMinutes = 5, Name = "MyApp" };
        var synx = SynxFormat.Serialize(obj);
        Assert.Contains("retryCount 3", synx);
        Assert.Contains("retryDelayMinutes 5", synx);
        Assert.Contains("name MyApp", synx);
        // Round-trip
        var map = SynxFormat.Parse(synx);
        Assert.Equal(new SynxValue.Int(3), map["retryCount"]);
    }

    [Fact]
    public void Deserialize_runtime_type()
    {
        var text = "name Wario\nage 30";
        var result = SynxFormat.Deserialize(text, typeof(Dictionary<string, object>));
        Assert.NotNull(result);
    }

    [Fact]
    public async Task DeserializeAsync_from_stream()
    {
        var text = "name Wario\nage 30";
        using var stream = new MemoryStream(System.Text.Encoding.UTF8.GetBytes(text));
        var map = await SynxFormat.DeserializeAsync<Dictionary<string, object>>(stream);
        Assert.NotNull(map);
        Assert.True(map!.ContainsKey("name"));
    }

    [Fact]
    public async Task SerializeAsync_to_stream()
    {
        var obj = new { RetryCount = 3, Name = "MyApp" };
        using var stream = new MemoryStream();
        await SynxFormat.SerializeAsync(stream, obj);
        stream.Position = 0;
        using var reader = new StreamReader(stream);
        var synx = await reader.ReadToEndAsync();
        Assert.Contains("retryCount 3", synx);
        Assert.Contains("name MyApp", synx);
    }

    [Fact]
    public async Task SerializeAsync_DeserializeAsync_roundtrip()
    {
        var obj = new { Host = "localhost", Port = 8080 };
        using var stream = new MemoryStream();
        await SynxFormat.SerializeAsync(stream, obj);
        stream.Position = 0;
        var result = await SynxFormat.DeserializeAsync<Dictionary<string, object>>(stream);
        Assert.NotNull(result);
        Assert.True(result!.ContainsKey("host"));
    }

    // ── Format ──

    [Fact]
    public void Format_sorts_keys_and_normalizes()
    {
        var text = "zebra 1\nalpha 2\nmiddle 3\n";
        var formatted = SynxFormat.Format(text);
        var lines = formatted.TrimEnd().Split('\n');
        Assert.Equal("alpha 2", lines[0]);
        Assert.Equal("middle 3", lines[1]);
        Assert.Equal("zebra 1", lines[2]);
    }

    [Fact]
    public void Format_preserves_directives()
    {
        var text = "!active\n\nport 8080\nhost localhost\n";
        var formatted = SynxFormat.Format(text);
        Assert.StartsWith("!active", formatted);
        // Keys should be sorted
        Assert.True(formatted.IndexOf("host") < formatted.IndexOf("port"));
    }

    // ── Diff ──

    [Fact]
    public void Diff_detects_added_removed_changed()
    {
        var a = SynxFormat.Parse("name Wario\nage 30\ncolor red");
        var b = SynxFormat.Parse("name Mario\nage 30\npower fire");
        var diff = SynxFormat.Diff(a, b);
        Assert.Contains("name", diff.Changed.Keys);
        Assert.Equal(new SynxValue.Str("Wario"), diff.Changed["name"].From);
        Assert.Equal(new SynxValue.Str("Mario"), diff.Changed["name"].To);
        Assert.Contains("color", diff.Removed.Keys);
        Assert.Contains("power", diff.Added.Keys);
        Assert.Contains("age", diff.Unchanged);
    }

    [Fact]
    public void DiffJson_returns_valid_json()
    {
        var json = SynxFormat.DiffJson("a 1\nb 2", "a 1\nc 3");
        Assert.Contains("\"added\"", json);
        Assert.Contains("\"removed\"", json);
        Assert.Contains("\"unchanged\"", json);
    }

    // ── Compile / Decompile ──

    [Fact]
    public void Compile_decompile_roundtrip()
    {
        var text = "name Wario\nage 30\ntags\n  - alpha\n  - beta\n";
        var binary = SynxFormat.Compile(text);
        Assert.True(SynxFormat.IsSynxb(binary));
        var decompiled = SynxFormat.Decompile(binary);
        // Round-trip: parse both and compare JSON
        var orig = SynxFormat.ToJson(SynxFormat.Parse(text));
        var restored = SynxFormat.ToJson(SynxFormat.Parse(decompiled));
        Assert.Equal(orig, restored);
    }

    [Fact]
    public void IsSynxb_rejects_invalid()
    {
        Assert.False(SynxFormat.IsSynxb(new byte[] { 0, 1, 2, 3 }));
        Assert.False(SynxFormat.IsSynxb(System.Text.Encoding.UTF8.GetBytes("hello")));
    }

    [Fact]
    public void Compile_with_active_directive()
    {
        var text = "!active\n\nbase 100\ntax:calc base * 0.2\n";
        var binary = SynxFormat.Compile(text, resolved: false);
        Assert.True(SynxFormat.IsSynxb(binary));
        var decompiled = SynxFormat.Decompile(binary);
        Assert.Contains("!active", decompiled);
    }

    // ── File I/O helpers ──

    [Fact]
    public async Task LoadFileAsync_SaveFileAsync_roundtrip()
    {
        var tmp = Path.Combine(Path.GetTempPath(), $"synx_test_{Guid.NewGuid():N}.synx");
        try
        {
            var obj = new { Name = "Wario", Level = 42 };
            await SynxFormat.SaveFileAsync(tmp, obj);
            Assert.True(File.Exists(tmp));
            var result = await SynxFormat.LoadFileAsync<Dictionary<string, object>>(tmp);
            Assert.NotNull(result);
            Assert.True(result!.ContainsKey("name"));
        }
        finally { File.Delete(tmp); }
    }

    [Fact]
    public void LoadFile_SaveFile_sync_roundtrip()
    {
        var tmp = Path.Combine(Path.GetTempPath(), $"synx_test_{Guid.NewGuid():N}.synx");
        try
        {
            var obj = new { Host = "localhost", Port = 8080 };
            SynxFormat.SaveFile(tmp, obj);
            var text = File.ReadAllText(tmp);
            Assert.Contains("host localhost", text);
            Assert.Contains("port 8080", text);
        }
        finally { File.Delete(tmp); }
    }

    [Fact]
    public void FromJson_converts_json_to_synx()
    {
        var json = """{"name":"Wario","age":30}""";
        var synx = SynxFormat.FromJson(json);
        Assert.Contains("age 30", synx);
        Assert.Contains("name Wario", synx);
        // Round-trip
        var map = SynxFormat.Parse(synx);
        Assert.Equal(new SynxValue.Int(30), map["age"]);
    }
}
