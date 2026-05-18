package com.aperturesyndicate.synx;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class SynxJsonTest {

    @Test
    void primitives() {
        assertEquals("null",  SynxJson.encode(SynxValue.ofNull()));
        assertEquals("true",  SynxJson.encode(SynxValue.ofBool(true)));
        assertEquals("42",    SynxJson.encode(SynxValue.ofInt(42)));
        assertEquals("\"hi\"", SynxJson.encode(SynxValue.ofString("hi")));
    }

    @Test
    void secret_redacted() {
        assertEquals("\"[SECRET]\"", SynxJson.encode(SynxValue.ofSecret("xxx")));
    }

    @Test
    void object_sorted_keys() {
        SynxObject o = new SynxObject();
        o.set("b", SynxValue.ofInt(2));
        o.set("a", SynxValue.ofInt(1));
        String j = SynxJson.encode(SynxValue.ofObject(o));
        int a = j.indexOf("\"a\"");
        int b = j.indexOf("\"b\"");
        assertTrue(a >= 0 && b >= 0);
        assertTrue(a < b);
    }

    @Test
    void escapes() {
        String j = SynxJson.encode(SynxValue.ofString("line\nbreak\ttab\"quote\\back"));
        assertTrue(j.contains("\\n"));
        assertTrue(j.contains("\\t"));
        assertTrue(j.contains("\\\""));
        assertTrue(j.contains("\\\\"));
    }
}
