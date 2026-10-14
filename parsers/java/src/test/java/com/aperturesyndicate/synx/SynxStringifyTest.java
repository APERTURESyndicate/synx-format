package com.aperturesyndicate.synx;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class SynxStringifyTest {

    @Test
    void basic_roundtrip() {
        SynxParseResult r = SynxParser.parse("active true\nage 30\nname Wario\n");
        String out = SynxStringify.stringify(r.root);
        assertTrue(out.contains("name Wario"));
        assertTrue(out.contains("age 30"));
        assertTrue(out.contains("active true"));
    }

    @Test
    void multiline_uses_pipe() {
        SynxObject v = new SynxObject();
        v.set("rules", SynxValue.ofString("a\nb\nc"));
        String out = SynxStringify.stringify(SynxValue.ofObject(v));
        assertTrue(out.contains("rules |"));
    }

    @Test
    void formatter_sorts_keys() {
        String out = SynxFormatter.format("b 2\na 1\nc 3\n");
        int a = out.indexOf("a 1");
        int b = out.indexOf("b 2");
        assertTrue(a >= 0 && b >= 0);
        assertTrue(a < b);
    }

    @Test
    void formatter_preserves_directive() {
        String out = SynxFormatter.format("!active\nname X\n");
        assertTrue(out.startsWith("!active"));
    }

    @Test
    void stringify_prints_floats_like_rust() {
        SynxObject o = new SynxObject();
        o.set("a", SynxValue.ofFloat(99.5));
        o.set("b", SynxValue.ofFloat(3.0));
        o.set("c", SynxValue.ofFloat(0.1));
        o.set("d", SynxValue.ofFloat(1e20));
        o.set("e", SynxValue.ofFloat(0.0000001));
        String out = SynxStringify.stringify(SynxValue.ofObject(o));
        java.util.List<String> lines = java.util.Arrays.asList(out.split("\n"));
        assertTrue(lines.contains("a 99.5"), out);
        assertTrue(lines.contains("b 3.0"), out);
        assertTrue(lines.contains("c 0.1"), out);
        assertTrue(lines.contains("d 100000000000000000000.0"), out);
        assertTrue(lines.contains("e 0.0000001"), out);
    }

    @Test
    void stringify_float_roundtrips_through_parse() {
        SynxObject back = Synx.parse(Synx.stringify(Synx.parse("a 99.5\nb 3.0\n")));
        assertEquals(SynxValue.ofFloat(99.5), back.get("a"));
        assertEquals(SynxValue.ofFloat(3.0), back.get("b"));
    }
}
