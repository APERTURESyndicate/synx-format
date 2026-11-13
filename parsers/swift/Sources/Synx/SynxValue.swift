// SYNX value tree, metadata and parse-result types.
// Parity with crates/synx-core/src/value.rs.
//
// Keys and strings compare as UTF-8 bytes, as in Rust: Swift's own `==` on
// `String` applies canonical equivalence, which would make `é` (U+00E9) and
// `e` + U+0301 one key.
import Foundation

/// SYNX value variants. Not `indirect`: the recursion goes through `Array` and
/// `SynxObject`, which are already references, and boxing every case cost one
/// allocation per value.
public enum SynxValue: Equatable, Sendable {
    case null
    case bool(Bool)
    case int(Int64)
    case float(Double)
    case string(String)
    case array([SynxValue])
    case object(SynxObject)
    /// Redacted in JSON / stringify output as `[SECRET]`.
    case secret(String)

    public static func == (lhs: SynxValue, rhs: SynxValue) -> Bool {
        switch (lhs, rhs) {
        case (.null, .null): return true
        case (.bool(let a), .bool(let b)): return a == b
        case (.int(let a), .int(let b)): return a == b
        case (.float(let a), .float(let b)): return a == b
        case (.string(let a), .string(let b)), (.secret(let a), .secret(let b)): return a.utf8.elementsEqual(b.utf8)
        case (.array(let a), .array(let b)): return a == b
        case (.object(let a), .object(let b)): return a == b
        default: return false
        }
    }
}

/// Insertion-ordered key/value list — objects keep source order, as in Rust
/// (`IndexMap`); JSON output re-sorts for byte-stable diffs.
public struct SynxObject: Equatable, Sendable {
    public private(set) var entries: [(key: String, value: SynxValue)]

    public init() { self.entries = [] }
    public init<S: Sequence>(_ entries: S) where S.Element == (key: String, value: SynxValue) {
        self.entries = Array(entries)
    }

    public var count: Int { entries.count }
    public var isEmpty: Bool { entries.isEmpty }

    /// Position of `key`, compared as UTF-8 bytes.
    public func index(of key: String) -> Int? {
        for i in 0..<entries.count where entries[i].key.utf8.elementsEqual(key.utf8) { return i }
        return nil
    }

    public subscript(key: String) -> SynxValue? {
        get {
            guard let i = index(of: key) else { return nil }
            return entries[i].value
        }
        set {
            if let v = newValue {
                set(key, to: v)
            } else {
                _ = remove(key)
            }
        }
    }

    /// Insert or replace; a replaced entry keeps its position.
    public mutating func set(_ key: String, to value: SynxValue) {
        if let i = index(of: key) {
            entries[i].value = value
        } else {
            entries.append((key, value))
        }
    }

    @discardableResult
    public mutating func remove(_ key: String) -> Bool {
        guard let i = index(of: key) else { return false }
        entries.remove(at: i)
        return true
    }

    public func contains(_ key: String) -> Bool { index(of: key) != nil }

    public var keys: [String] { entries.map(\.key) }

    public static func == (lhs: SynxObject, rhs: SynxObject) -> Bool {
        guard lhs.entries.count == rhs.entries.count else { return false }
        // Order-insensitive, as Rust compares maps.
        for (k, v) in lhs.entries {
            guard let r = rhs[k], r == v else { return false }
        }
        return true
    }
}

public extension SynxValue {
    /// True if `self == .null`.
    var isNull: Bool { if case .null = self { return true }; return false }

    var boolValue: Bool? { if case .bool(let b) = self { return b }; return nil }
    var intValue: Int64? { if case .int(let n) = self { return n }; return nil }
    var floatValue: Double? { if case .float(let f) = self { return f }; return nil }
    var stringValue: String? { if case .string(let s) = self { return s }; return nil }
    var secretValue: String? { if case .secret(let s) = self { return s }; return nil }

    var arrayValue: [SynxValue]? {
        if case .array(let a) = self { return a }; return nil
    }
    var objectValue: SynxObject? {
        if case .object(let o) = self { return o }; return nil
    }

    /// Numeric coercion: Int/Float → Double; nil otherwise.
    var asDouble: Double? {
        switch self {
        case .int(let n):   return Double(n)
        case .float(let f): return f
        case .bool(let b):  return b ? 1.0 : 0.0
        case .string(let s): return Double(s)
        default: return nil
        }
    }

    /// Diagnostic type tag matching Rust `Value::type_name`.
    var typeName: String {
        switch self {
        case .null:   return "null"
        case .bool:   return "bool"
        case .int:    return "int"
        case .float:  return "float"
        case .string: return "string"
        case .array:  return "array"
        case .object: return "object"
        case .secret: return "secret"
        }
    }

    /// Object accessor: returns the value behind `key` if `self` is an object.
    subscript(key: String) -> SynxValue? {
        if case .object(let o) = self { return o[key] }
        return nil
    }
}

// MARK: - Ordered map

/// Insertion-ordered string-keyed map — Rust `indexmap::IndexMap`. Keys compare
/// as UTF-8 bytes; assigning to an existing key keeps its position; removing
/// shifts later entries down. Reads like a `Dictionary` (`map[key]` is optional).
public struct SynxIndexMap<Value>: Sequence, ExpressibleByDictionaryLiteral {
    public private(set) var entries: [(key: String, value: Value)] = []
    private var index = KeyIndex()

    public init() {}
    public init(dictionaryLiteral elements: (String, Value)...) {
        for (k, v) in elements { self[k] = v }
    }

    public var count: Int { entries.count }
    public var isEmpty: Bool { entries.isEmpty }
    public var keys: [String] { entries.map(\.key) }

    public func makeIterator() -> IndexingIterator<[(key: String, value: Value)]> { entries.makeIterator() }

    /// Position of `key`, or nil.
    public func position(of key: String) -> Int? {
        if entries.count <= 12 {
            for i in 0..<entries.count where entries[i].key.utf8.elementsEqual(key.utf8) { return i }
            return nil
        }
        return index.find(key, in: entries.count) { entries[$0].key }
    }

    public subscript(key: String) -> Value? {
        get {
            guard let i = position(of: key) else { return nil }
            return entries[i].value
        }
        set {
            if let v = newValue {
                if let i = position(of: key) {
                    entries[i].value = v
                } else {
                    entries.append((key, v))
                    syncIndex()
                }
            } else {
                removeValue(forKey: key)
            }
        }
    }

    /// The value for `key`, appended as `fallback()` first when absent.
    public subscript(key: String, default fallback: @autoclosure () -> Value) -> Value {
        get { self[key] ?? fallback() }
        set { self[key] = newValue }
    }

    /// Mutate the value for `key` in place, appending `fallback` first when absent.
    public mutating func update(_ key: String, default fallback: @autoclosure () -> Value, _ body: (inout Value) -> Void) {
        let i: Int
        if let at = position(of: key) {
            i = at
        } else {
            entries.append((key, fallback()))
            syncIndex()
            i = entries.count - 1
        }
        body(&entries[i].value)
    }

    @discardableResult
    public mutating func removeValue(forKey key: String) -> Value? {
        guard let i = position(of: key) else { return nil }
        let v = entries.remove(at: i).value
        index = KeyIndex()
        syncIndex()
        return v
    }

    /// Keep only the entries for which `keep(key, value)` is true, in order.
    public mutating func retain(_ keep: (String, Value) -> Bool) {
        let before = entries.count
        entries.removeAll { !keep($0.key, $0.value) }
        if entries.count != before {
            index = KeyIndex()
            syncIndex()
        }
    }

    private mutating func syncIndex() {
        if entries.count > 12 { index.sync(count: entries.count) { entries[$0].key } }
    }
}

extension SynxIndexMap: Equatable where Value: Equatable {
    public static func == (lhs: SynxIndexMap, rhs: SynxIndexMap) -> Bool {
        guard lhs.count == rhs.count else { return false }
        for (k, v) in lhs.entries {
            guard let r = rhs[k], r == v else { return false }
        }
        return true
    }
}

extension SynxIndexMap: Sendable where Value: Sendable {}

// MARK: - Mode

public enum SynxMode: Sendable {
    case `static`
    case active
}

// MARK: - Constraints / Meta

public struct SynxConstraints: Equatable, Sendable {
    public var min: Double?
    public var max: Double?
    public var typeName: String?
    public var required: Bool
    public var readonly: Bool
    public var pattern: String?
    public var enumValues: [String]?

    public init(min: Double? = nil,
                max: Double? = nil,
                typeName: String? = nil,
                required: Bool = false,
                readonly: Bool = false,
                pattern: String? = nil,
                enumValues: [String]? = nil) {
        self.min = min
        self.max = max
        self.typeName = typeName
        self.required = required
        self.readonly = readonly
        self.pattern = pattern
        self.enumValues = enumValues
    }
}

public struct SynxMeta: Equatable, Sendable {
    public var markers: [String]
    /// Arguments taken from the value: `:random` weights, the `:inherit` parent.
    public var args: [String]
    public var typeHint: String?
    public var constraints: SynxConstraints?

    public init(markers: [String] = [],
                args: [String] = [],
                typeHint: String? = nil,
                constraints: SynxConstraints? = nil) {
        self.markers = markers
        self.args = args
        self.typeHint = typeHint
        self.constraints = constraints
    }

    public func hasMarker(_ name: String) -> Bool { markers.contains { $0.utf8.elementsEqual(name.utf8) } }

    public func markerIndex(_ name: String) -> Int? {
        for (i, m) in markers.enumerated() where m.utf8.elementsEqual(name.utf8) { return i }
        return nil
    }
}

/// `key -> SynxMeta` for one object, in declaration order.
public typealias SynxMetaMap = SynxIndexMap<SynxMeta>
/// Declarations per object, keyed by the dot-path of the object holding the
/// keys: `""` is the root, `server.db` a nested object, `servers.#2` the third
/// item of the list `servers`; a `.` or `\` inside a key is escaped (`a\.b`).
public typealias SynxMetadataTree = SynxIndexMap<SynxMetaMap>

// MARK: - Directives

public struct SynxIncludeDirective: Equatable, Sendable {
    public var path: String
    public var alias: String
    public init(path: String, alias: String) {
        self.path = path
        self.alias = alias
    }
}

public struct SynxUseDirective: Equatable, Sendable {
    public var package: String
    public var alias: String
    public init(package: String, alias: String) {
        self.package = package
        self.alias = alias
    }
}

// MARK: - ParseResult

public struct SynxParseResult: Sendable {
    public var root: SynxValue
    public var mode: SynxMode
    public var locked: Bool
    public var tool: Bool
    public var schema: Bool
    public var llm: Bool
    public var metadata: SynxMetadataTree
    public var includes: [SynxIncludeDirective]
    public var uses: [SynxUseDirective]
    /// Input was dropped: the text exceeded the size or line cap, or a
    /// multiline body its byte cap.
    public var truncated: Bool

    public init(root: SynxValue = .object(SynxObject()),
                mode: SynxMode = .static,
                locked: Bool = false,
                tool: Bool = false,
                schema: Bool = false,
                llm: Bool = false,
                metadata: SynxMetadataTree = [:],
                includes: [SynxIncludeDirective] = [],
                uses: [SynxUseDirective] = [],
                truncated: Bool = false) {
        self.root = root
        self.mode = mode
        self.locked = locked
        self.tool = tool
        self.schema = schema
        self.llm = llm
        self.metadata = metadata
        self.includes = includes
        self.uses = uses
        self.truncated = truncated
    }
}

// safety: the default string forms (`"\(value)"`, `print`, `String(reflecting:)`,
// `dump`, an `XCTAssertEqual` failure) reflected the enum and printed the text of
// `.secret`. Every form is now the canonical JSON, where a secret is `[SECRET]`.
extension SynxValue: CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {
    public var description: String { SynxJSON.encode(self) }
    public var debugDescription: String { "SynxValue(\(SynxJSON.encode(self)))" }
    public var customMirror: Mirror { Mirror(self, children: ["json": SynxJSON.encode(self)]) }
}

extension SynxObject: CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {
    public var description: String { SynxJSON.encode(.object(self)) }
    public var debugDescription: String { "SynxObject(\(SynxJSON.encode(.object(self))))" }
    public var customMirror: Mirror { Mirror(self, children: ["json": SynxJSON.encode(.object(self))]) }
}
