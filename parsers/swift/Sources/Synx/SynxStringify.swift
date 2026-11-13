// Value → SYNX text. Port of `serialize` in synx-core/src/lib.rs, which writes
// through the SYNXL §14.1 block writer (`synxl::write_synx_entry`): strings that
// would cast or be cut as a comment are quoted or carried as `|+`, list objects
// lead with a key that fits the `- ` line — the text reads back as the same data.

public enum SynxStringify {

    public static let maxDepth = 128

    public static func stringify(_ value: SynxValue) -> String {
        guard case .object(let map) = value else { return formatPrimitive(value) }
        var w = Writer()
        let names = KeyNames(map)
        for e in sortedEntries(map) { w.entry(names.name(e.key), e.value, 0, 0) }
        return String(decoding: w.out, as: UTF8.self)
    }

    /// A non-object value as Rust's `format_primitive` writes it.
    public static func formatPrimitive(_ v: SynxValue) -> String {
        switch v {
        case .string(let s): return s
        case .int(let n): return String(n)
        case .float(let f): return SynxJSON.displayString(f)
        case .bool(let b): return b ? "true" : "false"
        case .null: return "null"
        case .array(let arr): return "[" + arr.map { formatPrimitive($0) }.joined(separator: ", ") + "]"
        case .object: return "[Object]"
        case .secret: return "[SECRET]"
        }
    }

    static func sortedEntries(_ map: SynxObject) -> [(key: String, value: SynxValue)] {
        map.entries.sorted { $0.key.bytesLess($1.key) }
    }
}

/// The names the keys of one object are written under: a key that reads back as
/// itself keeps its name; any other has every character that would end it or
/// start something else written as `_`, and two keys never share a name — one
/// already taken gets a number (`a_b`, `a_b_2`, …, in the order of the keys).
///
/// safety: SYNX has no quoting for keys, and a key was written as it came.
/// `port:env` gave the next reader a marker, `!active` on a line of its own
/// switched the document to active mode, a line break began new lines — a map
/// with caller-chosen keys, saved and loaded, could read the environment.
struct KeyNames {
    private var renamed: [String: String] = [:]

    init(_ map: SynxObject) {
        let keys = map.entries.map { $0.key }
        if keys.allSatisfy(KeyNames.readsBack) { return }
        let present = Set(keys)
        var taken = Set<String>()
        var next: [String: Int] = [:]
        for key in keys where !KeyNames.readsBack(key) {
            let base = KeyNames.writable(key)
            var name = base
            while present.contains(name) || taken.contains(name) {
                let n = next[base] ?? 2
                name = base + "_" + String(n)
                next[base] = n + 1
            }
            taken.insert(name)
            renamed[key] = name
        }
    }

    func name(_ key: String) -> String { renamed[key] ?? key }

    static func isReserved(_ k: String) -> Bool { k == "__proto__" || k == "constructor" || k == "prototype" }

    static func isControl(_ c: UInt32) -> Bool { c < 0x20 || (c >= 0x7F && c <= 0x9F) }

    /// Whether `key`, written at the start of a line, reads back as that key and nothing else.
    static func readsBack(_ key: String) -> Bool {
        guard let first = key.utf8.first else { return false }
        if [0x2D, 0x23, 0x2F, 0x5B, 0x3A, 0x28].contains(first) { return false }
        // Lines the parser takes for directives, and keys it drops with what hangs under them.
        if ["!active", "!lock", "!tool", "!schema", "!llm", "!include", "!use"].contains(key) || isReserved(key) { return false }
        // Whitespace at an edge is trimmed away with the line; inside, only a space
        // or a tab ends a key (U+00A0 is a letter like any other).
        if key.rustTrimmed.utf8.count != key.utf8.count { return false }
        return !key.unicodeScalars.contains { isControl($0.value) || $0 == " " || $0 == "[" || $0 == ":" || $0 == "(" }
    }

    static func writable(_ key: String) -> String {
        let scalars = Array(key.unicodeScalars)
        var out = String.UnicodeScalarView()
        for (i, c) in scalars.enumerated() {
            let edge = i == 0 || i == scalars.count - 1
            let breaks = isControl(c.value) || c == " " || c == "[" || c == ":" || c == "("
                || (edge && String(c).rustTrimmed.isEmpty)
                || (i == 0 && (c == "-" || c == "#" || c == "/" || c == "!"))
            out.append(breaks ? "_" : c)
        }
        var name = out.isEmpty ? "_" : String(out)
        // safety: `constructor`, `prototype` and `__proto__` are keys the reader
        // refuses with what hangs under them; such a key gets a `_` after it.
        if isReserved(name) { name += "_" }
        return name
    }
}

// MARK: - §14 writer

private struct Writer {
    var out: [UInt8] = []

    init() { out.reserveCapacity(1024) }

    mutating func indent(_ n: Int) { for _ in 0..<n { out.append(0x20) } }
    mutating func put(_ s: String) { out.append(contentsOf: s.utf8) }

    mutating func entry(_ key: String, _ value: SynxValue, _ ind: Int, _ depth: Int) {
        if depth > SynxParser.maxNestingDepth { return }
        switch value {
        case .object(let map):
            indent(ind)
            put(key)
            out.append(0x0A)
            let names = KeyNames(map)
            for e in SynxStringify.sortedEntries(map) { entry(names.name(e.key), e.value, ind + 2, depth + 1) }
        case .array(let items):
            indent(ind)
            put(key)
            // A bare key reads back as `{}`; `:join` is the SYNX surface of `[]`.
            if items.isEmpty { put(":join") }
            out.append(0x0A)
            for item in items { self.item(item, ind + 2, depth + 1) }
        case .string(let s) where s.utf8.contains(0x0A):
            multiline(key, s, ind)
        default:
            if let text = Writer.scalar(value) {
                indent(ind)
                put(key)
                out.append(0x20)
                put(text)
                out.append(0x0A)
            } else {
                // No single token carries it: an indent-preserving block.
                multiline(key, Writer.strOf(value), ind)
            }
        }
    }

    mutating func item(_ item: SynxValue, _ ind: Int, _ depth: Int) {
        if depth > SynxParser.maxNestingDepth { return }
        guard case .object(let map) = item else {
            indent(ind)
            put("- ")
            // safety: a list item is one line and SYNX has no block form for it,
            // so a line break is written as a space; written as it was, the rest
            // of the text became lines of their own.
            put(Writer.scalar(item) ?? String(decoding: Writer.strOf(item).utf8.map { $0 == 0x0A || $0 == 0x0D ? 0x20 : $0 }, as: UTF8.self))
            out.append(0x0A)
            return
        }
        let keys = SynxStringify.sortedEntries(map)
        // An empty object in a list has no key for the `- ` line: a bare dash.
        if keys.isEmpty {
            indent(ind)
            put("-\n")
            return
        }
        // The `- ` line carries a key whose value fits on it; a one-key item, or
        // one with no head that fits, writes its head again in full below.
        let headSafe = keys.firstIndex { Writer.dashLineSafe($0.value) }
        let head = headSafe ?? 0
        let repeatHead = headSafe == nil || keys.count == 1
        let names = KeyNames(map)
        indent(ind)
        put("- ")
        put(names.name(keys[head].key))
        if let text = Writer.scalar(keys[head].value), !text.isEmpty {
            out.append(0x20)
            put(text)
        }
        out.append(0x0A)
        for (i, e) in keys.enumerated() where !(i == head && !repeatHead) {
            entry(names.name(e.key), e.value, ind + 2, depth + 1)
        }
    }

    /// `key |+` plus an indented body.
    mutating func multiline(_ key: String, _ body: String, _ ind: Int) {
        indent(ind)
        put(key)
        put(" |+\n")
        indent(ind + 2)
        for c in body.utf8 {
            out.append(c)
            if c == 0x0A { indent(ind + 2) }
        }
        out.append(0x0A)
    }

    // MARK: scalars

    static func strOf(_ v: SynxValue) -> String {
        switch v {
        case .string(let s), .secret(let s): return s
        default: return ""
        }
    }

    /// May this value share the `- ` line of a list item?
    static func dashLineSafe(_ v: SynxValue) -> Bool {
        switch v {
        case .object(let o): return o.isEmpty
        case .array: return false
        case .string(let s): return !s.utf8.contains(0x0A) && scalar(v) != nil && !s.isEmpty
        default: return true
        }
    }

    /// A scalar as one SYNX value token, or nil when no token can carry it. A
    /// value is comment-stripped at ` #` / ` //` before casting, so a value
    /// holding either cannot be rescued by quoting.
    static func scalar(_ v: SynxValue) -> String? {
        switch v {
        case .null: return "null"
        case .bool(let b): return b ? "true" : "false"
        case .int(let n): return String(n)
        case .float(let f): return f.isFinite ? SynxJSON.displayString(f) : "null"
        // safety: a secret is written as its mask, never as the plaintext.
        case .secret: return "[SECRET]"
        case .object, .array: return nil
        case .string(var s):
            let verdict: Verdict = s.withUTF8 { classify($0) }
            switch verdict {
            case .block: return nil
            case .bare: return s
            case .quote:
                if !s.utf8.contains(0x22) { return "\"" + s + "\"" }
                if !s.utf8.contains(0x27) { return "'" + s + "'" }
                return nil
            }
        }
    }

    enum Verdict { case bare, quote, block }

    /// One pass over a string: does it need a block, quotes, or neither?
    static func classify(_ b: UnsafeBufferPointer<UInt8>) -> Verdict {
        let n = b.count
        if n == 0 { return .quote }
        var i = 0
        while i < n {
            let c = b[i]
            if c == 0x0A { return .block }
            if c == 0x20 && i + 1 < n {
                if b[i + 1] == 0x23 { return .block }                                  // " #"
                if b[i + 1] == 0x2F && i + 2 < n && b[i + 2] == 0x2F { return .block }  // " //"
            }
            i += 1
        }
        // Unicode whitespace at either edge would be trimmed on read.
        if wsLen(b, 0, n) != 0 || wsLenBefore(b, 0, n) != 0 { return .quote }
        if n == 1 && b[0] == 0x7C { return .quote }                    // |
        if n == 2 && b[0] == 0x7C && b[1] == 0x2B { return .quote }    // |+
        let wrapped = n >= 2 && ((b[0] == 0x22 && b[n - 1] == 0x22) || (b[0] == 0x27 && b[n - 1] == 0x27))
        if wrapped { return .quote }
        return autoCastKeepsString(b) ? .bare : .quote
    }

    /// Does the automatic cast give back the string itself?
    static func autoCastKeepsString(_ b: UnsafeBufferPointer<UInt8>) -> Bool {
        let n = b.count
        if n == 4 && b.elementsEqual("true".utf8) { return false }
        if n == 5 && b.elementsEqual("false".utf8) { return false }
        if n == 4 && b.elementsEqual("null".utf8) { return false }
        let start = (n > 0 && b[0] == 0x2D) ? 1 : 0
        if n <= start || !(b[start] >= 0x30 && b[start] <= 0x39) { return true }
        var dot = -1
        for j in start..<n {
            let c = b[j]
            if c == 0x2E {
                if dot >= 0 { return true }
                dot = j
            } else if c < 0x30 || c > 0x39 {
                return true
            }
        }
        if dot < 0 { return RustNumber.parseI64(b) == nil }
        return !(dot > start && dot < n - 1 && RustNumber.parseF64(b) != nil)
    }
}
