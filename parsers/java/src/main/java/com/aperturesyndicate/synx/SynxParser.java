package com.aperturesyndicate.synx;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ThreadLocalRandom;

/**
 * SYNX text-to-tree parser. Parity with {@code crates/synx-core/src/parser.rs} (SYNX 3.8.0).
 *
 * <p>Byte quantities of the reference — indentation, the input and multiline caps — are measured
 * in UTF-8 bytes, not UTF-16 units: an indent of U+00A0 counts 2, U+3000 counts 3 (SYNX §4).
 */
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

    /**
     * Drops one leading U+FEFF (SYNX §2.1) and truncates to the longest prefix of at most
     * {@link #MAX_INPUT_BYTES} UTF-8 bytes that does not split a character.
     */
    public static String clampText(String text) {
        if (!text.isEmpty() && text.charAt(0) == '\uFEFF') text = text.substring(1);
        if (text.length() * 3L <= MAX_INPUT_BYTES) return text;
        int end = utf8PrefixLen(text, 0, text.length(), MAX_INPUT_BYTES);
        return end == text.length() ? text : text.substring(0, end);
    }

    public static SynxParseResult parse(String rawText) {
        String text = clampText(rawText);
        // Input a limit dropped before parsing sets truncated; a leading BOM is not such input.
        boolean bom = !rawText.isEmpty() && rawText.charAt(0) == 0xFEFF;
        boolean dropped = text.length() < rawText.length() - (bom ? 1 : 0);
        int end = parseEnd(text);
        if (end < text.length()) {
            text = text.substring(0, end);
            dropped = true;
        }

        SynxObject root = new SynxObject();
        Parser p = new Parser(text);
        p.result.root = SynxValue.ofObject(root);
        p.result.truncated = dropped;
        p.stack.add(new Frame(-1, Frame.ROOT, null, 0, 0, root));
        p.run();
        return p.result;
    }

    /** Length to parse: all of it, or up to the newline that would exceed MAX_LINE_STARTS lines. */
    private static int parseEnd(String text) {
        int maxNewlines = MAX_LINE_STARTS - 1;
        int seen = 0;
        int scan = 0;
        while (true) {
            int nl = text.indexOf('\n', scan);
            if (nl < 0) return text.length();
            if (seen >= maxNewlines) return nl;
            seen++;
            scan = nl + 1;
        }
    }

    // ─── parser state ───────────────────────────────────────────────────────

    /** One open object; {@code obj} is the object itself, so no re-navigation from the root. */
    private record Frame(int indent, int kind, String key, int itemIdx, int parent, SynxObject obj) {
        static final int ROOT = 0, KEY = 1, LIST_ITEM = 2;
    }

    private static final class BlockState {
        final int indent;
        final String key;
        final SynxObject parent;
        final boolean preserveIndent;
        final StringBuilder content = new StringBuilder();
        int contentBytes;
        int baseIndent = -1;

        BlockState(int indent, String key, SynxObject parent, boolean preserveIndent) {
            this.indent = indent; this.key = key; this.parent = parent; this.preserveIndent = preserveIndent;
        }
    }

    private static final class ListState {
        final int indent;
        final String key;
        final int stackIdx;
        final SynxObject parent;

        ListState(int indent, String key, int stackIdx, SynxObject parent) {
            this.indent = indent; this.key = key; this.stackIdx = stackIdx; this.parent = parent;
        }
    }

    static final class ParsedLine {
        String key = "";
        String typeHint;           // null when absent; "" for `key()`
        String value = "";
        List<String> markers = List.of();
        List<String> markerArgs = new ArrayList<>();
        SynxConstraints constraints;
    }

    private static final class Parser {
        final String text;
        final SynxParseResult result = new SynxParseResult();
        final List<Frame> stack = new ArrayList<>(16);
        String metaPath = "";
        boolean metaPathValid;

        Parser(String text) { this.text = text; }

        void push(Frame f) { stack.add(f); metaPathValid = false; }
        void pop() { stack.remove(stack.size() - 1); metaPathValid = false; }
        Frame top() { return stack.get(stack.size() - 1); }

        /** Metadata path of the top frame, following the frame chain (a list item's parent is the list's holder). */
        String currentPath() {
            if (metaPathValid) return metaPath;
            metaPathValid = true;
            if (stack.size() <= 1) return metaPath = "";
            int[] chain = new int[MAX_NESTING_DEPTH + 1];
            int n = 0;
            for (int idx = stack.size() - 1; idx > 0 && n < chain.length; ) {
                chain[n++] = idx;
                Frame f = stack.get(idx);
                idx = f.kind == Frame.LIST_ITEM ? f.parent : idx - 1;
            }
            StringBuilder sb = new StringBuilder();
            for (int i = n - 1; i >= 0; i--) {
                Frame f = stack.get(chain[i]);
                if (sb.length() > 0) sb.append('.');
                appendKeySegment(sb, f.key);
                if (f.kind == Frame.LIST_ITEM) sb.append(".#").append(f.itemIdx);
            }
            return metaPath = sb.toString();
        }

        void run() {
            SynxParseResult r = result;
            BlockState block = null;
            // Open lists, outermost first.
            // spec: a list inside a list item ends at the next item of the outer list (§8.5).
            // One variable held "the" open list, so a nested `tags` replaced its parent:
            // `- name b` after it matched no list and was dropped without a word, and the
            // keys under it landed in the item before.
            ArrayList<ListState> lists = new ArrayList<>();
            boolean inBlockComment = false;
            // Indent of a refused key whose block is being dropped, or -1.
            int droppedBelow = -1;
            int len = text.length();
            int pos = 0;
            while (pos <= len) {
                int start = pos;
                int nl = text.indexOf('\n', start);
                if (nl < 0) nl = len;
                pos = nl + 1;
                int end = nl;
                if (end > start && text.charAt(end - 1) == '\r') end--;
                // The line is trimmed by index: one substring per line, for its trimmed text.
                int a = start;
                int indent = 0;
                while (a < end) {
                    char c = text.charAt(a);
                    if (!isWs(c)) break;
                    indent += c < 0x80 ? 1 : c < 0x800 ? 2 : 3;
                    a++;
                }
                // A blank line is never structural and never body (SYNX §8.4.1).
                if (a == end) continue;
                if (droppedBelow >= 0) {
                    if (indent > droppedBelow) continue;
                    droppedBelow = -1;
                }
                int b = end;
                while (b > a && isWs(text.charAt(b - 1))) b--;
                String trimmed = text.substring(a, b);

                // Block continuation runs before every line-class rule: a `|` / `|+` body is raw
                // text, so `#`, `//`, `###` and `!…` lines are ordinary content there.
                if (block != null) {
                    if (indent > block.indent) {
                        if (block.contentBytes < MAX_MULTILINE_BYTES) {
                            if (block.content.length() > 0) {
                                block.content.append('\n');
                                block.contentBytes++;
                            }
                            int room = MAX_MULTILINE_BYTES - block.contentBytes;
                            if (room > 0) {
                                String slice;
                                if (block.preserveIndent) {
                                    if (block.baseIndent < 0) block.baseIndent = indent;
                                    int strip = Math.min(block.baseIndent, indent);
                                    // The raw line with its trailing whitespace cut, minus the base indent.
                                    int cut = start;
                                    int off = 0;
                                    while (cut < b && off < strip && isWs(text.charAt(cut))) {
                                        char c = text.charAt(cut);
                                        off += c < 0x80 ? 1 : c < 0x800 ? 2 : 3;
                                        cut++;
                                    }
                                    slice = text.substring(cut, b);
                                } else {
                                    slice = trimmed;
                                }
                                int n = utf8PrefixLen(slice, 0, slice.length(), room);
                                block.content.append(slice, 0, n);
                                block.contentBytes += utf8Len(slice, 0, n);
                                if (n < slice.length()) r.truncated = true;
                            }
                        } else {
                            r.truncated = true;
                        }
                        continue;
                    }
                    block.parent.set(block.key, SynxValue.ofString(block.content.toString()));
                    block = null;
                }

                if (trimmed.equals("###")) { inBlockComment = !inBlockComment; continue; }
                if (inBlockComment) continue;

                if ((trimmed.charAt(0) == '!' || trimmed.startsWith("#!mode:")) && directive(trimmed)) continue;

                if (trimmed.charAt(0) == '#' || trimmed.startsWith("//")) continue;

                if (trimmed.startsWith("- ")) {
                    // The list this item belongs to: the innermost open one whose key sits to
                    // the left of the dash. Lists nested deeper than the dash end here — but
                    // only when a list does take the line; a dash no list accepts is skipped
                    // and closes nothing, as it always was.
                    int accepting = lists.size();
                    while (accepting > 0 && indent < lists.get(accepting - 1).indent) accepting--;
                    if (accepting > 0 && indent > lists.get(accepting - 1).indent) {
                        lists.subList(accepting, lists.size()).clear();
                    }
                    if (!lists.isEmpty() && indent > lists.get(lists.size() - 1).indent) {
                        ListState list = lists.get(lists.size() - 1);
                        // Pop every frame of the previous item — its own and any object inside it.
                        while (stack.size() > 1 && top().indent >= indent) pop();
                        String valStr = stripComment(trim(trimmed.substring(2)));

                        // The item is an object when the next line of content is deeper and
                        // is not another item.
                        boolean nested = false;
                        int[] peek = nextContentLine(text, pos);
                        if (peek != null) {
                            String pl = text.substring(peek[0], peek[1]);
                            if (indentLen(pl) > indent && !trim(pl).startsWith("- ")) nested = true;
                        }

                        SynxValue cur = list.parent.get(list.key);
                        if (cur == null) {
                            cur = SynxValue.ofArray();
                            list.parent.set(list.key, cur);
                        }
                        if (!(cur instanceof SynxValue.Arr arr)) continue;
                        if (arr.values().size() >= MAX_LIST_ITEMS) {
                            // The cap dropped an item: say so, like every other cap.
                            r.truncated = true;
                            continue;
                        }
                        if (nested) {
                            SynxObject item = new SynxObject();
                            ParsedLine pl = parseLine(valStr);
                            SynxMeta headMeta = null;
                            if (pl != null) {
                                SynxValue v;
                                if (pl.typeHint != null) v = castTyped(pl.value, pl.typeHint);
                                else if (!pl.value.isEmpty()) v = castValue(pl.value);
                                else v = SynxValue.ofObject();
                                if (!isReservedJsKey(pl.key)) {
                                    item.set(pl.key, v);
                                    if (r.mode == SynxMode.ACTIVE
                                            && (!pl.markers.isEmpty() || pl.constraints != null || pl.typeHint != null)) {
                                        headMeta = new SynxMeta();
                                        headMeta.markers = new ArrayList<>(pl.markers);
                                        headMeta.args = pl.markerArgs;
                                        headMeta.typeHint = pl.typeHint;
                                        headMeta.constraints = pl.constraints;
                                    }
                                }
                            } else {
                                item.set("_value", castValue(valStr));
                            }
                            int itemIdx = arr.values().size();
                            arr.values().add(SynxValue.ofObject(item));
                            if (stack.size() < MAX_NESTING_DEPTH) {
                                push(new Frame(indent, Frame.LIST_ITEM, list.key, itemIdx, list.stackIdx, item));
                                // spec: the key on the `- ` line is a key line like the ones
                                // under it. Its markers and constraints were read and thrown
                                // away: `- token:secret abc` stayed in the clear and
                                // `- host:env H` kept the variable's name.
                                if (headMeta != null) {
                                    r.metadata.computeIfAbsent(currentPath(), k -> new LinkedHashMap<>()).put(pl.key, headMeta);
                                }
                            }
                        } else {
                            arr.values().add(castValue(valStr));
                        }
                        continue;
                    }
                } else {
                    // A non-item line closes every list it is at or below the indent of.
                    boolean closed = false;
                    while (!lists.isEmpty() && indent <= lists.get(lists.size() - 1).indent) {
                        lists.remove(lists.size() - 1);
                        closed = true;
                    }
                    if (closed) {
                        while (stack.size() > 1 && top().kind == Frame.LIST_ITEM && top().indent >= indent) pop();
                    }
                }

                ParsedLine pl = parseLine(trimmed);
                if (pl == null) continue;
                // safety: the lines under a refused key go with it. Only the key
                // line was skipped, so its block fell to whatever was open above
                // it — `profile` / `__proto__` / `  role admin` gave `profile` the
                // role — and under `prototype |` each line of the text was read as
                // a line of the document.
                if (isReservedJsKey(pl.key)) {
                    droppedBelow = indent;
                    continue;
                }
                while (stack.size() > 1 && top().indent >= indent) pop();
                int parentIdx = stack.size() - 1;
                SynxObject parent = stack.get(parentIdx).obj;

                // A key that replaces an object or list replaces its declarations too.
                if (r.mode == SynxMode.ACTIVE && !r.metadata.isEmpty()) {
                    SynxValue old = parent.get(pl.key);
                    if (old instanceof SynxValue.Obj || old instanceof SynxValue.Arr) {
                        String prefix = currentPath();
                        StringBuilder sb = new StringBuilder(prefix);
                        if (!prefix.isEmpty()) sb.append('.');
                        appendKeySegment(sb, pl.key);
                        String pfx = sb.toString();
                        r.metadata.keySet().removeIf(path -> path.equals(pfx)
                            || (path.length() > pfx.length() && path.startsWith(pfx) && path.charAt(pfx.length()) == '.'));
                    }
                }

                if (r.mode == SynxMode.ACTIVE
                        && (!pl.markers.isEmpty() || pl.constraints != null || pl.typeHint != null)) {
                    SynxMeta meta = new SynxMeta();
                    meta.markers = new ArrayList<>(pl.markers);
                    meta.args = pl.markerArgs;
                    meta.typeHint = pl.typeHint;
                    meta.constraints = pl.constraints;
                    r.metadata.computeIfAbsent(currentPath(), k -> new LinkedHashMap<>()).put(pl.key, meta);
                }

                boolean isBlock = pl.value.equals("|") || pl.value.equals("|+");
                boolean isListMarker = false;
                for (String m : pl.markers) {
                    if (m.equals("random") || m.equals("unique") || m.equals("geo") || m.equals("join")) {
                        isListMarker = true;
                        break;
                    }
                }

                if (isBlock) {
                    parent.set(pl.key, SynxValue.ofString(""));
                    block = new BlockState(indent, pl.key, parent, pl.value.equals("|+"));
                } else if (isListMarker && pl.value.isEmpty()) {
                    parent.set(pl.key, SynxValue.ofArray());
                    lists.add(new ListState(indent, pl.key, parentIdx, parent));
                } else if (pl.value.isEmpty()) {
                    int[] peek = nextContentLine(text, pos);
                    if (peek != null && trim(text.substring(peek[0], peek[1])).startsWith("- ")) {
                        parent.set(pl.key, SynxValue.ofArray());
                        lists.add(new ListState(indent, pl.key, parentIdx, parent));
                        continue;
                    }
                    SynxObject child = new SynxObject();
                    parent.set(pl.key, SynxValue.ofObject(child));
                    if (stack.size() < MAX_NESTING_DEPTH) {
                        push(new Frame(indent, Frame.KEY, pl.key, 0, 0, child));
                    }
                } else {
                    parent.set(pl.key, pl.typeHint != null ? castTyped(pl.value, pl.typeHint) : castValue(pl.value));
                }
            }
            if (block != null) block.parent.set(block.key, SynxValue.ofString(block.content.toString()));
        }

        /** Applies one directive line; false for an unknown `!…` line, which is parsed as usual. */
        boolean directive(String t) {
            SynxParseResult r = result;
            switch (t) {
                case "!active": r.mode = SynxMode.ACTIVE; return true;
                case "!lock":   r.locked = true;          return true;
                case "!tool":   r.tool = true;            return true;
                case "!schema": r.schema = true;          return true;
                case "!llm":    r.llm = true;             return true;
                default: break;
            }
            if (t.startsWith("!include ")) {
                if (r.includes.size() < MAX_INCLUDES) {
                    String rest = trim(t.substring(9));
                    String path = rest;
                    String alias = null;
                    for (int i = 0; i < rest.length(); i++) {
                        if (isWs(rest.charAt(i))) {
                            path = rest.substring(0, i);
                            alias = trim(rest.substring(i + 1));
                            break;
                        }
                    }
                    if (alias == null) {
                        String name = path;
                        int slash = Math.max(name.lastIndexOf('/'), name.lastIndexOf('\\'));
                        if (slash >= 0) name = name.substring(slash + 1);
                        if (name.endsWith(".synx") || name.endsWith(".SYNX")) name = name.substring(0, name.length() - 5);
                        alias = name;
                    }
                    r.includes.add(new SynxIncludeDirective(path, alias));
                }
                return true;
            }
            if (t.startsWith("!use ")) {
                String rest = trim(t.substring(5));
                if (rest.startsWith("@")) {
                    int as = rest.indexOf(" as ");
                    String pkg = trim(as >= 0 ? rest.substring(0, as) : rest);
                    String alias;
                    if (as >= 0) {
                        alias = trim(rest.substring(as + 4));
                    } else {
                        int slash = pkg.lastIndexOf('/');
                        alias = slash >= 0 ? pkg.substring(slash + 1) : pkg;
                    }
                    if (!pkg.isEmpty()) r.uses.add(new SynxUseDirective(pkg, alias));
                }
                return true;
            }
            if (t.startsWith("#!mode:")) {
                r.mode = "active".equals(trim(t.substring(7))) ? SynxMode.ACTIVE : SynxMode.STATIC;
                return true;
            }
            return false;
        }
    }

    /**
     * The first line at or after {@code from} that is content — not blank and not a comment —
     * as {start, end} without a trailing CR.
     *
     * <p>spec: a comment line is ignored (§5), so it cannot stand between a key and its list
     * or between a `- ` line and the keys of its item. Only blank lines were skipped here:
     * `items` / `# note` / `- a` read as an empty object and lost every item, and a comment
     * after `- name a` turned the item into the text `name a` and sent its keys to the parent.
     */
    private static int[] nextContentLine(String text, int from) {
        int p = from;
        int len = text.length();
        boolean inComment = false;
        while (p <= len) {
            int nl = text.indexOf('\n', p);
            if (nl < 0) nl = len;
            int end = nl;
            if (end > p && text.charAt(end - 1) == '\r') end--;
            int a = p;
            while (a < end && isWs(text.charAt(a))) a++;
            int b = end;
            while (b > a && isWs(text.charAt(b - 1))) b--;
            if (b - a == 3 && text.startsWith("###", a)) {
                inComment = !inComment;
            } else if (!inComment && a < b && text.charAt(a) != '#' && !text.startsWith("//", a)) {
                return new int[] { p, end };
            }
            p = nl + 1;
        }
        return null;
    }

    private static boolean isBlank(String s, int from, int to) {
        for (int i = from; i < to; i++) if (!isWs(s.charAt(i))) return false;
        return true;
    }

    // ─── !tool reshape ──────────────────────────────────────────────────────

    public static SynxValue reshapeToolOutput(SynxValue root, boolean schema) {
        if (!(root instanceof SynxValue.Obj obj)) return root;
        SynxObject map = obj.map();
        if (schema) {
            List<String> keys = new ArrayList<>(map.keys());
            keys.sort(SynxParser::compareCodePoints);
            List<SynxValue> tools = new ArrayList<>();
            for (String k : keys) {
                SynxObject def = new SynxObject();
                def.set("name", SynxValue.ofString(k));
                def.set("params", SynxObject.deepCopy(map.get(k)));
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
        keys.sort(SynxParser::compareCodePoints);
        String firstKey = keys.get(0);
        SynxValue firstVal = map.get(firstKey);
        SynxValue params = firstVal instanceof SynxValue.Obj ? SynxObject.deepCopy(firstVal) : SynxValue.ofObject();
        SynxObject out = new SynxObject();
        out.set("tool", SynxValue.ofString(firstKey));
        out.set("params", params);
        return SynxValue.ofObject(out);
    }

    // ─── line decomposer ────────────────────────────────────────────────────

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
            int close = trimmed.indexOf(')', start);
            if (close >= 0) {
                out.typeHint = trimmed.substring(start, close);
                pos = close + 1;
            } else {
                pos++;
            }
        }

        // Balanced scan so `^[A-Z]{2}$` does not close the block at its own `]`.
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
                int rel = trimmed.indexOf(']', cstart);
                if (rel >= 0) {
                    out.constraints = parseConstraints(trimmed.substring(cstart, rel));
                    pos = rel + 1;
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
            List<String> markers = new ArrayList<>();
            int p = mstart;
            while (markers.size() < MAX_MARKER_SEGMENTS) {
                int colon = trimmed.indexOf(':', p);
                if (colon < 0 || colon >= mend) {
                    markers.add(trimmed.substring(p, mend));
                    break;
                }
                markers.add(trimmed.substring(p, colon));
                p = colon + 1;
            }
            out.markers = markers;
            pos = mend;
        }

        while (pos < len && (trimmed.charAt(pos) == ' ' || trimmed.charAt(pos) == '\t')) pos++;
        if (pos < len) out.value = stripComment(trimmed.substring(pos));

        if (!out.markers.isEmpty() && !out.value.isEmpty()) {
            // `:random` reads its weights from the value.
            if (out.markers.contains("random")) {
                List<String> nums = new ArrayList<>();
                // spec: a weight is `digits[.digits]`. Anything a float parser reads counted,
                // so `a:random inf`, `-5` or `nan` lost its value to a weight list.
                for (String tok : splitWhitespace(out.value)) {
                    if (isWeight(tok)) nums.add(tok);
                }
                if (!nums.isEmpty()) {
                    out.markerArgs = nums;
                    out.value = "";
                }
            }
            // `:inherit` with a value names the parent instead of a scalar.
            if (!out.value.isEmpty() && out.markers.contains("inherit")) {
                out.markerArgs = new ArrayList<>(List.of(trim(out.value)));
                out.value = "";
            }
        }
        return out;
    }

    /** Rust {@code split_whitespace} (Unicode White_Space). */
    private static Double finiteOrNull(Double d) {
        return d != null && Double.isFinite(d) ? d : null;
    }

    /** {@code digits[.digits]} — a {@code :random} weight. */
    static boolean isWeight(String s) {
        int dot = s.indexOf('.');
        if (dot < 0) return isDigits(s, 0, s.length());
        return isDigits(s, 0, dot) && isDigits(s, dot + 1, s.length());
    }

    private static boolean isDigits(String s, int from, int to) {
        if (from >= to) return false;
        for (int i = from; i < to; i++) {
            char c = s.charAt(i);
            if (c < '0' || c > '9') return false;
        }
        return true;
    }

    static List<String> splitWhitespace(String s) {
        List<String> out = new ArrayList<>();
        int i = 0;
        int n = s.length();
        while (i < n) {
            while (i < n && isWs(s.charAt(i))) i++;
            int start = i;
            while (i < n && !isWs(s.charAt(i))) i++;
            if (i > start) out.add(s.substring(start, i));
        }
        return out;
    }

    /** Parses the body of a {@code […]} block. */
    static SynxConstraints parseConstraints(String raw) {
        SynxConstraints c = new SynxConstraints();
        int from = 0;
        while (from <= raw.length()) {
            int comma = raw.indexOf(',', from);
            String part = trim(comma < 0 ? raw.substring(from) : raw.substring(from, comma));
            from = comma < 0 ? raw.length() + 1 : comma + 1;
            if (part.isEmpty()) continue;
            if (part.equals("required")) { c.required = true; continue; }
            if (part.equals("readonly")) { c.readonly = true; continue; }
            int colon = part.indexOf(':');
            if (colon < 0) continue;
            String k = trim(part.substring(0, colon));
            String v = trim(part.substring(colon + 1));
            switch (k) {
                // A bound is a finite number: `inf` and `nan` bound nothing sensible
                // (`[max:-inf]` rejected every value).
                case "min":     c.min = finiteOrNull(SynxNumbers.parseF64(v)); break;
                case "max":     c.max = finiteOrNull(SynxNumbers.parseF64(v)); break;
                case "type":    c.typeName = v; break;
                case "pattern": c.pattern = v;  break;
                case "enum": {
                    List<String> vals = new ArrayList<>();
                    int p = 0;
                    while (vals.size() < MAX_ENUM_PARTS) {
                        int bar = v.indexOf('|', p);
                        if (bar < 0) { vals.add(v.substring(p)); break; }
                        vals.add(v.substring(p, bar));
                        p = bar + 1;
                    }
                    c.enumValues = vals;
                    break;
                }
                default: break;
            }
        }
        return c;
    }

    // ─── casting ────────────────────────────────────────────────────────────

    /** SYNX §8.3 automatic casting. */
    static SynxValue castValue(String val) {
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
            case "":      return SynxValue.ofString("");
            default: break;
        }
        int start = 0;
        if (val.charAt(0) == '-') {
            if (val.length() == 1) return SynxValue.ofString(val);
            start = 1;
        }
        if (SynxNumbers.isDigit(val.charAt(start))) {
            int dotPos = -1;
            boolean allNumeric = true;
            for (int j = start; j < val.length(); j++) {
                char c = val.charAt(j);
                if (c == '.') {
                    if (dotPos >= 0) { allNumeric = false; break; }
                    dotPos = j;
                } else if (!SynxNumbers.isDigit(c)) {
                    allNumeric = false;
                    break;
                }
            }
            if (allNumeric) {
                if (dotPos >= 0) {
                    if (dotPos > start && dotPos < val.length() - 1) {
                        Double d = SynxNumbers.parseF64(val);
                        if (d != null) return SynxValue.ofFloat(d);
                    }
                } else {
                    Long n = SynxNumbers.parseI64(val);
                    if (n != null) return SynxValue.ofInt(n);
                }
            }
        }
        return SynxValue.ofString(val);
    }

    /** SYNX §8.3 typed casting. */
    static SynxValue castTyped(String val, String hint) {
        switch (hint) {
            case "int": {
                Long n = SynxNumbers.parseI64(val);
                return SynxValue.ofInt(n == null ? 0 : n);
            }
            case "float": {
                Double d = SynxNumbers.parseF64(val);
                return SynxValue.ofFloat(d == null ? 0.0 : d);
            }
            case "bool":
                return SynxValue.ofBool(trim(val).equals("true"));
            case "string":
                return SynxValue.ofString(val);
            case "random":
            case "random:int":
                return SynxValue.ofInt(ThreadLocalRandom.current().nextLong(2_147_483_647L));
            case "random:float":
                return SynxValue.ofFloat(ThreadLocalRandom.current().nextInt(10_000) / 10_000.0);
            case "random:bool":
                return SynxValue.ofBool(ThreadLocalRandom.current().nextBoolean());
            default:
                return castValue(val);
        }
    }

    // ─── text helpers ───────────────────────────────────────────────────────

    static boolean isReservedJsKey(String key) {
        return key.equals("__proto__") || key.equals("constructor") || key.equals("prototype");
    }

    /** Unicode White_Space — Rust {@code char::is_whitespace}; {@code Character.isWhitespace} misses NBSP and NEL. */
    static boolean isWs(char c) {
        return (c >= 0x09 && c <= 0x0D) || c == 0x20 || c == 0x85 || c == 0xA0 || c == 0x1680
            || (c >= 0x2000 && c <= 0x200A) || c == 0x2028 || c == 0x2029 || c == 0x202F
            || c == 0x205F || c == 0x3000;
    }

    static String trim(String s) {
        int a = 0;
        int b = s.length();
        while (a < b && isWs(s.charAt(a))) a++;
        while (b > a && isWs(s.charAt(b - 1))) b--;
        return (a == 0 && b == s.length()) ? s : s.substring(a, b);
    }

    static String trimEnd(String s) {
        int b = s.length();
        while (b > 0 && isWs(s.charAt(b - 1))) b--;
        return b == s.length() ? s : s.substring(0, b);
    }

    static String trimStart(String s) {
        int a = 0;
        while (a < s.length() && isWs(s.charAt(a))) a++;
        return a == 0 ? s : s.substring(a);
    }

    /** UTF-8 byte length of the leading Unicode whitespace (SYNX §4). */
    static int indentLen(String s) {
        int bytes = 0;
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (!isWs(c)) break;
            bytes += c < 0x80 ? 1 : c < 0x800 ? 2 : 3;
        }
        return bytes;
    }

    /** Index of the first character at or past {@code strip} UTF-8 bytes of leading whitespace. */
    static int stripBoundary(String s, int strip) {
        int off = 0;
        int i = 0;
        while (i < s.length() && off < strip && isWs(s.charAt(i))) {
            char c = s.charAt(i);
            off += c < 0x80 ? 1 : c < 0x800 ? 2 : 3;
            i++;
        }
        return i;
    }

    /** UTF-8 length of {@code s[from, to)}. */
    static long utf8Len(String s, int from, int to) {
        long n = 0;
        for (int i = from; i < to; i++) {
            char c = s.charAt(i);
            if (c < 0x80) n++;
            else if (c < 0x800) n += 2;
            else if (Character.isHighSurrogate(c) && i + 1 < to && Character.isLowSurrogate(s.charAt(i + 1))) {
                n += 4;
                i++;
            } else n += 3;
        }
        return n;
    }

    /** End index of the longest prefix of {@code s[from, to)} within {@code limit} UTF-8 bytes. */
    static int utf8PrefixLen(String s, int from, int to, long limit) {
        long n = 0;
        int i = from;
        while (i < to) {
            char c = s.charAt(i);
            int w;
            int step = 1;
            if (c < 0x80) w = 1;
            else if (c < 0x800) w = 2;
            else if (Character.isHighSurrogate(c) && i + 1 < to && Character.isLowSurrogate(s.charAt(i + 1))) {
                w = 4;
                step = 2;
            } else w = 3;
            if (n + w > limit) break;
            n += w;
            i += step;
        }
        return i;
    }

    /** Cuts an inline {@code  //} or {@code  #} comment and trailing whitespace off a value. */
    static String stripComment(String val) {
        if (val.indexOf(' ') >= 0) {
            int p = val.indexOf(" //");
            if (p >= 0) val = val.substring(0, p);
            p = val.indexOf(" #");
            if (p >= 0) val = val.substring(0, p);
        }
        return trimEnd(val);
    }

    /**
     * Appends {@code key} to a metadata path as one segment: {@code .} and {@code \} inside the key
     * are escaped with {@code \}, so the object {@code a.b} and {@code b} inside {@code a} never share
     * a path. A list item adds {@code .#<index>}.
     */
    static void appendKeySegment(StringBuilder sb, String key) {
        if (key.indexOf('.') < 0 && key.indexOf('\\') < 0) {
            sb.append(key);
            return;
        }
        for (int i = 0; i < key.length(); i++) {
            char c = key.charAt(i);
            if (c == '.' || c == '\\') sb.append('\\');
            sb.append(c);
        }
    }

    static String keySegment(String key) {
        if (key.indexOf('.') < 0 && key.indexOf('\\') < 0) return key;
        StringBuilder sb = new StringBuilder(key.length() + 2);
        appendKeySegment(sb, key);
        return sb.toString();
    }

    /** The unescaped segments of a metadata path: {@code a\.b.c} is ["a.b", "c"]. */
    public static List<String> metaPathSegments(String path) {
        List<String> out = new ArrayList<>();
        if (path.isEmpty()) return out;
        StringBuilder seg = new StringBuilder();
        for (int i = 0; i < path.length(); i++) {
            char c = path.charAt(i);
            if (c == '\\') {
                if (i + 1 < path.length()) seg.append(path.charAt(++i));
            } else if (c == '.') {
                out.add(seg.toString());
                seg.setLength(0);
            } else {
                seg.append(c);
            }
        }
        out.add(seg.toString());
        return out;
    }

    /** Orders strings by Unicode code point (the order of Rust's byte-wise {@code str} comparison). */
    static int compareCodePoints(String a, String b) {
        int n = Math.min(a.length(), b.length());
        for (int i = 0; i < n; i++) {
            char x = a.charAt(i), y = b.charAt(i);
            if (x == y) continue;
            // A surrogate (a supplementary character) sorts after every BMP character.
            boolean sx = Character.isSurrogate(x), sy = Character.isSurrogate(y);
            if (sx != sy) return sx ? 1 : -1;
            return x - y;
        }
        return a.length() - b.length();
    }

    static final java.util.Comparator<Map.Entry<String, SynxValue>> BY_KEY =
        (x, y) -> compareCodePoints(x.getKey(), y.getKey());
}
