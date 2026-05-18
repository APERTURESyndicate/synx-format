package com.aperturesyndicate.synx;

import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/** SYNX text-to-tree parser. Parity with {@code crates/synx-core/src/parser.rs}. */
public final class SynxParser {

    private SynxParser() {}

    // ─── Resource caps (fuzz / hostile input) ────────────────────────────────
    public static final int MAX_INPUT_BYTES      = 16 * 1024 * 1024;
    public static final int MAX_LINE_STARTS      = 2_000_000;
    public static final int MAX_NESTING_DEPTH    = 128;
    public static final int MAX_MULTILINE_BYTES  = 1024 * 1024;
    public static final int MAX_LIST_ITEMS       = 1 << 20;
    public static final int MAX_INCLUDES         = 4096;
    public static final int MAX_ENUM_PARTS       = 4096;
    public static final int MAX_MARKER_SEGMENTS  = 512;

    /** Truncate to a UTF-8-safe prefix (used by parse and the canonical formatter). */
    public static String clampText(String text) {
        byte[] bytes = text.getBytes(StandardCharsets.UTF_8);
        if (bytes.length <= MAX_INPUT_BYTES) return text;
        int end = MAX_INPUT_BYTES;
        while (end > 0 && (bytes[end] & 0xC0) == 0x80) end--;
        return new String(bytes, 0, end, StandardCharsets.UTF_8);
    }

    public static SynxParseResult parse(String rawText) {
        byte[] bytes = clampText(rawText).getBytes(StandardCharsets.UTF_8);

        // Bound number of indexed newlines.
        int maxNl = MAX_LINE_STARTS - 1;
        int seen = 0;
        int limit = bytes.length;
        for (int scan = 0; scan < limit; scan++) {
            if (bytes[scan] == '\n') {
                if (seen >= maxNl) { limit = scan; break; }
                seen++;
            }
        }
        if (limit != bytes.length) {
            byte[] cut = new byte[limit];
            System.arraycopy(bytes, 0, cut, 0, limit);
            bytes = cut;
        }

        // Index line starts.
        List<Integer> lineStarts = new ArrayList<>(64);
        lineStarts.add(0);
        for (int scan = 0; scan < bytes.length; scan++) {
            if (bytes[scan] == '\n') lineStarts.add(scan + 1);
        }
        int lineCount = lineStarts.size();

        SynxParseResult result = new SynxParseResult();
        SynxObject rootObj = new SynxObject();
        List<StackFrame> stack = new ArrayList<>();
        stack.add(new StackFrame(-1, StackEntry.Root.INSTANCE));

        BlockState block = null;
        ListState list = null;
        boolean inBlockComment = false;

        int i = 0;
        while (i < lineCount) {
            byte[] raw = lineBytes(bytes, lineStarts, i);
            String rawStr = new String(raw, StandardCharsets.UTF_8);
            String t = trim(rawStr);

            // Directives
            switch (t) {
                case "!active": result.mode = SynxMode.ACTIVE; i++; continue;
                case "!lock":   result.locked = true;          i++; continue;
                case "!tool":   result.tool = true;            i++; continue;
                case "!schema": result.schema = true;          i++; continue;
                case "!llm":    result.llm = true;             i++; continue;
                default: break;
            }
            if (t.startsWith("!include ")) {
                if (result.includes.size() < MAX_INCLUDES) {
                    String rest = trim(t.substring(9));
                    String path = rest;
                    String alias = "";
                    int ws = firstWs(rest);
                    if (ws >= 0) {
                        path = rest.substring(0, ws);
                        alias = trim(rest.substring(ws));
                    }
                    if (alias.isEmpty()) {
                        String base = path;
                        int slash = Math.max(base.lastIndexOf('/'), base.lastIndexOf('\\'));
                        if (slash >= 0) base = base.substring(slash + 1);
                        if (base.endsWith(".synx") || base.endsWith(".SYNX")) {
                            base = base.substring(0, base.length() - 5);
                        }
                        alias = base;
                    }
                    result.includes.add(new SynxIncludeDirective(path, alias));
                }
                i++; continue;
            }
            if (t.startsWith("!use ")) {
                String rest = trim(t.substring(5));
                if (!rest.isEmpty() && rest.charAt(0) == '@') {
                    String pkg = rest;
                    String alias = "";
                    int as = rest.indexOf(" as ");
                    if (as >= 0) {
                        pkg = trim(rest.substring(0, as));
                        alias = trim(rest.substring(as + 4));
                    }
                    if (alias.isEmpty()) {
                        int slash = pkg.lastIndexOf('/');
                        alias = slash >= 0 ? pkg.substring(slash + 1) : pkg;
                    }
                    if (!pkg.isEmpty()) {
                        result.uses.add(new SynxUseDirective(pkg, alias));
                    }
                }
                i++; continue;
            }
            if (t.startsWith("#!mode:")) {
                String declared = trim(t.substring(7));
                result.mode = "active".equals(declared) ? SynxMode.ACTIVE : SynxMode.STATIC;
                i++; continue;
            }

            if (t.equals("###")) { inBlockComment = !inBlockComment; i++; continue; }
            if (inBlockComment) { i++; continue; }
            if (t.isEmpty() || t.startsWith("#") || t.startsWith("//")) { i++; continue; }

            int indent = indentOf(raw);

            // Continue multiline block
            if (block != null) {
                if (indent > block.indent) {
                    if (block.content.length() < MAX_MULTILINE_BYTES) {
                        if (block.content.length() > 0) block.content.append('\n');
                        int room = MAX_MULTILINE_BYTES - block.content.length();
                        int n = Math.min(t.length(), room);
                        block.content.append(t, 0, n);
                    }
                    i++; continue;
                }
                insertValue(rootObj, stack, block.stackIdx, block.key,
                        SynxValue.ofString(block.content.toString()));
                block = null;
            }

            // Continue list items
            if (t.startsWith("- ")) {
                if (list != null && indent > list.indent) {
                    while (stack.size() > 1) {
                        var back = stack.get(stack.size() - 1);
                        if (back.entry() instanceof StackEntry.ListItem && back.indent() >= indent) {
                            stack.remove(stack.size() - 1);
                        } else break;
                    }
                    String valStr = stripComment(trim(t.substring(2)));

                    // Peek for nested object form
                    boolean nested = false;
                    int peek = i + 1;
                    while (peek < lineCount) {
                        byte[] pl = lineBytes(bytes, lineStarts, peek);
                        String pt = trim(new String(pl, StandardCharsets.UTF_8));
                        if (pt.isEmpty()) { peek++; continue; }
                        int pi = indentOf(pl);
                        if (pi > indent && !pt.startsWith("- ") && !pt.startsWith("#") && !pt.startsWith("//")) {
                            nested = true;
                        }
                        break;
                    }

                    final String listKey = list.key;
                    final int listStackIdx = list.stackIdx;
                    final boolean nestedF = nested;
                    final String valStrF = valStr;
                    final int indentF = indent;
                    int[] newIdx = { -1 };
                    mutateArray(rootObj, stack, listStackIdx, listKey, arr -> {
                        if (arr.size() >= MAX_LIST_ITEMS) return;
                        if (nestedF) {
                            SynxObject itemObj = new SynxObject();
                            ParsedLine parsed = parseLine(valStrF);
                            if (parsed != null) {
                                SynxValue v;
                                if (parsed.typeHint != null) {
                                    v = castTyped(parsed.value, parsed.typeHint);
                                } else if (parsed.value.isEmpty()) {
                                    v = SynxValue.ofObject();
                                } else {
                                    v = castValue(parsed.value);
                                }
                                itemObj.set(parsed.key, v);
                            } else {
                                itemObj.set("_value", castValue(valStrF));
                            }
                            newIdx[0] = arr.size();
                            arr.add(SynxValue.ofObject(itemObj));
                        } else {
                            arr.add(castValue(valStrF));
                        }
                    });
                    if (newIdx[0] >= 0 && stack.size() < MAX_NESTING_DEPTH) {
                        stack.add(new StackFrame(indentF, new StackEntry.ListItem(listKey, newIdx[0])));
                    }
                    i++; continue;
                }
            } else if (list != null && indent <= list.indent) {
                list = null;
                while (stack.size() > 1) {
                    var back = stack.get(stack.size() - 1);
                    if (back.entry() instanceof StackEntry.ListItem && back.indent() >= indent) {
                        stack.remove(stack.size() - 1);
                    } else break;
                }
            }

            // Key line
            ParsedLine parsed = parseLine(t);
            if (parsed == null) { i++; continue; }
            if ("__proto__".equals(parsed.key) || "constructor".equals(parsed.key) || "prototype".equals(parsed.key)) {
                i++; continue;
            }
            while (stack.size() > 1 && stack.get(stack.size() - 1).indent() >= indent) {
                stack.remove(stack.size() - 1);
            }
            int parentIdx = stack.size() - 1;

            if (result.mode == SynxMode.ACTIVE
                && (!parsed.markers.isEmpty() || parsed.constraints != null || parsed.typeHint != null)) {
                String path = buildPath(stack);
                SynxMeta meta = new SynxMeta();
                meta.markers = parsed.markers;
                meta.args = parsed.markerArgs;
                meta.typeHint = parsed.typeHint;
                meta.constraints = parsed.constraints;
                result.metadata.computeIfAbsent(path, k -> new HashMap<>()).put(parsed.key, meta);
            }

            boolean isBlock = parsed.value.equals("|");
            boolean isListMarker = false;
            for (String m : parsed.markers) {
                if (m.equals("random") || m.equals("unique") || m.equals("geo") || m.equals("join")) {
                    isListMarker = true; break;
                }
            }

            if (isBlock) {
                insertValue(rootObj, stack, parentIdx, parsed.key, SynxValue.ofString(""));
                block = new BlockState(indent, parsed.key, new StringBuilder(), parentIdx);
            } else if (isListMarker && parsed.value.isEmpty()) {
                insertValue(rootObj, stack, parentIdx, parsed.key, SynxValue.ofArray());
                list = new ListState(indent, parsed.key, parentIdx);
            } else if (parsed.value.isEmpty()) {
                int peek = i + 1;
                boolean becameList = false;
                while (peek < lineCount) {
                    byte[] pl = lineBytes(bytes, lineStarts, peek);
                    String pt = trim(new String(pl, StandardCharsets.UTF_8));
                    if (!pt.isEmpty()) {
                        if (pt.startsWith("- ")) {
                            insertValue(rootObj, stack, parentIdx, parsed.key, SynxValue.ofArray());
                            list = new ListState(indent, parsed.key, parentIdx);
                            becameList = true;
                        }
                        break;
                    }
                    peek++;
                }
                if (!becameList) {
                    insertValue(rootObj, stack, parentIdx, parsed.key, SynxValue.ofObject());
                    if (stack.size() < MAX_NESTING_DEPTH) {
                        stack.add(new StackFrame(indent, new StackEntry.Key(parsed.key)));
                    }
                }
            } else {
                SynxValue v = parsed.typeHint != null
                    ? castTyped(parsed.value, parsed.typeHint)
                    : castValue(parsed.value);
                insertValue(rootObj, stack, parentIdx, parsed.key, v);
            }
            i++;
        }

        if (block != null) {
            insertValue(rootObj, stack, block.stackIdx, block.key,
                    SynxValue.ofString(block.content.toString()));
        }

        result.root = SynxValue.ofObject(rootObj);
        return result;
    }

    // ─── !tool reshape ──────────────────────────────────────────────────────
    public static SynxValue reshapeToolOutput(SynxValue root, boolean schema) {
        if (!(root instanceof SynxValue.Obj obj)) return root;
        SynxObject map = obj.map();

        if (schema) {
            List<String> keys = new ArrayList<>(map.keys());
            java.util.Collections.sort(keys);
            List<SynxValue> tools = new ArrayList<>();
            for (String k : keys) {
                SynxObject def = new SynxObject();
                def.set("name", SynxValue.ofString(k));
                SynxValue v = map.get(k);
                def.set("params", v != null ? v : SynxValue.ofNull());
                tools.add(SynxValue.ofObject(def));
            }
            SynxObject out = new SynxObject();
            out.set("tools", SynxValue.ofArray(tools));
            return SynxValue.ofObject(out);
        }
        if (map.isEmpty()) {
            SynxObject out = new SynxObject();
            out.set("tool", SynxValue.ofNull());
            out.set("params", SynxValue.ofObject());
            return SynxValue.ofObject(out);
        }
        List<String> keys = new ArrayList<>(map.keys());
        java.util.Collections.sort(keys);
        String firstKey = keys.get(0);
        SynxValue firstVal = map.get(firstKey);
        SynxValue params = firstVal instanceof SynxValue.Obj ? firstVal : SynxValue.ofObject();
        SynxObject out = new SynxObject();
        out.set("tool", SynxValue.ofString(firstKey));
        out.set("params", params);
        return SynxValue.ofObject(out);
    }

    // ─── private helpers ────────────────────────────────────────────────────

    private static byte[] lineBytes(byte[] bytes, List<Integer> starts, int idx) {
        int s = starts.get(idx);
        int e = (idx + 1 < starts.size()) ? starts.get(idx + 1) - 1 : bytes.length;
        if (e > s && bytes[e - 1] == '\r') e--;
        byte[] out = new byte[e - s];
        System.arraycopy(bytes, s, out, 0, e - s);
        return out;
    }

    private static int indentOf(byte[] line) {
        int i = 0;
        while (i < line.length && (line[i] == ' ' || line[i] == '\t')) i++;
        return i;
    }

    static String trim(String s) {
        int a = 0;
        while (a < s.length() && (s.charAt(a) == ' ' || s.charAt(a) == '\t' || s.charAt(a) == '\r')) a++;
        int b = s.length();
        while (b > a && (s.charAt(b - 1) == ' ' || s.charAt(b - 1) == '\t' || s.charAt(b - 1) == '\r')) b--;
        return s.substring(a, b);
    }

    private static int firstWs(String s) {
        for (int i = 0; i < s.length(); i++) {
            if (s.charAt(i) == ' ' || s.charAt(i) == '\t') return i;
        }
        return -1;
    }

    private static String stripComment(String val) {
        String r = val;
        int p = r.indexOf(" //");
        if (p >= 0) r = r.substring(0, p);
        p = r.indexOf(" #");
        if (p >= 0) r = r.substring(0, p);
        int end = r.length();
        while (end > 0 && (r.charAt(end - 1) == ' ' || r.charAt(end - 1) == '\t' || r.charAt(end - 1) == '\r')) end--;
        return r.substring(0, end);
    }

    private static SynxValue castValue(String val) {
        if (val.length() >= 2) {
            char f = val.charAt(0), l = val.charAt(val.length() - 1);
            if ((f == '"' && l == '"') || (f == '\'' && l == '\'')) {
                return SynxValue.ofString(val.substring(1, val.length() - 1));
            }
        }
        switch (val) {
            case "true":  return SynxValue.ofBool(true);
            case "false": return SynxValue.ofBool(false);
            case "null":  return SynxValue.ofNull();
            default: break;
        }
        if (val.isEmpty()) return SynxValue.ofString("");

        int start = 0;
        if (val.charAt(0) == '-') {
            if (val.length() == 1) return SynxValue.ofString(val);
            start = 1;
        }
        char c0 = val.charAt(start);
        if (c0 < '0' || c0 > '9') return SynxValue.ofString(val);

        boolean seenDot = false;
        int dotPos = -1;
        boolean allNumeric = true;
        for (int j = start; j < val.length(); j++) {
            char c = val.charAt(j);
            if (c == '.') {
                if (seenDot) { allNumeric = false; break; }
                seenDot = true;
                dotPos = j;
            } else if (c < '0' || c > '9') {
                allNumeric = false;
                break;
            }
        }
        if (!allNumeric) return SynxValue.ofString(val);
        if (seenDot) {
            if (dotPos > start && dotPos < val.length() - 1) {
                try { return SynxValue.ofFloat(Double.parseDouble(val)); }
                catch (NumberFormatException ignored) {}
            }
            return SynxValue.ofString(val);
        }
        try { return SynxValue.ofInt(Long.parseLong(val)); }
        catch (NumberFormatException e) { return SynxValue.ofString(val); }
    }

    private static SynxValue castTyped(String val, String hint) {
        switch (hint) {
            case "int":
                try { return SynxValue.ofInt(Long.parseLong(val)); }
                catch (NumberFormatException e) { return SynxValue.ofInt(0); }
            case "float":
                try { return SynxValue.ofFloat(Double.parseDouble(val)); }
                catch (NumberFormatException e) { return SynxValue.ofFloat(0.0); }
            case "bool":
                return SynxValue.ofBool(trim(val).equals("true"));
            case "string":
                return SynxValue.ofString(val);
            case "random":
            case "random:int":
                // Match Rust `rng::random_i64()` — full signed 64-bit range including negatives.
                // ThreadLocalRandom.nextLong() (no args) returns the full Long range.
                return SynxValue.ofInt(java.util.concurrent.ThreadLocalRandom.current().nextLong());
            case "random:float":
                return SynxValue.ofFloat(java.util.concurrent.ThreadLocalRandom.current().nextDouble());
            case "random:bool":
                return SynxValue.ofBool(java.util.concurrent.ThreadLocalRandom.current().nextBoolean());
            default:
                return castValue(val);
        }
    }

    static ParsedLine parseLine(String trimmed) {
        if (trimmed.isEmpty()) return null;
        char first = trimmed.charAt(0);
        if (first == '#' || trimmed.startsWith("//") || trimmed.startsWith("- ")) return null;
        if (first == '[' || first == ':' || first == '-' || first == '/' || first == '(') return null;

        int len = trimmed.length();
        int pos = 0;
        while (pos < len) {
            char ch = trimmed.charAt(pos);
            if (ch == ' ' || ch == '\t' || ch == '[' || ch == ':' || ch == '(') break;
            pos++;
        }
        ParsedLine out = new ParsedLine();
        out.key = trimmed.substring(0, pos);

        if (pos < len && trimmed.charAt(pos) == '(') {
            int start = pos + 1;
            int scan = start;
            while (scan < len && trimmed.charAt(scan) != ')') scan++;
            if (scan < len) {
                out.typeHint = trimmed.substring(start, scan);
                pos = scan + 1;
            } else {
                pos = start;
            }
        }

        if (pos < len && trimmed.charAt(pos) == '[') {
            int cstart = pos + 1;
            int depth = 1;
            int scan = cstart;
            while (scan < len && depth > 0) {
                char b = trimmed.charAt(scan);
                if (b == '[') depth++;
                else if (b == ']') {
                    depth--;
                    if (depth == 0) break;
                }
                scan++;
            }
            if (depth == 0) {
                out.constraints = parseConstraints(trimmed.substring(cstart, scan));
                pos = scan + 1;
            } else {
                int sweep = cstart;
                while (sweep < len && trimmed.charAt(sweep) != ']') sweep++;
                if (sweep < len) {
                    out.constraints = parseConstraints(trimmed.substring(cstart, sweep));
                    pos = sweep + 1;
                } else {
                    out.constraints = parseConstraints(trimmed.substring(cstart));
                    pos = len;
                }
            }
        }

        if (pos < len && trimmed.charAt(pos) == ':') {
            int mstart = pos + 1;
            int mend = mstart;
            while (mend < len && trimmed.charAt(mend) != ' ' && trimmed.charAt(mend) != '\t') mend++;
            String chain = trimmed.substring(mstart, mend);
            int segs = 0;
            int p = 0;
            while (p <= chain.length() && segs < MAX_MARKER_SEGMENTS) {
                int colon = chain.indexOf(':', p);
                String seg = chain.substring(p, colon < 0 ? chain.length() : colon);
                out.markers.add(seg);
                segs++;
                if (colon < 0) break;
                p = colon + 1;
            }
            pos = mend;
        }

        while (pos < len && (trimmed.charAt(pos) == ' ' || trimmed.charAt(pos) == '\t')) pos++;

        out.value = (pos < len) ? stripComment(trimmed.substring(pos)) : "";

        if (out.markers.contains("random") && !out.value.isEmpty()) {
            List<String> nums = new ArrayList<>();
            for (String tok : out.value.split("[ \\t]+")) {
                if (tok.isEmpty()) continue;
                try { Double.parseDouble(tok); nums.add(tok); }
                catch (NumberFormatException ignored) {}
            }
            if (!nums.isEmpty()) {
                out.markerArgs = nums;
                out.value = "";
            }
        }
        if (out.markers.contains("inherit") && !out.value.isEmpty()) {
            out.markerArgs = new ArrayList<>();
            out.markerArgs.add(trim(out.value));
            out.value = "";
        }
        return out;
    }

    static SynxConstraints parseConstraints(String raw) {
        SynxConstraints c = new SynxConstraints();
        for (String rawPart : raw.split(",")) {
            String part = trim(rawPart);
            if (part.isEmpty()) continue;
            if (part.equals("required")) { c.required = true; continue; }
            if (part.equals("readonly")) { c.readonly = true; continue; }
            int colon = part.indexOf(':');
            if (colon < 0) continue;
            String k = trim(part.substring(0, colon));
            String v = trim(part.substring(colon + 1));
            switch (k) {
                case "min":
                    try { c.min = Double.parseDouble(v); } catch (NumberFormatException ignored) {}
                    break;
                case "max":
                    try { c.max = Double.parseDouble(v); } catch (NumberFormatException ignored) {}
                    break;
                case "type":    c.typeName = v; break;
                case "pattern": c.pattern = v;  break;
                case "enum":
                    List<String> vals = new ArrayList<>();
                    int count = 0;
                    for (String piece : v.split("\\|", -1)) {
                        if (count >= MAX_ENUM_PARTS) break;
                        vals.add(piece);
                        count++;
                    }
                    c.enumValues = vals;
                    break;
                default: break;
            }
        }
        return c;
    }

    // ─── tree helpers ───────────────────────────────────────────────────────

    static String buildPath(List<StackFrame> stack) {
        StringBuilder sb = new StringBuilder();
        boolean first = true;
        for (int i = 1; i < stack.size(); i++) {
            var e = stack.get(i).entry();
            if (e instanceof StackEntry.Key k) {
                if (!first) sb.append('.');
                sb.append(k.name());
                first = false;
            }
        }
        return sb.toString();
    }

    static void insertValue(SynxObject root, List<StackFrame> stack, int parentIdx,
                            String key, SynxValue value) {
        if (parentIdx == 0) { root.set(key, value); return; }
        List<StackEntry> path = new ArrayList<>(parentIdx);
        for (int i = 1; i <= parentIdx; i++) path.add(stack.get(i).entry());
        setValue(root, path, 0, key, value);
    }

    private static void setValue(SynxObject obj, List<StackEntry> path, int idx,
                                 String key, SynxValue value) {
        if (idx >= path.size()) { obj.set(key, value); return; }
        StackEntry head = path.get(idx);
        if (head instanceof StackEntry.Key kk) {
            SynxValue v = obj.get(kk.name());
            if (!(v instanceof SynxValue.Obj childObj)) return;
            setValue(childObj.map(), path, idx + 1, key, value);
        } else if (head instanceof StackEntry.ListItem li) {
            SynxValue v = obj.get(li.listKey());
            if (!(v instanceof SynxValue.Arr a)) return;
            if (li.itemIdx() >= a.values().size()) return;
            SynxValue item = a.values().get(li.itemIdx());
            if (!(item instanceof SynxValue.Obj io)) return;
            setValue(io.map(), path, idx + 1, key, value);
        }
    }

    static void mutateArray(SynxObject root, List<StackFrame> stack, int parentIdx,
                            String listKey, java.util.function.Consumer<List<SynxValue>> transform) {
        List<StackEntry> path = new ArrayList<>(parentIdx);
        for (int i = 1; i <= parentIdx; i++) path.add(stack.get(i).entry());
        mutateArrayPath(root, path, 0, listKey, transform);
    }

    private static void mutateArrayPath(SynxObject obj, List<StackEntry> path, int idx,
                                         String listKey,
                                         java.util.function.Consumer<List<SynxValue>> transform) {
        if (idx >= path.size()) {
            SynxValue cur = obj.get(listKey);
            List<SynxValue> arr;
            if (cur instanceof SynxValue.Arr a) {
                arr = a.values();
            } else {
                arr = new ArrayList<>();
            }
            transform.accept(arr);
            obj.set(listKey, SynxValue.ofArray(arr));
            return;
        }
        StackEntry head = path.get(idx);
        if (head instanceof StackEntry.Key kk) {
            SynxValue v = obj.get(kk.name());
            if (!(v instanceof SynxValue.Obj childObj)) return;
            mutateArrayPath(childObj.map(), path, idx + 1, listKey, transform);
        } else if (head instanceof StackEntry.ListItem li) {
            SynxValue v = obj.get(li.listKey());
            if (!(v instanceof SynxValue.Arr a)) return;
            if (li.itemIdx() >= a.values().size()) return;
            SynxValue item = a.values().get(li.itemIdx());
            if (!(item instanceof SynxValue.Obj io)) return;
            mutateArrayPath(io.map(), path, idx + 1, listKey, transform);
        }
    }

    // ─── internal types ─────────────────────────────────────────────────────

    record StackFrame(int indent, StackEntry entry) {}

    sealed interface StackEntry permits StackEntry.Root, StackEntry.Key, StackEntry.ListItem {
        final class Root implements StackEntry {
            public static final Root INSTANCE = new Root();
            private Root() {}
        }
        record Key(String name) implements StackEntry {}
        record ListItem(String listKey, int itemIdx) implements StackEntry {}
    }

    static final class ParsedLine {
        String key = "";
        String typeHint;
        String value = "";
        List<String> markers = new ArrayList<>();
        List<String> markerArgs = new ArrayList<>();
        SynxConstraints constraints;
    }

    private static final class BlockState {
        final int indent;
        final String key;
        final StringBuilder content;
        final int stackIdx;
        BlockState(int indent, String key, StringBuilder content, int stackIdx) {
            this.indent = indent; this.key = key; this.content = content; this.stackIdx = stackIdx;
        }
    }

    private static final class ListState {
        final int indent;
        final String key;
        final int stackIdx;
        ListState(int indent, String key, int stackIdx) {
            this.indent = indent; this.key = key; this.stackIdx = stackIdx;
        }
    }
}
