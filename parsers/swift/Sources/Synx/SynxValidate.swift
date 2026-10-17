// `[…]` constraint enforcement and `[pattern]` matching. Failures are written
// INTO the value as `CONSTRAINT_ERR: …`, exactly like crates/synx-core/src/engine.rs.
import Foundation

enum SynxValidate {

    /// Longest pattern a constraint will compile (bytes); longer ones are skipped.
    static let maxPatternBytes = 256

    /// Does the pattern repeat something that already repeats — the `(a+)+` shape
    /// behind exponential backtracking? Port of Rust `has_nested_quantifier`.
    static func hasNestedQuantifier(_ pattern: String) -> Bool {
        let p = Array(pattern.unicodeScalars)
        var opens: [Int] = []
        var i = 0
        while i < p.count {
            switch p[i] {
            case "\\":
                i += 1
            case "[":
                // A character class is skipped wholesale; quantifiers in it are literal.
                while i < p.count && p[i] != "]" {
                    if p[i] == "\\" { i += 1 }
                    i += 1
                }
            case "(":
                opens.append(i)
            case ")":
                if let start = opens.popLast(), i + 1 < p.count, p[i + 1] == "*" || p[i + 1] == "+" || p[i + 1] == "{" {
                    var body: [Unicode.Scalar] = []
                    var k = start + 1
                    while k < i {
                        if p[k] == "\\" { k += 2; continue }
                        body.append(p[k])
                        k += 1
                    }
                    for (n, c) in body.enumerated() {
                        if c == "*" || c == "+" { return true }
                        if c == "{", n + 1 < body.count, body[n + 1].value >= 0x30, body[n + 1].value <= 0x39 { return true }
                    }
                }
            default:
                break
            }
            i += 1
        }
        return false
    }

    /// Syntax the reference's `regex` crate refuses (so the constraint is skipped
    /// there) but ICU would run: look-around, back-references, atomic groups and
    /// possessive quantifiers.
    static func usesSyntaxRustRefuses(_ pattern: String) -> Bool {
        let p = Array(pattern.utf8)
        var i = 0
        var inClass = false
        while i < p.count {
            let c = p[i]
            if c == 0x5C {  // backslash
                if !inClass, i + 1 < p.count, p[i + 1] >= 0x31 && p[i + 1] <= 0x39 { return true }
                if i + 1 < p.count, p[i + 1] == 0x6B { return true }  // \k<name>
                i += 2
                continue
            }
            if inClass {
                if c == 0x5D { inClass = false }
            } else if c == 0x5B {
                inClass = true
            } else if c == 0x28, i + 2 < p.count, p[i + 1] == 0x3F {
                let k = p[i + 2]
                if k == 0x3D || k == 0x21 || k == 0x3E { return true }                        // (?= (?! (?>
                if k == 0x3C, i + 3 < p.count, p[i + 3] == 0x3D || p[i + 3] == 0x21 { return true }  // (?<= (?<!
            } else if (c == 0x2A || c == 0x2B || c == 0x3F || c == 0x7D), i + 1 < p.count, p[i + 1] == 0x2B {
                return true  // possessive *+ ++ ?+ {n}+
            }
            i += 1
        }
        return false
    }

    private static let cacheLock = NSLock()
    nonisolated(unsafe) private static var cache: [String: NSRegularExpression?] = [:]

    /// Does `s` fail `[pattern:…]`? Refused and invalid patterns never reject.
    /// perf: compiled patterns are cached (bounded).
    static func patternRejects(_ pattern: String, _ s: String) -> Bool {
        cacheLock.lock()
        var re: NSRegularExpression?
        if let hit = cache[pattern] {
            re = hit
        } else {
            if cache.count >= 256 { cache.removeAll() }
            if !hasNestedQuantifier(pattern) && !usesSyntaxRustRefuses(pattern) {
                re = try? NSRegularExpression(pattern: pattern)
            }
            cache[pattern] = re
        }
        cacheLock.unlock()
        guard let r = re else { return false }
        return r.firstMatch(in: s, options: [], range: NSRange(s.startIndex..<s.endIndex, in: s)) == nil
    }
}

enum Constraints {

    /// The error a value earns against its constraints, or nil (Rust
    /// `constraint_violation`: required, null, type, enum, min/max, pattern).
    static func violation(_ key: String, _ val: MValue, _ c: SynxConstraints) -> String? {
        let joined = { c.enumValues!.joined(separator: "|") }
        if c.required {
            var empty = val.isNull
            if case .string(let s) = val, s.isEmpty { empty = true }
            if empty { return "CONSTRAINT_ERR: '\(key)' is required" }
        }
        // An absent value (null) is checked against `enum` only, read as an empty
        // string; type, range and pattern apply to values that exist.
        if val.isNull {
            if let ev = c.enumValues, !ev.contains(where: { $0.isEmpty }) {
                return "CONSTRAINT_ERR: '\(key)' must be one of [\(joined())]"
            }
            return nil
        }
        if let tn = c.typeName {
            var ok = true
            switch (tn, val) {
            case ("int", .int), ("float", .float), ("float", .int), ("bool", .bool), ("string", .string), ("string", .secret):
                ok = true
            case ("int", _), ("float", _), ("bool", _), ("string", _):
                ok = false
            default:
                ok = true
            }
            if !ok { return "CONSTRAINT_ERR: '\(key)' expected type '\(tn)'" }
        }
        if let ev = c.enumValues {
            let text: String
            switch val {
            case .string(let s): text = s
            case .int(let n): text = String(n)
            case .float(let f): text = RustNumber.display(f)
            case .bool(let b): text = b ? "true" : "false"
            default: text = ""
            }
            if !ev.contains(where: { $0.bytesEqual(text) }) {
                return "CONSTRAINT_ERR: '\(key)' must be one of [\(joined())]"
            }
        }
        // Numbers by value; strings by their count of Unicode scalar values.
        var measured: Double? = nil
        switch val {
        case .int(let n): measured = Double(n)
        case .float(let f): measured = f
        case .string(let s) where c.min != nil || c.max != nil: measured = Double(s.unicodeScalars.count)
        default: break
        }
        if let n = measured {
            if let lo = c.min, n < lo {
                return "CONSTRAINT_ERR: '\(key)' value \(RustNumber.display(n)) is below min \(RustNumber.display(lo))"
            }
            if let hi = c.max, n > hi {
                return "CONSTRAINT_ERR: '\(key)' value \(RustNumber.display(n)) exceeds max \(RustNumber.display(hi))"
            }
        }
        if let pat = c.pattern, pat.utf8.count <= SynxValidate.maxPatternBytes, case .string(let s) = val,
           SynxValidate.patternRejects(pat, s) {
            return "CONSTRAINT_ERR: '\(key)' does not match pattern /\(pat)/"
        }
        return nil
    }
}

// MARK: - JSON for `:watch`

/// JSON text → value with serde_json's reading: integers that fit i64 are Int
/// (a larger u64 and `-0` are Float), objects come out with sorted keys and the
/// last duplicate wins, depth is capped at 128; anything else is not JSON.
enum JSONReader {
    static func parse(_ text: String) -> MValue? {
        var r = Reader(Array(text.utf8))
        guard let v = r.value(0) else { return nil }
        r.ws()
        return r.pos == r.b.count ? v : nil
    }

    private struct Reader {
        let b: [UInt8]
        var pos = 0
        init(_ b: [UInt8]) { self.b = b }

        mutating func ws() { while pos < b.count && (b[pos] == 0x20 || b[pos] == 0x09 || b[pos] == 0x0A || b[pos] == 0x0D) { pos += 1 } }

        mutating func lit(_ w: String, _ v: MValue) -> MValue? {
            let u = Array(w.utf8)
            guard pos + u.count <= b.count, b[pos..<(pos + u.count)].elementsEqual(u) else { return nil }
            pos += u.count
            return v
        }

        mutating func value(_ depth: Int) -> MValue? {
            ws()
            guard pos < b.count else { return nil }
            switch b[pos] {
            case 0x6E: return lit("null", .null)
            case 0x74: return lit("true", .bool(true))
            case 0x66: return lit("false", .bool(false))
            case 0x22: return string().map { .string($0) }
            case 0x5B:
                if depth >= 128 { return nil }
                pos += 1
                var items: [MValue] = []
                ws()
                if pos < b.count && b[pos] == 0x5D { pos += 1; return .array(MArray(items)) }
                while true {
                    guard let v = value(depth + 1) else { return nil }
                    items.append(v)
                    ws()
                    guard pos < b.count else { return nil }
                    if b[pos] == 0x2C { pos += 1; continue }
                    if b[pos] == 0x5D { pos += 1; break }
                    return nil
                }
                return .array(MArray(items))
            case 0x7B:
                if depth >= 128 { return nil }
                pos += 1
                let obj = MObject()
                ws()
                if pos < b.count && b[pos] == 0x7D { pos += 1; return .object(obj) }
                while true {
                    ws()
                    guard pos < b.count, b[pos] == 0x22, let k = string() else { return nil }
                    ws()
                    guard pos < b.count, b[pos] == 0x3A else { return nil }
                    pos += 1
                    guard let v = value(depth + 1) else { return nil }
                    obj.set(k, v)
                    ws()
                    guard pos < b.count else { return nil }
                    if b[pos] == 0x2C { pos += 1; continue }
                    if b[pos] == 0x7D { pos += 1; break }
                    return nil
                }
                // serde_json's default map is a BTreeMap.
                let order = (0..<obj.count).sorted { obj.keys[$0].bytesLess(obj.keys[$1]) }
                let sorted = MObject()
                for i in order { sorted.keys.append(obj.keys[i]); sorted.values.append(obj.values[i]) }
                return .object(sorted)
            default:
                return number()
            }
        }

        mutating func hex4() -> UInt32? {
            guard pos + 4 <= b.count else { return nil }
            var v: UInt32 = 0
            for k in 0..<4 {
                let c = b[pos + k]
                let d: UInt32
                switch c {
                case 0x30...0x39: d = UInt32(c - 0x30)
                case 0x61...0x66: d = UInt32(c - 0x61 + 10)
                case 0x41...0x46: d = UInt32(c - 0x41 + 10)
                default: return nil
                }
                v = v * 16 + d
            }
            pos += 4
            return v
        }

        mutating func string() -> String? {
            pos += 1
            var out: [UInt8] = []
            while pos < b.count {
                let c = b[pos]
                if c == 0x22 { pos += 1; return String(decoding: out, as: UTF8.self) }
                if c < 0x20 { return nil }
                if c != 0x5C { out.append(c); pos += 1; continue }
                pos += 1
                guard pos < b.count else { return nil }
                let e = b[pos]
                pos += 1
                switch e {
                case 0x22: out.append(0x22)
                case 0x5C: out.append(0x5C)
                case 0x2F: out.append(0x2F)
                case 0x62: out.append(0x08)
                case 0x66: out.append(0x0C)
                case 0x6E: out.append(0x0A)
                case 0x72: out.append(0x0D)
                case 0x74: out.append(0x09)
                case 0x75:
                    guard var cp = hex4() else { return nil }
                    if cp >= 0xDC00 && cp <= 0xDFFF { return nil }
                    if cp >= 0xD800 && cp <= 0xDBFF {
                        guard pos + 2 <= b.count, b[pos] == 0x5C, b[pos + 1] == 0x75 else { return nil }
                        pos += 2
                        guard let lo = hex4(), lo >= 0xDC00 && lo <= 0xDFFF else { return nil }
                        cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00)
                    }
                    out.append(contentsOf: Array(String(Unicode.Scalar(cp)!).utf8))
                default: return nil
                }
            }
            return nil
        }

        mutating func number() -> MValue? {
            let start = pos
            var neg = false
            if pos < b.count && b[pos] == 0x2D { neg = true; pos += 1 }
            let intStart = pos
            guard pos < b.count, b[pos] >= 0x30 && b[pos] <= 0x39 else { return nil }
            if b[pos] == 0x30 {
                pos += 1
                if pos < b.count && b[pos] >= 0x30 && b[pos] <= 0x39 { return nil }
            } else {
                while pos < b.count && b[pos] >= 0x30 && b[pos] <= 0x39 { pos += 1 }
            }
            let intEnd = pos
            var isFloat = false
            if pos < b.count && b[pos] == 0x2E {
                isFloat = true
                pos += 1
                let d = pos
                while pos < b.count && b[pos] >= 0x30 && b[pos] <= 0x39 { pos += 1 }
                if pos == d { return nil }
            }
            if pos < b.count && (b[pos] == 0x65 || b[pos] == 0x45) {
                isFloat = true
                pos += 1
                if pos < b.count && (b[pos] == 0x2B || b[pos] == 0x2D) { pos += 1 }
                let d = pos
                while pos < b.count && b[pos] >= 0x30 && b[pos] <= 0x39 { pos += 1 }
                if pos == d { return nil }
            }
            if !isFloat, let mag = RustNumber.parseU64(String(decoding: b[intStart..<intEnd], as: UTF8.self)) {
                if !neg && mag <= UInt64(Int64.max) { return .int(Int64(mag)) }
                if neg && mag != 0 && mag <= 9223372036854775808 { return .int(Int64(bitPattern: 0 &- mag)) }
            }
            // serde_json refuses a number out of f64 range.
            guard let f = RustNumber.parseF64(b[start..<pos]), !f.isInfinite else { return nil }
            return .float(f)
        }
    }
}
