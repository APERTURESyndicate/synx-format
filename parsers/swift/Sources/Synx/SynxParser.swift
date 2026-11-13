// SYNX text-to-tree parser. Port of crates/synx-core/src/parser.rs.
import Foundation

public enum SynxParser {

    // MARK: - Resource caps (fuzz / hostile input)
    public static let maxInputBytes      = 16 * 1024 * 1024
    public static let maxLineStarts      = 2_000_000
    public static let maxNestingDepth    = 128
    public static let maxMultilineBytes  = 1024 * 1024
    public static let maxListItems       = 1 << 20
    public static let maxIncludes        = 4096
    public static let maxEnumParts       = 4096
    public static let maxMarkerSegments  = 512

    /// Drop a single leading U+FEFF (SYNX §2.1; §6 matches directives with a
    /// leading BOM ignored): it is not whitespace, and left in place it would
    /// turn `!active` into a key.
    static func stripBOM(_ text: String) -> String {
        guard text.unicodeScalars.first == "\u{FEFF}" else { return text }
        return String(text.unicodeScalars.dropFirst())
    }

    /// Truncate to a UTF-8-safe prefix of at most `maxInputBytes`.
    public static func clamp(_ text: String) -> String {
        let s = stripBOM(text)
        if s.utf8.count <= maxInputBytes { return s }
        return utf8Prefix(s, maxInputBytes)
    }

    /// Parse SYNX text into a `SynxParseResult`.
    public static func parse(_ rawText: String) -> SynxParseResult {
        let (root, result) = parseTree(rawText)
        var r = result
        r.root = .object(root.toObject())
        return r
    }

    /// Parse into the working tree (the engine resolves it in place).
    static func parseTree(_ rawText: String) -> (MObject, SynxParseResult) {
        var bytes = Array(clamp(rawText).utf8)
        // A leading U+FEFF is dropped (§2.1), not lost content: it does not count.
        let original = rawText.utf8.count - (rawText.unicodeScalars.first == "\u{FEFF}" ? 3 : 0)
        // Line cap: keep at most maxLineStarts - 1 newlines.
        var seen = 0
        for i in 0..<bytes.count where bytes[i] == 0x0A {
            if seen >= maxLineStarts - 1 {
                bytes.removeSubrange(i...)
                break
            }
            seen += 1
        }
        var result = SynxParseResult()
        // Either cap having bitten means input was dropped before parsing began.
        result.truncated = bytes.count < original
        let root = MObject()
        bytes.withUnsafeBufferPointer { buf in
            var p = Parser(buf, root, result)
            p.run()
            result = p.result
        }
        return (root, result)
    }

    /// Reshape a parsed tree for `!tool` mode: `{ tool, params }`, or
    /// `{ tools: [{ name, params }] }` with `!schema`. Keys in code-point order.
    public static func reshapeToolOutput(_ root: SynxValue, schema: Bool) -> SynxValue {
        guard case .object(let map) = root else { return root }
        let sorted = map.entries.sorted { $0.key.bytesLess($1.key) }
        if schema {
            var tools: [SynxValue] = []
            for e in sorted {
                var def = SynxObject()
                def.set("name", to: .string(e.key))
                def.set("params", to: e.value)
                tools.append(.object(def))
            }
            var out = SynxObject()
            out.set("tools", to: .array(tools))
            return .object(out)
        }
        var out = SynxObject()
        guard let first = sorted.first else {
            out.set("tool", to: .null)
            out.set("params", to: .object(SynxObject()))
            return .object(out)
        }
        out.set("tool", to: .string(first.key))
        if case .object = first.value {
            out.set("params", to: first.value)
        } else {
            out.set("params", to: .object(SynxObject()))
        }
        return .object(out)
    }
}

// MARK: - casting and constraints (shared with the engine and the writer)

enum SynxCast {

    /// SYNX §8.3 automatic casting (Rust `parser::cast`).
    static func cast<C: Collection>(_ v: C) -> MValue where C.Element == UInt8, C.Index == Int {
        let n = v.count
        let s = v.startIndex
        if n >= 2 {
            let f = v[s], l = v[s + n - 1]
            if (f == 0x22 && l == 0x22) || (f == 0x27 && l == 0x27) {
                return .string(String(decoding: v[(s + 1)..<(s + n - 1)], as: UTF8.self))
            }
        }
        if v.elementsEqual("true".utf8) { return .bool(true) }
        if v.elementsEqual("false".utf8) { return .bool(false) }
        if v.elementsEqual("null".utf8) { return .null }
        if n == 0 { return .string("") }
        var start = s
        if v[s] == 0x2D {
            if n == 1 { return .string("-") }
            start = s + 1
        }
        if v[start] >= 0x30 && v[start] <= 0x39 {
            var dot = -1
            var numeric = true
            var j = start
            while j < s + n {
                let c = v[j]
                if c == 0x2E {
                    if dot >= 0 { numeric = false; break }
                    dot = j
                } else if c < 0x30 || c > 0x39 {
                    numeric = false
                    break
                }
                j += 1
            }
            if numeric {
                if dot >= 0 {
                    if dot > start && dot < s + n - 1, let f = RustNumber.parseF64(v) { return .float(f) }
                } else if let i = RustNumber.parseI64(v) {
                    return .int(i)
                }
            }
        }
        return .string(String(decoding: v, as: UTF8.self))
    }

    static func cast(_ s: String) -> MValue { cast(Array(s.utf8)) }

    /// SYNX §8.3 typed casting (Rust `parser::cast_typed_with`, hints enabled).
    static func castTyped(_ v: ArraySlice<UInt8>, hint: String) -> MValue {
        switch hint {
        case "int": return .int(RustNumber.parseI64(v) ?? 0)
        case "float": return .float(RustNumber.parseF64(v) ?? 0)
        case "bool": return .bool(String(decoding: v, as: UTF8.self).rustTrimmed == "true")
        case "string": return .string(String(decoding: v, as: UTF8.self))
        case "random", "random:int": return .int(SynxRng.randomI64())
        case "random:float": return .float(SynxRng.randomF64())
        case "random:bool": return .bool(SynxRng.randomBool())
        default: return cast(v)
        }
    }

    /// The body of a `[…]` constraint block (Rust `parse_constraints`).
    static func parseConstraints(_ raw: String) -> SynxConstraints {
        var c = SynxConstraints()
        for partRaw in splitBytes(raw, 0x2C) {
            let part = partRaw.rustTrimmed
            if part.isEmpty { continue }
            if part == "required" {
                c.required = true
            } else if part == "readonly" {
                c.readonly = true
            } else if let (kRaw, vRaw) = splitOnce(part, 0x3A) {
                let k = kRaw.rustTrimmed
                let v = vRaw.rustTrimmed
                switch k {
                // A bound is a finite number: `inf` and `nan` bound nothing
                // sensible (`[max:-inf]` rejected every value).
                case "min": c.min = RustNumber.parseF64(v).flatMap { $0.isFinite ? $0 : nil }
                case "max": c.max = RustNumber.parseF64(v).flatMap { $0.isFinite ? $0 : nil }
                case "type": c.typeName = v
                case "pattern": c.pattern = v
                case "enum": c.enumValues = Array(splitBytes(v, 0x7C).prefix(SynxParser.maxEnumParts))
                default: break
                }
            }
        }
        return c
    }
}

// MARK: - the line parser

/// One key line, as byte ranges of the source.
private struct ParsedLine {
    var key: Range<Int> = 0..<0
    var typeHint: Range<Int>? = nil
    var value: Range<Int> = 0..<0
    var markers: [Range<Int>] = []
    var markerArgs: [String] = []
    var constraints: SynxConstraints? = nil
}

private enum FrameKind { case root, key, listItem }

private struct Frame {
    var indent: Int
    var kind: FrameKind
    var key: String = ""        // Key: its key; ListItem: the list's key
    var itemIdx: Int = 0
    var parent: Int = 0         // ListItem: stack index of the frame holding the list
    var obj: MObject            // the object this frame inserts into
}

private struct BlockState {
    var indent: Int
    var key: String
    var content: [UInt8]
    var stackIdx: Int
    var preserveIndent: Bool
    var baseIndent: Int
}

private struct ListState {
    var indent: Int
    var key: String
    var stackIdx: Int
}

private struct Parser {
    let b: UnsafeBufferPointer<UInt8>
    var result: SynxParseResult
    var stack: [Frame]
    var block: BlockState? = nil
    /// Open lists, outermost first.
    ///
    /// spec: a list inside a list item ends at the next item of the outer list
    /// (§8.5). One variable held "the" open list, so a nested `tags` replaced its
    /// parent: `- name b` after it matched no list and was dropped without a
    /// word, and the keys under it landed in the item before.
    var lists: [ListState] = []
    var metaPath = ""
    var metaPathValid = false

    init(_ b: UnsafeBufferPointer<UInt8>, _ root: MObject, _ result: SynxParseResult) {
        self.b = b
        self.result = result
        self.stack = [Frame(indent: -1, kind: .root, obj: root)]
        stack.reserveCapacity(16)
    }

    func str(_ r: Range<Int>) -> String { String(decoding: UnsafeBufferPointer(rebasing: b[r]), as: UTF8.self) }
    func eq(_ r: Range<Int>, _ s: StaticString) -> Bool {
        let n = s.utf8CodeUnitCount
        if r.count != n { return false }
        let p = s.utf8Start
        for k in 0..<n where b[r.lowerBound + k] != p[k] { return false }
        return true
    }
    func startsWith(_ r: Range<Int>, _ s: StaticString) -> Bool {
        let n = s.utf8CodeUnitCount
        if r.count < n { return false }
        let p = s.utf8Start
        for k in 0..<n where b[r.lowerBound + k] != p[k] { return false }
        return true
    }
    func find(_ r: Range<Int>, _ c: UInt8) -> Int? {
        var i = r.lowerBound
        while i < r.upperBound { if b[i] == c { return i }; i += 1 }
        return nil
    }
    func findSeq(_ r: Range<Int>, _ s: StaticString) -> Int? {
        let n = s.utf8CodeUnitCount
        let p = s.utf8Start
        var i = r.lowerBound
        while i + n <= r.upperBound {
            var k = 0
            while k < n && b[i + k] == p[k] { k += 1 }
            if k == n { return i }
            i += 1
        }
        return nil
    }

    /// Cut an inline ` //` or ` #` comment and trailing whitespace.
    func stripComment(_ r: Range<Int>) -> Range<Int> {
        var e = r.upperBound
        if find(r, 0x20) != nil {
            if let p = findSeq(r.lowerBound..<e, " //") { e = p }
            if let p = findSeq(r.lowerBound..<e, " #") { e = p }
        }
        return r.lowerBound..<trimEndRange(b, r.lowerBound, e)
    }

    func parseLine(_ t: Range<Int>) -> ParsedLine? {
        if t.isEmpty || b[t.lowerBound] == 0x23 || startsWith(t, "//") || startsWith(t, "- ") { return nil }
        let first = b[t.lowerBound]
        // [ : - # / (
        if first == 0x5B || first == 0x3A || first == 0x2D || first == 0x23 || first == 0x2F || first == 0x28 { return nil }
        let len = t.upperBound
        var pos = t.lowerBound
        while pos < len {
            let ch = b[pos]
            if ch == 0x20 || ch == 0x09 || ch == 0x5B || ch == 0x3A || ch == 0x28 { break }
            pos += 1
        }
        var out = ParsedLine()
        out.key = t.lowerBound..<pos
        // Optional `(type)`.
        if pos < len && b[pos] == 0x28 {
            let start = pos + 1
            if let close = find(start..<len, 0x29) {
                out.typeHint = start..<close
                pos = close + 1
            } else {
                pos += 1
            }
        }
        // Optional `[constraints]` — balanced, so `^[A-Z]{2}$` keeps its own `]`.
        if pos < len && b[pos] == 0x5B {
            let cstart = pos + 1
            var depth = 1
            var scan = cstart
            while scan < len && depth > 0 {
                if b[scan] == 0x5B { depth += 1 }
                else if b[scan] == 0x5D { depth -= 1; if depth == 0 { break } }
                scan += 1
            }
            if depth == 0 {
                out.constraints = SynxCast.parseConstraints(str(cstart..<scan))
                pos = scan + 1
            } else if let rel = find(cstart..<len, 0x5D) {
                out.constraints = SynxCast.parseConstraints(str(cstart..<rel))
                pos = rel + 1
            } else {
                out.constraints = SynxCast.parseConstraints(str(cstart..<len))
                pos = len
            }
        }
        // Optional `:markers`.
        if pos < len && b[pos] == 0x3A {
            let mstart = pos + 1
            var mend = mstart
            while mend < len && b[mend] != 0x20 && b[mend] != 0x09 { mend += 1 }
            var p = mstart
            while out.markers.count < SynxParser.maxMarkerSegments {
                if let colon = find(p..<mend, 0x3A) {
                    out.markers.append(p..<colon)
                    p = colon + 1
                } else {
                    out.markers.append(p..<mend)
                    break
                }
            }
            pos = mend
        }
        while pos < len && (b[pos] == 0x20 || b[pos] == 0x09) { pos += 1 }
        out.value = pos < len ? stripComment(pos..<len) : len..<len
        if !out.markers.isEmpty && !out.value.isEmpty {
            // `:random` — the value carries the weight percentages.
            if out.markers.contains(where: { eq($0, "random") }) {
                var nums: [String] = []
                var i = out.value.lowerBound
                let e = out.value.upperBound
                while i < e {
                    let n = wsLen(b, i, e)
                    if n != 0 { i += n; continue }
                    let s = i
                    while i < e && wsLen(b, i, e) == 0 { i += utf8Len(b[i]) }
                    // spec: a weight is `digits[.digits]`. Anything a float parser
                    // read counted, so `a:random inf`, `-5` or `nan` lost its value.
                    if Parser.isWeight(b[s..<i]) { nums.append(str(s..<i)) }
                }
                if !nums.isEmpty {
                    out.markerArgs = nums
                    out.value = out.value.upperBound..<out.value.upperBound
                }
            }
            // `:inherit` — a non-empty value names the parent: the line opens a group.
            if !out.value.isEmpty && out.markers.contains(where: { eq($0, "inherit") }) {
                out.markerArgs = [str(trimRange(b, out.value.lowerBound, out.value.upperBound))]
                out.value = out.value.upperBound..<out.value.upperBound
            }
        }
        return out
    }

    // MARK: tree

    mutating func popFrame() {
        stack.removeLast()
        metaPathValid = false
    }

    mutating func pushFrame(_ f: Frame) {
        stack.append(f)
        metaPathValid = false
    }

    /// The frames from the root to `target` (root excluded), in descending order.
    /// A ListItem's parent is the frame that holds its list, which need not be the
    /// frame below it.
    func frameChain(_ target: Int) -> [Int] {
        var chain: [Int] = []
        var idx = target
        while idx > 0 && chain.count <= SynxParser.maxNestingDepth {
            chain.append(idx)
            idx = stack[idx].kind == .listItem ? stack[idx].parent : idx - 1
        }
        return chain.reversed()
    }

    mutating func currentMetaPath() -> String {
        if !metaPathValid {
            var p = ""
            if stack.count > 1 {
                for idx in frameChain(stack.count - 1) {
                    let f = stack[idx]
                    if !p.isEmpty { p.append(".") }
                    MetaPath.pushKey(&p, f.key)
                    if f.kind == .listItem { MetaPath.pushItem(&p, f.itemIdx) }
                }
            }
            metaPath = p
            metaPathValid = true
        }
        return metaPath
    }

    /// The first line at or after `from` that is content — not blank, not a
    /// comment, not inside a `###` block — CR excluded.
    ///
    /// spec: a comment line is ignored (§5), so it cannot stand between a key
    /// and its list or between a `- ` line and the keys of its item. Only blank
    /// lines were skipped here: `items` / `# note` / `- a` read as an empty
    /// object and lost every item.
    func nextContentLine(_ from: Int) -> Range<Int>? {
        var p = from
        var inBlockComment = false
        while p <= b.count {
            var nl = p
            while nl < b.count && b[nl] != 0x0A { nl += 1 }
            let end = (nl > p && b[nl - 1] == 0x0D) ? nl - 1 : nl
            let t = trimRange(b, p, end)
            if eq(t, "###") {
                inBlockComment.toggle()
            } else if !inBlockComment && !t.isEmpty && b[t.lowerBound] != 0x23 && !startsWith(t, "//") {
                return p..<end
            }
            p = nl + 1
        }
        return nil
    }

    static func isReserved(_ k: String) -> Bool { k == "__proto__" || k == "constructor" || k == "prototype" }

    /// `digits[.digits]` — a `:random` weight.
    static func isWeight(_ s: Slice<UnsafeBufferPointer<UInt8>>) -> Bool {
        func digits(_ t: Slice<UnsafeBufferPointer<UInt8>>) -> Bool {
            !t.isEmpty && t.allSatisfy { $0 >= 0x30 && $0 <= 0x39 }
        }
        if let dot = s.firstIndex(of: 0x2E) {
            return digits(s[s.startIndex..<dot]) && digits(s[(dot + 1)..<s.endIndex])
        }
        return digits(s)
    }

    mutating func handleListItem(_ t: Range<Int>, _ indent: Int, _ pos: Int) {
        // Pop every frame of the previous item — its own and any object nested in it.
        while stack.count > 1 && stack[stack.count - 1].indent >= indent { popFrame() }
        let inner = trimRange(b, t.lowerBound + 2, t.upperBound)
        let valStr = stripComment(inner)
        // A more deeply indented next line of content that is not an item
        // makes this item an object.
        var nested = false
        if let peek = nextContentLine(pos) {
            let pt = trimRange(b, peek.lowerBound, peek.upperBound)
            let pi = indentLen(b, peek.lowerBound, peek.upperBound)
            if pi > indent && !startsWith(pt, "- ") { nested = true }
        }
        let ls = lists[lists.count - 1]
        guard ls.stackIdx < stack.count else { return }
        let parent = stack[ls.stackIdx].obj
        let arr: MArray
        if let at = parent.find(ls.key) {
            guard let a = parent.values[at].array else { return }
            arr = a
        } else {
            arr = MArray()
            parent.keys.append(ls.key)
            parent.values.append(.array(arr))
        }
        if arr.items.count >= SynxParser.maxListItems {
            // The cap dropped an item: say so, like every other cap.
            result.truncated = true
            return
        }
        if nested {
            let item = MObject()
            var head: (String, SynxMeta)? = nil
            if let p = parseLine(valStr) {
                let v: MValue = p.typeHint.map { SynxCast.castTyped(ArraySlice(UnsafeBufferPointer(rebasing: b[p.value])), hint: str($0)) }
                    ?? (p.value.isEmpty ? .object(MObject()) : SynxCast.cast(UnsafeBufferPointer(rebasing: b[p.value])))
                let k = str(p.key)
                if !Parser.isReserved(k) {
                    item.set(k, v)
                    if result.mode == .active && (!p.markers.isEmpty || p.constraints != nil || p.typeHint != nil) {
                        var m = SynxMeta()
                        m.markers = p.markers.map(str)
                        m.args = p.markerArgs
                        m.typeHint = p.typeHint.map(str)
                        m.constraints = p.constraints
                        head = (k, m)
                    }
                }
            } else {
                item.set("_value", SynxCast.cast(UnsafeBufferPointer(rebasing: b[valStr])))
            }
            let itemIdx = arr.items.count
            arr.items.append(.object(item))
            if stack.count < SynxParser.maxNestingDepth {
                pushFrame(Frame(indent: indent, kind: .listItem, key: ls.key, itemIdx: itemIdx, parent: ls.stackIdx, obj: item))
                // spec: the key on the `- ` line is a key line like the ones under
                // it. Its markers and constraints were read and thrown away:
                // `- token:secret abc` stayed in the clear and `- host:env H` kept
                // the variable's name.
                if let (k, m) = head {
                    let path = currentMetaPath()
                    result.metadata.update(path, default: SynxMetaMap()) { $0[k] = m }
                }
            }
        } else {
            arr.items.append(SynxCast.cast(UnsafeBufferPointer(rebasing: b[valStr])))
        }
    }

    mutating func handleKeyLine(_ p: ParsedLine, _ key: String, _ indent: Int, _ pos: Int) {
        while stack.count > 1 && stack[stack.count - 1].indent >= indent { popFrame() }
        let parentIdx = stack.count - 1
        let parent = stack[parentIdx].obj

        // A key that replaces an object or a list replaces its declarations too.
        if result.mode == .active && !result.metadata.isEmpty, let at = parent.find(key), parent.values[at].isObjectOrArray {
            var prefix = currentMetaPath()
            if !prefix.isEmpty { prefix.append(".") }
            MetaPath.pushKey(&prefix, key)
            result.metadata.retain { path, _ in
                !(path.bytesEqual(prefix) || (path.bytesHasPrefix(prefix) && path.utf8.count > prefix.utf8.count
                    && Array(path.utf8)[prefix.utf8.count] == 0x2E))
            }
        }

        if result.mode == .active && (!p.markers.isEmpty || p.constraints != nil || p.typeHint != nil) {
            var m = SynxMeta()
            m.markers = p.markers.map(str)
            m.args = p.markerArgs
            m.typeHint = p.typeHint.map(str)
            m.constraints = p.constraints
            let path = currentMetaPath()
            result.metadata.update(path, default: SynxMetaMap()) { $0[key] = m }
        }

        let isBlock = eq(p.value, "|") || eq(p.value, "|+")
        let isListMarker = p.markers.contains { eq($0, "random") || eq($0, "unique") || eq($0, "geo") || eq($0, "join") }

        if isBlock {
            parent.set(key, .string(""))
            block = BlockState(indent: indent, key: key, content: [], stackIdx: parentIdx, preserveIndent: eq(p.value, "|+"), baseIndent: -1)
        } else if isListMarker && p.value.isEmpty {
            // The key exists even when the list stays empty.
            parent.set(key, .array(MArray()))
            lists.append(ListState(indent: indent, key: key, stackIdx: parentIdx))
        } else if p.value.isEmpty {
            if let peek = nextContentLine(pos), startsWith(trimRange(b, peek.lowerBound, peek.upperBound), "- ") {
                parent.set(key, .array(MArray()))
                lists.append(ListState(indent: indent, key: key, stackIdx: parentIdx))
                return
            }
            let child = MObject()
            parent.set(key, .object(child))
            // Past the cap the object is still inserted; nesting stops growing.
            if stack.count < SynxParser.maxNestingDepth {
                pushFrame(Frame(indent: indent, kind: .key, key: key, obj: child))
            }
        } else {
            let v = p.typeHint.map { SynxCast.castTyped(ArraySlice(UnsafeBufferPointer(rebasing: b[p.value])), hint: str($0)) }
                ?? SynxCast.cast(UnsafeBufferPointer(rebasing: b[p.value]))
            parent.set(key, v)
        }
    }

    mutating func flushBlock() {
        guard let blk = block else { return }
        block = nil
        stack[blk.stackIdx].obj.set(blk.key, .string(String(decoding: blk.content, as: UTF8.self)))
    }

    mutating func run() {
        var inBlockComment = false
        // Indent of a refused key whose block is being dropped.
        var droppedBelow: Int? = nil
        let total = b.count
        var pos = 0
        while pos <= total {
            let start = pos
            var nl = start
            while nl < total && b[nl] != 0x0A { nl += 1 }
            pos = nl + 1
            let end = (nl > start && b[nl - 1] == 0x0D) ? nl - 1 : nl
            let t = trimRange(b, start, end)
            // A blank line is never structural and never body (§8.4.1).
            if t.isEmpty { continue }
            let indent = indentLen(b, start, end)

            if let limit = droppedBelow {
                if indent > limit { continue }
                droppedBelow = nil
            }

            // Multiline body first: `#`, `//`, `###` and `!…` are content there.
            if block != nil {
                if indent > block!.indent {
                    if block!.content.count < SynxParser.maxMultilineBytes {
                        if !block!.content.isEmpty { block!.content.append(0x0A) }
                        let room = SynxParser.maxMultilineBytes - block!.content.count
                        if room > 0 {
                            var slice: Range<Int>
                            if block!.preserveIndent {
                                if block!.baseIndent < 0 { block!.baseIndent = indent }
                                let strip = min(block!.baseIndent, indent)
                                let rawEnd = trimEndRange(b, start, end)
                                let cut = stripBoundary(b, start, rawEnd, strip)
                                slice = cut < rawEnd ? cut..<rawEnd : rawEnd..<rawEnd
                            } else {
                                slice = t
                            }
                            let n = utf8PrefixLen(b, slice.lowerBound, slice.count, room)
                            block!.content.append(contentsOf: UnsafeBufferPointer(rebasing: b[slice.lowerBound..<(slice.lowerBound + n)]))
                            if n < slice.count { result.truncated = true }
                        }
                    } else {
                        result.truncated = true
                    }
                    continue
                }
                flushBlock()
            }

            if eq(t, "###") {
                inBlockComment.toggle()
                continue
            }
            if inBlockComment { continue }

            // Directives (§6).
            if b[t.lowerBound] == 0x21 || startsWith(t, "#!mode:") {
                if eq(t, "!active") { result.mode = .active; continue }
                if eq(t, "!lock") { result.locked = true; continue }
                if eq(t, "!tool") { result.tool = true; continue }
                if eq(t, "!schema") { result.schema = true; continue }
                if eq(t, "!llm") { result.llm = true; continue }
                if startsWith(t, "!include ") {
                    if result.includes.count < SynxParser.maxIncludes {
                        let rest = trimRange(b, t.lowerBound + 9, t.upperBound)
                        var split = rest.lowerBound
                        while split < rest.upperBound && wsLen(b, split, rest.upperBound) == 0 { split += utf8Len(b[split]) }
                        let path = str(rest.lowerBound..<split)
                        var alias: String
                        if split < rest.upperBound {
                            alias = str(trimRange(b, split + wsLen(b, split, rest.upperBound), rest.upperBound))
                        } else {
                            // Alias from the file name: `./db.synx` → `db`.
                            let name = Array(afterLast(path, [0x2F, 0x5C]).utf8)
                            let ext = name.suffix(5)
                            let strip = ext.elementsEqual(".synx".utf8) || ext.elementsEqual(".SYNX".utf8)
                            alias = String(decoding: strip ? name.dropLast(5) : name[...], as: UTF8.self)
                        }
                        result.includes.append(SynxIncludeDirective(path: path, alias: alias))
                    }
                    continue
                }
                if startsWith(t, "!use ") {
                    let rest = trimRange(b, t.lowerBound + 5, t.upperBound)
                    if !rest.isEmpty && b[rest.lowerBound] == 0x40 {
                        let asPos = findSeq(rest, " as ")
                        let package = str(trimRange(b, rest.lowerBound, asPos ?? rest.upperBound))
                        var alias: String
                        if let a = asPos {
                            alias = str(trimRange(b, a + 4, rest.upperBound))
                        } else {
                            alias = afterLast(package, [0x2F])
                        }
                        if !package.isEmpty { result.uses.append(SynxUseDirective(package: package, alias: alias)) }
                    }
                    continue
                }
                if startsWith(t, "#!mode:") {
                    result.mode = eq(trimRange(b, t.lowerBound + 7, t.upperBound), "active") ? .active : .static
                    continue
                }
            }

            if b[t.lowerBound] == 0x23 || startsWith(t, "//") { continue }

            if startsWith(t, "- ") {
                // The list this item belongs to: the innermost open one whose key
                // sits to the left of the dash. Lists nested deeper than the dash
                // end here — but only when a list does take the line; a dash no
                // list accepts is skipped and closes nothing, as it always was.
                var accepting = lists.count
                while accepting > 0 && indent < lists[accepting - 1].indent { accepting -= 1 }
                if accepting > 0 && indent > lists[accepting - 1].indent { lists.removeSubrange(accepting...) }
                if let l = lists.last, indent > l.indent {
                    handleListItem(t, indent, pos)
                    continue
                }
            } else {
                // A non-item line closes every list it is at or below the indent of.
                var closed = false
                while let l = lists.last, indent <= l.indent {
                    lists.removeLast()
                    closed = true
                }
                if closed {
                    while stack.count > 1 && stack[stack.count - 1].kind == .listItem && stack[stack.count - 1].indent >= indent {
                        popFrame()
                    }
                }
            }

            guard let parsed = parseLine(t) else { continue }
            let key = str(parsed.key)
            // `__proto__` / `constructor` / `prototype` never reach a JS consumer.
            //
            // safety: the lines under such a key go with it. Only the key line was
            // skipped, so its block fell to whatever was open above it — `profile`
            // / `__proto__` / `  role admin` gave `profile` the role — and under
            // `prototype |` each line of the text was read as a line of the document.
            if Parser.isReserved(key) {
                droppedBelow = indent
                continue
            }
            handleKeyLine(parsed, key, indent, pos)
        }
        flushBlock()
    }
}

// MARK: - metadata paths

enum MetaPath {
    /// Append `key` as one segment; `.` and `\` inside it are escaped with `\`.
    static func pushKey(_ path: inout String, _ key: String) {
        if !key.utf8.contains(0x2E) && !key.utf8.contains(0x5C) {
            path.append(key)
            return
        }
        var out: [UInt8] = []
        for c in key.utf8 {
            if c == 0x2E || c == 0x5C { out.append(0x5C) }
            out.append(c)
        }
        path.append(String(decoding: out, as: UTF8.self))
    }

    static func segment(_ key: String) -> String {
        var s = ""
        pushKey(&s, key)
        return s
    }

    /// `.#<idx>` — the segment of list item `idx`.
    static func pushItem(_ path: inout String, _ idx: Int) {
        path.append(".#")
        path.append(String(idx))
    }

    /// The unescaped segments: `a\.b.c` is `["a.b", "c"]`.
    static func segments(_ path: String) -> [String] {
        if path.isEmpty { return [] }
        var out: [String] = []
        var seg: [UInt8] = []
        var escaped = false
        for c in path.utf8 {
            if escaped { seg.append(c); escaped = false; continue }
            if c == 0x5C { escaped = true; continue }
            if c == 0x2E { out.append(String(decoding: seg, as: UTF8.self)); seg = []; continue }
            seg.append(c)
        }
        out.append(String(decoding: seg, as: UTF8.self))
        return out
    }
}
