// SYNX text-to-tree parser. Parity with crates/synx-core/src/parser.rs.
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

    /// Truncate to a UTF-8-safe prefix.
    public static func clamp(_ text: String) -> String {
        let bytes = Array(text.utf8)
        if bytes.count <= maxInputBytes { return text }
        var end = maxInputBytes
        // Back off until previous byte is not a UTF-8 continuation (10xxxxxx).
        while end > 0 && (bytes[end] & 0xC0) == 0x80 { end -= 1 }
        return String(decoding: bytes[..<end], as: UTF8.self)
    }

    /// Parse a SYNX text into a `SynxParseResult`.
    public static func parse(_ rawText: String) -> SynxParseResult {
        var text = clamp(rawText)
        var bytes = Array(text.utf8)

        // Bound number of indexed newlines.
        if maxLineStarts > 0 {
            let maxNl = maxLineStarts - 1
            var seen = 0
            var scan = 0
            while scan < bytes.count {
                if bytes[scan] == 0x0A {
                    if seen >= maxNl {
                        bytes = Array(bytes[..<scan])
                        text = String(decoding: bytes, as: UTF8.self)
                        break
                    }
                    seen += 1
                }
                scan += 1
            }
        }

        // Index line starts.
        var lineStarts: [Int] = [0]
        var scan = 0
        while scan < bytes.count {
            if bytes[scan] == 0x0A {
                lineStarts.append(scan + 1)
            }
            scan += 1
        }
        let lineCount = lineStarts.count

        var result = SynxParseResult()
        var rootObj = SynxObject()
        var stack: [(Int, StackEntry)] = [(-1, .root)]

        var block: BlockState?
        var listState: ListState?
        var inBlockComment = false

        var i = 0
        while i < lineCount {
            let rawSlice = lineSlice(bytes: bytes, starts: lineStarts, index: i)
            let raw = String(decoding: rawSlice, as: UTF8.self)
            let t = raw.trimmingTrailing().trimmingLeading()

            // Directives
            switch t {
            case "!active": result.mode = .active;  i += 1; continue
            case "!lock":   result.locked = true;   i += 1; continue
            case "!tool":   result.tool = true;     i += 1; continue
            case "!schema": result.schema = true;   i += 1; continue
            case "!llm":    result.llm = true;      i += 1; continue
            default: break
            }
            if t.hasPrefix("!include ") {
                if result.includes.count < maxIncludes {
                    let rest = String(t.dropFirst(9)).trimmingLeadingTrailing()
                    var path = rest
                    var alias = ""
                    if let wsIdx = rest.firstIndex(where: { $0 == " " || $0 == "\t" }) {
                        path = String(rest[..<wsIdx])
                        alias = String(rest[wsIdx...]).trimmingLeadingTrailing()
                    }
                    if alias.isEmpty {
                        var base = path
                        if let slashIdx = base.lastIndex(where: { $0 == "/" || $0 == "\\" }) {
                            base = String(base[base.index(after: slashIdx)...])
                        }
                        if base.hasSuffix(".synx") {
                            base = String(base.dropLast(5))
                        } else if base.hasSuffix(".SYNX") {
                            base = String(base.dropLast(5))
                        }
                        alias = base
                    }
                    result.includes.append(SynxIncludeDirective(path: path, alias: alias))
                }
                i += 1; continue
            }
            if t.hasPrefix("!use ") {
                let rest = String(t.dropFirst(5)).trimmingLeadingTrailing()
                if rest.first == "@" {
                    var pkg = rest
                    var alias = ""
                    if let asRange = rest.range(of: " as ") {
                        pkg = String(rest[..<asRange.lowerBound]).trimmingLeadingTrailing()
                        alias = String(rest[asRange.upperBound...]).trimmingLeadingTrailing()
                    }
                    if alias.isEmpty {
                        if let slashIdx = pkg.lastIndex(of: "/") {
                            alias = String(pkg[pkg.index(after: slashIdx)...])
                        } else {
                            alias = pkg
                        }
                    }
                    if !pkg.isEmpty {
                        result.uses.append(SynxUseDirective(package: pkg, alias: alias))
                    }
                }
                i += 1; continue
            }
            if t.hasPrefix("#!mode:") {
                let declared = String(t.dropFirst(7)).trimmingLeadingTrailing()
                result.mode = (declared == "active") ? .active : .static
                i += 1; continue
            }

            if t == "###" { inBlockComment.toggle(); i += 1; continue }
            if inBlockComment { i += 1; continue }
            if t.isEmpty || t.first == "#" || t.hasPrefix("//") { i += 1; continue }

            let indent = indentOf(slice: rawSlice)

            // Continue multiline block
            if var blk = block {
                if indent > blk.indent {
                    if blk.content.utf8.count < maxMultilineBytes {
                        if !blk.content.isEmpty { blk.content.append("\n") }
                        let room = maxMultilineBytes - blk.content.utf8.count
                        if room > 0 {
                            let n = min(t.utf8.count, room)
                            if n == t.utf8.count {
                                blk.content.append(t)
                            } else {
                                let prefix = t.utf8.prefix(n)
                                blk.content.append(String(decoding: prefix, as: UTF8.self))
                            }
                        }
                    }
                    block = blk
                    i += 1; continue
                }
                insertValue(into: &rootObj, stack: stack, parentIdx: blk.stackIdx,
                            key: blk.key, value: .string(blk.content))
                block = nil
            }

            // List items
            if t.hasPrefix("- ") {
                if let lst = listState, indent > lst.indent {
                    while stack.count > 1 {
                        if case (let d, .listItem) = stack.last!, d >= indent {
                            stack.removeLast()
                        } else { break }
                    }
                    let valStr = stripComment(String(t.dropFirst(2)).trimmingLeadingTrailing())

                    // Peek for nested
                    var peek = i + 1
                    var nested = false
                    while peek < lineCount {
                        let pl = lineSlice(bytes: bytes, starts: lineStarts, index: peek)
                        let plStr = String(decoding: pl, as: UTF8.self)
                        let pt = plStr.trimmingLeadingTrailing()
                        if pt.isEmpty { peek += 1; continue }
                        let pi = indentOf(slice: pl)
                        if pi > indent && !pt.hasPrefix("- ") && pt.first != "#" && !pt.hasPrefix("//") {
                            nested = true
                        }
                        break
                    }

                    let listKey = lst.key
                    let listStackIdx = lst.stackIdx
                    var newItemIdx: Int? = nil
                    mutateArray(in: &rootObj, stack: stack, parentIdx: listStackIdx,
                                listKey: listKey) { arr in
                        if arr.count >= maxListItems { return }
                        if nested {
                            var itemObj = SynxObject()
                            if let parsed = parseLine(valStr) {
                                let v: SynxValue
                                if let hint = parsed.typeHint {
                                    v = castTyped(parsed.value, hint: hint)
                                } else if parsed.value.isEmpty {
                                    v = .object(SynxObject())
                                } else {
                                    v = castValue(parsed.value)
                                }
                                itemObj.set(parsed.key, to: v)
                            } else {
                                itemObj.set("_value", to: castValue(valStr))
                            }
                            newItemIdx = arr.count
                            arr.append(.object(itemObj))
                        } else {
                            arr.append(castValue(valStr))
                        }
                    }
                    if let idx = newItemIdx, stack.count < maxNestingDepth {
                        stack.append((indent, .listItem(listKey: listKey, itemIdx: idx)))
                    }
                    i += 1; continue
                }
            } else if let lst = listState, indent <= lst.indent {
                listState = nil
                while stack.count > 1 {
                    if case (let d, .listItem) = stack.last!, d >= indent {
                        stack.removeLast()
                    } else { break }
                }
            }

            // Key line
            guard let parsed = parseLine(t) else { i += 1; continue }
            if parsed.key == "__proto__" || parsed.key == "constructor" || parsed.key == "prototype" {
                i += 1; continue
            }
            while stack.count > 1, stack.last!.0 >= indent { stack.removeLast() }
            let parentIdx = stack.count - 1

            if result.mode == .active && (!parsed.markers.isEmpty
                || parsed.constraints != nil
                || parsed.typeHint != nil) {
                let path = buildPath(stack: stack)
                let meta = SynxMeta(markers: parsed.markers, args: parsed.markerArgs,
                                    typeHint: parsed.typeHint, constraints: parsed.constraints)
                result.metadata[path, default: [:]][parsed.key] = meta
            }

            let isBlock = (parsed.value == "|")
            let isListMarker = parsed.markers.contains { ["random", "unique", "geo", "join"].contains($0) }

            if isBlock {
                insertValue(into: &rootObj, stack: stack, parentIdx: parentIdx,
                            key: parsed.key, value: .string(""))
                block = BlockState(indent: indent, key: parsed.key, content: "", stackIdx: parentIdx)
            } else if isListMarker && parsed.value.isEmpty {
                insertValue(into: &rootObj, stack: stack, parentIdx: parentIdx,
                            key: parsed.key, value: .array([]))
                listState = ListState(indent: indent, key: parsed.key, stackIdx: parentIdx)
            } else if parsed.value.isEmpty {
                var peek = i + 1
                var becameList = false
                while peek < lineCount {
                    let pl = lineSlice(bytes: bytes, starts: lineStarts, index: peek)
                    let plStr = String(decoding: pl, as: UTF8.self)
                    let pt = plStr.trimmingLeadingTrailing()
                    if !pt.isEmpty {
                        if pt.hasPrefix("- ") {
                            insertValue(into: &rootObj, stack: stack, parentIdx: parentIdx,
                                        key: parsed.key, value: .array([]))
                            listState = ListState(indent: indent, key: parsed.key, stackIdx: parentIdx)
                            becameList = true
                        }
                        break
                    }
                    peek += 1
                }
                if !becameList {
                    insertValue(into: &rootObj, stack: stack, parentIdx: parentIdx,
                                key: parsed.key, value: .object(SynxObject()))
                    if stack.count < maxNestingDepth {
                        stack.append((indent, .key(parsed.key)))
                    }
                }
            } else {
                let v: SynxValue
                if let hint = parsed.typeHint {
                    v = castTyped(parsed.value, hint: hint)
                } else {
                    v = castValue(parsed.value)
                }
                insertValue(into: &rootObj, stack: stack, parentIdx: parentIdx,
                            key: parsed.key, value: v)
            }
            i += 1
        }

        if let blk = block {
            insertValue(into: &rootObj, stack: stack, parentIdx: blk.stackIdx,
                        key: blk.key, value: .string(blk.content))
        }

        result.root = .object(rootObj)
        return result
    }

    // MARK: - !tool reshaping

    public static func reshapeToolOutput(_ root: SynxValue, schema: Bool) -> SynxValue {
        guard case .object(let map) = root else { return root }

        if schema {
            let sortedKeys = map.keys.sorted()
            var tools: [SynxValue] = []
            for key in sortedKeys {
                var def = SynxObject()
                def.set("name", to: .string(key))
                def.set("params", to: map[key] ?? .null)
                tools.append(.object(def))
            }
            var out = SynxObject()
            out.set("tools", to: .array(tools))
            return .object(out)
        }

        if map.isEmpty {
            var out = SynxObject()
            out.set("tool", to: .null)
            out.set("params", to: .object(SynxObject()))
            return .object(out)
        }

        let firstKey = map.keys.sorted().first!
        let firstValue = map[firstKey] ?? .null
        let params: SynxValue = {
            if case .object = firstValue { return firstValue }
            return .object(SynxObject())
        }()
        var out = SynxObject()
        out.set("tool", to: .string(firstKey))
        out.set("params", to: params)
        return .object(out)
    }
}

// MARK: - Internal types

private enum StackEntry {
    case root
    case key(String)
    case listItem(listKey: String, itemIdx: Int)
}

private struct BlockState {
    var indent: Int
    var key: String
    var content: String
    var stackIdx: Int
}

private struct ListState {
    var indent: Int
    var key: String
    var stackIdx: Int
}

private struct ParsedLine {
    var key: String
    var typeHint: String?
    var value: String
    var markers: [String]
    var markerArgs: [String]
    var constraints: SynxConstraints?
}

// MARK: - Line slicing helpers

private func lineSlice(bytes: [UInt8], starts: [Int], index: Int) -> [UInt8] {
    let s = starts[index]
    var e = (index + 1 < starts.count) ? starts[index + 1] - 1 : bytes.count
    if e > s && bytes[e - 1] == 0x0D /* \r */ { e -= 1 }
    return Array(bytes[s..<e])
}

private func indentOf(slice: [UInt8]) -> Int {
    var i = 0
    while i < slice.count && (slice[i] == 0x20 /* ' ' */ || slice[i] == 0x09 /* \t */) {
        i += 1
    }
    return i
}

// MARK: - Line parser

private func parseLine(_ trimmed: String) -> ParsedLine? {
    if trimmed.isEmpty
        || trimmed.first == "#"
        || trimmed.hasPrefix("//")
        || trimmed.hasPrefix("- ") {
        return nil
    }
    if let f = trimmed.first {
        if "[:-#/(".contains(f) { return nil }
    }
    let bytes = Array(trimmed.utf8)
    let len = bytes.count
    var pos = 0
    while pos < len {
        let ch = bytes[pos]
        if ch == 0x20 || ch == 0x09 || ch == 0x5B /*[*/ || ch == 0x3A /*:*/ || ch == 0x28 /*(*/ {
            break
        }
        pos += 1
    }
    let key = String(decoding: bytes[0..<pos], as: UTF8.self)

    var typeHint: String? = nil
    if pos < len && bytes[pos] == 0x28 /*(*/ {
        let start = pos + 1
        var scan = start
        while scan < len && bytes[scan] != 0x29 /*)*/ { scan += 1 }
        if scan < len {
            typeHint = String(decoding: bytes[start..<scan], as: UTF8.self)
            pos = scan + 1
        } else {
            pos = start
        }
    }

    var constraints: SynxConstraints? = nil
    if pos < len && bytes[pos] == 0x5B /*[*/ {
        let cstart = pos + 1
        var depth = 1
        var scan = cstart
        while scan < len && depth > 0 {
            switch bytes[scan] {
            case 0x5B: depth += 1
            case 0x5D:
                depth -= 1
                if depth == 0 { break }
            default: break
            }
            if depth == 0 { break }
            scan += 1
        }
        if depth == 0 {
            constraints = parseConstraints(String(decoding: bytes[cstart..<scan], as: UTF8.self))
            pos = scan + 1
        } else {
            // Unbalanced — find first `]` if any.
            var sweep = cstart
            while sweep < len && bytes[sweep] != 0x5D { sweep += 1 }
            if sweep < len {
                constraints = parseConstraints(String(decoding: bytes[cstart..<sweep], as: UTF8.self))
                pos = sweep + 1
            } else {
                constraints = parseConstraints(String(decoding: bytes[cstart..<len], as: UTF8.self))
                pos = len
            }
        }
    }

    var markers: [String] = []
    var markerArgs: [String] = []
    if pos < len && bytes[pos] == 0x3A /*:*/ {
        let mstart = pos + 1
        var mend = mstart
        while mend < len && bytes[mend] != 0x20 && bytes[mend] != 0x09 { mend += 1 }
        let chain = String(decoding: bytes[mstart..<mend], as: UTF8.self)
        var segs = 0
        for seg in chain.split(separator: ":", omittingEmptySubsequences: false) {
            if segs >= SynxParser.maxMarkerSegments { break }
            markers.append(String(seg))
            segs += 1
        }
        pos = mend
    }
    while pos < len && (bytes[pos] == 0x20 || bytes[pos] == 0x09) { pos += 1 }
    var rawValue = ""
    if pos < len {
        rawValue = stripComment(String(decoding: bytes[pos..<len], as: UTF8.self))
    }

    if markers.contains("random") && !rawValue.isEmpty {
        var nums: [String] = []
        for tok in rawValue.split(whereSeparator: { $0 == " " || $0 == "\t" }) {
            if Double(String(tok)) != nil { nums.append(String(tok)) }
        }
        if !nums.isEmpty {
            markerArgs = nums
            rawValue = ""
        }
    }
    if markers.contains("inherit") && !rawValue.isEmpty {
        markerArgs = [rawValue.trimmingLeadingTrailing()]
        rawValue = ""
    }

    return ParsedLine(key: key, typeHint: typeHint, value: rawValue,
                      markers: markers, markerArgs: markerArgs, constraints: constraints)
}

private func parseConstraints(_ raw: String) -> SynxConstraints {
    var c = SynxConstraints()
    for raw_part in raw.split(separator: ",") {
        let part = raw_part.trimmingLeadingTrailing()
        if part.isEmpty { continue }
        if part == "required" { c.required = true; continue }
        if part == "readonly" { c.readonly = true; continue }
        guard let colonIdx = part.firstIndex(of: ":") else { continue }
        let key = part[..<colonIdx].trimmingLeadingTrailing()
        let val = part[part.index(after: colonIdx)...].trimmingLeadingTrailing()
        switch key {
        case "min":     c.min = Double(val)
        case "max":     c.max = Double(val)
        case "type":    c.typeName = val
        case "pattern": c.pattern = val
        case "enum":
            var vals: [String] = []
            var count = 0
            for piece in val.split(separator: "|", omittingEmptySubsequences: false) {
                if count >= SynxParser.maxEnumParts { break }
                vals.append(String(piece))
                count += 1
            }
            c.enumValues = vals
        default: break
        }
    }
    return c
}

// MARK: - Casting

private func castValue(_ val: String) -> SynxValue {
    if val.count >= 2, let first = val.first, let last = val.last {
        if (first == "\"" && last == "\"") || (first == "'" && last == "'") {
            return .string(String(val.dropFirst().dropLast()))
        }
    }
    switch val {
    case "true":  return .bool(true)
    case "false": return .bool(false)
    case "null":  return .null
    default: break
    }
    if val.isEmpty { return .string("") }
    var start = val.startIndex
    if val.first == "-" {
        if val.count == 1 { return .string(val) }
        start = val.index(after: start)
    }
    guard let firstDigit = val[start...].first, firstDigit.isASCII,
          let scalar = firstDigit.asciiValue, scalar >= 0x30 && scalar <= 0x39 else {
        return .string(val)
    }
    var seenDot = false
    var dotPos = -1
    var pos = 0
    var allNumeric = true
    for (i, ch) in val.enumerated() {
        if start != val.startIndex && i == 0 { continue /* skip leading '-' */ }
        if ch == "." {
            if seenDot { allNumeric = false; break }
            seenDot = true
            dotPos = i
        } else if !(ch >= "0" && ch <= "9") {
            allNumeric = false
            break
        }
        pos = i
    }
    _ = pos
    if !allNumeric { return .string(val) }
    let startOffset = (val.first == "-") ? 1 : 0
    if seenDot {
        if dotPos > startOffset && dotPos < val.count - 1 {
            if let f = Double(val) { return .float(f) }
        }
        return .string(val)
    }
    if let n = Int64(val) { return .int(n) }
    return .string(val)
}

private func castTyped(_ val: String, hint: String) -> SynxValue {
    switch hint {
    case "int":          return .int(Int64(val) ?? 0)
    case "float":        return .float(Double(val) ?? 0.0)
    case "bool":         return .bool(val.trimmingLeadingTrailing() == "true")
    case "string":       return .string(val)
    case "random", "random:int":
        // Match Rust `rng::random_i64()` — full signed 64-bit range including negatives.
        return .int(Int64.random(in: Int64.min...Int64.max))
    case "random:float": return .float(Double.random(in: 0.0..<1.0))
    case "random:bool":  return .bool(Bool.random())
    default: return castValue(val)
    }
}

private func stripComment(_ val: String) -> String {
    var r = val
    if let range = r.range(of: " //") { r = String(r[..<range.lowerBound]) }
    if let range = r.range(of: " #")  { r = String(r[..<range.lowerBound]) }
    while let last = r.last, last == " " || last == "\t" || last == "\r" { r.removeLast() }
    return r
}

// MARK: - Tree helpers

private func buildPath(stack: [(Int, StackEntry)]) -> String {
    var parts: [String] = []
    for (_, entry) in stack.dropFirst() {
        if case .key(let k) = entry { parts.append(k) }
    }
    return parts.joined(separator: ".")
}

/// Insert a value at the parent located by `stack[0...parentIdx]`. If any
/// path segment is missing or has the wrong kind, the write is silently
/// dropped (mirrors Rust `insert_value` behaviour on malformed input).
private func insertValue(into root: inout SynxObject,
                         stack: [(Int, StackEntry)],
                         parentIdx: Int,
                         key: String,
                         value: SynxValue) {
    if parentIdx == 0 {
        root.set(key, to: value)
        return
    }
    let path = Array(stack.prefix(parentIdx + 1).dropFirst())
    setValue(in: &root, path: ArraySlice(path), key: key, value: value)
}

/// Mutate the array stored at the parent location named by `stack[0...parentIdx]`
/// under `listKey`. The closure receives the array by `inout`.
private func mutateArray(in root: inout SynxObject,
                         stack: [(Int, StackEntry)],
                         parentIdx: Int,
                         listKey: String,
                         transform: (inout [SynxValue]) -> Void) {
    let path = Array(stack.prefix(parentIdx + 1).dropFirst())
    mutateArrayPath(in: &root, path: ArraySlice(path), listKey: listKey, transform: transform)
}

private func mutateArrayPath(in obj: inout SynxObject,
                             path: ArraySlice<(Int, StackEntry)>,
                             listKey: String,
                             transform: (inout [SynxValue]) -> Void) {
    if path.isEmpty {
        var arr: [SynxValue] = []
        if case .array(let existing) = (obj[listKey] ?? .null) { arr = existing }
        transform(&arr)
        obj.set(listKey, to: .array(arr))
        return
    }
    let head = path.first!
    let rest = path.dropFirst()
    switch head.1 {
    case .root:
        mutateArrayPath(in: &obj, path: rest, listKey: listKey, transform: transform)
    case .key(let k):
        guard case .object(var child) = (obj[k] ?? .null) else { return }
        mutateArrayPath(in: &child, path: rest, listKey: listKey, transform: transform)
        obj.set(k, to: .object(child))
    case .listItem(let lk, let idx):
        guard case .array(var arr) = (obj[lk] ?? .null) else { return }
        if idx >= arr.count { return }
        guard case .object(var item) = arr[idx] else { return }
        mutateArrayPath(in: &item, path: rest, listKey: listKey, transform: transform)
        arr[idx] = .object(item)
        obj.set(lk, to: .array(arr))
    }
}

private func setValue(in obj: inout SynxObject,
                      path: ArraySlice<(Int, StackEntry)>,
                      key: String,
                      value: SynxValue) {
    if path.isEmpty {
        obj.set(key, to: value)
        return
    }
    let head = path.first!
    let rest = path.dropFirst()
    switch head.1 {
    case .root:
        setValue(in: &obj, path: rest, key: key, value: value)
    case .key(let k):
        guard case .object(var child) = (obj[k] ?? .null) else { return }
        setValue(in: &child, path: rest, key: key, value: value)
        obj.set(k, to: .object(child))
    case .listItem(let listKey, let itemIdx):
        guard case .array(var arr) = (obj[listKey] ?? .null) else { return }
        if itemIdx >= arr.count { return }
        guard case .object(var itemObj) = arr[itemIdx] else { return }
        setValue(in: &itemObj, path: rest, key: key, value: value)
        arr[itemIdx] = .object(itemObj)
        obj.set(listKey, to: .array(arr))
    }
}

// MARK: - String trimming helpers

extension String {
    fileprivate func trimmingLeadingTrailing() -> String {
        return self.trimmingCharacters(in: .whitespaces)
    }
    fileprivate func trimmingLeading() -> String {
        var i = self.startIndex
        while i < endIndex, self[i] == " " || self[i] == "\t" || self[i] == "\r" {
            i = self.index(after: i)
        }
        return String(self[i...])
    }
    fileprivate func trimmingTrailing() -> String {
        var s = self
        while let last = s.last, last == " " || last == "\t" || last == "\r" {
            s.removeLast()
        }
        return s
    }
}

extension Substring {
    fileprivate func trimmingLeadingTrailing() -> String {
        return self.trimmingCharacters(in: .whitespaces)
    }
}
