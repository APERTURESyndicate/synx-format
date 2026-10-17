// Canonical SYNX text reformatter. Port of `fmt_canonical` in synx-core/src/lib.rs:
// keys sorted (stable) by code point of their lowercase, 2-space indentation,
// `|` / `|+` bodies kept as text, comments dropped, directives kept on top.

public enum SynxFormatter {

    public static let maxParseDepth = 128

    /// A header line with what belongs to it, as ranges of the input.
    private final class Node {
        let header: Range<Int>
        var children: [Node] = []
        var listItems: [Range<Int>] = []
        var isMultiline = false
        init(_ header: Range<Int>) { self.header = header }
    }

    public static func format(_ text: String) -> String {
        let bytes = Array(SynxParser.clamp(text).utf8)
        return bytes.withUnsafeBufferPointer { b -> String in
            // Rust `str::lines`: split at `\n`, one trailing `\r` dropped, no final empty line.
            var lines: [Range<Int>] = []
            var start = 0
            while start < b.count {
                var nl = start
                while nl < b.count && b[nl] != 0x0A { nl += 1 }
                let end = (nl > start && b[nl - 1] == 0x0D) ? nl - 1 : nl
                lines.append(start..<end)
                start = nl + 1
            }
            var directives: [Range<Int>] = []
            var bodyStart = 0
            for (i, l) in lines.enumerated() {
                let t = trimRange(b, l.lowerBound, l.upperBound)
                if eq(b, t, "!active") || eq(b, t, "!lock") || eq(b, t, "!tool") || eq(b, t, "!schema")
                    || eq(b, t, "!llm") || eq(b, t, "#!mode:active") {
                    directives.append(t)
                    bodyStart = i + 1
                } else if t.isEmpty || b[t.lowerBound] == 0x23 || starts(b, t, "//") {
                    bodyStart = i + 1
                } else {
                    break
                }
            }
            var nodes: [Node] = []
            _ = parse(b, lines, bodyStart, 0, 0, &nodes)
            sort(b, &nodes)
            var out: [UInt8] = []
            out.reserveCapacity(b.count + 64)
            for (i, d) in directives.enumerated() {
                if i > 0 { out.append(0x0A) }
                out.append(contentsOf: UnsafeBufferPointer(rebasing: b[d]))
            }
            if !directives.isEmpty { out.append(0x0A); out.append(0x0A) }
            emit(b, nodes, 0, &out)
            // Trailing blank lines go; the text ends in exactly one newline.
            let end = out.withUnsafeBufferPointer { trimEndRange($0, 0, $0.count) }
            out.removeSubrange(end...)
            out.append(0x0A)
            return String(decoding: out, as: UTF8.self)
        }
    }

    private static func eq(_ b: UnsafeBufferPointer<UInt8>, _ r: Range<Int>, _ s: StaticString) -> Bool {
        r.count == s.utf8CodeUnitCount && starts(b, r, s)
    }

    private static func starts(_ b: UnsafeBufferPointer<UInt8>, _ r: Range<Int>, _ s: StaticString) -> Bool {
        let n = s.utf8CodeUnitCount
        if r.count < n { return false }
        let p = s.utf8Start
        for k in 0..<n where b[r.lowerBound + k] != p[k] { return false }
        return true
    }

    private static func ends(_ b: UnsafeBufferPointer<UInt8>, _ r: Range<Int>, _ s: StaticString) -> Bool {
        let n = s.utf8CodeUnitCount
        if r.count < n { return false }
        let p = s.utf8Start
        for k in 0..<n where b[r.upperBound - n + k] != p[k] { return false }
        return true
    }

    private static func parse(_ b: UnsafeBufferPointer<UInt8>, _ lines: [Range<Int>], _ start: Int, _ base: Int,
                              _ depth: Int, _ nodes: inout [Node]) -> Int {
        if depth > maxParseDepth {
            // safety: the deeper subtree is dropped, but its lines are consumed —
            // returning `start` left the caller on the same line forever.
            var i = start
            while i < lines.count {
                let l = lines[i]
                if !trimRange(b, l.lowerBound, l.upperBound).isEmpty
                    && indentLen(b, l.lowerBound, l.upperBound) < base { break }
                i += 1
            }
            return i
        }
        var i = start
        while i < lines.count {
            let l = lines[i]
            let t = trimRange(b, l.lowerBound, l.upperBound)
            if t.isEmpty { i += 1; continue }
            let ind = indentLen(b, l.lowerBound, l.upperBound)
            if ind < base { break }
            if ind > base { i += 1; continue }
            if starts(b, t, "- ") || b[t.lowerBound] == 0x23 || starts(b, t, "//") { i += 1; continue }
            // `|+` opens a block too: its lines are text, kept relative to the first.
            let preserve = ends(b, t, " |+") || eq(b, t, "|+")
            let node = Node(t)
            node.isMultiline = preserve || ends(b, t, " |") || eq(b, t, "|")
            var blockBase = -1
            i += 1
            while i < lines.count {
                let cl = lines[i]
                let ct = trimRange(b, cl.lowerBound, cl.upperBound)
                if ct.isEmpty { i += 1; continue }
                let ci = indentLen(b, cl.lowerBound, cl.upperBound)
                if ci <= base { break }
                if preserve {
                    if blockBase < 0 { blockBase = ci }
                    let bodyEnd = trimEndRange(b, cl.lowerBound, cl.upperBound)
                    node.listItems.append(stripBoundary(b, cl.lowerBound, bodyEnd, Swift.min(blockBase, ci))..<bodyEnd)
                    i += 1
                } else if node.isMultiline || starts(b, ct, "- ") {
                    node.listItems.append(ct)
                    i += 1
                } else if b[ct.lowerBound] == 0x23 || starts(b, ct, "//") {
                    i += 1
                } else {
                    i = parse(b, lines, i, ci, depth + 1, &node.children)
                }
            }
            nodes.append(node)
        }
        return i
    }

    /// The key of a header, lowercased: up to the first whitespace, `[`, `:` or `(`.
    private static func sortKey(_ b: UnsafeBufferPointer<UInt8>, _ h: Range<Int>) -> [UInt8] {
        var end = h.lowerBound
        var ascii = true
        while end < h.upperBound {
            let c = b[end]
            if c == 0x5B || c == 0x3A || c == 0x28 || wsLen(b, end, h.upperBound) != 0 { break }
            if c >= 0x80 { ascii = false }
            end += utf8Len(c)
        }
        end = Swift.min(end, h.upperBound)
        if ascii {
            var k = Array(UnsafeBufferPointer(rebasing: b[h.lowerBound..<end]))
            for i in 0..<k.count where k[i] >= 0x41 && k[i] <= 0x5A { k[i] += 32 }
            return k
        }
        let key = String(decoding: UnsafeBufferPointer(rebasing: b[h.lowerBound..<end]), as: UTF8.self)
        return Array(rustLowercased(key).utf8)
    }

    private static func sort(_ b: UnsafeBufferPointer<UInt8>, _ nodes: inout [Node]) {
        // Stable, by code point of the lowercased key; each key computed once.
        if nodes.count > 1 {
            let keyed = nodes.map { (k: sortKey(b, $0.header), n: $0) }
            nodes = Resolver.mergeSort(keyed) { $0.k.lexicographicallyPrecedes($1.k) }.map { $0.n }
        }
        for n in nodes where !n.children.isEmpty { sort(b, &n.children) }
    }

    private static func emit(_ b: UnsafeBufferPointer<UInt8>, _ nodes: [Node], _ indent: Int, _ out: inout [UInt8]) {
        for n in nodes {
            for _ in 0..<indent { out.append(0x20) }
            out.append(contentsOf: UnsafeBufferPointer(rebasing: b[n.header]))
            out.append(0x0A)
            if !n.children.isEmpty { emit(b, n.children, indent + 2, &out) }
            for li in n.listItems {
                for _ in 0..<(indent + 2) { out.append(0x20) }
                out.append(contentsOf: UnsafeBufferPointer(rebasing: b[li]))
                out.append(0x0A)
            }
            if indent == 0 && (!n.children.isEmpty || !n.listItems.isEmpty) { out.append(0x0A) }
        }
    }
}
