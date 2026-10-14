using System.Diagnostics;
using Synx;
using Xunit;

namespace Synx.Tests;

/// <summary>Constraint failures land in the value exactly as Rust writes them (SYNX 3.7.4).</summary>
public class ConstraintTests
{
    private static string Json(string text) => SynxFormat.ToJson(SynxFormat.ParseActive(text));

    [Fact]
    public void Redos_shaped_pattern_is_refused_quickly_and_leaves_value_unchanged()
    {
        var value = new string('a', 45) + "!";
        var sw = Stopwatch.StartNew();
        var map = SynxFormat.ParseActive($"!active\nv[pattern:^(a+)+$] {value}\n");
        sw.Stop();
        Assert.Equal(new SynxValue.Str(value), map["v"]);
        Assert.True(sw.ElapsedMilliseconds < 500, $"took {sw.ElapsedMilliseconds} ms");
    }

    [Fact]
    public void Flat_patterns_still_validate()
    {
        Assert.Equal(
            "{\"a\":\"abc\",\"b\":\"CONSTRAINT_ERR: 'b' does not match pattern /^[a-z]+$/\"}",
            Json("!active\na[pattern:^[a-z]+$] abc\nb[pattern:^[a-z]+$] ABC\n"));
    }

    [Fact]
    public void Invalid_and_overlong_patterns_are_skipped_silently()
    {
        var longPattern = new string('a', 300);
        Assert.Equal("{\"p\":\"abc\",\"q\":\"abc\"}",
            Json($"!active\np[pattern:(] abc\nq[pattern:{longPattern}] abc\n"));
    }

    [Fact]
    public void Length_counts_unicode_scalar_values()
    {
        Assert.Equal(
            "{\"cyr\":\"мир\",\"emoji\":\"😀😀\",\"long\":\"CONSTRAINT_ERR: 'long' value 5 exceeds max 4\"}",
            Json("!active\ncyr[max:4] мир\nemoji[max:2] 😀😀\nlong[max:4] мирок\n"));
    }

    [Fact]
    public void Numbers_print_like_rust_display()
    {
        Assert.Equal(
            "{\"a\":\"CONSTRAINT_ERR: 'a' value 0.25 is below min 0.5\",\"b\":\"CONSTRAINT_ERR: 'b' value 70000 exceeds max 65535\"," +
            "\"c\":\"CONSTRAINT_ERR: 'c' value 123456789012345690000000 exceeds max 5\"}",
            Json("!active\na[min:0.5] 0.25\nb[max:65535] 70000\nc[max:5] 123456789012345678901234.5\n"));
    }

    [Fact]
    public void Field_registry_merges_declarations_across_the_document()
    {
        Assert.Equal(
            "{\"a\":{\"port\":8080},\"b\":{\"port\":\"CONSTRAINT_ERR: 'port' value 70000 exceeds max 65535\"}," +
            "\"c\":{\"port\":\"CONSTRAINT_ERR: 'port' expected type 'int'\"}}",
            Json("!active\na\n  port[type:int, min:1, max:65535] 8080\nb\n  port 70000\nc\n  port abc\n"));
    }

    [Fact]
    public void Per_key_error_is_not_revalidated_by_the_registry_pass()
    {
        // The registry pass would otherwise measure the error text instead of the original value.
        Assert.Equal("{\"x\":\"CONSTRAINT_ERR: 'x' value 0 is below min 1\"}", Json("!active\nx[min:1] 0\n"));
    }

    [Fact]
    public void Type_registry_reports_type_err()
    {
        Assert.Equal(
            "{\"boss\":{\"hp\":\"TYPE_ERR: 'hp' expected int but got string\"},\"hp\":100}",
            Json("!active\nhp(int) 100\nboss\n  hp lots\n"));
    }
}
