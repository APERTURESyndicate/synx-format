package com.aperturesyndicate.synx;

/** SYNX top-level facade. Mirrors the Rust {@code Synx} struct in {@code synx-core/src/lib.rs}. */
public final class Synx {

    private Synx() {}

    /** Parse SYNX text and return the top-level object (static mode only). */
    public static SynxObject parse(String text) {
        SynxParseResult r = SynxParser.parse(text);
        if (r.root instanceof SynxValue.Obj o) return o.map();
        return new SynxObject();
    }

    /** Parse and resolve {@code !active} markers. */
    public static SynxObject parseActive(String text, SynxOptions opts) {
        SynxParseResult r = SynxParser.parse(text);
        if (r.mode == SynxMode.ACTIVE) SynxEngine.resolve(r, opts == null ? new SynxOptions() : opts);
        if (r.root instanceof SynxValue.Obj o) return o.map();
        return new SynxObject();
    }

    public static SynxObject parseActive(String text) { return parseActive(text, new SynxOptions()); }

    /** Parse and return the full ParseResult. */
    public static SynxParseResult parseFull(String text) { return SynxParser.parse(text); }

    /** Parse, resolve, return full ParseResult. */
    public static SynxParseResult parseFullActive(String text, SynxOptions opts) {
        SynxParseResult r = SynxParser.parse(text);
        if (r.mode == SynxMode.ACTIVE) SynxEngine.resolve(r, opts == null ? new SynxOptions() : opts);
        return r;
    }

    /** Parse a {@code !tool} envelope into {@code { tool, params }} or {@code { tools: [...] }}. */
    public static SynxObject parseTool(String text, SynxOptions opts) {
        SynxParseResult r = SynxParser.parse(text);
        if (r.mode == SynxMode.ACTIVE) SynxEngine.resolve(r, opts == null ? new SynxOptions() : opts);
        SynxValue shaped = SynxParser.reshapeToolOutput(r.root, r.schema);
        if (shaped instanceof SynxValue.Obj o) return o.map();
        return new SynxObject();
    }

    public static SynxObject parseTool(String text) { return parseTool(text, new SynxOptions()); }

    public static String toJson(SynxValue value)   { return SynxJson.encode(value); }
    public static String toJson(SynxObject object) { return SynxJson.encode(SynxValue.ofObject(object)); }

    public static String stringify(SynxValue value)   { return SynxStringify.stringify(value); }
    public static String stringify(SynxObject object) { return SynxStringify.stringify(SynxValue.ofObject(object)); }

    public static String format(String text) { return SynxFormatter.format(text); }

    public static SynxBinary.Outcome<byte[]> compile(String text, boolean resolved) {
        SynxParseResult r = SynxParser.parse(text);
        if (resolved && r.mode == SynxMode.ACTIVE) SynxEngine.resolve(r, new SynxOptions());
        return SynxBinary.compile(r, resolved);
    }

    public static SynxBinary.Outcome<String> decompile(byte[] bytes) {
        var res = SynxBinary.decompile(bytes);
        if (!res.ok) return SynxBinary.Outcome.failure(res.error);
        SynxParseResult pr = res.value;
        StringBuilder out = new StringBuilder();
        if (pr.tool)            out.append("!tool\n");
        if (pr.schema)          out.append("!schema\n");
        if (pr.llm)             out.append("!llm\n");
        if (pr.mode == SynxMode.ACTIVE) out.append("!active\n");
        if (pr.locked)          out.append("!lock\n");
        if (out.length() > 0)   out.append('\n');
        out.append(SynxStringify.stringify(pr.root));
        return SynxBinary.Outcome.success(out.toString());
    }

    public static boolean isSynxb(byte[] data)         { return SynxBinary.isSynxb(data); }

    public static SynxDiff.Result diff(SynxObject a, SynxObject b) { return SynxDiff.diff(a, b); }

    public static SynxValue diffToValue(SynxDiff.Result d) { return SynxDiff.toValue(d); }
}
