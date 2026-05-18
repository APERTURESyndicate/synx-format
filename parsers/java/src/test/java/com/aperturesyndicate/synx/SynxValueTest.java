package com.aperturesyndicate.synx;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class SynxValueTest {

    @Test
    void object_set_get_remove() {
        SynxObject o = new SynxObject();
        o.set("a", SynxValue.ofInt(1));
        o.set("b", SynxValue.ofString("two"));
        assertEquals(SynxValue.ofInt(1), o.get("a"));
        assertTrue(o.contains("b"));
        assertTrue(o.remove("a"));
        assertFalse(o.contains("a"));
    }

    @Test
    void value_equality_order_insensitive() {
        SynxObject a = new SynxObject();
        a.set("x", SynxValue.ofInt(1));
        a.set("y", SynxValue.ofInt(2));
        SynxObject b = new SynxObject();
        b.set("y", SynxValue.ofInt(2));
        b.set("x", SynxValue.ofInt(1));
        assertEquals(SynxValue.ofObject(a), SynxValue.ofObject(b));
    }

    @Test
    void type_helpers() {
        assertTrue(SynxValue.ofNull().isNull());
        assertEquals(5L, (long) SynxValue.ofInt(5).asInt());
        assertEquals(3.14, SynxValue.ofFloat(3.14).asDouble());
        assertNull(SynxValue.ofString("x").asInt());
    }
}
