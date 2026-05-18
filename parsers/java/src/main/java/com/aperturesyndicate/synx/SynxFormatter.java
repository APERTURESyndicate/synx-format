package com.aperturesyndicate.synx;

import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;

/** Canonical SYNX text reformatter. Mirrors {@code fmt_canonical} in {@code synx-core/src/lib.rs}. */
public final class SynxFormatter {

    private SynxFormatter() {}
    public static final int MAX_PARSE_DEPTH = 128;

    public static String format(String text) {
        String clamped = SynxParser.clampText(text);
        String[] lines = clamped.split("\n", -1);
        for (int i = 0; i < lines.length; i++) {
            String l = lines[i];
            if (!l.isEmpty() && l.charAt(l.length() - 1) == '\r') {
                lines[i] = l.substring(0, l.length() - 1);
            }
        }
        List<String> directives = new ArrayList<>();
        int bodyStart = 0;
        for (int i = 0; i < lines.length; i++) {
            String t = lines[i].strip();
            if (t.equals("!active") || t.equals("!lock") || t.equals("!tool")
                || t.equals("!schema") || t.equals("!llm") || t.equals("#!mode:active")) {
                directives.add(t);
                bodyStart = i + 1;
            } else if (t.isEmpty() || t.startsWith("#") || t.startsWith("//")) {
                bodyStart = i + 1;
            } else break;
        }
        List<FmtNode> nodes = new ArrayList<>();
        fmtParse(lines, bodyStart, 0, 0, nodes);
        fmtSort(nodes);

        StringBuilder out = new StringBuilder();
        if (!directives.isEmpty()) {
            out.append(String.join("\n", directives)).append("\n\n");
        }
        fmtEmit(nodes, 0, out);
        // Trim trailing whitespace, ensure single \n.
        int end = out.length();
        while (end > 0) {
            char c = out.charAt(end - 1);
            if (c == '\n' || c == ' ' || c == '\t') end--;
            else break;
        }
        out.setLength(end);
        out.append('\n');
        return out.toString();
    }

    private static final class FmtNode {
        String header;
        List<FmtNode> children = new ArrayList<>();
        List<String> listItems = new ArrayList<>();
        boolean isMultiline;
    }

    private static int indentOf(String line) {
        int i = 0;
        while (i < line.length() && (line.charAt(i) == ' ' || line.charAt(i) == '\t')) i++;
        return i;
    }

    private static int fmtParse(String[] lines, int start, int base, int depth, List<FmtNode> nodes) {
        if (depth > MAX_PARSE_DEPTH) return start;
        int i = start;
        while (i < lines.length) {
            String raw = lines[i];
            String t = raw.strip();
            if (t.isEmpty()) { i++; continue; }
            int ind = indentOf(raw);
            if (ind < base) break;
            if (ind > base) { i++; continue; }
            if (t.startsWith("- ") || t.startsWith("#") || t.startsWith("//")) { i++; continue; }

            FmtNode node = new FmtNode();
            node.header = t;
            node.isMultiline = t.equals("|") || t.endsWith(" |");
            i++;
            while (i < lines.length) {
                String cr = lines[i];
                String ct = cr.strip();
                if (ct.isEmpty()) { i++; continue; }
                int ci = indentOf(cr);
                if (ci <= base) break;
                if (node.isMultiline || ct.startsWith("- ")) {
                    node.listItems.add(ct);
                    i++;
                } else if (ct.startsWith("#") || ct.startsWith("//")) {
                    i++;
                } else {
                    List<FmtNode> subs = new ArrayList<>();
                    i = fmtParse(lines, i, ci, depth + 1, subs);
                    node.children.addAll(subs);
                }
            }
            nodes.add(node);
        }
        return i;
    }

    private static String sortKey(String header) {
        int end = 0;
        while (end < header.length()) {
            char c = header.charAt(end);
            if (c == ' ' || c == '\t' || c == '[' || c == ':' || c == '(') break;
            end++;
        }
        return header.substring(0, end).toLowerCase();
    }

    private static void fmtSort(List<FmtNode> nodes) {
        nodes.sort(Comparator.comparing(n -> sortKey(n.header)));
        for (FmtNode n : nodes) fmtSort(n.children);
    }

    private static void fmtEmit(List<FmtNode> nodes, int indentLvl, StringBuilder out) {
        String sp = " ".repeat(indentLvl);
        String itemSp = " ".repeat(indentLvl + 2);
        for (FmtNode n : nodes) {
            out.append(sp).append(n.header).append('\n');
            if (!n.children.isEmpty()) fmtEmit(n.children, indentLvl + 2, out);
            for (String li : n.listItems) out.append(itemSp).append(li).append('\n');
            if (indentLvl == 0 && (!n.children.isEmpty() || !n.listItems.isEmpty())) {
                out.append('\n');
            }
        }
    }
}
