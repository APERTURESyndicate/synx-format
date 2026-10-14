package com.aperturesyndicate.synx;

import java.util.HashMap;
import java.util.Map;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

/** SYNX 3.7.3/3.7.4 engine parity: {@code [type:string]} env text, exact i64, secret masking, BOM. */
class EnvTypeStringTest {

    private static SynxObject resolveEnv(String text) {
        SynxOptions opts = new SynxOptions();
        opts.env = new HashMap<>(Map.of("ZIP", "007", "SNOW", "1550184245153112174", "Q", "\"12\""));
        return Synx.parseActive(text, opts);
    }

    @Test
    void env_type_string_constraint_keeps_text() {
        SynxObject o = resolveEnv(
            "!active\nid[type:string]:env SNOW\nzip[type:string]:env ZIP\nfallback[type:string]:env:default:0042 MISSING\nplain:env ZIP\n");
        assertEquals(SynxValue.ofString("1550184245153112174"), o.get("id"));
        assertEquals(SynxValue.ofString("007"), o.get("zip"));
        assertEquals(SynxValue.ofString("0042"), o.get("fallback"));
        assertEquals(SynxValue.ofInt(7), o.get("plain"));
    }

    @Test
    void type_string_constraint_accepts_secret() {
        SynxObject o = resolveEnv("!active\ntoken[type:string]:secret:env SNOW\n");
        assertEquals(SynxValue.ofSecret("1550184245153112174"), o.get("token"));
    }

    @Test
    void type_string_hint_keeps_secret() {
        SynxObject o = resolveEnv("!active\ntoken(string):secret:env SNOW\n");
        assertEquals(SynxValue.ofSecret("1550184245153112174"), o.get("token"));
    }

    @Test
    void standalone_default_honours_type_string_constraint() {
        SynxObject o = Synx.parseActive("!active\ncode[type:string]:default:007 null\nnum:default:007 null\n");
        assertEquals(SynxValue.ofString("007"), o.get("code"));
        assertEquals(SynxValue.ofInt(7), o.get("num"));
    }

    @Test
    void env_cast_keeps_64bit_integers_exact() {
        SynxObject o = resolveEnv("!active\nraw:env SNOW\n");
        assertEquals(SynxValue.ofInt(1550184245153112174L), o.get("raw"));
    }

    @Test
    void env_reference_example_matches_rust() {
        SynxObject o = resolveEnv(
            "!active\nz:env ZIP\nzs[type:string]:env ZIP\nid[type:string]:env SNOW\nraw:env SNOW\n"
            + "d[type:string]:env:default:007 MISSING\nsec[type:string]:secret:env SNOW\nq:env Q\n");
        assertEquals(
            "{\"d\":\"007\",\"id\":\"1550184245153112174\",\"q\":\"12\",\"raw\":1550184245153112174,"
            + "\"sec\":\"[SECRET]\",\"z\":7,\"zs\":\"007\"}",
            Synx.toJson(o));
    }

    @Test
    void interpolation_of_secret_is_masked() {
        SynxObject o = Synx.parseActive("!active\ntoken:secret abc\nmsg hello {token} world\n");
        assertEquals(SynxValue.ofString("hello [SECRET] world"), o.get("msg"));
        assertFalse(Synx.toJson(o).contains("abc"));
    }

    @Test
    void nested_interpolation_does_not_observe_root_secret() {
        SynxObject o = Synx.parseActive("!active\ntoken:secret abc123\nsection\n  msg use {token} now\n");
        SynxObject section = ((SynxValue.Obj) o.get("section")).map();
        assertEquals(SynxValue.ofString("use [SECRET] now"), section.get("msg"));
        assertFalse(Synx.toJson(o).contains("abc123"));
    }

    @Test
    void interpolation_keeps_non_ascii_literal_text() {
        SynxObject o = Synx.parseActive("!active\nname Привет\nmsg мир {name} 東京\n");
        assertEquals(SynxValue.ofString("мир Привет 東京"), o.get("msg"));
    }

    @Test
    void interpolation_prints_floats_like_rust() {
        SynxObject o = Synx.parseActive("!active\nratio 3.14\nmsg r={ratio}\n");
        assertEquals(SynxValue.ofString("r=3.14"), o.get("msg"));
    }

    @Test
    void leading_bom_does_not_disable_active_mode() {
        SynxObject o = Synx.parseActive("﻿!active\nsum:calc 2 + 3\n");
        assertEquals(SynxValue.ofInt(5), o.get("sum"));
    }

    @Test
    void leading_bom_is_not_glued_to_first_key() {
        SynxObject o = Synx.parse("﻿name Wario\n");
        assertNotNull(o.get("name"));
        assertNull(o.get("﻿name"));
    }

    @Test
    void json_floats_use_shortest_round_trip_digits() {
        assertEquals("{\"a\":1e20,\"b\":1e-6,\"c\":0.00001,\"d\":1234567890123456.5,"
            + "\"e\":1.2345678901234568e16,\"f\":3.0,\"g\":0.1,\"h\":1000000.0}",
            Synx.toJson(Synx.parse(
                "a 100000000000000000000.5\nb 0.000001\nc 0.00001\nd 1234567890123456.5\n"
                + "e 12345678901234567.5\nf 3.0\ng 0.1\nh 1000000.0\n")));
    }
}
