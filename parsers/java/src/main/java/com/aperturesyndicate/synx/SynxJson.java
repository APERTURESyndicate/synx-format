package com.aperturesyndicate.synx;

import java.util.List;
import java.util.Map;

/**
 * Canonical JSON encoder, byte-identical to {@code synx_core::to_json}: object keys sorted by code
 * point at every level, secrets redacted, floats in the layout of Rust's {@code ryu}.
 */
public final class SynxJson {

    private SynxJson() {}

    public static final int MAX_DEPTH = 128;

    public static String encode(SynxValue v) {
        StringBuilder sb = new StringBuilder(2048);
        write(sb, v, 0);
        return sb.toString();
    }

    private static void write(StringBuilder out, SynxValue v, int depth) {
        if (depth > MAX_DEPTH) { out.append("null"); return; }
        if (v instanceof SynxValue.Null) {
            out.append("null");
        } else if (v instanceof SynxValue.Bool b) {
            out.append(b.value() ? "true" : "false");
        } else if (v instanceof SynxValue.Int i) {
            out.append(i.value());
        } else if (v instanceof SynxValue.Float f) {
            double d = f.value();
            if (Double.isNaN(d) || Double.isInfinite(d)) out.append("null");
            else out.append(formatFloat(d));
        } else if (v instanceof SynxValue.Str s) {
            writeString(out, s.value());
        } else if (v instanceof SynxValue.Secret) {
            out.append("\"[SECRET]\"");
        } else if (v instanceof SynxValue.Arr a) {
            out.append('[');
            List<SynxValue> arr = a.values();
            for (int i = 0; i < arr.size(); i++) {
                if (i > 0) out.append(',');
                write(out, arr.get(i), depth + 1);
            }
            out.append(']');
        } else if (v instanceof SynxValue.Obj o) {
            out.append('{');
            boolean first = true;
            for (Map.Entry<String, SynxValue> e : SynxStringify.sortedEntries(o.map())) {
                if (!first) out.append(',');
                first = false;
                writeString(out, e.getKey());
                out.append(':');
                write(out, e.getValue() == null ? SynxValue.ofNull() : e.getValue(), depth + 1);
            }
            out.append('}');
        } else {
            out.append("null");
        }
    }

    /** Shortest round-trip digits in the layout of Rust's {@code ryu::Buffer::format}. */
    static String formatFloat(double d) {
        return SynxNumbers.ryu(d);
    }

    /** Rust f64 {@code Display} with {@code .0} on a whole number (the stringify scalar form). */
    static String formatPlain(double d) {
        String s = SynxNumbers.display(d);
        return s.indexOf('.') < 0 ? s + ".0" : s;
    }

    /** A JSON string literal; unescaped runs are copied whole. */
    static void writeString(StringBuilder out, String s) {
        out.append('"');
        int start = 0;
        int n = s.length();
        for (int i = 0; i < n; i++) {
            char c = s.charAt(i);
            if (c >= 0x20 && c != '"' && c != '\\') continue;
            out.append(s, start, i);
            switch (c) {
                case '"':  out.append("\\\""); break;
                case '\\': out.append("\\\\"); break;
                case '\n': out.append("\\n");  break;
                case '\r': out.append("\\r");  break;
                case '\t': out.append("\\t");  break;
                default:
                    out.append("\\u00").append(HEX[c >> 4]).append(HEX[c & 0xF]);
            }
            start = i + 1;
        }
        out.append(s, start, n);
        out.append('"');
    }

    private static final char[] HEX = "0123456789abcdef".toCharArray();
}
