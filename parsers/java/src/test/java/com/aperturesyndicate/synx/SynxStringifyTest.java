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
}
