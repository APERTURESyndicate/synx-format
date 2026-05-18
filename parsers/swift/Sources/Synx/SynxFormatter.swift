// Canonical SYNX text reformatter — sorts keys, normalises indentation,
// preserves leading directives. Mirrors `fmt_canonical` in synx-core/src/lib.rs.
import Foundation

public enum SynxFormatter {

    public static let maxParseDepth = 128

    public static func format(_ text: String) -> String {
        let clamped = SynxParser.clamp(text)
        let lines = clamped.split(separator: "\n", omittingEmptySubsequences: false)
            .map { (sub: Substring) -> String in
                // Strip trailing \r for CRLF inputs.
                if sub.last == "\r" { return String(sub.dropLast()) }
                return String(sub)
            }

        var directives: [String] = []
        var bodyStart = 0
        for (i, line) in lines.enumerated() {
            let t = line.trimmingCharacters(in: .whitespaces)
            if t == "!active" || t == "!lock" || t == "!tool"
                || t == "!schema" || t == "!llm" || t == "#!mode:active" {
                directives.append(t)
                bodyStart = i + 1
            } else if t.isEmpty || t.hasPrefix("#") || t.hasPrefix("//") {
                bodyStart = i + 1
            } else {
                break
            }
        }

        var nodes: [FmtNode] = []
        _ = fmtParse(lines: lines, start: bodyStart, base: 0, depth: 0, nodes: &nodes)
        fmtSort(&nodes)

        var out = ""
        out.reserveCapacity(min(clamped.utf8.count, SynxParser.maxInputBytes) + 64)
        if !directives.isEmpty {
            out.append(directives.joined(separator: "\n"))
            out.append("\n\n")
        }
        fmtEmit(nodes, indentLvl: 0, into: &out)

        // Trim trailing whitespace / newlines, then exactly one newline.
        while let last = out.last, last == "\n" || last == " " || last == "\t" {
            out.removeLast()
        }
        out.append("\n")
        return out
    }

    private struct FmtNode {
        var header: String
        var children: [FmtNode] = []
        var listItems: [String] = []
        var isMultiline: Bool = false
    }

    private static func indentOf(_ line: String) -> Int {
        var i = 0
        let chars = Array(line)
        while i < chars.count && (chars[i] == " " || chars[i] == "\t") { i += 1 }
        return i
    }

    private static func fmtParse(lines: [String], start: Int, base: Int,
                                 depth: Int, nodes: inout [FmtNode]) -> Int {
        if depth > maxParseDepth { return start }
        var i = start
        while i < lines.count {
            let raw = lines[i]
            let t = raw.trimmingCharacters(in: .whitespaces)
            if t.isEmpty { i += 1; continue }
            let ind = indentOf(raw)
            if ind < base { break }
            if ind > base { i += 1; continue }
            if t.hasPrefix("- ") || t.hasPrefix("#") || t.hasPrefix("//") { i += 1; continue }

            var node = FmtNode(header: t)
            node.isMultiline = (t == "|") || (t.hasSuffix(" |"))
            i += 1

            while i < lines.count {
                let cr = lines[i]
                let ct = cr.trimmingCharacters(in: .whitespaces)
                if ct.isEmpty { i += 1; continue }
                let ci = indentOf(cr)
                if ci <= base { break }
                if node.isMultiline || ct.hasPrefix("- ") {
                    node.listItems.append(ct)
                    i += 1
                } else if ct.hasPrefix("#") || ct.hasPrefix("//") {
                    i += 1
                } else {
                    var subs: [FmtNode] = []
                    i = fmtParse(lines: lines, start: i, base: ci, depth: depth + 1, nodes: &subs)
                    node.children.append(contentsOf: subs)
                }
            }
            nodes.append(node)
        }
        return i
    }

    private static func sortKey(_ header: String) -> String {
        var out = ""
        for ch in header {
            if ch == " " || ch == "\t" || ch == "[" || ch == ":" || ch == "(" { break }
            out.append(ch)
        }
        return out.lowercased()
    }

    private static func fmtSort(_ nodes: inout [FmtNode]) {
        nodes.sort { sortKey($0.header) < sortKey($1.header) }
        for i in 0..<nodes.count {
            fmtSort(&nodes[i].children)
        }
    }

    private static func fmtEmit(_ nodes: [FmtNode], indentLvl: Int, into out: inout String) {
        let sp = String(repeating: " ", count: indentLvl)
        let itemSp = String(repeating: " ", count: indentLvl + 2)
        for n in nodes {
            out.append(sp); out.append(n.header); out.append("\n")
            if !n.children.isEmpty {
                fmtEmit(n.children, indentLvl: indentLvl + 2, into: &out)
            }
            for li in n.listItems {
                out.append(itemSp); out.append(li); out.append("\n")
            }
            if indentLvl == 0 && (!n.children.isEmpty || !n.listItems.isEmpty) {
                out.append("\n")
            }
        }
    }
}
