// `[…]` constraint and `(type)` hint enforcement. Failures are written INTO the
// value as `CONSTRAINT_ERR: …` / `TYPE_ERR: …` strings, exactly like
// crates/synx-core/src/engine.rs (`validate_constraints`,
// `validate_field_constraints`, `validate_field_types`).
import Foundation

enum SynxValidate {

    /// Longest pattern a constraint will compile (bytes); longer ones are skipped.
    static let maxPatternBytes = 256

    static let constraintPrefix = "CONSTRAINT_ERR:"
    static let typePrefix = "TYPE_ERR:"

    // MARK: - one value against one constraint set

    /// First failing check wins, in Rust's order: required, type, enum, min/max, pattern.
    static func constraintError(key: String, value val: SynxValue, c: SynxConstraints) -> String? {
        if c.required {
            var empty = false
            if case .null = val { empty = true }
            if case .string(let s) = val, s.isEmpty { empty = true }
            if empty { return "\(constraintPrefix) '\(key)' is required" }
        }

        if let tn = c.typeName {
            let ok: Bool
            switch (tn, val) {
            case ("int", .int):                            ok = true
            case ("int", _):                               ok = false
            case ("float", .float), ("float", .int):       ok = true
            case ("float", _):                             ok = false
            case ("bool", .bool):                          ok = true
            case ("bool", _):                              ok = false
            case ("string", .string), ("string", .secret): ok = true // a secret is a masked string
            case ("string", _):                            ok = false
            default:                                       ok = true
            }
            if !ok { return "\(constraintPrefix) '\(key)' expected type '\(tn)'" }
        }

        if let ev = c.enumValues {
            let text: String
            switch val {
            case .string(let s): text = s
            case .int(let n):    text = String(n)
            case .float(let f):  text = SynxJSON.displayNumber(f)
            case .bool(let b):   text = b ? "true" : "false"
            default:             text = ""
            }
            if !ev.contains(text) {
                return "\(constraintPrefix) '\(key)' must be one of [\(ev.joined(separator: "|"))]"
            }
        }

        // Numbers compare by value; strings by count of Unicode scalar values.
        var measured: Double? = nil
        switch val {
        case .int(let n):   measured = Double(n)
        case .float(let f): measured = f
        case .string(let s) where c.min != nil || c.max != nil:
            measured = Double(s.unicodeScalars.count)
        default: break
        }
        if let n = measured {
            if let lo = c.min, n < lo {
                return "\(constraintPrefix) '\(key)' value \(SynxJSON.displayNumber(n)) is below min \(SynxJSON.displayNumber(lo))"
            }
            if let hi = c.max, n > hi {
                return "\(constraintPrefix) '\(key)' value \(SynxJSON.displayNumber(n)) exceeds max \(SynxJSON.displayNumber(hi))"
            }
        }

        if let pat = c.pattern, pat.utf8.count <= maxPatternBytes, case .string(let s) = val {
            // nil = refused or uncompilable pattern: skipped silently.
            if matchesLinear(pattern: pat, value: s) == false {
                return "\(constraintPrefix) '\(key)' does not match pattern /\(pat)/"
            }
        }
        return nil
    }

    // MARK: - regex with a ReDoS guard

    /// Test `value` against `pattern`, refusing patterns that can backtrack
    /// catastrophically. `nil` means refused or uncompilable (constraint skipped).
    /// Port of `matchesLinear` in packages/synx-js/src/synxl.ts.
    static func matchesLinear(pattern: String, value: String) -> Bool? {
        if hasNestedQuantifier(pattern) { return nil }
        guard let re = try? NSRegularExpression(pattern: pattern) else { return nil }
        let range = NSRange(value.startIndex..<value.endIndex, in: value)
        return re.firstMatch(in: value, options: [], range: range) != nil
    }

    /// Does the pattern repeat something that already repeats — the `(a+)+` shape
    /// behind exponential backtracking? Deliberately conservative: a quantified
    /// group whose body also contains a quantifier.
    static func hasNestedQuantifier(_ pattern: String) -> Bool {
        let p = Array(pattern.unicodeScalars)
        var opens: [Int] = []
        var i = 0
        while i < p.count {
            let ch = p[i]
            if ch == "\\" { i += 2; continue }
            if ch == "[" {
                // Skip a character class wholesale; quantifiers inside it are literal.
                while i < p.count && p[i] != "]" {
                    if p[i] == "\\" { i += 1 }
                    i += 1
                }
                i += 1
                continue
            }
            if ch == "(" { opens.append(i) }
            else if ch == ")", let start = opens.popLast() {
                if i + 1 < p.count, p[i + 1] == "*" || p[i + 1] == "+" || p[i + 1] == "{" {
                    // Body with escapes (`\x`) removed, then look for `*`, `+` or `{<digit>`.
                    var body: [Unicode.Scalar] = []
                    var k = start + 1
                    while k < i {
                        if p[k] == "\\" { k += 2; continue }
                        body.append(p[k])
                        k += 1
                    }
                    for (n, c) in body.enumerated() {
                        if c == "*" || c == "+" { return true }
                        if c == "{", n + 1 < body.count,
                           body[n + 1].value >= 0x30, body[n + 1].value <= 0x39 { return true }
                    }
                }
            }
            i += 1
        }
        return false
    }
}

// MARK: - whole-document passes

extension Resolver {

    /// Echo a failure to stderr when `strict` is set (the value carries it either way).
    func reportViolation(_ message: String) {
        guard options.strict else { return }
        FileHandle.standardError.write(Data("synx: \(message)\n".utf8))
    }

    /// Field-name registry of merged `[…]` declarations across the whole document.
    private func buildConstraintRegistry() -> [String: SynxConstraints] {
        var registry: [String: SynxConstraints] = [:]
        for path in result.metadata.keys.sorted() {
            guard let fields = result.metadata[path] else { continue }
            for key in fields.keys.sorted() {
                guard let incoming = fields[key]?.constraints else { continue }
                if var base = registry[key] {
                    if incoming.required { base.required = true }
                    if incoming.readonly { base.readonly = true }
                    // Stricter numeric bounds win.
                    if let b = incoming.min { base.min = base.min.map { Swift.max($0, b) } ?? b }
                    if let b = incoming.max { base.max = base.max.map { Swift.min($0, b) } ?? b }
                    // First declaration of type / pattern / enum wins.
                    if base.typeName == nil { base.typeName = incoming.typeName }
                    if base.pattern == nil { base.pattern = incoming.pattern }
                    if base.enumValues == nil { base.enumValues = incoming.enumValues }
                    registry[key] = base
                } else {
                    registry[key] = incoming
                }
            }
        }
        return registry
    }

    private func buildTypeRegistry() -> [String: String] {
        var registry: [String: String] = [:]
        for path in result.metadata.keys.sorted() {
            guard let fields = result.metadata[path] else { continue }
            for key in fields.keys.sorted() {
                if let hint = fields[key]?.typeHint, registry[key] == nil { registry[key] = hint }
            }
        }
        return registry
    }

    /// Validate every object field that has a registered constraint rule. Values
    /// already holding an error string are skipped so the message is not re-judged.
    func validateFieldConstraints() {
        let registry = buildConstraintRegistry()
        if registry.isEmpty { return }
        guard case .object(var root) = result.root else { return }
        constraintPass(&root, registry)
        result.root = .object(root)
    }

    private func constraintPass(_ obj: inout SynxObject, _ registry: [String: SynxConstraints]) {
        for key in obj.keys {
            if let c = registry[key], let v = obj[key] {
                var errored = false
                if case .string(let s) = v {
                    errored = s.hasPrefix(SynxValidate.constraintPrefix) || s.hasPrefix(SynxValidate.typePrefix)
                }
                if !errored, let err = SynxValidate.constraintError(key: key, value: v, c: c) {
                    reportViolation(err)
                    obj.set(key, to: .string(err))
                }
            }
            switch obj[key] {
            case .object(var child)?:
                constraintPass(&child, registry)
                obj.set(key, to: .object(child))
            case .array(var arr)?:
                for i in arr.indices {
                    if case .object(var item) = arr[i] {
                        constraintPass(&item, registry)
                        arr[i] = .object(item)
                    }
                }
                obj.set(key, to: .array(arr))
            default: break
            }
        }
    }

    /// Replace values that contradict a `(type)` hint declared for the same field name.
    func validateFieldTypes() {
        let registry = buildTypeRegistry()
        if registry.isEmpty { return }
        guard case .object(var root) = result.root else { return }
        typePass(&root, registry)
        result.root = .object(root)
    }

    private static func matchesType(_ v: SynxValue, _ expected: String) -> Bool {
        switch (expected, v) {
        case ("int", .int), ("float", .float), ("float", .int), ("bool", .bool),
             ("string", .string), ("string", .secret), ("array", .array), ("object", .object):
            return true
        case ("int", _), ("float", _), ("bool", _), ("string", _), ("array", _), ("object", _):
            return false
        default:
            return true // unknown types are accepted
        }
    }

    private func typePass(_ obj: inout SynxObject, _ registry: [String: String]) {
        for key in obj.keys {
            if let expected = registry[key], let v = obj[key], !Resolver.matchesType(v, expected) {
                let err = "\(SynxValidate.typePrefix) '\(key)' expected \(expected) but got \(v.typeName)"
                reportViolation(err)
                obj.set(key, to: .string(err))
            }
            switch obj[key] {
            case .object(var child)?:
                typePass(&child, registry)
                obj.set(key, to: .object(child))
            case .array(var arr)?:
                for i in arr.indices {
                    if case .object(var item) = arr[i] {
                        typePass(&item, registry)
                        arr[i] = .object(item)
                    }
                }
                obj.set(key, to: .array(arr))
            default: break
            }
        }
    }
}
