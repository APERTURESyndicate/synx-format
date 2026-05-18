package com.aperturesyndicate.synx;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class SynxCalcTest {

    @Test
    void basic_ops() {
        assertEquals(5.0, SynxCalc.evaluate("2 + 3").value);
        assertEquals(6.0, SynxCalc.evaluate("10 - 4").value);
        assertEquals(21.0, SynxCalc.evaluate("3 * 7").value);
        assertEquals(5.0, SynxCalc.evaluate("20 / 4").value);
        assertEquals(1.0, SynxCalc.evaluate("10 % 3").value);
    }

    @Test
    void precedence_and_parens() {
        assertEquals(14.0, SynxCalc.evaluate("2 + 3 * 4").value);
        assertEquals(20.0, SynxCalc.evaluate("(2 + 3) * 4").value);
    }

    @Test
    void negatives() {
        assertEquals(-2.0, SynxCalc.evaluate("-5 + 3").value);
        assertEquals(-20.0, SynxCalc.evaluate("10 * -2").value);
    }

    @Test
    void div_zero() {
        SynxCalc.Result r = SynxCalc.evaluate("10 / 0");
        assertFalse(r.ok);
        assertFalse(r.error.isEmpty());
    }

    @Test
    void empty() {
        assertTrue(SynxCalc.evaluate("").ok);
        assertEquals(0.0, SynxCalc.evaluate("").value);
    }
}
