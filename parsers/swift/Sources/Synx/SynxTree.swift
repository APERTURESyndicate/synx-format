// Mutable working tree for the parser and the engine. Reference nodes play the
// part of the pointers synx-core uses: an open object keeps its identity while
// keys are added to it, and resolution reads the root while it writes a child.
// Converted to `SynxValue` once, at the end.

final class MObject {
    var keys: [String] = []
    var values: [MValue] = []
    private var index = KeyIndex()

    init() {}

    init(_ o: SynxObject) {
        keys.reserveCapacity(o.count)
        values.reserveCapacity(o.count)
        for e in o.entries {
            if let i = find(e.key) {
                values[i] = MValue(e.value)
            } else {
                keys.append(e.key)
                values.append(MValue(e.value))
            }
        }
    }

    var count: Int { keys.count }

    /// Position of `key` (UTF-8 bytes), or nil. Hashed once the object outgrows a scan.
    func find(_ key: String) -> Int? {
        if keys.count < 16 {
            for i in 0..<keys.count where keys[i].bytesEqual(key) { return i }
            return nil
        }
        index.sync(count: keys.count) { keys[$0] }
        return index.find(key, in: keys.count) { keys[$0] }
    }

    func get(_ key: String) -> MValue? {
        guard let i = find(key) else { return nil }
        return values[i]
    }

    /// Insert or replace; a replaced entry keeps its position (`IndexMap::insert`).
    func set(_ key: String, _ v: MValue) {
        if let i = find(key) {
            values[i] = v
        } else {
            keys.append(key)
            values.append(v)
        }
    }

    /// Keep the entries `keep` accepts, in order.
    func retain(_ keep: (String) -> Bool) {
        var k: [String] = [], v: [MValue] = []
        for i in 0..<keys.count where keep(keys[i]) {
            k.append(keys[i])
            v.append(values[i])
        }
        if k.count != keys.count {
            keys = k
            values = v
            index = KeyIndex()
        }
    }

    func toObject() -> SynxObject {
        var entries: [(key: String, value: SynxValue)] = []
        entries.reserveCapacity(keys.count)
        for i in 0..<keys.count { entries.append((keys[i], values[i].toValue())) }
        return SynxObject(entries)
    }
}

final class MArray {
    var items: [MValue]
    init(_ items: [MValue] = []) { self.items = items }
}

enum MValue {
    case null
    case bool(Bool)
    case int(Int64)
    case float(Double)
    case string(String)
    case secret(String)
    case array(MArray)
    case object(MObject)

    init(_ v: SynxValue) {
        switch v {
        case .null: self = .null
        case .bool(let b): self = .bool(b)
        case .int(let n): self = .int(n)
        case .float(let f): self = .float(f)
        case .string(let s): self = .string(s)
        case .secret(let s): self = .secret(s)
        case .array(let a): self = .array(MArray(a.map(MValue.init)))
        case .object(let o): self = .object(MObject(o))
        }
    }

    func toValue() -> SynxValue {
        switch self {
        case .null: return .null
        case .bool(let b): return .bool(b)
        case .int(let n): return .int(n)
        case .float(let f): return .float(f)
        case .string(let s): return .string(s)
        case .secret(let s): return .secret(s)
        case .array(let a): return .array(a.items.map { $0.toValue() })
        case .object(let o): return .object(o.toObject())
        }
    }

    /// A deep copy (inherited subtrees are copies, not shared).
    func deepCopy() -> MValue {
        switch self {
        case .array(let a): return .array(MArray(a.items.map { $0.deepCopy() }))
        case .object(let o):
            let c = MObject()
            c.keys = o.keys
            c.values = o.values.map { $0.deepCopy() }
            return .object(c)
        default: return self
        }
    }

    var object: MObject? { if case .object(let o) = self { return o }; return nil }
    var array: MArray? { if case .array(let a) = self { return a }; return nil }
    var string: String? { if case .string(let s) = self { return s }; return nil }
    var isNull: Bool { if case .null = self { return true }; return false }
    var isSecret: Bool { if case .secret = self { return true }; return false }
    var isObjectOrArray: Bool {
        switch self {
        case .object, .array: return true
        default: return false
        }
    }

    /// Int or Float as f64 (Rust `value_as_number`).
    var number: Double? {
        switch self {
        case .int(let n): return Double(n)
        case .float(let f): return f
        default: return nil
        }
    }

    var typeName: String {
        switch self {
        case .null: return "null"
        case .bool: return "bool"
        case .int: return "int"
        case .float: return "float"
        case .string: return "string"
        case .secret: return "secret"
        case .array: return "array"
        case .object: return "object"
        }
    }

    /// Text of a value for interpolation, `:join`, enum checks… (Rust `value_to_string`).
    var text: String {
        switch self {
        case .string(let s): return s
        case .int(let n): return String(n)
        case .float(let f): return RustNumber.formatNumber(f)
        case .bool(let b): return b ? "true" : "false"
        case .null: return "null"
        // safety: a `:secret` value MUST NOT reach a string sink as plaintext.
        case .secret: return "[SECRET]"
        case .array, .object: return ""
        }
    }
}
