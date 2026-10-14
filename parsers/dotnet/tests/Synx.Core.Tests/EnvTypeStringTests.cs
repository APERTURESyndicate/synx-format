using Synx;
using Xunit;

namespace Synx.Tests;

/// <summary>SYNX 3.7.3/3.7.4 engine parity: <c>[type:string]</c> env text, exact i64, secret masking, BOM.</summary>
public class EnvTypeStringTests
{
    private static Dictionary<string, SynxValue> ResolveEnv(string text) =>
        SynxFormat.ParseActive(text, new SynxOptions
        {
            Env = new Dictionary<string, string>
            {
                ["ZIP"] = "007",
                ["SNOW"] = "1550184245153112174",
                ["Q"] = "\"12\"",
            },
        });

    [Fact]
    public void Env_type_string_constraint_keeps_text()
    {
        var map = ResolveEnv(
            "!active\nid[type:string]:env SNOW\nzip[type:string]:env ZIP\nfallback[type:string]:env:default:0042 MISSING\nplain:env ZIP\n");
        Assert.Equal(new SynxValue.Str("1550184245153112174"), map["id"]);
        Assert.Equal(new SynxValue.Str("007"), map["zip"]);
        Assert.Equal(new SynxValue.Str("0042"), map["fallback"]);
        Assert.Equal(new SynxValue.Int(7), map["plain"]);
    }

    [Fact]
    public void Type_string_constraint_accepts_secret()
    {
        var map = ResolveEnv("!active\ntoken[type:string]:secret:env SNOW\n");
        Assert.Equal(new SynxValue.Secret("1550184245153112174"), map["token"]);
    }

    [Fact]
    public void Standalone_default_honours_type_string_constraint()
    {
        var map = SynxFormat.ParseActive("!active\ncode[type:string]:default:007 null\nnum:default:007 null\n");
        Assert.Equal(new SynxValue.Str("007"), map["code"]);
        Assert.Equal(new SynxValue.Int(7), map["num"]);
    }

    [Fact]
    public void Env_cast_keeps_64bit_integers_exact()
    {
        var map = ResolveEnv("!active\nraw:env SNOW\n");
        Assert.Equal(new SynxValue.Int(1550184245153112174L), map["raw"]);
    }

    [Fact]
    public void Env_reference_example_matches_rust()
    {
        var map = ResolveEnv(
            "!active\nz:env ZIP\nzs[type:string]:env ZIP\nid[type:string]:env SNOW\nraw:env SNOW\n" +
            "d[type:string]:env:default:007 MISSING\nsec[type:string]:secret:env SNOW\nq:env Q\n");
        Assert.Equal(
            "{\"d\":\"007\",\"id\":\"1550184245153112174\",\"q\":\"12\",\"raw\":1550184245153112174,\"sec\":\"[SECRET]\",\"z\":7,\"zs\":\"007\"}",
            SynxFormat.ToJson(map));
    }

    [Fact]
    public void Interpolation_of_secret_is_masked()
    {
        var map = SynxFormat.ParseActive("!active\ntoken:secret abc\nmsg hello {token} world\n");
        Assert.Equal(new SynxValue.Str("hello [SECRET] world"), map["msg"]);
        Assert.DoesNotContain("abc", SynxFormat.ToJson(map));
    }

    [Fact]
    public void Nested_interpolation_does_not_observe_root_secret()
    {
        var map = SynxFormat.ParseActive("!active\ntoken:secret abc123\nsection\n  msg use {token} now\n");
        var section = Assert.IsType<SynxValue.Obj>(map["section"]);
        Assert.Equal(new SynxValue.Str("use [SECRET] now"), section.Map["msg"]);
        Assert.DoesNotContain("abc123", SynxFormat.ToJson(map));
    }

    [Fact]
    public void Interpolation_keeps_non_ascii_literal_text()
    {
        var map = SynxFormat.ParseActive("!active\nname Привет\nmsg мир {name} 東京\n");
        Assert.Equal(new SynxValue.Str("мир Привет 東京"), map["msg"]);
    }

    [Fact]
    public void Leading_bom_does_not_disable_active_mode()
    {
        var map = SynxFormat.ParseActive("﻿!active\nsum:calc 2 + 3\n");
        Assert.Equal(new SynxValue.Int(5), map["sum"]);
    }

    [Fact]
    public void Leading_bom_is_not_glued_to_first_key()
    {
        var map = SynxFormat.Parse("﻿name Wario\n");
        Assert.True(map.ContainsKey("name"));
        Assert.False(map.ContainsKey("﻿name"));
    }

    [Fact]
    public void Non_ascii_key_keeps_its_value()
    {
        var map = SynxFormat.Parse("ключ значение\n東京(int) 5\n");
        Assert.Equal(new SynxValue.Str("значение"), map["ключ"]);
        Assert.Equal(new SynxValue.Int(5), map["東京"]);
    }

    [Theory]
    [InlineData("!foo bar\nx 1\n", "{\"!foo\":\"bar\",\"x\":1}")]
    [InlineData("!foo\nx 1\n", "{\"!foo\":{},\"x\":1}")]
    [InlineData("!foo bar baz\nx 1\n", "{\"!foo\":\"bar baz\",\"x\":1}")]
    [InlineData("!bar baz qux\n  child 1\nx 1\n#!zzz\ny 2\n", "{\"!bar\":\"baz qux\",\"child\":1,\"x\":1,\"y\":2}")]
    public void Unknown_bang_line_is_an_ordinary_key_like_rust(string text, string expected)
    {
        Assert.Equal(expected, SynxFormat.ToJson(SynxFormat.Parse(text)));
    }

    [Fact]
    public void Unknown_bang_line_survives_in_active_documents()
    {
        var map = SynxFormat.ParseActive("!active\n!foo bar\nx:calc 1 + 2\n");
        Assert.Equal("{\"!foo\":\"bar\",\"x\":3}", SynxFormat.ToJson(map));
    }
}
