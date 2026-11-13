using Synx;
using Xunit;

namespace Synx.Tests;

/// <summary>
/// Documents whose reading changed after 3.8.0: a secret is hidden until its own turn and
/// keeps the shape of what it masks, the key on a `- ` line is a key line, a comment is no
/// content, and a list inside a list item ends at the next item of the outer list. The
/// expected value is the reference CLI's output for the same input.
/// </summary>
public class ParityTests
{
    public static TheoryData<string, string, string> Cases() => new()
    {
        { "sec_alias_before", "!active\nleak:alias password\npassword:secret:default:x hunter2\n",
            "{\"leak\":\"[SECRET]\",\"password\":\"[SECRET]\"}" },
        { "sec_interp_before", "!active\na\n  x {password}\npassword:secret:replace:q:z hunter2\n",
            "{\"a\":{\"x\":\"[SECRET]\"},\"password\":\"[SECRET]\"}" },
        { "sec_ref_before", "!active\nleak:ref password\npassword:secret:format:%s hunter2\n",
            "{\"leak\":\"[SECRET]\",\"password\":\"[SECRET]\"}" },
        { "sec_group_before", "!active\ne\n  x {creds.pass}\n  y:alias creds.pass\ncreds:secret\n  pass hunter2\n",
            "{\"creds\":{\"pass\":\"[SECRET]\"},\"e\":{\"x\":\"[SECRET]\",\"y\":\"[SECRET]\"}}" },
        { "sec_map_before", "!active\npick:map:password\n  - hunter2 leaked\n  - other no\npassword:secret:fallback:zz hunter2\n",
            "{\"password\":\"[SECRET]\",\"pick\":null}" },
        { "sec_split", "!active\npassword:secret:split:, hunter2,x\n",
            "{\"password\":[\"[SECRET]\",\"[SECRET]\"]}" },
        { "sec_list", "!active\nkeys:secret\n  - hunter2\n  - other\n",
            "{\"keys\":[\"[SECRET]\",\"[SECRET]\"]}" },
        { "sec_default_novalue", "!active\npassword:secret:default:hunter2\n",
            "{\"password\":{}}" },
        { "sec_group", "!active\ncreds:secret\n  user bob\n  pass hunter2\n",
            "{\"creds\":{\"pass\":\"[SECRET]\",\"user\":\"[SECRET]\"}}" },
        { "sec_novalue", "!active\ns:secret\n",
            "{\"s\":{}}" },
        { "sec_chain", "!active\nf:secret:format:%05d 5\nd:secret:default:abc null\nw:secret:watch nope.txt\nc:secret:calc 2 * 21\nlater {f} {d} {c}\nn:secret 007\n",
            "{\"c\":\"[SECRET]\",\"d\":\"[SECRET]\",\"f\":\"[SECRET]\",\"later\":\"[SECRET] [SECRET] [SECRET]\",\"n\":\"[SECRET]\",\"w\":\"WATCH_ERR: file not found: nope.txt\"}" },
        { "sec_after", "!active\npassword:secret hunter2\nleak {password}\nref:ref password\nali:alias password\njoined:join\n  - a\n  - {password}\n",
            "{\"ali\":\"[SECRET]\",\"joined\":\"a,[SECRET]\",\"leak\":\"[SECRET]\",\"password\":\"[SECRET]\",\"ref\":\"[SECRET]\"}" },
        { "item_secret", "!active\nusers\n  - password:secret hunter2\n    name bob\n",
            "{\"users\":[{\"name\":\"bob\",\"password\":\"[SECRET]\"}]}" },
        { "item_markers", "!active\nt\n  - c:calc 1 + 2\n    v 1\n  - d:split a,b\n    v 2\n  - x:version:>=:1 1.2\n    v 3\n  - y:clamp:1:5 9\n    v 4\n",
            "{\"t\":[{\"c\":3,\"v\":1},{\"d\":[\"a\",\"b\"],\"v\":2},{\"v\":3,\"x\":1.2},{\"v\":4,\"y\":5}]}" },
        { "item_constraint", "!active\nt\n  - port[type:int] abc\n    v 1\n  - n(int) 7\n    v 2\n",
            "{\"t\":[{\"port\":\"CONSTRAINT_ERR: 'port' expected type 'int'\",\"v\":1},{\"n\":7,\"v\":2}]}" },
        { "item_env_default", "!active\nt\n  - u:env:default:fallback SYNX_PROBE_UNSET\n    v 1\n  - host:env SYNX_PROBE_UNSET\n",
            "{\"t\":[{\"u\":\"fallback\",\"v\":1},\"host:env SYNX_PROBE_UNSET\"]}" },
        { "list_comment_first", "k\n# note\n  - a\n  - b\n",
            "{\"k\":[\"a\",\"b\"]}" },
        { "list_comment_same_indent", "0\n#\n- 0",
            "{\"0\":[]}" },
        { "list_nested", "items\n  - name a\n    tags\n      - x\n      - y\n  - name b\n    tags\n      - z\n  - name c\nafter 1\n",
            "{\"after\":1,\"items\":[{\"name\":\"a\",\"tags\":[\"x\",\"y\"]},{\"name\":\"b\",\"tags\":[\"z\"]},\"name c\"]}" },
        { "list_empty_last_key", "servers\n  - name a\n    tags\n  - name b\n    tags\n  - name c\n    port 1\n",
            "{\"servers\":[{\"name\":\"a\",\"tags\":[]},{\"name\":\"b\",\"tags\":[]},{\"name\":\"c\",\"port\":1}]}" },
        { "list_empty_key_scalars", "p\n  - u 1\n    e\n  - h\n  - z\n",
            "{\"p\":[{\"e\":[],\"u\":1},\"h\",\"z\"]}" },
        { "list_item_comment", "l\n  - name a\n    # c\n    port 1\n  - name b\n    ###\n    hidden 1\n    ###\n    port 2\n",
            "{\"l\":[{\"name\":\"a\",\"port\":1},{\"name\":\"b\",\"port\":2}]}" },
        { "list_three_levels", "a\n  - b\n    c\n      - d\n        e\n          - f 1\n          - f 2\n      - g 3\n  - h 4\nz 9\n",
            "{\"a\":[{\"b\":{},\"c\":[{\"d\":{},\"e\":[\"f 1\",\"f 2\"]},\"g 3\"]},\"h 4\"],\"z\":9}" },
        { "list_dash_no_list", "a 1\n- stray\nb\n  c 2\n  - also stray\n  d 3\n",
            "{\"a\":1,\"b\":{\"c\":2,\"d\":3}}" },
        { "list_block_comment_between", "k\n###\nold\n  - x\n###\n  - a\n",
            "{\"k\":[\"a\"]}" },
    };

    [Theory]
    [MemberData(nameof(Cases))]
    public void Document_reads_as_synx_core_reads_it(string name, string input, string expected)
    {
        var dir = Directory.CreateTempSubdirectory("synx-parity-").FullName;
        try
        {
            var options = new SynxOptions { Env = new Dictionary<string, string>(), BasePath = dir };
            var json = SynxFormat.ToJson(SynxFormat.ParseFullActive(input, options).Root);
            Assert.True(expected == json, $"{name}{Environment.NewLine}  got:      {json}{Environment.NewLine}  expected: {expected}");
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    [Fact]
    public void A_secret_include_is_masked_in_the_alias_scope_too()
    {
        var dir = Directory.CreateTempSubdirectory("synx-parity-").FullName;
        try
        {
            File.WriteAllText(Path.Combine(dir, "creds.synx"), "password hunter2\nuser bob\n");
            var options = new SynxOptions { Env = new Dictionary<string, string>(), BasePath = dir };
            var json = SynxFormat.ToJson(SynxFormat.ParseActive(
                "!active\ndb:secret:include creds.synx\nx {password:db}\ny {password:include}\nz {db.password}\n", options));
            Assert.Equal("{\"db\":{\"password\":\"[SECRET]\",\"user\":\"[SECRET]\"},\"x\":\"[SECRET]\",\"y\":\"[SECRET]\",\"z\":\"[SECRET]\"}", json);
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }
}
