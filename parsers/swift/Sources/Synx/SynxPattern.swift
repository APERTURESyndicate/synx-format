// `[pattern:…]`: the syntax of the `regex` crate synx-core checks patterns
// with, matched in time linear in the value.
//
// safety: NSRegularExpression (ICU) backtracks — `^(.*)(.*)(.*)(.*)(.*)(.*)(.*)x$`
// against a few thousand characters did not answer in 40 s — and reads ICU's
// dialect, not the crate's. A pattern synx-core does not compile is not
// checked there, so a dialect gap was a value accepted by one implementation
// and refused by the other. This is a Thompson NFA run as a Pike VM over
// Unicode scalars, with the crate's own Unicode tables and simple case
// folding (`SynxPatternTables.swift`): every step costs at most the size of
// the program. The same engine as the C++ port's `pattern.cpp`.

typealias ScalarRanges = [(UInt32, UInt32)]

final class SynxPattern {
    /// Most instructions a compiled pattern may hold; past it the pattern counts
    /// as one synx-core does not compile (its own limit is in bytes of program).
    static let maxProgram = 100_000

    fileprivate enum Op: UInt8 { case char, split, jmp, assert, match }
    fileprivate struct Inst {
        var op: Op
        var a: Int
        var b: Int
    }

    fileprivate var prog: [Inst] = []
    fileprivate var sets: [ScalarRanges] = []
    fileprivate var anchoredStart = false

    private init() {}

    /// The pattern compiled, or nil when the `regex` crate would refuse it.
    static func compile(_ source: String) -> SynxPattern? {
        var parser = PatternParser(source.unicodeScalars.map { $0.value })
        guard let root = parser.parse() else { return nil }
        let p = SynxPattern()
        // One set per character node, however many times a repetition emits it.
        p.sets = parser.sets
        var builder = PatternBuilder(out: p)
        guard builder.node(root) else { return nil }
        guard builder.emit(.match) != nil else { return nil }
        p.anchoredStart = startsAnchored(root)
        return p
    }

    /// Whether the pattern matches anywhere in `text` (Rust `Regex::is_match`).
    func isMatch(_ text: String) -> Bool {
        let cps = text.unicodeScalars.map { $0.value }
        let n = prog.count
        var clist = SparseSet(n), nlist = SparseSet(n)
        var stack: [Int] = []
        func close(_ set: inout SparseSet, _ start: Int, _ pos: Int) -> Bool {
            stack.append(start)
            while let pc = stack.popLast() {
                if set.contains(pc) { continue }
                set.add(pc)
                let inst = prog[pc]
                switch inst.op {
                case .match:
                    stack.removeAll(keepingCapacity: true)
                    return true
                case .jmp: stack.append(inst.a)
                case .split:
                    stack.append(inst.b)
                    stack.append(inst.a)
                case .assert:
                    if PatternAssert.holds(inst.a, cps, pos) { stack.append(pc + 1) }
                case .char: break
                }
            }
            return false
        }
        var pos = 0
        while pos <= cps.count {
            if pos == 0 || !anchoredStart {
                if close(&clist, 0, pos) { return true }
            }
            if clist.count == 0 {
                if anchoredStart { return false }
                pos += 1
                continue
            }
            if pos == cps.count { break }
            let c = cps[pos]
            nlist.clear()
            for k in 0..<clist.count {
                let pc = clist.dense[k]
                if prog[pc].op == .char && RangeSet.has(sets[prog[pc].a], c) {
                    if close(&nlist, pc + 1, pos + 1) { return true }
                }
            }
            swap(&clist, &nlist)
            pos += 1
        }
        return false
    }

    fileprivate static func startsAnchored(_ n: PatternNode) -> Bool {
        switch n {
        case .assert(let k): return k == PatternAssert.startText
        case .concat(let items): return items.first.map(startsAnchored) ?? false
        case .alt(let bs): return bs.allSatisfy(startsAnchored)
        case .repeat(let inner, let min, _): return min > 0 && startsAnchored(inner)
        default: return false
        }
    }
}

private struct SparseSet {
    var dense: [Int]
    var sparse: [Int]
    var count = 0
    init(_ n: Int) {
        dense = Array(repeating: 0, count: n)
        sparse = Array(repeating: 0, count: n)
    }
    func contains(_ v: Int) -> Bool { sparse[v] < count && dense[sparse[v]] == v }
    mutating func add(_ v: Int) {
        sparse[v] = count
        dense[count] = v
        count += 1
    }
    mutating func clear() { count = 0 }
}

enum PatternAssert {
    static let startText = 0, endText = 1, startLine = 2, endLine = 3, startLineCrlf = 4, endLineCrlf = 5
    static let word = 6, notWord = 7, wordStart = 8, wordEnd = 9, wordStartHalf = 10, wordEndHalf = 11
    /// Added to a word assertion that reads ASCII word characters only (`(?-u)`).
    static let asciiOffset = 100

    static func holds(_ kind: Int, _ cps: [UInt32], _ pos: Int) -> Bool {
        let hasPrev = pos > 0, hasNext = pos < cps.count
        let prev: UInt32 = hasPrev ? cps[pos - 1] : 0, next: UInt32 = hasNext ? cps[pos] : 0
        switch kind {
        case startText: return pos == 0
        case endText: return pos == cps.count
        case startLine: return pos == 0 || prev == 0x0A
        case endLine: return pos == cps.count || next == 0x0A
        case startLineCrlf: return pos == 0 || prev == 0x0A || (prev == 0x0D && !(hasNext && next == 0x0A))
        case endLineCrlf: return pos == cps.count || next == 0x0D || (next == 0x0A && !(hasPrev && prev == 0x0D))
        default: break
        }
        let ascii = kind >= asciiOffset
        let k = ascii ? kind - asciiOffset : kind
        let wp = hasPrev && (ascii ? isAsciiWord(prev) : RangeSet.has(RangeSet.word, prev))
        let wn = hasNext && (ascii ? isAsciiWord(next) : RangeSet.has(RangeSet.word, next))
        switch k {
        case word: return wp != wn
        case notWord: return wp == wn
        case wordStart: return !wp && wn
        case wordEnd: return wp && !wn
        case wordStartHalf: return !wp
        case wordEndHalf: return !wn
        default: return false
        }
    }

    static func isAsciiWord(_ c: UInt32) -> Bool {
        (c >= 0x30 && c <= 0x39) || (c >= 0x41 && c <= 0x5A) || (c >= 0x61 && c <= 0x7A) || c == 0x5F
    }
}

// MARK: - sets of scalars

enum RangeSet {
    static let maxScalar: UInt32 = 0x10FFFF

    static func canon(_ r: ScalarRanges) -> ScalarRanges {
        let sorted = r.sorted { $0.0 != $1.0 ? $0.0 < $1.0 : $0.1 < $1.1 }
        var out: ScalarRanges = []
        for x in sorted {
            if let last = out.last, x.0 <= last.1 &+ 1 {
                out[out.count - 1].1 = max(last.1, x.1)
            } else {
                out.append(x)
            }
        }
        return out
    }

    static func unite(_ a: ScalarRanges, _ b: ScalarRanges) -> ScalarRanges { canon(a + b) }

    /// The complement among Unicode scalar values (no surrogates), as the crate's.
    static func negate(_ r: ScalarRanges) -> ScalarRanges {
        var out: ScalarRanges = []
        var next: UInt32 = 0
        var done = false
        for x in r {
            if x.0 > next { out.append((next, x.0 - 1)) }
            if x.1 >= maxScalar { done = true; break }
            next = x.1 + 1
        }
        if !done && next <= maxScalar { out.append((next, maxScalar)) }
        var cut: ScalarRanges = []
        for x in out {
            if x.1 < 0xD800 || x.0 > 0xDFFF {
                cut.append(x)
            } else {
                if x.0 < 0xD800 { cut.append((x.0, 0xD7FF)) }
                if x.1 > 0xDFFF { cut.append((0xE000, x.1)) }
            }
        }
        return cut
    }

    static func intersect(_ a: ScalarRanges, _ b: ScalarRanges) -> ScalarRanges {
        var out: ScalarRanges = []
        var i = 0, j = 0
        while i < a.count && j < b.count {
            let lo = max(a[i].0, b[j].0), hi = min(a[i].1, b[j].1)
            if lo <= hi { out.append((lo, hi)) }
            if a[i].1 < b[j].1 { i += 1 } else { j += 1 }
        }
        return out
    }

    static func minus(_ a: ScalarRanges, _ b: ScalarRanges) -> ScalarRanges { intersect(a, negate(b)) }

    static func symdiff(_ a: ScalarRanges, _ b: ScalarRanges) -> ScalarRanges {
        minus(unite(a, b), intersect(a, b))
    }

    static func has(_ r: ScalarRanges, _ c: UInt32) -> Bool {
        var lo = 0, hi = r.count
        while lo < hi {
            let mid = (lo + hi) / 2
            if c < r[mid].0 { hi = mid } else if c > r[mid].1 { lo = mid + 1 } else { return true }
        }
        return false
    }

    /// Simple case folding of a set (`ClassUnicode::case_fold_simple`); without
    /// Unicode, ASCII letters only.
    static func fold(_ r: ScalarRanges, unicode: Bool) -> ScalarRanges {
        var add: ScalarRanges = []
        if !unicode {
            for x in r {
                var l = max(x.0, 0x41), h = min(x.1, 0x5A)
                if l <= h { add.append((l + 32, h + 32)) }
                l = max(x.0, 0x61)
                h = min(x.1, 0x7A)
                if l <= h { add.append((l - 32, h - 32)) }
            }
        } else {
            let fold = PatternTables.fold, targets = PatternTables.foldTargets
            let entries = fold.count / 3
            for x in r {
                var lo = 0, hi = entries
                while lo < hi {
                    let mid = (lo + hi) / 2
                    if fold[mid * 3] < x.0 { lo = mid + 1 } else { hi = mid }
                }
                var e = lo
                while e < entries && fold[e * 3] <= x.1 {
                    let off = Int(fold[e * 3 + 1]), n = Int(fold[e * 3 + 2])
                    for k in 0..<n { add.append((targets[off + k], targets[off + k])) }
                    e += 1
                }
            }
        }
        return unite(r, add)
    }

    static func table(_ name: String) -> ScalarRanges? {
        guard let (off, count) = PatternTables.sets[name] else { return nil }
        let all = PatternTables.setRanges
        var out: ScalarRanges = []
        out.reserveCapacity(count)
        for k in 0..<count { out.append((all[(off + k) * 2], all[(off + k) * 2 + 1])) }
        return out
    }

    static let word: ScalarRanges = table("perl_word") ?? []

    static func gcUnion(_ leaves: [String]) -> ScalarRanges? {
        var r: ScalarRanges = []
        for l in leaves {
            guard let t = table("gc:" + l) else { return nil }
            r = unite(r, t)
        }
        return r
    }

    static let leafCategories = [
        "Uppercase_Letter", "Lowercase_Letter", "Titlecase_Letter", "Modifier_Letter", "Other_Letter",
        "Nonspacing_Mark", "Spacing_Mark", "Enclosing_Mark", "Decimal_Number", "Letter_Number", "Other_Number",
        "Connector_Punctuation", "Dash_Punctuation", "Open_Punctuation", "Close_Punctuation", "Initial_Punctuation",
        "Final_Punctuation", "Other_Punctuation", "Math_Symbol", "Currency_Symbol", "Modifier_Symbol", "Other_Symbol",
        "Space_Separator", "Line_Separator", "Paragraph_Separator", "Control", "Format", "Private_Use",
    ]

    /// A General_Category by its canonical name (the crate's `gencat`).
    static func gencat(_ name: String) -> ScalarRanges? {
        switch name {
        case "Any": return [(0, 0xD7FF), (0xE000, maxScalar)]
        case "ASCII": return [(0, 0x7F)]
        case "Assigned": return gcUnion(leafCategories)
        case "Unassigned": return gcUnion(leafCategories).map(negate)
        case "Letter": return gcUnion(["Uppercase_Letter", "Lowercase_Letter", "Titlecase_Letter", "Modifier_Letter", "Other_Letter"])
        case "Cased_Letter": return gcUnion(["Uppercase_Letter", "Lowercase_Letter", "Titlecase_Letter"])
        case "Mark": return gcUnion(["Nonspacing_Mark", "Spacing_Mark", "Enclosing_Mark"])
        case "Number": return gcUnion(["Decimal_Number", "Letter_Number", "Other_Number"])
        case "Punctuation":
            return gcUnion(["Connector_Punctuation", "Dash_Punctuation", "Open_Punctuation", "Close_Punctuation",
                            "Initial_Punctuation", "Final_Punctuation", "Other_Punctuation"])
        case "Symbol": return gcUnion(["Math_Symbol", "Currency_Symbol", "Modifier_Symbol", "Other_Symbol"])
        case "Separator": return gcUnion(["Space_Separator", "Line_Separator", "Paragraph_Separator"])
        case "Other":
            guard let c = gcUnion(["Control", "Format", "Private_Use"]), let all = gcUnion(leafCategories) else { return nil }
            return unite(c, negate(all))
        default: return table("gc:" + name)  // a leaf, or Surrogate: no table, refused
        }
    }

    /// UAX44-LM3: no `is` prefix, no space, `_` or `-`, ASCII lower case.
    static func normalize(_ name: String) -> String {
        var u = Array(name.utf8)
        let isPrefix = u.count >= 2 && (u[0] | 0x20) == 0x69 && (u[1] | 0x20) == 0x73
        if isPrefix { u.removeFirst(2) }
        var out: [UInt8] = []
        for b in u where !(b == 0x20 || b == 0x5F || b == 0x2D || b > 0x7F) {
            out.append(b >= 0x41 && b <= 0x5A ? b + 32 : b)
        }
        // `isc` is an alias of the Other category, not `c` with the prefix dropped.
        if isPrefix && out == [0x63] { return "isc" }
        return String(decoding: out, as: UTF8.self)
    }

    /// `\pL`, `\p{Greek}`, `\p{Alphabetic}` (regex-syntax `canonical_binary`).
    static func binaryQuery(_ name: String) -> ScalarRanges? {
        let norm = normalize(name)
        if norm != "cf" && norm != "sc" && norm != "lc", let prop = PatternTables.propertyNames[norm] {
            if prop == "Decimal_Number" { return gencat("Decimal_Number") }
            return table("bool:" + prop)
        }
        switch norm {
        case "any": return gencat("Any")
        case "assigned": return gencat("Assigned")
        case "ascii": return gencat("ASCII")
        default: break
        }
        if let gc = PatternTables.generalCategories[norm] { return gencat(gc) }
        if let sc = PatternTables.scripts[norm] { return table("sc:" + sc) }
        return nil
    }

    /// `\p{name=value}`; Script_Extensions, Age and the segmentation
    /// properties carry no table here.
    static func valueQuery(_ name: String, _ value: String) -> ScalarRanges? {
        guard let prop = PatternTables.propertyNames[normalize(name)] else { return nil }
        let v = normalize(value)
        switch prop {
        case "General_Category":
            switch v {
            case "any": return gencat("Any")
            case "assigned": return gencat("Assigned")
            case "ascii": return gencat("ASCII")
            default: return PatternTables.generalCategories[v].flatMap(gencat)
            }
        case "Script": return PatternTables.scripts[v].flatMap { table("sc:" + $0) }
        default: return nil
        }
    }
}

// MARK: - syntax tree

fileprivate indirect enum PatternNode {
    case empty
    /// An index into the parser's `sets`.
    case char(Int)
    case assert(Int)
    case concat([PatternNode])
    case alt([PatternNode])
    case `repeat`(PatternNode, Int, Int?)
    case flagsOnly

    var isFlagsOnly: Bool { if case .flagsOnly = self { return true }; return false }
}

private struct PatternFlags {
    var i = false, m = false, s = false, u = true, x = false, r = false
}

private struct Refused: Error {}

// MARK: - parser (regex-syntax 0.8 rules)

private struct PatternParser {
    let p: [UInt32]
    var pos = 0
    var flags = PatternFlags()
    var names: [[UInt32]] = []
    var sets: [ScalarRanges] = []

    mutating func charNode(_ r: ScalarRanges) -> PatternNode {
        sets.append(r)
        return .char(sets.count - 1)
    }

    init(_ p: [UInt32]) { self.p = p }

    var eof: Bool { pos >= p.count }
    var c: UInt32 { p[pos] }

    mutating func parse() -> PatternNode? {
        do {
            let n = try alternation()
            if !eof { throw Refused() }  // an unopened `)`
            return n
        } catch {
            return nil
        }
    }

    static func isSpace(_ ch: UInt32) -> Bool {
        ch == 0x20 || (ch >= 0x09 && ch <= 0x0D) || ch == 0x85 || ch == 0xA0 || ch == 0x1680
            || (ch >= 0x2000 && ch <= 0x200A) || ch == 0x2028 || ch == 0x2029 || ch == 0x202F || ch == 0x205F
            || ch == 0x3000
    }

    static func isAsciiLetter(_ c: UInt32) -> Bool { (c >= 0x41 && c <= 0x5A) || (c >= 0x61 && c <= 0x7A) }

    mutating func skipSpace() {
        guard flags.x else { return }
        while !eof {
            if PatternParser.isSpace(c) {
                pos += 1
            } else if c == 0x23 {
                while !eof && c != 0x0A { pos += 1 }
            } else {
                break
            }
        }
    }

    mutating func alternation() throws -> PatternNode {
        var branches = [try concat()]
        while !eof && c == 0x7C {
            pos += 1
            branches.append(try concat())
        }
        return branches.count == 1 ? branches[0] : .alt(branches)
    }

    mutating func concat() throws -> PatternNode {
        var items: [PatternNode] = []
        while true {
            skipSpace()
            if eof { break }
            let ch = c
            if ch == 0x7C || ch == 0x29 { break }
            if ch == 0x2A || ch == 0x2B || ch == 0x3F {
                pos += 1
                try wrapLast(&items, ch == 0x2B ? 1 : 0, ch == 0x3F ? 1 : nil)
                continue
            }
            if ch == 0x7B {
                try counted(&items)
                continue
            }
            items.append(try atom())
        }
        let real = items.filter { !$0.isFlagsOnly }
        if real.isEmpty { return .empty }
        return real.count == 1 ? real[0] : .concat(real)
    }

    mutating func wrapLast(_ items: inout [PatternNode], _ min: Int, _ max: Int?) throws {
        // Repetition operator missing expression.
        guard let last = items.last, !last.isFlagsOnly else { throw Refused() }
        items[items.count - 1] = .repeat(last, min, max)
        skipSpace()
        if !eof && c == 0x3F { pos += 1 }  // lazy: the same set of matches
    }

    mutating func counted(_ items: inout [PatternNode]) throws {
        guard let last = items.last, !last.isFlagsOnly else { throw Refused() }
        pos += 1  // {
        func space(_ s: inout PatternParser) { while !s.eof && PatternParser.isSpace(s.c) { s.pos += 1 } }
        func decimal(_ s: inout PatternParser) throws -> Int {
            space(&s)
            let start = s.pos
            var v: UInt64 = 0
            while !s.eof && s.c >= 0x30 && s.c <= 0x39 {
                v = v * 10 + UInt64(s.c - 0x30)
                if v > 0xFFFF_FFFF { throw Refused() }
                s.pos += 1
            }
            if s.pos == start { throw Refused() }
            space(&s)
            return Int(v)
        }
        let min = try decimal(&self)
        var max: Int? = min
        if !eof && c == 0x2C {
            pos += 1
            space(&self)
            max = (!eof && c == 0x7D) ? nil : try decimal(&self)
        }
        if eof || c != 0x7D { throw Refused() }
        pos += 1
        if let m = max, min > m { throw Refused() }
        items[items.count - 1] = .repeat(last, min, max)
        skipSpace()
        if !eof && c == 0x3F { pos += 1 }
    }

    func literal(_ ch: UInt32) -> ScalarRanges {
        flags.i ? RangeSet.fold([(ch, ch)], unicode: flags.u) : [(ch, ch)]
    }

    mutating func atom() throws -> PatternNode {
        let ch = c
        switch ch {
        case 0x28: return try group()
        case 0x5B:
            pos += 1
            return charNode(try bracket())
        case 0x5E:
            pos += 1
            return .assert(flags.m ? (flags.r ? PatternAssert.startLineCrlf : PatternAssert.startLine) : PatternAssert.startText)
        case 0x24:
            pos += 1
            return .assert(flags.m ? (flags.r ? PatternAssert.endLineCrlf : PatternAssert.endLine) : PatternAssert.endText)
        case 0x2E:
            pos += 1
            if !flags.u { throw Refused() }  // could match invalid UTF-8
            if flags.s { return charNode(RangeSet.gencat("Any")!) }
            var nl: ScalarRanges = [(0x0A, 0x0A)]
            if flags.r { nl.append((0x0D, 0x0D)) }
            return charNode(RangeSet.negate(RangeSet.canon(nl)))
        case 0x5C:
            pos += 1
            return try escape()
        default:
            pos += 1
            return charNode(literal(ch))
        }
    }

    mutating func group() throws -> PatternNode {
        pos += 1  // (
        let saved = flags
        if !eof && c == 0x3F {
            pos += 1
            if eof { throw Refused() }
            if c == 0x50 || c == 0x3C {
                // (?P<name>…) / (?<name>…)
                if c == 0x50 {
                    pos += 1
                    if eof || c != 0x3C { throw Refused() }
                }
                pos += 1
                let start = pos
                while !eof && c != 0x3E { pos += 1 }
                if eof { throw Refused() }
                let name = Array(p[start..<pos])
                pos += 1
                if !PatternParser.validGroupName(name) || names.contains(name) { throw Refused() }
                names.append(name)
                return try groupBody(saved)
            }
            if c == 0x3A {
                pos += 1
                return try groupBody(saved)
            }
            // Flags: `(?flags)` or `(?flags:…)`.
            var next = flags
            var negate = false, any = false, dangling = false
            var seen: Set<UInt32> = []
            while true {
                if eof { throw Refused() }
                let f = c
                if f == 0x29 || f == 0x3A { break }
                pos += 1
                if f == 0x2D {
                    if negate { throw Refused() }
                    negate = true
                    dangling = true
                    continue
                }
                if !seen.insert(f).inserted { throw Refused() }
                let on = !negate
                switch f {
                case 0x69: next.i = on
                case 0x6D: next.m = on
                case 0x73: next.s = on
                case 0x55: break  // swaps greed; the same set of matches
                case 0x75: next.u = on
                case 0x78: next.x = on
                case 0x52: next.r = on
                default: throw Refused()  // lookaround, unknown flag
                }
                any = true
                dangling = false
            }
            if !any || dangling { throw Refused() }
            if c == 0x29 {
                pos += 1
                flags = next  // to the end of the enclosing group
                return .flagsOnly
            }
            pos += 1  // :
            flags = next
            return try groupBody(saved)
        }
        return try groupBody(saved)
    }

    mutating func groupBody(_ saved: PatternFlags) throws -> PatternNode {
        let inner = try alternation()
        if eof || c != 0x29 { throw Refused() }
        pos += 1
        flags = saved
        return .concat([inner])
    }

    /// regex-syntax `is_capture_char`: `_` or a letter first, then `_`, `.`,
    /// `[`, `]`, letters and digits.
    static func validGroupName(_ name: [UInt32]) -> Bool {
        if name.isEmpty { return false }
        let alpha = RangeSet.table("bool:Alphabetic") ?? []
        let number = RangeSet.gencat("Number") ?? []
        for (k, ch) in name.enumerated() {
            let ok: Bool
            if ch == 0x5F {
                ok = true
            } else if k == 0 {
                ok = RangeSet.has(alpha, ch)
            } else {
                ok = ch == 0x2E || ch == 0x5B || ch == 0x5D || RangeSet.has(alpha, ch) || RangeSet.has(number, ch)
            }
            if !ok { return false }
        }
        return true
    }

    func word(_ kind: Int) -> Int { flags.u ? kind : kind + PatternAssert.asciiOffset }

    mutating func escape() throws -> PatternNode {
        if eof { throw Refused() }
        let e = c
        switch e {
        case 0x62:  // b
            pos += 1
            // `\b{start}` and the like; `\b{2}` is a repeated boundary.
            if pos + 1 < p.count && c == 0x7B && (PatternParser.isAsciiLetter(p[pos + 1]) || p[pos + 1] == 0x2D) {
                pos += 1
                let start = pos
                while !eof && c != 0x7D { pos += 1 }
                if eof { throw Refused() }
                let name = String(decoding: p[start..<pos].map { UInt8($0 & 0x7F) }, as: UTF8.self)
                pos += 1
                switch name {
                case "start": return .assert(word(PatternAssert.wordStart))
                case "end": return .assert(word(PatternAssert.wordEnd))
                case "start-half": return .assert(word(PatternAssert.wordStartHalf))
                case "end-half": return .assert(word(PatternAssert.wordEndHalf))
                default: throw Refused()
                }
            }
            return .assert(word(PatternAssert.word))
        case 0x42: pos += 1; return .assert(word(PatternAssert.notWord))
        case 0x41: pos += 1; return .assert(PatternAssert.startText)
        case 0x7A: pos += 1; return .assert(PatternAssert.endText)
        case 0x3C: pos += 1; return .assert(word(PatternAssert.wordStart))
        case 0x3E: pos += 1; return .assert(word(PatternAssert.wordEnd))
        default: break
        }
        let (set, _) = try escapeSet(fold: true)
        return charNode(set)
    }

    /// An escape that stands for scalars, at `\` + 1, and the scalar when it
    /// stands for one. Inside brackets (`fold` false) a literal stays unfolded:
    /// the bracket folds once, at its end.
    mutating func escapeSet(fold: Bool) throws -> (ScalarRanges, UInt32?) {
        let e = c
        pos += 1
        switch e {
        case 0x64, 0x44: return (try perl(0, e == 0x44), nil)
        case 0x73, 0x53: return (try perl(1, e == 0x53), nil)
        case 0x77, 0x57: return (try perl(2, e == 0x57), nil)
        case 0x70, 0x50: return (try unicodeClass(e == 0x50), nil)
        default: break
        }
        let cp = try literalEscape(e)
        return (fold ? literal(cp) : [(cp, cp)], cp)
    }

    mutating func literalEscape(_ e: UInt32) throws -> UInt32 {
        switch e {
        case 0x6E: return 0x0A
        case 0x74: return 0x09
        case 0x72: return 0x0D
        case 0x66: return 0x0C
        case 0x76: return 0x0B
        case 0x61: return 0x07
        case 0x78: return try hexEscape(2)
        case 0x75: return try hexEscape(4)
        case 0x55: return try hexEscape(8)
        default: break
        }
        // Any ASCII character that is not a letter or a digit may be escaped.
        if e < 0x80 && !(e >= 0x30 && e <= 0x39) && !PatternParser.isAsciiLetter(e) { return e }
        throw Refused()
    }

    static func hexDigit(_ ch: UInt32) -> UInt32? {
        if ch >= 0x30 && ch <= 0x39 { return ch - 0x30 }
        if ch >= 0x61 && ch <= 0x66 { return ch - 0x61 + 10 }
        if ch >= 0x41 && ch <= 0x46 { return ch - 0x41 + 10 }
        return nil
    }

    mutating func hexEscape(_ digits: Int) throws -> UInt32 {
        var v: UInt32 = 0
        if !eof && c == 0x7B {
            pos += 1
            let start = pos
            while !eof && c != 0x7D {
                guard let d = PatternParser.hexDigit(c) else { throw Refused() }
                v = v * 16 + d
                if v > RangeSet.maxScalar { throw Refused() }
                pos += 1
            }
            if eof || pos == start { throw Refused() }
            pos += 1
        } else {
            for _ in 0..<digits {
                if eof { throw Refused() }
                guard let d = PatternParser.hexDigit(c) else { throw Refused() }
                v = v * 16 + d
                pos += 1
            }
        }
        if v > RangeSet.maxScalar || (v >= 0xD800 && v <= 0xDFFF) { throw Refused() }
        // Without Unicode a byte past ASCII is not a character.
        if !flags.u && v > 0x7F { throw Refused() }
        return v
    }

    /// `\d` (0), `\s` (1), `\w` (2), negated when `neg`.
    func perl(_ kind: Int, _ neg: Bool) throws -> ScalarRanges {
        var r: ScalarRanges
        if flags.u {
            switch kind {
            case 0: guard let d = RangeSet.gencat("Decimal_Number") else { throw Refused() }; r = d
            case 1: guard let s = RangeSet.table("bool:White_Space") else { throw Refused() }; r = s
            default: r = RangeSet.word
            }
        } else {
            switch kind {
            case 0: r = [(0x30, 0x39)]
            case 1: r = [(0x09, 0x0D), (0x20, 0x20)]
            default: r = [(0x30, 0x39), (0x41, 0x5A), (0x5F, 0x5F), (0x61, 0x7A)]
            }
        }
        if neg {
            // A negated class without Unicode could match invalid UTF-8.
            if !flags.u { throw Refused() }
            return RangeSet.negate(r)
        }
        return r
    }

    /// `\p…` / `\P…`, at the character after the letter; folded, then negated.
    mutating func unicodeClass(_ negated: Bool) throws -> ScalarRanges {
        if !flags.u || eof { throw Refused() }
        var neg = negated
        var found: ScalarRanges?
        if c == 0x7B {
            pos += 1
            let start = pos
            while !eof && c != 0x7D { pos += 1 }
            if eof { throw Refused() }
            let body = String(String.UnicodeScalarView(p[start..<pos].compactMap(Unicode.Scalar.init)))
            pos += 1
            if let r = body.range(of: "!=") {
                neg.toggle()
                found = RangeSet.valueQuery(String(body[..<r.lowerBound]), String(body[r.upperBound...]))
            } else if let r = body.range(of: ":") {
                found = RangeSet.valueQuery(String(body[..<r.lowerBound]), String(body[r.upperBound...]))
            } else if let r = body.range(of: "=") {
                found = RangeSet.valueQuery(String(body[..<r.lowerBound]), String(body[r.upperBound...]))
            } else {
                found = RangeSet.binaryQuery(body)
            }
        } else {
            if c == 0x5C { throw Refused() }
            let one = Unicode.Scalar(c).map { String($0) } ?? ""
            pos += 1
            found = RangeSet.binaryQuery(one)
        }
        guard var r = found else { throw Refused() }
        if flags.i { r = RangeSet.fold(r, unicode: true) }
        return neg ? RangeSet.negate(r) : r
    }

    // MARK: bracketed classes

    mutating func classSpace() {
        guard flags.x else { return }
        while !eof && PatternParser.isSpace(c) { pos += 1 }
    }

    var atOp: Bool {
        guard pos + 1 < p.count else { return false }
        let a = p[pos], b = p[pos + 1]
        return (a == 0x26 && b == 0x26) || (a == 0x2D && b == 0x2D) || (a == 0x7E && b == 0x7E)
    }

    /// `[…]`, after the `[`. Items are collected as written; the class is
    /// case-folded once, at its end, then negated (regex-syntax order).
    mutating func bracket() throws -> ScalarRanges {
        var negated = false
        if !eof && c == 0x5E {
            pos += 1
            negated = true
        }
        var set = try classOps()
        if eof || c != 0x5D { throw Refused() }
        pos += 1
        if flags.i { set = RangeSet.fold(set, unicode: flags.u) }
        if negated {
            if !flags.u { throw Refused() }  // could match invalid UTF-8
            return RangeSet.negate(set)
        }
        return set
    }

    /// Unions joined by `&&`, `--` and `~~`, left to right; both sides of an
    /// operator are case-folded before it applies.
    mutating func classOps() throws -> ScalarRanges {
        var acc = try classUnion(first: true)
        while true {
            classSpace()
            if !atOp { return acc }
            let a = c
            pos += 2
            var rhs = try classUnion(first: false)
            if flags.i {
                acc = RangeSet.fold(acc, unicode: flags.u)
                rhs = RangeSet.fold(rhs, unicode: flags.u)
            }
            acc = a == 0x26 ? RangeSet.intersect(acc, rhs) : (a == 0x2D ? RangeSet.minus(acc, rhs) : RangeSet.symdiff(acc, rhs))
        }
    }

    /// One literal item for a range end, as written; nil for a class escape.
    mutating func classLiteral() throws -> UInt32? {
        if c == 0x5C {
            pos += 1
            if eof { throw Refused() }
            let e = c
            if [0x64, 0x44, 0x73, 0x53, 0x77, 0x57, 0x70, 0x50].contains(e) { return nil }
            if [0x62, 0x42, 0x41, 0x7A, 0x3C, 0x3E].contains(e) { throw Refused() }  // assertions
            pos += 1
            return try literalEscape(e)
        }
        let ch = c
        pos += 1
        return ch
    }

    mutating func classUnion(first: Bool) throws -> ScalarRanges {
        var parts: ScalarRanges = []
        var leading = first
        while true {
            classSpace()
            if eof { throw Refused() }
            if c == 0x5D && !leading { break }
            // A `-` that opens the class is itself; any other operator may stand
            // first, with an empty left side.
            if leading && c == 0x2D {
                pos += 1
                parts.append((0x2D, 0x2D))
                leading = false
                continue
            }
            if atOp { break }
            leading = false
            if c == 0x5B {
                if let ascii = try asciiClass() {
                    parts = RangeSet.unite(parts, ascii)
                    continue
                }
                pos += 1
                parts = RangeSet.unite(parts, try bracket())
                continue
            }
            let before = pos
            guard let lo = try classLiteral() else {
                // `\d`, `\p{…}`: a class, which cannot start a range.
                pos = before + 1
                let (set, _) = try escapeSet(fold: false)
                classSpace()
                if !eof && c == 0x2D && pos + 1 < p.count && p[pos + 1] != 0x5D && p[pos + 1] != 0x2D { throw Refused() }
                parts = RangeSet.unite(parts, set)
                continue
            }
            classSpace()
            if !eof && c == 0x2D && pos + 1 < p.count && p[pos + 1] != 0x5D && p[pos + 1] != 0x2D {
                pos += 1
                classSpace()
                if eof || c == 0x5B { throw Refused() }
                guard let hi = try classLiteral() else { throw Refused() }  // a class escape cannot end a range
                if lo > hi { throw Refused() }
                parts.append((lo, hi))
            } else {
                parts.append((lo, lo))
            }
        }
        return RangeSet.canon(parts)
    }

    /// `[:name:]` / `[:^name:]` at a `[`, or nil (nothing read) when the text
    /// there is not one — then it is a nested class.
    mutating func asciiClass() throws -> ScalarRanges? {
        guard pos + 1 < p.count, p[pos + 1] == 0x3A else { return nil }
        var k = pos + 2
        var neg = false
        if k < p.count && p[k] == 0x5E {
            neg = true
            k += 1
        }
        let start = k
        while k < p.count && p[k] != 0x3A { k += 1 }
        guard k + 1 < p.count, p[k + 1] == 0x5D else { return nil }
        guard p[start..<k].allSatisfy({ $0 < 0x80 }) else { return nil }
        let name = String(decoding: p[start..<k].map { UInt8($0) }, as: UTF8.self)
        var r: ScalarRanges
        switch name {
        case "alnum": r = [(0x30, 0x39), (0x41, 0x5A), (0x61, 0x7A)]
        case "alpha": r = [(0x41, 0x5A), (0x61, 0x7A)]
        case "ascii": r = [(0, 0x7F)]
        case "blank": r = [(0x09, 0x09), (0x20, 0x20)]
        case "cntrl": r = [(0, 0x1F), (0x7F, 0x7F)]
        case "digit": r = [(0x30, 0x39)]
        case "graph": r = [(0x21, 0x7E)]
        case "lower": r = [(0x61, 0x7A)]
        case "print": r = [(0x20, 0x7E)]
        case "punct": r = [(0x21, 0x2F), (0x3A, 0x40), (0x5B, 0x60), (0x7B, 0x7E)]
        case "space": r = [(0x09, 0x0D), (0x20, 0x20)]
        case "upper": r = [(0x41, 0x5A)]
        case "word": r = [(0x30, 0x39), (0x41, 0x5A), (0x5F, 0x5F), (0x61, 0x7A)]
        case "xdigit": r = [(0x30, 0x39), (0x41, 0x46), (0x61, 0x66)]
        default: return nil
        }
        pos = k + 2
        if flags.i { r = RangeSet.fold(r, unicode: flags.u) }
        if neg {
            if !flags.u { throw Refused() }
            r = RangeSet.negate(r)
        }
        return r
    }
}

// MARK: - compiler

private struct PatternBuilder {
    let out: SynxPattern

    @discardableResult
    func emit(_ op: SynxPattern.Op, _ a: Int = 0, _ b: Int = 0) -> Int? {
        if out.prog.count >= SynxPattern.maxProgram { return nil }
        out.prog.append(SynxPattern.Inst(op: op, a: a, b: b))
        return out.prog.count - 1
    }

    var here: Int { out.prog.count }

    /// False when the program outgrows its limit.
    mutating func node(_ n: PatternNode) -> Bool {
        switch n {
        case .empty, .flagsOnly:
            return true
        case .char(let id):
            return emit(.char, id) != nil
        case .assert(let k):
            return emit(.assert, k) != nil
        case .concat(let items):
            for i in items where !node(i) { return false }
            return true
        case .alt(let bs):
            var jumps: [Int] = []
            for (k, b) in bs.enumerated() {
                if k + 1 < bs.count {
                    guard let split = emit(.split) else { return false }
                    out.prog[split].a = here
                    if !node(b) { return false }
                    guard let j = emit(.jmp) else { return false }
                    jumps.append(j)
                    out.prog[split].b = here
                } else if !node(b) {
                    return false
                }
            }
            for j in jumps { out.prog[j].a = here }
            return true
        case .repeat(let inner, let min, let max):
            for _ in 0..<min where !node(inner) { return false }
            if let max = max {
                var exits: [Int] = []
                var k = min
                while k < max {
                    guard let split = emit(.split) else { return false }
                    out.prog[split].a = here
                    exits.append(split)
                    if !node(inner) { return false }
                    k += 1
                }
                for s in exits { out.prog[s].b = here }
            } else {
                guard let split = emit(.split) else { return false }
                out.prog[split].a = here
                if !node(inner) { return false }
                guard emit(.jmp, split) != nil else { return false }
                out.prog[split].b = here
            }
            return true
        }
    }
}
