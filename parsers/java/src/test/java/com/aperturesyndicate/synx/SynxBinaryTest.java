package com.aperturesyndicate.synx;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class SynxBinaryTest {

    @Test
    void static_roundtrip() {
        SynxParseResult r = SynxParser.parse("name App\nport 8080\n");
        var compiled = SynxBinary.compile(r, false);
        assertTrue(compiled.ok, compiled.error);
        assertTrue(SynxBinary.isSynxb(compiled.value));
        var restored = SynxBinary.decompile(compiled.value);
        assertTrue(restored.ok, restored.error);
        assertEquals(r.root, restored.value.root);
    }

    @Test
    void magic_check() {
        assertTrue(SynxBinary.isSynxb(new byte[] { 'S','Y','N','X','B', 1, 0 }));
        assertFalse(SynxBinary.isSynxb(new byte[] { 'J','S','O','N' }));
    }

    @Test
    void invalid_magic_rejected() {
        byte[] bad = new byte[11];
        bad[0] = 'W'; bad[1] = 'R'; bad[2] = 'O'; bad[3] = 'N'; bad[4] = 'G';
        var r = SynxBinary.decompile(bad);
        assertFalse(r.ok);
    }
}
