package com.aperturesyndicate.synx;

import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;

/**
 * Canonical reformatter — a port of {@code fmt_canonical} in {@code crates/synx-core/src/lib.rs}:
 * keys sorted (stable, code point order of the lowercased key) at every level, two spaces per
 * level, a blank line after each top-level block, comments dropped, leading directives kept.
 * A {@code |} / {@code |+} body is content: never parsed as keys nor sorted, and a {@code |+} body
 * keeps its relative indentation.
 */
public final class SynxFormatter {

    private SynxFormatter() {}

    public static final int MAX_PARSE_DEPTH = 128;

    public static String format(String text) {
        String clamped = SynxParser.clampText(text);
        List<String> lines = SynxEngine.rustLines(clamped);

        List<String> directives = new ArrayList<>();
        int bodyStart = 0;
        for (int i = 0; i < lines.size(); i++) {
            String t = SynxParser.trim(lines.get(i));
            if (t.equals("!active") || t.equals("!lock") || t.equals("!tool") || t.equals("!schema")
                    || t.equals("!llm") || t.equals("#!mode:active")) {
                directives.add(t);
                bodyStart = i + 1;
            } else if (t.isEmpty() || t.startsWith("#") || t.startsWith("//")) {
                bodyStart = i + 1;
            } else {
                break;
            }
        }

        List<Node> nodes = new ArrayList<>();
        parse(lines, bodyStart, 0, 0, nodes);
        sort(nodes);

        StringBuilder out = new StringBuilder(Math.max(64, clamped.length()));
        if (!directives.isEmpty()) {
            out.append(String.join("\n", directives)).append("\n\n");
        }
        emit(nodes, 0, out);
        return SynxParser.trimEnd(out.toString()) + "\n";
    }

    private static final class Node {
        final String header;
        String sortKey;
        final List<Node> children = new ArrayList<>();
        final List<String> listItems = new ArrayList<>();
        final boolean multiline;
        Node(String header, boolean multiline) { this.header = header; this.multiline = multiline; }
    }

    /** Parses the nodes at indent {@code base} from line {@code start}; returns the next line. */
    private static int parse(List<String> lines, int start, int base, int depth, List<Node> out) {
        if (depth > MAX_PARSE_DEPTH) {
            // Past the cap the subtree is dropped, but its lines are consumed: up to the next
            // content line indented less than this level.
            int i = start;
            while (i < lines.size() && (SynxParser.trim(lines.get(i)).isEmpty()
                    || SynxParser.indentLen(lines.get(i)) >= base)) i++;
            return i;
        }
        int i = start;
        while (i < lines.size()) {
            String raw = lines.get(i);
            String t = SynxParser.trim(raw);
            if (t.isEmpty()) { i++; continue; }
            int ind = SynxParser.indentLen(raw);
            if (ind < base) break;
            if (ind > base) { i++; continue; }
            if (t.startsWith("- ") || t.startsWith("#") || t.startsWith("//")) { i++; continue; }
            boolean preserve = t.endsWith(" |+") || t.equals("|+");
            Node node = new Node(t, preserve || t.endsWith(" |") || t.equals("|"));
            int blockBase = -1;
            i++;
            while (i < lines.size()) {
                String cr = lines.get(i);
                String ct = SynxParser.trim(cr);
                if (ct.isEmpty()) { i++; continue; }
                int ci = SynxParser.indentLen(cr);
                if (ci <= base) break;
                if (preserve) {
                    if (blockBase < 0) blockBase = ci;
                    String body = SynxParser.trimEnd(cr);
                    node.listItems.add(body.substring(SynxParser.stripBoundary(body, Math.min(blockBase, ci))));
                    i++;
                } else if (node.multiline || ct.startsWith("- ")) {
                    node.listItems.add(ct);
                    i++;
                } else if (ct.startsWith("#") || ct.startsWith("//")) {
                    i++;
                } else {
                    List<Node> subs = new ArrayList<>();
                    int ni = parse(lines, i, ci, depth + 1, subs);
                    node.children.addAll(subs);
                    i = ni;
                }
            }
            out.add(node);
        }
        return i;
    }

    private static void sort(List<Node> nodes) {
        boolean surrogates = false;
        for (Node n : nodes) {
            n.sortKey = sortKey(n.header);
            for (int j = 0; j < n.sortKey.length() && !surrogates; j++) surrogates = Character.isSurrogate(n.sortKey.charAt(j));
        }
        // Without surrogates UTF-16 order is code point order, and String.compareTo is fast.
        nodes.sort(surrogates ? Comparator.comparing((Node n) -> n.sortKey, SynxParser::compareCodePoints)
                              : Comparator.comparing((Node n) -> n.sortKey));
        for (Node n : nodes) sort(n.children);
    }

    /** The header's key, lowercased as Rust's {@code str::to_lowercase} does. */
    private static String sortKey(String header) {
        int end = header.length();
        for (int i = 0; i < header.length(); i++) {
            char c = header.charAt(i);
            if (c == '[' || c == ':' || c == '(' || SynxParser.isWs(c)) { end = i; break; }
        }
        return toLowerRust(header.substring(0, end));
    }

    /**
     * {@code str::to_lowercase}: the full lowercase mapping (U+0130 becomes {@code i̇}) with the
     * Final_Sigma rule, independent of the default locale.
     */
    static String toLowerRust(String s) {
        return s.toLowerCase(java.util.Locale.ROOT);
    }

    private static void emit(List<Node> nodes, int indent, StringBuilder out) {
        String sp = " ".repeat(indent);
        String itemSp = " ".repeat(indent + 2);
        for (Node n : nodes) {
            out.append(sp).append(n.header).append('\n');
            if (!n.children.isEmpty()) emit(n.children, indent + 2, out);
            for (String li : n.listItems) out.append(itemSp).append(li).append('\n');
            if (indent == 0 && (!n.children.isEmpty() || !n.listItems.isEmpty())) out.append('\n');
        }
    }
}
