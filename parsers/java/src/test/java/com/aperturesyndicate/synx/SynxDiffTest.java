package com.aperturesyndicate.synx;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class SynxDiffTest {

    @Test
    void identical() {
        SynxObject a = new SynxObject();
        a.set("x", SynxValue.ofInt(1));
        a.set("y", SynxValue.ofInt(2));
        SynxObject b = new SynxObject();
        b.set("x", SynxValue.ofInt(1));
        b.set("y", SynxValue.ofInt(2));
        SynxDiff.Result d = SynxDiff.diff(a, b);
        assertTrue(d.added.isEmpty());
        assertTrue(d.removed.isEmpty());
        assertTrue(d.changed.isEmpty());
        assertEquals(2, d.unchanged.size());
    }

    @Test
    void added_removed() {
        SynxObject a = new SynxObject(); a.set("x", SynxValue.ofInt(1));
        SynxObject b = new SynxObject(); b.set("y", SynxValue.ofInt(2));
        SynxDiff.Result d = SynxDiff.diff(a, b);
        assertEquals(1, d.added.size());
        assertEquals(1, d.removed.size());
    }

    @Test
    void changed() {
        SynxObject a = new SynxObject(); a.set("name", SynxValue.ofString("Alice"));
        SynxObject b = new SynxObject(); b.set("name", SynxValue.ofString("Bob"));
        SynxDiff.Result d = SynxDiff.diff(a, b);
        assertEquals(1, d.changed.size());
        assertEquals("name", d.changed.get(0).key());
    }
}
