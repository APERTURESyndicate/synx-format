package com.aperturesyndicate.synx;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;

/** Value → SYNX text. Mirrors {@code serialize} in {@code crates/synx-core/src/lib.rs}. */
public final class SynxStringify {

    private SynxStringify() {}
    public static final int MAX_DEPTH = 128;

    public static String stringify(SynxValue v) {
        StringBuilder sb = new StringBuilder(2048);
        serialize(v, 0, sb);
        return sb.toString();
    }

    private static void serialize(SynxValue v, int depth, StringBuilder out) {
        if (depth > MAX_DEPTH) { out.append("[synx:max-depth]\n"); return; }
        if (!(v instanceof SynxValue.Obj obj)) {
            out.append(formatPrimitive(v));
            return;
        }
        SynxObject map = obj.map();
        String indent = " ".repeat(depth * 2);
        List<String> keys = new ArrayList<>(map.keys());
        Collections.sort(keys);
        for (String key : keys) {
            SynxValue val = map.get(key);
            if (val instanceof SynxValue.Arr a) {
                out.append(indent).append(key).append('\n');
                for (SynxValue item : a.values()) {
                    if (item instanceof SynxValue.Obj inner) {
                        var innerEntries = inner.map().entriesList();
                        if (!innerEntries.isEmpty()) {
                            var first = innerEntries.get(0);
                            out.append(indent).append("  - ").append(first.getKey())
                               .append(' ').append(formatPrimitive(first.getValue())).append('\n');
                            for (int j = 1; j < innerEntries.size(); j++) {
                                var e = innerEntries.get(j);
                                out.append(indent).append("    ").append(e.getKey())
                                   .append(' ').append(formatPrimitive(e.getValue())).append('\n');
                            }
                        }
                    } else {
                        out.append(indent).append("  - ").append(formatPrimitive(item)).append('\n');
                    }
                }
            } else if (val instanceof SynxValue.Obj) {
                out.append(indent).append(key).append('\n');
                serialize(val, depth + 1, out);
            } else if (val instanceof SynxValue.Str s && s.value().indexOf('\n') >= 0) {
                out.append(indent).append(key).append(" |\n");
                for (String line : s.value().split("\n", -1)) {
                    out.append(indent).append("  ").append(line).append('\n');
                }
            } else {
                out.append(indent).append(key).append(' ').append(formatPrimitive(val)).append('\n');
            }
        }
    }

    public static String formatPrimitive(SynxValue v) {
        if (v instanceof SynxValue.Str s) return s.value();
        if (v instanceof SynxValue.Int i) return Long.toString(i.value());
        if (v instanceof SynxValue.Float f) {
            double d = f.value();
            if (Double.isNaN(d) || Double.isInfinite(d)) return "null";
            String s = String.format(java.util.Locale.ROOT, "%.17g", d);
            if (s.indexOf('.') < 0 && s.indexOf('e') < 0 && s.indexOf('E') < 0) s += ".0";
            return s;
        }
        if (v instanceof SynxValue.Bool b) return b.value() ? "true" : "false";
        if (v instanceof SynxValue.Null) return "null";
        if (v instanceof SynxValue.Arr a) {
            List<String> parts = new ArrayList<>();
            for (SynxValue x : a.values()) parts.add(formatPrimitive(x));
            return "[" + String.join(", ", parts) + "]";
        }
        if (v instanceof SynxValue.Obj) return "[Object]";
        if (v instanceof SynxValue.Secret) return "[SECRET]";
        return "";
    }
}
