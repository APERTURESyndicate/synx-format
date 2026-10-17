package com.aperturesyndicate.synx;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/**
 * SYNX text writer that parses back to the same data: the SYNXL §14.1 block writer of
 * {@code crates/synx-core} ({@code synxl::write_synx_entry}). Strings that would read back as
 * another type or be cut as a comment are quoted or carried as {@code |+}, multi-line text keeps
 * its relative indent, a list item leads with a key that fits its {@code - } line, and secrets are
 * written as {@code [SECRET]}.
 */
public final class SynxStringify {

    private SynxStringify() {}

    public static final int MAX_DEPTH = 128;

    public static String stringify(SynxValue v) {
        StringBuilder sb = new StringBuilder(2048);
        serialize(sb, v, 0);
        return sb.toString();
    }

    private static void serialize(StringBuilder out, SynxValue v, int depth) {
        if (depth > MAX_DEPTH) {
            out.append("[synx:max-depth]\n");
            return;
        }
        if (!(v instanceof SynxValue.Obj o)) {
            out.append(formatPrimitive(v));
            return;
        }
        for (var e : sortedEntries(o.map())) writeEntry(out, e.getKey(), e.getValue(), depth * 2, depth);
    }

    /** The object's entries in key order (code point order). */
    static List<Map.Entry<String, SynxValue>> sortedEntries(SynxObject o) {
        return o.sortedEntries();
    }

    /** One value as Rust's {@code format_primitive} renders it. */
    public static String formatPrimitive(SynxValue v) {
        if (v instanceof SynxValue.Str s) return s.value();
        if (v instanceof SynxValue.Int i) return Long.toString(i.value());
        if (v instanceof SynxValue.Float f) {
            String s = SynxNumbers.display(f.value());
            return s.indexOf('.') < 0 ? s + ".0" : s;
        }
        if (v instanceof SynxValue.Bool b) return b.value() ? "true" : "false";
        if (v instanceof SynxValue.Null) return "null";
        if (v instanceof SynxValue.Arr a) {
            List<String> parts = new ArrayList<>(a.values().size());
            for (SynxValue item : a.values()) parts.add(formatPrimitive(item));
            return "[" + String.join(", ", parts) + "]";
        }
        if (v instanceof SynxValue.Obj) return "[Object]";
        return "[SECRET]";
    }

    // ─── §14.1 block writer ─────────────────────────────────────────────────

    private static final String SPACES = " ".repeat(256);

    private static void indent(StringBuilder out, int n) {
        while (n > SPACES.length()) {
            out.append(SPACES);
            n -= SPACES.length();
        }
        out.append(SPACES, 0, n);
    }

    private static void writeEntry(StringBuilder out, String key, SynxValue v, int ind, int depth) {
        if (depth > SynxParser.MAX_NESTING_DEPTH) return;
        if (v instanceof SynxValue.Obj o) {
            indent(out, ind);
            out.append(key).append('\n');
            for (var e : sortedEntries(o.map())) writeEntry(out, e.getKey(), e.getValue(), ind + 2, depth + 1);
            return;
        }
        if (v instanceof SynxValue.Arr a) {
            indent(out, ind);
            out.append(key);
            // A bare key reads back as an empty object; `:join` yields an empty list.
            if (a.values().isEmpty()) out.append(":join");
            out.append('\n');
            for (SynxValue item : a.values()) writeItem(out, item, ind + 2, depth + 1);
            return;
        }
        if (v instanceof SynxValue.Int i) {
            // perf: written in place, no intermediate String.
            indent(out, ind);
            out.append(key).append(' ').append(i.value()).append('\n');
            return;
        }
        if (v instanceof SynxValue.Str s && s.value().indexOf('\n') >= 0) {
            writeMultiline(out, key, s.value(), ind);
            return;
        }
        String text = scalar(v);
        if (text != null) {
            indent(out, ind);
            out.append(key).append(' ').append(text).append('\n');
            return;
        }
        // Not representable as one token: an indent-preserving block, never comment-stripped nor cast.
        writeMultiline(out, key, v instanceof SynxValue.Str s ? s.value() : "", ind);
    }

    private static void writeItem(StringBuilder out, SynxValue item, int ind, int depth) {
        if (depth > SynxParser.MAX_NESTING_DEPTH) return;
        if (!(item instanceof SynxValue.Obj o)) {
            indent(out, ind);
            out.append("- ");
            String text = scalar(item);
            if (text != null) out.append(text);
            else if (item instanceof SynxValue.Str s) out.append(s.value());
            out.append('\n');
            return;
        }
        List<Map.Entry<String, SynxValue>> entries = sortedEntries(o.map());
        // An empty object in a list is a bare dash.
        if (entries.isEmpty()) {
            indent(out, ind);
            out.append("-\n");
            return;
        }
        // The `- ` line has to carry a key whose value fits on it; SYNX attaches everything deeper
        // to the item, not to that key.
        int head = -1;
        for (int i = 0; i < entries.size(); i++) {
            if (dashLineSafe(entries.get(i).getValue())) { head = i; break; }
        }
        // An item is an object only when a deeper line follows its `- ` line: a one-key item, or
        // one whose head could not take its value, writes the head again in full below.
        boolean repeatHead = head < 0 || entries.size() == 1;
        if (head < 0) head = 0;
        indent(out, ind);
        out.append("- ").append(entries.get(head).getKey());
        String headText = scalar(entries.get(head).getValue());
        if (headText != null && !headText.isEmpty()) out.append(' ').append(headText);
        out.append('\n');
        for (int i = 0; i < entries.size(); i++) {
            if (i == head && !repeatHead) continue;
            writeEntry(out, entries.get(i).getKey(), entries.get(i).getValue(), ind + 2, depth + 1);
        }
    }

    /** May this value share the {@code - } line of a list item? */
    private static boolean dashLineSafe(SynxValue v) {
        if (v instanceof SynxValue.Obj o) return o.map().isEmpty();
        if (v instanceof SynxValue.Arr) return false;
        if (v instanceof SynxValue.Str s) {
            return s.value().indexOf('\n') < 0 && !s.value().isEmpty() && scalar(v) != null;
        }
        return true;
    }

    private static void writeMultiline(StringBuilder out, String key, String body, int ind) {
        indent(out, ind);
        out.append(key).append(" |+\n");
        int from = 0;
        while (true) {
            int nl = body.indexOf('\n', from);
            indent(out, ind + 2);
            out.append(body, from, nl < 0 ? body.length() : nl).append('\n');
            if (nl < 0) break;
            from = nl + 1;
        }
    }

    /**
     * A scalar as one SYNX value token, or {@code null} when no token can carry it. A value is
     * comment-stripped at {@code  #} / {@code  //} before casting, so a string containing either
     * cannot be rescued by quoting.
     */
    static String scalar(SynxValue v) {
        if (v instanceof SynxValue.Null) return "null";
        if (v instanceof SynxValue.Bool b) return b.value() ? "true" : "false";
        if (v instanceof SynxValue.Int i) return Long.toString(i.value());
        if (v instanceof SynxValue.Float f) {
            double d = f.value();
            if (Double.isNaN(d) || Double.isInfinite(d)) return "null";
            String s = SynxNumbers.display(d);
            return s.indexOf('.') < 0 ? s + ".0" : s;
        }
        if (v instanceof SynxValue.Secret) return "[SECRET]";
        if (v instanceof SynxValue.Str st) {
            String s = st.value();
            if (breaksOrComments(s)) return null;
            if (!needsQuote(s)) return s;
            if (s.indexOf('"') < 0) return '"' + s + '"';
            if (s.indexOf('\'') < 0) return '\'' + s + '\'';
            return null;
        }
        return null;
    }

    /** Does {@code s} hold a line break, {@code  #} or {@code  //}? One pass. */
    private static boolean breaksOrComments(String s) {
        int n = s.length();
        for (int i = 0; i < n; i++) {
            char c = s.charAt(i);
            if (c == '\n') return true;
            if (c == ' ' && i + 1 < n) {
                char d = s.charAt(i + 1);
                if (d == '#' || (d == '/' && i + 2 < n && s.charAt(i + 2) == '/')) return true;
            }
        }
        return false;
    }

    /** Would this string be misread as a bare SYNX value? */
    private static boolean needsQuote(String s) {
        return s.isEmpty() || !s.equals(SynxParser.trim(s)) || s.equals("|") || s.equals("|+")
            || isQuoteWrapped(s) || !autoCastKeepsString(s);
    }

    /** Does the automatic cast give back the string itself? */
    private static boolean autoCastKeepsString(String s) {
        if (isQuoteWrapped(s)) return true;
        if (s.equals("true") || s.equals("false") || s.equals("null")) return false;
        int start = !s.isEmpty() && s.charAt(0) == '-' ? 1 : 0;
        if (s.length() <= start || !SynxNumbers.isDigit(s.charAt(start))) return true;
        int dot = -1;
        for (int j = start; j < s.length(); j++) {
            char c = s.charAt(j);
            if (c == '.') {
                if (dot >= 0) return true;
                dot = j;
            } else if (!SynxNumbers.isDigit(c)) {
                return true;
            }
        }
        if (dot < 0) return SynxNumbers.parseI64(s) == null;
        if (dot > start && dot < s.length() - 1) return SynxNumbers.parseF64(s) == null;
        return true;
    }

    private static boolean isQuoteWrapped(String s) {
        return s.length() >= 2 && ((s.charAt(0) == '"' && s.charAt(s.length() - 1) == '"')
            || (s.charAt(0) == '\'' && s.charAt(s.length() - 1) == '\''));
    }
}
