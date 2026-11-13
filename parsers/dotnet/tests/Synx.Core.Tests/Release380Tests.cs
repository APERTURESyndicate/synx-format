using System.Text;
using Synx;
using Xunit;

namespace Synx.Tests;

/// <summary>SYNX 3.8.0 parity: every expected value below is synx-core's output for the same input.</summary>
public class Release380Tests
{
    private static string Active(string body) => SynxFormat.ToJson(SynxFormat.ParseActive("!active\n" + body));
    private static string Static(string text) => SynxFormat.ToJson(SynxFormat.Parse(text));

    // ── Active mode: order and declarations ──

    [Fact]
    public void Markers_resolve_in_document_order_at_any_depth()
    {
        Assert.Equal("{\"limit\":5,\"price\":100,\"svc\":{\"limit\":100,\"note\":\"Total is 120\",\"price\":200},\"tax\":20,\"total\":120}",
            Active("price 100\ntax:calc price * 0.2\ntotal:calc price + tax\nlimit 5\nsvc\n  limit 100\n  price:calc limit * 2\n  note Total is {total}\n"));
    }

    [Fact]
    public void Calc_reaches_special_keys_and_local_shadows_root()
    {
        Assert.Equal("{\"2\":5,\"both\":21,\"hp\":3,\"inf\":1,\"left\":7,\"max-hp\":10,\"n.m\":4,\"price\":100,\"prod\":12,\"q\":2,\"svc\":{\"hp\":50,\"x\":250,\"y\":150},\"z\":25,\"цена\":7}",
            Active("price 100\nцена 7\nmax-hp 10\nhp 3\nleft:calc max-hp - hp\nboth:calc цена * hp\nn.m 4\nprod:calc n.m * hp\nsvc\n  hp 50\n  x:calc hp * 2\n  y:calc price + hp\n2 5\nz:calc 2 * 2\ninf 1\nq:calc inf + 1\n"));
    }

    [Fact]
    public void Calc_errors_read_as_in_synx_core()
    {
        var json = Active("x:calc 1 / 0\nz:calc (2+3)*4\nw:calc 1.2.3 + 1\nv:calc -\ns:calc цена * 2\nn:calc )\nm:calc 5 5\nt:calc (1 + 2\n");
        Assert.Equal("{\"m\":\"CALC_ERR: SYNX :calc — unexpected token at position 1\",\"n\":\"CALC_ERR: SYNX :calc — unexpected token: ')'\","
            + "\"s\":\"CALC_ERR: SYNX :calc — unexpected character: 'ц' in expression\",\"t\":\"CALC_ERR: SYNX :calc — missing closing parenthesis\","
            + "\"v\":\"CALC_ERR: SYNX :calc — invalid number: '-'\",\"w\":\"CALC_ERR: SYNX :calc — invalid number: '1.2.3'\","
            + "\"x\":\"CALC_ERR: SYNX :calc — division by zero\",\"z\":20}", json);
    }

    [Fact]
    public void Every_list_item_keeps_its_own_declarations()
    {
        Assert.Equal("{\"servers\":[{\"name\":\"a\",\"port\":\"CONSTRAINT_ERR: 'port' value 7001 exceeds max 9\"},{\"name\":\"b\",\"port\":\"CONSTRAINT_ERR: 'port' value 8080 exceeds max 9\"},"
                     + "{\"name\":\"c\",\"port\":\"CONSTRAINT_ERR: 'port' value 77 exceeds max 9\"},{\"name\":\"d\",\"port\":5}]}",
            Active("servers\n  - name a\n    port:env:default:7001 SYNX_UNSET_Q\n  - name b\n    port 8080\n  - name c\n    port[type:int, max:9] 77\n  - name d\n    port 5\n"));
        var r = SynxFormat.ParseFull("!active\nservers\n  - name a\n    port:env:default:7001 X\n");
        Assert.True(r.Metadata.ContainsKey("servers.#0"));
    }

    [Fact]
    public void Dotted_key_and_nested_key_never_share_declarations()
    {
        Assert.Equal("{\"a\":{\"b\":{\"x\":\"hello\",\"y\":\"c,d\"}},\"a.b\":{\"x\":9,\"y\":[\"a\",\"b\"]}}",
            Active("a.b\n  x:env:default:9 SYNX_UNSET_Q\n  y:split a,b\na\n  b\n    x hello\n    y c,d\n"));
        Assert.True(SynxFormat.ParseFull("!active\na.b\n  x:env Q\n").Metadata.ContainsKey("a\\.b"));
    }

    [Fact]
    public void Replaced_subtree_drops_its_declarations()
    {
        Assert.Equal("{\"a\":5,\"b\":{\"y\":1},\"c\":{\"y\":\"TYPE_ERR: 'y' expected int but got string\"},\"k1\":\"6\",\"port\":{},\"x\":\"hello\"}",
            Active("port\n  port[enum:a|b] x\nport\na\n  x(int) 1\na 5\nx hello\nk1(int) 5\nk1(string) 6\nb\n  y(int) 1\nc\n  y(string) z\n"));
    }

    [Fact]
    public void Inherit_carries_declarations_merges_parents_and_leaves_lists_alone()
    {
        Assert.Equal("{\"extra\":{\"q\":4,\"weight\":3},\"listchild\":[\"a\"],\"multi\":{\"port\":5,\"q\":4,\"sub\":{\"deep\":6},\"weight\":3,\"z\":1},"
                     + "\"plain\":{\"port\":9000,\"sub\":{\"deep\":6},\"weight\":1},\"scalar\":{\"port\":5,\"sub\":{\"deep\":6},\"weight\":1},\"steel\":{\"port\":5,\"sub\":{\"deep\":6},\"weight\":2}}",
            Active("_base\n  port:env:default:5 SYNX_UNSET_Q\n  weight 1\n  sub\n    deep:calc 2 * 3\nsteel:inherit:_base\n  weight 2\nplain:inherit _base\n  port 9000\n"
                   + "listchild:inherit:_base\n  - a\nscalar:inherit:_base 5\nmulti:inherit:_base:extra\n  z 1\nextra\n  weight 3\n  q:calc 2 + 2\n"));
    }

    [Fact]
    public void Null_checks_only_enum_and_strings_count_scalars()
    {
        Assert.Equal("{\"e2\":null,\"mode\":\"CONSTRAINT_ERR: 'mode' must be one of [a|b]\",\"name\":null,\"port\":null,\"word\":\"CONSTRAINT_ERR: 'word' value 4 exceeds max 3\",\"word2\":\"мир\"}",
            Active("mode[enum:a|b]:env SYNX_UNSET_Q\nport[type:int]:env SYNX_UNSET_Q\nname[min:3]:env SYNX_UNSET_Q\ne2[enum:a|]:env SYNX_UNSET_Q\nword[min:2, max:3] мира\nword2[max:3] мир\n"));
    }

    // ── Values as text ──

    [Fact]
    public void Numbers_read_as_plain_decimal_text()
    {
        Assert.Equal("{\"a\":1e-7,\"c\":2e-7,\"e\":1e-7,\"h\":1.2345678901234569e23,\"l\":\"0.00000025,2.5\",\"t\":\"v=0.0000001 h=123456789012345690000000\"}",
            Active("a 0.0000001\nh 123456789012345678901234.5\nt v={a} h={h}\nc:calc a * 2\nl:join\n  - 0.00000025\n  - 2.5\ne[enum:0.0000001] 0.0000001\n"));
        Assert.Equal("{\"a\":\"See !\",\"b\":\"Value null.\",\"nothing\":null,\"obj\":{\"k\":1}}", Active("obj\n  k 1\nnothing null\na See {obj}!\nb Value {nothing}.\n"));
    }

    [Fact]
    public void Format_follows_rust_formatting()
    {
        Assert.Equal("{\"a\":\"0.12\",\"b\":\"2\",\"d\":\"00-42\",\"e\":\"1.235e5\",\"f\":\"1.23456789e+5\",\"g\":\"    7\",\"j\":\"0.00e0\",\"k\":\"2e1\",\"l\":\"00012\","
                     + "\"m\":\"9223372036854775807\",\"n\":\"-0.00\",\"s\":\"3.14159\",\"t\":\"5\"}",
            Active("a:format:%.2f 0.125\nb:format:%.0f 2.5\nd:format:%05d -42\ne:format:%.3e 123456.789\nf:format:%e 123456.789\ng:format:%+5d 7\nj:format:%.2e 0\n"
                   + "k:format:%.0e 15\nl:format:%05d 12.0\nm:format:%d 9223372036854775807\nn:format:%.2f -0.0\ns:format:%.2f \"3.14159\"\nt:format:%x 5\n"));
    }

    [Fact]
    public void Sort_sum_replace_and_version()
    {
        Assert.Equal("{\"r1\":\"f00 b00\",\"s1\":[9,10,\"B\",\"a\",\"b\"],\"s2\":[3,2,1.5],\"s3\":[\"Z\",\"e\",\"zz\",\"é\"],\"sum1\":6.5,\"v1\":true,\"v3\":true}",
            Active("s1:sort\n  - b\n  - a\n  - 10\n  - 9\n  - B\ns2:sort:desc\n  - 3\n  - 1.5\n  - 2\ns3:sort\n  - zz\n  - é\n  - e\n  - Z\nsum1:sum\n  - 1\n  - 2.5\n  - \"3\"\n  - x\n"
                   + "r1:replace:o:0 foo boo\nv1:version:>=:1.2.3 1.2.3-beta\nv3:version:<:2 1.x.9\n"));
    }

    // ── Parser ──

    [Fact]
    public void Indentation_is_measured_in_utf8_bytes()
    {
        // U+00A0 counts 2 bytes, U+3000 counts 3; tab and space 1. `|+` cuts whole characters.
        var nb = ((char)0xA0).ToString();
        var id = ((char)0x3000).ToString();
        var src = $"a\n{nb}b 1\n{nb}{nb}c 2\n\t d 3\n{id}e 4\nf\n{id}g 5\n  h 6\nlist\n{nb}- x\n{nb}- y\ncode |+\n{nb}{nb}line1\n{nb}{nb}{nb}{nb}line2\n{nb}{nb} line3\ntxt |\n  {nb}a  \n{nb} \n  b{id}\nlast 1{nb}\n";
        Assert.Equal($"{{\"a\":{{\"b\":1,\"c\":2,\"d\":3,\"e\":4}},\"code\":\"line1\\n{nb}{nb}line2\\n line3\",\"f\":{{\"g\":5,\"h\":6}},\"last\":1,\"list\":[\"x\",\"y\"],\"txt\":\"a\\nb\"}}", Static(src));
    }

    [Fact]
    public void List_items_get_the_right_parent()
    {
        Assert.Equal("{\"after\":1,\"items\":[{\"name\":\"a\",\"sub\":{\"k\":1}},{\"name\":\"b\",\"v\":2,\"w\":3}],\"lst\":5,\"objs\":[\"one\",{\"three\":3,\"two\":2},4],\"x\":[],\"x_1\":[{\"k\":\"v\",\"y\":2}]}",
            Static("items\n  - name a\n    sub\n      k 1\n  - name b\n    v 2\n    w 3\nafter 1\nx:join\n x_1\n # c\n  - k v\n   y 2\nlst\n  - a\n  lst 5\n  - b\nobjs\n  - one\n  - two 2\n    three 3\n  - 4\n"));
    }

    [Fact]
    public void Item_of_a_redeclared_list_pops_its_frames_before_it_is_dropped()
    {
        // `a` was re-declared as an object, so `- {cfg.v}` is dropped — but the frames at
        // or deeper than its indent are popped first, and `name` lands at the root.
        Assert.Equal("{\"a\":{\"x_1\":\"[x]\"},\"name\":{}}",
            Static("a\n  - base_hp 's'\n  a\n      x_1 [x]\n  - {cfg.v}\n   name\n"));
    }

    [Fact]
    public void Use_packages_stay_inside_the_packages_directory()
    {
        var dir = Directory.CreateTempSubdirectory("synx-use-").FullName;
        try
        {
            Directory.CreateDirectory(Path.Combine(dir, "synx_packages", "@a", "ok", "src"));
            File.WriteAllText(Path.Combine(dir, "synx_packages", "@a", "ok", "src", "main.synx"), "v 1\n");
            Directory.CreateDirectory(Path.Combine(dir, "secret", "src"));
            File.WriteAllText(Path.Combine(dir, "secret", "src", "main.synx"), "leak 1\n");
            // A manifest path that leaves the package counts as absent: src/main.synx is read.
            Directory.CreateDirectory(Path.Combine(dir, "synx_packages", "@a", "evil", "src"));
            File.WriteAllText(Path.Combine(dir, "synx_packages", "@a", "evil", "synx-pkg.synx"), "main ../../../secret/src/main.synx\n");
            File.WriteAllText(Path.Combine(dir, "synx_packages", "@a", "evil", "src", "main.synx"), "own 1\n");
            var json = SynxFormat.ToJson(SynxFormat.ParseActive(
                "!active\n!use @a/ok\n!use @a/../../secret as up\n!use @a/c:x as colon\n!use @a/evil\nx 1\n",
                new SynxOptions { BasePath = dir }));
            Assert.Equal("{\"evil\":{\"own\":1},\"ok\":{\"v\":1},\"x\":1}", json);
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    [Fact]
    public void Self_include_stops_at_the_first_repeat()
    {
        var dir = Directory.CreateTempSubdirectory("synx-self-").FullName;
        try
        {
            File.WriteAllText(Path.Combine(dir, "self.synx"), "!active\nme:include ./self.synx\nv 1\n");
            var sw = System.Diagnostics.Stopwatch.StartNew();
            var json = SynxFormat.ToJson(SynxFormat.ParseActive(
                File.ReadAllText(Path.Combine(dir, "self.synx")), new SynxOptions { BasePath = dir }));
            sw.Stop();
            // The text handed to ParseActive has no path; the file it includes is open once,
            // and including it again from inside itself is the cycle.
            Assert.Equal("{\"me\":{\"me\":\"INCLUDE_ERR: circular include: './self.synx'\",\"v\":1},\"v\":1}", json);
            Assert.True(sw.ElapsedMilliseconds < 3000, $"took {sw.ElapsedMilliseconds} ms");

            // The pre-pass result is no extra include: `{x:include}` still sees exactly one.
            File.WriteAllText(Path.Combine(dir, "common.synx"), "site Demo\n");
            Assert.Equal("{\"db\":{\"site\":\"Demo\"},\"x\":\"Demo / Demo\"}", SynxFormat.ToJson(SynxFormat.ParseActive(
                "!active\ndb:include ./common.synx\nx {site:include} / {site:db}\n", new SynxOptions { BasePath = dir })));
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    [Fact]
    public void Clamp_bounds_must_be_finite_numbers()
    {
        Assert.Equal("{\"a\":7,\"b\":7,\"c\":7,\"d\":7,\"e\":7,\"f\":10,\"g\":5}",
            Active("a:clamp:nan:5 7\nb:clamp:1:inf 7\nc:clamp:Infinity:5 7\nd:clamp:1e400:5 7\ne:clamp:5abc:9 7\nf:clamp:-1e3:1E1 70\ng:clamp:1:5 7\n"));
    }

    [Fact]
    public void A_leading_bom_alone_is_not_truncation()
    {
        var bom = ((char)0xFEFF).ToString();
        Assert.False(SynxFormat.ParseFull(bom + "name x\n").Truncated);
        Assert.Equal("{\"name\":\"x\"}", Static(bom + "name x\n"));
    }

    // ── Writers ──

    [Fact]
    public void Stringify_round_trips_data()
    {
        var src = "a \"007\"\nb \"true\"\nc \"x # y\"\nd \"\"\ne:join\nf -0.0\ng 2.0\ncode |+\n  if x\n    y\nlist\n  - item x\n    item y\n  - one 1\n    sub\n      k v\n";
        var parsed = SynxFormat.Parse(src);
        var text = SynxFormat.Stringify(parsed);
        Assert.Equal(Static(src), Static(text));
        Assert.Contains("e:join\n", text);
        Assert.Contains("f -0.0\n", text);
        Assert.Contains("code |+\n", text);
        Assert.Equal("[SECRET]", SynxFormat.Stringify(SynxFormat.ParseActive("!active\ns:secret hunter2\n")).Split(' ', '\n')[1]);
    }

    [Fact]
    public void Synxb_is_deterministic_and_round_trips()
    {
        var src = "!active\nb 2\na x\nc\n  - x\n  - x\n  - y\nport[min:1]:env:default:80 P\nbig 9223372036854775807\n";
        var one = SynxFormat.Compile(src);
        Assert.Equal(one, SynxFormat.Compile(src));
        var back = SynxFormat.Decompile(one);
        Assert.Equal(Static(src), Static(back));
        Assert.StartsWith("!active\n\n", back);
        Assert.Contains("big 9223372036854775807\n", back);
        // Not resolved: the file carries the metadata section (flag bit 2), as in synx-core.
        Assert.NotEqual(0, one[6] & 0b100);
    }

    [Fact]
    public void Format_keeps_block_bodies_and_sorts_by_code_point()
    {
        const string src = "code |+\n  zeta()\n    alpha()\n  beta\nname x\n";
        var output = SynxFormat.Format(src);
        Assert.Equal("code |+\n  zeta()\n    alpha()\n  beta\n\nname x\n", output);
        Assert.Equal(output, SynxFormat.Format(output));
        Assert.Equal("a_b 1\nab 2\n", SynxFormat.Format("ab 2\na_b 1\n"));
        Assert.Equal("{\"a\":\"second\",\"b\":1,\"c\":2}", Static(SynxFormat.Format("b 1\na first\nc 2\na second\n")));
    }

    [Fact]
    public void Format_past_the_depth_cap_drops_the_deeper_subtree()
    {
        // As synx-core: levels past the cap are dropped, the next shallower line goes on.
        var sb = new StringBuilder();
        for (var i = 0; i < 140; i++) sb.Append(' ', i).Append('k').Append(i).Append('\n');
        sb.Append("tail 1\n  t2 2\n");
        var expected = new StringBuilder();
        for (var i = 0; i <= 128; i++) expected.Append(' ', 2 * i).Append('k').Append(i).Append('\n');
        expected.Append("\ntail 1\n  t2 2\n");
        Assert.Equal(expected.ToString(), SynxFormat.Format(sb.ToString()));
    }

    [Fact]
    public void Tool_documents_are_resolved_before_reshaping()
    {
        Assert.Equal("{\"params\":{\"port\":8080,\"q\":6},\"tool\":\"web_search\"}",
            SynxFormat.ToJson(SynxFormat.ParseTool("!tool\n!active\nweb_search\n  port:env:default:8080 SYNX_UNSET_Q\n  q:calc 2 * 3\n")));
    }

    [Fact]
    public void Keys_sort_by_code_point_not_utf16_unit()
    {
        // Rust sorts UTF-8 bytes: U+E000 / U+FFFD come before U+1F600, whose UTF-16 lead unit is smaller.
        var pua = ((char)0xE000).ToString();
        var rep = ((char)0xFFFD).ToString();
        var emoji = char.ConvertFromUtf32(0x1F600);
        var src = $"{emoji} 2\n{pua} 1\na\n  {emoji} x\n  {rep} y\n";
        Assert.Equal($"{{\"a\":{{\"{rep}\":\"y\",\"{emoji}\":\"x\"}},\"{pua}\":1,\"{emoji}\":2}}", Static(src));
        Assert.StartsWith($"a\n  {rep} y\n", SynxFormat.Stringify(SynxFormat.Parse(src)));
    }

    [Fact]
    public void Json_floats_use_the_ryu_layout()
    {
        Assert.Equal("{\"a\":1.0,\"b\":0.5,\"c\":1e21,\"d\":1e-7,\"e\":-0.0,\"f\":1e16,\"g\":1000000000000000.0}",
            Static("a 1.0\nb 0.5\nc 1000000000000000000000.0\nd 0.0000001\ne -0.0\nf 10000000000000000.0\ng 1000000000000000.0\n"));
    }
}
