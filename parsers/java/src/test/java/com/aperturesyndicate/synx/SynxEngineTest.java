package com.aperturesyndicate.synx;

import java.util.HashMap;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class SynxEngineTest {

    @Test
    void env_default() {
        SynxOptions opts = new SynxOptions();
        opts.env = new HashMap<>(); opts.env.put("APP_PORT", "9090");
        SynxParseResult r = SynxParser.parse("!active\nport:env:default:3000 APP_PORT\n");
        SynxEngine.resolve(r, opts);
        SynxObject o = ((SynxValue.Obj) r.root).map();
        assertEquals(SynxValue.ofString("9090"), o.get("port"));
    }

    @Test
    void env_falls_back_to_default() {
        SynxOptions opts = new SynxOptions();
        opts.env = new HashMap<>();
        SynxParseResult r = SynxParser.parse("!active\nport:env:default:3000 NOT_SET\n");
        SynxEngine.resolve(r, opts);
        SynxObject o = ((SynxValue.Obj) r.root).map();
        assertEquals(SynxValue.ofString("3000"), o.get("port"));
    }

    @Test
    void calc_basic() {
        SynxParseResult r = SynxParser.parse("!active\nprice 100\ntax:calc price * 0.2\n");
        SynxEngine.resolve(r, new SynxOptions());
        SynxObject o = ((SynxValue.Obj) r.root).map();
        Double d = o.get("tax").asDouble();
        assertNotNull(d);
        assertEquals(20.0, d, 0.01);
    }

    @Test
    void secret_redacted_in_json() {
        SynxParseResult r = SynxParser.parse("!active\ntoken:secret abc123\n");
        SynxEngine.resolve(r, new SynxOptions());
        String json = SynxJson.encode(r.root);
        assertTrue(json.contains("[SECRET]"));
        assertFalse(json.contains("abc123"));
    }

    @Test
    void clamp() {
        SynxParseResult r = SynxParser.parse("!active\nx:clamp:0:10 99\n");
        SynxEngine.resolve(r, new SynxOptions());
        SynxObject o = ((SynxValue.Obj) r.root).map();
        assertEquals(10.0, o.get("x").asDouble());
    }

    @Test
    void format_padded() {
        SynxParseResult r = SynxParser.parse("!active\nnum:format:%05d 42\n");
        SynxEngine.resolve(r, new SynxOptions());
        SynxObject o = ((SynxValue.Obj) r.root).map();
        assertEquals(SynxValue.ofString("00042"), o.get("num"));
    }
}
