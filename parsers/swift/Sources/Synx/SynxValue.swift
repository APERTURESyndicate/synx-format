// SYNX value tree, metadata and parse-result types.
// Parity with crates/synx-core/src/value.rs.
import Foundation

/// SYNX value variants. `indirect` so Array / Object cases can hold child
/// `SynxValue` without resorting to class wrappers.
public indirect enum SynxValue: Equatable, Sendable {
    case null
    case bool(Bool)
    case int(Int64)
    case float(Double)
    case string(String)
    case array([SynxValue])
    case object(SynxObject)
    /// Redacted in JSON / stringify output as `[SECRET]`.
    case secret(String)
}

/// Insertion-ordered key/value list — chosen over `[String: SynxValue]` so
/// stringify and canonical reformat preserve author order, while JSON output
/// re-sorts for byte-stable diffs.
public struct SynxObject: Equatable, Sendable {
    public private(set) var entries: [(key: String, value: SynxValue)]

    public init() { self.entries = [] }
    public init<S: Sequence>(_ entries: S) where S.Element == (key: String, value: SynxValue) {
        self.entries = Array(entries)
    }

    public var count: Int { entries.count }
    public var isEmpty: Bool { entries.isEmpty }

    public subscript(key: String) -> SynxValue? {
        get {
            for e in entries where e.key == key { return e.value }
            return nil
        }
        set {
            if let v = newValue {
                set(key, to: v)
            } else {
                _ = remove(key)
            }
        }
    }

    public mutating func set(_ key: String, to value: SynxValue) {
        for i in 0..<entries.count where entries[i].key == key {
            entries[i] = (key, value)
            return
        }
        entries.append((key, value))
    }

    @discardableResult
    public mutating func remove(_ key: String) -> Bool {
        for i in 0..<entries.count where entries[i].key == key {
            entries.remove(at: i)
            return true
        }
        return false
    }

    public func contains(_ key: String) -> Bool { self[key] != nil }

    public var keys: [String] { entries.map(\.key) }

    public static func == (lhs: SynxObject, rhs: SynxObject) -> Bool {
        guard lhs.entries.count == rhs.entries.count else { return false }
        // Order-insensitive equality matches Rust HashMap.
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

    public func hasMarker(_ name: String) -> Bool { markers.contains(name) }

    public func markerIndex(_ name: String) -> Int? {
        for (i, m) in markers.enumerated() where m == name { return i }
        return nil
    }
}

public typealias SynxMetaMap = [String: SynxMeta]
public typealias SynxMetadataTree = [String: SynxMetaMap]

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

    public init(root: SynxValue = .object(SynxObject()),
                mode: SynxMode = .static,
                locked: Bool = false,
                tool: Bool = false,
                schema: Bool = false,
                llm: Bool = false,
                metadata: SynxMetadataTree = [:],
                includes: [SynxIncludeDirective] = [],
                uses: [SynxUseDirective] = []) {
        self.root = root
        self.mode = mode
        self.locked = locked
        self.tool = tool
        self.schema = schema
        self.llm = llm
        self.metadata = metadata
        self.includes = includes
        self.uses = uses
    }
}
