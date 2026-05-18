package com.aperturesyndicate.synx;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;

/** Canonical JSON encoder. Sorted keys, secrets redacted, floats with mandatory decimal. */
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
            if (Double.isNaN(d) || Double.isInfinite(d)) { out.append("null"); return; }
            String s = String.format(java.util.Locale.ROOT, "%.17g", d);
            if (s.indexOf('.') < 0 && s.indexOf('e') < 0 && s.indexOf('E') < 0) s += ".0";
            out.append(s);
        } else if (v instanceof SynxValue.Str s) {
            out.append('"'); escape(out, s.value()); out.append('"');
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
            SynxObject map = o.map();
            List<String> keys = new ArrayList<>(map.keys());
            Collections.sort(keys);
            boolean first = true;
            for (String key : keys) {
                if (!first) out.append(',');
                first = false;
                out.append('"'); escape(out, key); out.append("\":");
                SynxValue cv = map.get(key);
                write(out, cv == null ? SynxValue.ofNull() : cv, depth + 1);
            }
            out.append('}');
        }
    }

    private static void escape(StringBuilder out, String s) {
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"':  out.append("\\\""); break;
                case '\\': out.append("\\\\"); break;
                case '\n': out.append("\\n");  break;
                case '\r': out.append("\\r");  break;
                case '\t': out.append("\\t");  break;
                default:
                    if (c < 0x20) {
                        out.append(String.format("\\u%04x", (int) c));
                    } else {
                        out.append(c);
                    }
            }
        }
    }
}
