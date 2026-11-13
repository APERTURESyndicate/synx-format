package com.aperturesyndicate.synx;

import java.io.ByteArrayOutputStream;
import java.util.HashMap;
import java.util.zip.Inflater;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

/**
 * Regression tests for the SYNX 3.8.0 defects (CHANGELOG [3.8.0]). Every expected JSON string is
 * the output of the reference CLI ({@code synx parse}) for the same document.
 */
class Release380Test {

    private static String resolved(String src) {
        SynxOptions o = new SynxOptions();
        o.env = new HashMap<>();
        return Synx.toJson(Synx.parseFullActive(src, o).root);
    }

    private static void expect(String src, String want) {
        assertEquals(want, resolved(src));
    }

    @Test
    void calc_follows_document_order() {
        for (int i = 0; i < 20; i++) {
            expect("!active\nprice 100\ntax:calc price * 0.2\ntotal:calc price + tax\n",
                "{\"price\":100,\"tax\":20,\"total\":120}");
        }
    }

    @Test
    void calc_local_key_shadows_root_and_special_keys() {
        expect("!active\nlimit 5\nsvc\n  limit 100\n  price:calc limit * 2\nmax-hp 10\nhp 3\nleft:calc max-hp - hp\n",
            "{\"hp\":3,\"left\":7,\"limit\":5,\"max-hp\":10,\"svc\":{\"limit\":100,\"price\":200}}");
    }

    @Test
    void calc_errors_match_reference() {
        expect("!active\nbad:calc 1.2.3\nlone:calc -\nuni:calc цена + 1\ndiv:calc 1 / 0\nex:calc (1 + 2\nf:calc 0.1 + 0.2\nbig:calc 1e3\n",
            "{\"bad\":\"CALC_ERR: SYNX :calc — invalid number: '1.2.3'\",\"big\":\"CALC_ERR: SYNX :calc — unexpected character: 'e' in expression\","
            + "\"div\":\"CALC_ERR: SYNX :calc — division by zero\",\"ex\":\"CALC_ERR: SYNX :calc — missing closing parenthesis\",\"f\":0.30000000000000004,"
            + "\"lone\":\"CALC_ERR: SYNX :calc — invalid number: '-'\",\"uni\":\"CALC_ERR: SYNX :calc — unexpected character: 'ц' in expression\"}");
    }

    @Test
    void nested_marker_sees_parent_keys_above() {
        expect("!active\na:calc 2 * 3\nb\n  x {a}\n", "{\"a\":6,\"b\":{\"x\":\"6\"}}");
    }

    @Test
    void list_items_keep_their_own_markers() {
        expect("!active\nservers\n  - name a\n    port:env:default:7001 SYNX_UNSET_Z\n  - name b\n    port[max:9] 8080\n",
            "{\"servers\":[{\"name\":\"a\",\"port\":\"CONSTRAINT_ERR: 'port' value 7001 exceeds max 9\"},"
            + "{\"name\":\"b\",\"port\":\"CONSTRAINT_ERR: 'port' value 8080 exceeds max 9\"}]}");
        SynxParseResult r = SynxParser.parse("!active\nservers\n  - name a\n    port:env:default:7001 P\n");
        assertNotNull(r.metadata.get("servers.#0"), "list item metadata not filed under servers.#0: " + r.metadata.keySet());
    }

    @Test
    void inherit_carries_markers_and_merges_parents() {
        expect("!active\n_base\n  port:env:default:5 SYNX_UNSET_Z\n  weight 1\n  nested\n    q:calc 2 * 3\n_extra\n  weight 3\n"
                + "multi:inherit:_base:_extra\n  z 1\nlist:inherit:_base\n  - 1\nscalar:inherit:_base 5\n",
            "{\"list\":[1],\"multi\":{\"nested\":{\"q\":6},\"port\":5,\"weight\":3,\"z\":1},\"scalar\":{\"nested\":{\"q\":6},\"port\":5,\"weight\":1}}");
    }

    @Test
    void null_checks_only_enum() {
        expect("!active\nport[type:int]:env SYNX_UNSET_Z\nmode[enum:a|b]:env SYNX_UNSET_Z\nopt[enum:a|]:env SYNX_UNSET_Z\nlen[min:3] null\n",
            "{\"len\":null,\"mode\":\"CONSTRAINT_ERR: 'mode' must be one of [a|b]\",\"opt\":null,\"port\":null}");
    }

    @Test
    void overwritten_and_dotted_declarations() {
        expect("!active\nport\n  port[enum:a|b] x\nport\na.b\n  x:env:default:9 SYNX_UNSET_Z\na\n  b\n    x hello\n",
            "{\"a\":{\"b\":{\"x\":\"hello\"}},\"a.b\":{\"x\":9},\"port\":{}}");
        SynxParseResult r = SynxParser.parse("!active\na.b\n  x:env Q\n");
        assertNotNull(r.metadata.get("a\\.b"), "dotted key not escaped in its metadata path: " + r.metadata.keySet());
    }

    @Test
    void values_as_text() {
        expect("!active\nn 0.0000001\nm 123456789012345678901234.0\nt v={n} m={m}\nf:format:%05d 12.0\ng:format:%d 9007199254740993\n"
                + "h:format:%.3f \"1.5\"\nj:join\n  - 1.5\n  - 2.0\n  - true\n  - null\nobj\n  k 1\nk {obj}!\n",
            "{\"f\":\"00012\",\"g\":\"9007199254740993\",\"h\":\"1.5\",\"j\":\"1.5,2,true,null\",\"k\":\"!\",\"m\":1.2345678901234569e23,"
            + "\"n\":1e-7,\"obj\":{\"k\":1},\"t\":\"v=0.0000001 m=123456789012345690000000\"}");
    }

    @Test
    void format_patterns_round_like_rust() {
        expect("!active\na:format:%.1e 125\nb:format:%.0e 25\nc:format:%.2e 0\nd:format:%.2f -0.001\ne:format:%.1e -0.0\n"
                + "f:format:%.3e 0.000123456\ng:format:%.0e 35\nh:format:%.1f 0.05\ni:format:%.20f 0.1\nk:format:%.2e 9.995\n",
            "{\"a\":\"1.2e2\",\"b\":\"2e1\",\"c\":\"0.00e0\",\"d\":\"-0.00\",\"e\":\"-0.0e0\",\"f\":\"1.235e-4\",\"g\":\"4e1\","
            + "\"h\":\"0.1\",\"i\":\"0.10000000000000000555\",\"k\":\"9.99e0\"}");
    }

    @Test
    void sort_version_and_lengths() {
        expect("!active\ns:sort\n  - b\n  - B\n  - ä\n  - a\n  - 10\n  - 9\nd:sort:desc\n  - 1\n  - 3\n  - 2\n",
            "{\"d\":[3,2,1],\"s\":[9,10,\"B\",\"a\",\"b\",\"ä\"]}");
        expect("!active\nshort[max:4] мир\nlong[max:4] мирок\nv1:version:>=:1.2.3 1.2.3-beta\n",
            "{\"long\":\"CONSTRAINT_ERR: 'long' value 5 exceeds max 4\",\"short\":\"мир\",\"v1\":true}");
    }

    @Test
    void unicode_indentation_is_measured_in_utf8() {
        expect("a\n  x 1\n　y 2\nb\n\tc 3\n d 4\n", "{\"a\":{\"x\":1,\"y\":2},\"b\":{\"c\":3,\"d\":4}}");
        // `|+` strips the base indent by whole characters: two NBSPs stay.
        expect("v |+\n  line\n    deeper\n  x\n", "{\"v\":\"line\\n  deeper\\nx\"}");
        // A line of only U+00A0 is blank, also to look-ahead.
        expect("k\n  \n  - a\n", "{\"k\":[\"a\"]}");
    }

    @Test
    void list_item_frames() {
        expect("items\n  - name a\n    sub\n      k 1\n  - name b\n    v 2\nafter 1\n",
            "{\"after\":1,\"items\":[{\"name\":\"a\",\"sub\":{\"k\":1}},{\"name\":\"b\",\"v\":2}]}");
        // An object opened between a list key and its first item is not the item's parent.
        expect("items:join\n x_1\n   y 1\n   - k v\n     z 2\n", "{\"items\":[{\"k\":\"v\",\"z\":2}],\"x_1\":{\"y\":1}}");
        // An item dropped because its list key was re-declared still pops the deeper frames
        // first, so `w` lands in `o`, not in `p`.
        expect("k\n  - x 1\n k 5\n o\n     p\n       q 1\n   - z\n    w 1\n", "{\"k\":5,\"o\":{\"p\":{\"q\":1},\"w\":1}}");
    }

    @Test
    void stringify_round_trips() {
        SynxParseResult r = SynxParser.parse("a \"007\"\nb \"true\"\ncode |+\n  if x\n    y()\nempty:join\nitems\n  - only one\n  - k v\n    sub\n      z 1\nf -0.0\n");
        ((SynxValue.Obj) r.root).map().set("c", SynxValue.ofString("x # y"));
        String text = Synx.stringify(r.root);
        assertEquals("a \"007\"\nb \"true\"\nc |+\n  x # y\ncode |+\n  if x\n    y()\nempty:join\nf -0.0\nitems\n  - only one\n  - k v\n    sub\n      z 1\n", text);
        assertEquals(Synx.toJson(r.root), Synx.toJson(SynxParser.parse(text).root));

        SynxObject item = new SynxObject();
        item.set("k", SynxValue.ofString("v"));
        SynxObject one = new SynxObject();
        one.set("items", SynxValue.ofArray(new java.util.ArrayList<>(java.util.List.of(SynxValue.ofObject(item)))));
        assertEquals("items\n  - k v\n    k v\n", Synx.stringify(one));

        SynxObject sec = new SynxObject();
        sec.set("token", SynxValue.ofSecret("abc"));
        assertEquals("token [SECRET]\n", Synx.stringify(sec));
    }

    @Test
    void format_keeps_preserve_block_and_sorts_stably() {
        String out = Synx.format("code |+\n  zeta()\n    alpha()\n  beta\nname x\n");
        assertEquals("code |+\n  zeta()\n    alpha()\n  beta\n\nname x\n", out);
        assertEquals(out, Synx.format(out));
        String stable = Synx.format("b 1\na first\nc 2\na second\n");
        assertEquals(SynxValue.ofString("second"), Synx.parse(stable).get("a"));
    }

    @Test
    void format_deep_nesting_terminates() {
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < 200; i++) sb.append(" ".repeat(i)).append("k\n");
        String out = Synx.format(sb.toString());
        assertTrue(out.startsWith("k\n  k\n    k\n"));
    }

    @Test
    void format_drops_subtree_past_depth_cap() {
        // Past depth 128 the deeper subtree is dropped and formatting goes on with the next line
        // at or above the cap — the output of `synx format` for this file.
        StringBuilder src = new StringBuilder(), want = new StringBuilder();
        for (int i = 0; i < 140; i++) src.append(" ".repeat(i)).append('k').append(i).append('\n');
        src.append("after 1\n  tail 2\nlast x\n");
        want.append("after 1\n  tail 2\n\n");
        for (int i = 0; i <= 128; i++) want.append(" ".repeat(2 * i)).append('k').append(i).append('\n');
        want.append("\nlast x\n");
        assertEquals(want.toString(), Synx.format(src.toString()));
    }

    @Test
    void truncated_ignores_bom() {
        assertFalse(SynxParser.parse(((char) 0xFEFF) + "a 1\n").truncated, "a leading BOM alone set truncated");
        assertFalse(SynxParser.parse("a 1\n").truncated);
    }

    @Test
    void clamp_bounds_must_be_finite() {
        expect("!active\na:clamp:nan:5 9\nb:clamp:0:inf 9\nc:clamp:0:Infinity 9\nd:clamp:0:1e400 9\ne:clamp:0:5abc 9\n"
                + "f:clamp:0:5 9\ng:clamp:-inf:5 9\nh:clamp:1e2:2e2 9\n",
            "{\"a\":9,\"b\":9,\"c\":9,\"d\":9,\"e\":9,\"f\":5,\"g\":9,\"h\":100}");
    }

    private static void write(java.nio.file.Path root, String name, String content) throws Exception {
        java.nio.file.Path p = root.resolve(name);
        java.nio.file.Files.createDirectories(p.getParent());
        java.nio.file.Files.writeString(p, content);
    }

    @Test
    void use_stays_inside_packages(@org.junit.jupiter.api.io.TempDir java.nio.file.Path dir) throws Exception {
        // A bad `!use` name skips the package; a bad manifest `main` counts as absent.
        write(dir, "outside/src/main.synx", "leak yes\n");
        write(dir, "outside2/x.synx", "leak2 yes\n");
        write(dir, "base/synx_packages/@m/synx-pkg.synx", "main ../../../outside2/x.synx\n");
        write(dir, "base/synx_packages/@m/src/main.synx", "fallback yes\n");
        write(dir, "base/synx_packages/@e/synx-pkg.synx", "main ../x\nentry lib/ok.synx\n");
        write(dir, "base/synx_packages/@e/lib/ok.synx", "entry yes\n");
        write(dir, "base/synx_packages/@ok/src/main.synx", "plain yes\n");
        SynxOptions o = new SynxOptions();
        o.env = new HashMap<>();
        o.basePath = dir.resolve("base").toString();
        String src = "!active\n!use @a/../../../outside as p1\n!use @m as p2\n!use @e as p3\n!use @c:x as p4\n!use @ok as p5\napp ok\n";
        assertEquals("{\"app\":\"ok\",\"p2\":{\"fallback\":\"yes\"},\"p3\":{\"entry\":\"yes\"},\"p5\":{\"plain\":\"yes\"}}",
            Synx.toJson(Synx.parseFullActive(src, o).root));
        for (String bad : new String[] { "/abs", "\\abs", "C:x", "a/../b", "a\\..\\b", "" }) {
            assertFalse(SynxEngine.isContainedRelative(bad), bad);
        }
        for (String good : new String[] { "@scope/name", "src/main.synx", "a/..b/c", "./x" }) {
            assertTrue(SynxEngine.isContainedRelative(good), good);
        }
    }

    @Test
    void self_include_resolves_once_per_level(@org.junit.jupiter.api.io.TempDir java.nio.file.Path dir) throws Exception {
        // Stops at the first repeat of a file already being resolved, and each level is resolved
        // once: the `:include` marker reuses the pre-pass result.
        write(dir, "self.synx", "!active\nme:include self.synx\nn:tick 1\n");
        int[] calls = { 0 };
        SynxOptions o = new SynxOptions();
        o.env = new HashMap<>();
        o.basePath = dir.toString();
        o.markerFns.put("tick", (key, args, v) -> { calls[0]++; return v; });
        String got = Synx.toJson(Synx.parseFullActive(java.nio.file.Files.readString(dir.resolve("self.synx")), o).root);
        String want = "{\"me\":{\"me\":\"INCLUDE_ERR: circular include: 'self.synx'\",\"n\":1},\"n\":1}";
        assertEquals(want, got);
        assertEquals(2, calls[0], "one resolution per level");
    }

    @Test
    void single_include_key_serves_include_scope(@org.junit.jupiter.api.io.TempDir java.nio.file.Path dir) throws Exception {
        // The pre-pass results are not include aliases: with one `:include` key, `{x:include}`
        // still names that include.
        write(dir, "common.synx", "site_name Demo\n");
        SynxOptions o = new SynxOptions();
        o.env = new HashMap<>();
        o.basePath = dir.toString();
        assertEquals("{\"db\":{\"site_name\":\"Demo\"},\"greet\":\"Demo / Demo\"}",
            Synx.toJson(Synx.parseFullActive("!active\ndb:include common.synx\ngreet {site_name:include} / {site_name:db}\n", o).root));
    }

    @Test
    void synxb_is_deterministic_and_ordered_by_use() throws Exception {
        String src = "!active\nz rare\nlist\n  - common\n  - common\n  - common\nbig 9007199254740993\nneg -9000000000000000007\nport:env:default:1 P\n";
        byte[] first = SynxBinary.compile(SynxParser.parse(src), false).value;
        for (int i = 0; i < 5; i++) assertArrayEquals(first, SynxBinary.compile(SynxParser.parse(src), false).value);
        Inflater inf = new Inflater(true);
        inf.setInput(first, 11, first.length - 11);
        ByteArrayOutputStream payload = new ByteArrayOutputStream();
        byte[] buf = new byte[4096];
        while (!inf.finished()) payload.write(buf, 0, inf.inflate(buf));
        byte[] p = payload.toByteArray();
        // String table: count, then the most used string ("common", 3 uses) first.
        assertEquals(6, p[1]);
        assertEquals("common", new String(p, 2, 6, java.nio.charset.StandardCharsets.UTF_8));
        var back = SynxBinary.decompile(first);
        assertTrue(back.ok, back.error);
        assertEquals(Synx.toJson(SynxParser.parse(src).root), Synx.toJson(back.value.root));
        assertTrue(Synx.toJson(back.value.root).contains("9007199254740993"));
        assertEquals(java.util.List.of("env", "default", "1"), back.value.metadata.get("").get("port").markers);
    }

    @Test
    void block_comment_hides_directives() {
        assertEquals(SynxMode.STATIC, SynxParser.parse("###\n!active\n###\nx:calc 1 + 1\n").mode);
    }

    @Test
    void secret_and_alias() {
        expect("!active\ntoken:secret abc123\nmsg hello {token}\npick:alias token\na:alias b\nb:alias a\n",
            "{\"a\":\"ALIAS_ERR: circular alias detected: a → b\",\"b\":\"ALIAS_ERR: circular alias detected: a → b\","
            + "\"msg\":\"hello [SECRET]\",\"pick\":\"[SECRET]\",\"token\":\"[SECRET]\"}");
    }

    @Test
    void json_keys_sort_by_code_point() {
        SynxObject o = new SynxObject();
        o.set("～", SynxValue.ofInt(1));        // U+FF5E, BMP
        o.set("😀", SynxValue.ofInt(2));  // U+1F600, a surrogate pair in UTF-16
        assertEquals("{\"～\":1,\"😀\":2}", Synx.toJson(o));
    }

    @Test
    void number_text_follows_rust() {
        assertEquals("1e-7", SynxJson.formatFloat(1e-7));
        assertEquals("1.2345678901234569e23", SynxJson.formatFloat(1.2345678901234568e23));
        assertEquals("5e-324", SynxJson.formatFloat(Double.MIN_VALUE));
        assertEquals("0.0000001", SynxNumbers.display(1e-7));
        assertEquals("1000000000000000000000", SynxNumbers.display(1e21));
        assertEquals("12", SynxNumbers.formatNumber(12.0));
        assertNull(SynxNumbers.parseF64("1.5d"));
        assertNull(SynxNumbers.parseF64(" 1"));
        assertNull(SynxNumbers.parseF64("0x10"));
        assertEquals(Double.POSITIVE_INFINITY, SynxNumbers.parseF64("INF"));
        assertNull(SynxNumbers.parseI64("١٢"));
    }
}
