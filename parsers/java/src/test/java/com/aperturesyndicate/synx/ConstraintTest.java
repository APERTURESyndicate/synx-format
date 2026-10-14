package com.aperturesyndicate.synx;

import java.util.HashMap;
import java.util.Map;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

/** Constraint failures land in the value exactly as Rust writes them (SYNX 3.7.4). */
class ConstraintTest {

    private static String json(String text) {
        return Synx.toJson(Synx.parseActive(text));
    }

    @Test
    void redos_shaped_pattern_is_refused_quickly_and_leaves_value_unchanged() {
        String value = "a".repeat(45) + "!";
        long t0 = System.nanoTime();
        SynxObject o = Synx.parseActive("!active\nv[pattern:^(a+)+$] " + value + "\n");
        long ms = (System.nanoTime() - t0) / 1_000_000;
        assertEquals(SynxValue.ofString(value), o.get("v"));
        assertTrue(ms < 500, "took " + ms + " ms");
    }

    @Test
    void flat_patterns_still_validate() {
        assertEquals(
            "{\"a\":\"abc\",\"b\":\"CONSTRAINT_ERR: 'b' does not match pattern /^[a-z]+$/\"}",
            json("!active\na[pattern:^[a-z]+$] abc\nb[pattern:^[a-z]+$] ABC\n"));
    }

    @Test
    void invalid_and_overlong_patterns_are_skipped_silently() {
        String longPattern = "a".repeat(300);
        assertEquals("{\"p\":\"abc\",\"q\":\"abc\"}",
            json("!active\np[pattern:(] abc\nq[pattern:" + longPattern + "] abc\n"));
    }

    @Test
    void length_counts_unicode_scalar_values() {
        assertEquals(
            "{\"cyr\":\"мир\",\"emoji\":\"😀😀\",\"long\":\"CONSTRAINT_ERR: 'long' value 5 exceeds max 4\"}",
            json("!active\ncyr[max:4] мир\nemoji[max:2] 😀😀\nlong[max:4] мирок\n"));
    }

    @Test
    void numbers_print_like_rust_display() {
        assertEquals(
            "{\"a\":\"CONSTRAINT_ERR: 'a' value 0.25 is below min 0.5\",\"b\":\"CONSTRAINT_ERR: 'b' value 70000 exceeds max 65535\","
            + "\"c\":\"CONSTRAINT_ERR: 'c' value 123456789012345690000000 exceeds max 5\"}",
            json("!active\na[min:0.5] 0.25\nb[max:65535] 70000\nc[max:5] 123456789012345678901234.5\n"));
    }

    @Test
    void required_checks_null_and_empty() {
        assertEquals(
            "{\"blank\":\"CONSTRAINT_ERR: 'blank' is required\",\"missing\":\"CONSTRAINT_ERR: 'missing' is required\",\"title\":\"Hello\"}",
            json("!active\ntitle[required] Hello\nmissing[required] null\nblank[required] \"\"\n"));
    }

    @Test
    void field_registry_merges_declarations_across_the_document() {
        assertEquals(
            "{\"a\":{\"port\":8080},\"b\":{\"port\":\"CONSTRAINT_ERR: 'port' value 70000 exceeds max 65535\"},"
            + "\"c\":{\"port\":\"CONSTRAINT_ERR: 'port' expected type 'int'\"}}",
            json("!active\na\n  port[type:int, min:1, max:65535] 8080\nb\n  port 70000\nc\n  port abc\n"));
    }

    @Test
    void per_key_error_is_not_revalidated_by_the_registry_pass() {
        assertEquals("{\"x\":\"CONSTRAINT_ERR: 'x' value 0 is below min 1\"}", json("!active\nx[min:1] 0\n"));
    }

    @Test
    void constraints_run_after_all_markers_of_the_key() {
        assertEquals(
            "{\"base\":7,\"total\":\"CONSTRAINT_ERR: 'total' value 14 exceeds max 10\"}",
            json("!active\nbase 7\ntotal[max:10]:calc base * 2\n"));
    }

    @Test
    void type_registry_reports_type_err() {
        assertEquals(
            "{\"boss\":{\"hp\":\"TYPE_ERR: 'hp' expected int but got string\"},\"hp\":100}",
            json("!active\nhp(int) 100\nboss\n  hp lots\n"));
    }

    @Test
    void env_nan_and_inf_parse_as_floats_like_rust() {
        SynxOptions opts = new SynxOptions();
        Map<String, String> env = new HashMap<>();
        env.put("A", "nan");
        env.put("B", "inf");
        env.put("C", "-Infinity");
        env.put("D", "+NaN");
        env.put("E", "1e400");
        env.put("F", "infx");
        opts.env = env;
        SynxObject o = Synx.parseActive(
            "!active\na:env A\nb:env B\nc:env C\nd:env D\ne:env E\nf:env F\n", opts);
        assertTrue(o.get("a").isFloat() && Double.isNaN(o.get("a").asFloat()));
        assertEquals(Double.POSITIVE_INFINITY, o.get("b").asFloat());
        assertEquals(Double.NEGATIVE_INFINITY, o.get("c").asFloat());
        assertTrue(Double.isNaN(o.get("d").asFloat()));
        assertEquals(Double.POSITIVE_INFINITY, o.get("e").asFloat());
        assertEquals(SynxValue.ofString("infx"), o.get("f"));
        assertEquals("{\"a\":null,\"b\":null,\"c\":null,\"d\":null,\"e\":null,\"f\":\"infx\"}", Synx.toJson(o));
    }
}
