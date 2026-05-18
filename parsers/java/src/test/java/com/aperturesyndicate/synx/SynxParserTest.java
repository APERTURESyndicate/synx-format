package com.aperturesyndicate.synx;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class SynxParserTest {

    @Test
    void parse_simple_kv() {
        SynxParseResult r = SynxParser.parse("name Wario\nage 30\nactive true\nscore 99.5\nempty null");
        SynxObject o = ((SynxValue.Obj) r.root).map();
        assertEquals(SynxValue.ofString("Wario"), o.get("name"));
        assertEquals(SynxValue.ofInt(30), o.get("age"));
        assertEquals(SynxValue.ofBool(true), o.get("active"));
        assertEquals(SynxValue.ofFloat(99.5), o.get("score"));
        assertTrue(o.get("empty").isNull());
        assertEquals(SynxMode.STATIC, r.mode);
    }

    @Test
    void parse_nested_objects() {
        SynxParseResult r = SynxParser.parse("server\n  host 0.0.0.0\n  port 8080\n  ssl\n    enabled true");
        SynxObject o = ((SynxValue.Obj) r.root).map();
        SynxObject server = ((SynxValue.Obj) o.get("server")).map();
        assertEquals(SynxValue.ofInt(8080), server.get("port"));
        SynxObject ssl = ((SynxValue.Obj) server.get("ssl")).map();
        assertEquals(SynxValue.ofBool(true), ssl.get("enabled"));
    }

    @Test
    void parse_lists() {
        SynxParseResult r = SynxParser.parse("inventory\n  - Sword\n  - Shield\n  - Potion");
        SynxObject o = ((SynxValue.Obj) r.root).map();
        SynxValue.Arr arr = (SynxValue.Arr) o.get("inventory");
        assertEquals(3, arr.values().size());
    }

    @Test
    void parse_multiline_block() {
        SynxParseResult r = SynxParser.parse("rules |\n  Rule one.\n  Rule two.\n  Rule three.");
        SynxObject o = ((SynxValue.Obj) r.root).map();
        String s = ((SynxValue.Str) o.get("rules")).value();
        assertTrue(s.contains("\n"));
    }

    @Test
    void parse_active_metadata() {
        SynxParseResult r = SynxParser.parse("!active\nprice 100\ntax:calc price * 0.2");
        assertEquals(SynxMode.ACTIVE, r.mode);
        assertNotNull(r.metadata.get(""));
        assertEquals(java.util.List.of("calc"), r.metadata.get("").get("tax").markers);
    }

    @Test
    void parse_prototype_pollution_rejected() {
        SynxParseResult r = SynxParser.parse("__proto__ evil\nconstructor evil\nprototype evil\nname safe\n");
        SynxObject o = ((SynxValue.Obj) r.root).map();
        assertFalse(o.contains("__proto__"));
        assertFalse(o.contains("constructor"));
        assertFalse(o.contains("prototype"));
        assertTrue(o.contains("name"));
    }

    @Test
    void parse_constraints() {
        SynxParseResult r = SynxParser.parse("!active\nname[min:3, max:30, required] Wario");
        SynxConstraints c = r.metadata.get("").get("name").constraints;
        assertEquals(3.0, c.min);
        assertEquals(30.0, c.max);
        assertTrue(c.required);
    }

    @Test
    void parse_type_hint_string_keeps_string() {
        SynxParseResult r = SynxParser.parse("zip(string) 90210");
        SynxObject o = ((SynxValue.Obj) r.root).map();
        assertEquals(SynxValue.ofString("90210"), o.get("zip"));
    }

    @Test
    void parse_tool_directive() {
        SynxParseResult r = SynxParser.parse("!tool\nweb_search\n  query test\n  lang ru\n");
        assertTrue(r.tool);
        SynxValue shaped = SynxParser.reshapeToolOutput(r.root, false);
        SynxObject m = ((SynxValue.Obj) shaped).map();
        assertEquals(SynxValue.ofString("web_search"), m.get("tool"));
    }
}
