import CSynx
import Foundation

/// Failure from the `synx-c` FFI (`NULL` from `synx.h`).
public enum SynxEngineError: Error, Sendable {
    case operationFailed
    case invalidUTF8
}

/// Swift wrapper over **`synx-c`** — same engine as Rust **`synx-core` 3.6.x** (grammar, `!active`, `!tool`, `.synxb`, canonical JSON).
public enum SynxEngine {
    private static func takeString(_ ptr: UnsafeMutablePointer<CChar>?) throws -> String {
        guard let ptr else { throw SynxEngineError.operationFailed }
        defer { synx_free(ptr) }
        guard let s = String(validatingUTF8: ptr) else { throw SynxEngineError.invalidUTF8 }
        return s
    }

    /// Static parse → canonical JSON string.
    public static func parse(_ text: String) throws -> String {
        try text.withCString { c in
            try takeString(synx_parse(c))
        }
    }

    /// Parse with `!active` resolution → JSON string.
    public static func parseActive(_ text: String) throws -> String {
        try text.withCString { c in
            try takeString(synx_parse_active(c))
        }
    }

    /// JSON (UTF-8) → SYNX text.
    public static func stringify(json: String) throws -> String {
        try json.withCString { c in
            try takeString(synx_stringify(c))
        }
    }

    /// Canonical SYNX reformat.
    public static func format(_ text: String) throws -> String {
        try text.withCString { c in
            try takeString(synx_format(c))
        }
    }

    /// `!tool` reshape → JSON string.
    public static func parseTool(_ text: String) throws -> String {
        try text.withCString { c in
            try takeString(synx_parse_tool(c))
        }
    }

    /// Structural diff → JSON string.
    public static func diff(_ a: String, _ b: String) throws -> String {
        try a.withCString { ca in
            try b.withCString { cb in
                try takeString(synx_diff(ca, cb))
            }
        }
    }

    /// SYNX → `.synxb` bytes.
    public static func compile(_ text: String, resolved: Bool = false) throws -> Data {
        try text.withCString { c in
            var outLen: size_t = 0
            guard let p = synx_compile(c, resolved ? 1 : 0, &outLen) else {
                throw SynxEngineError.operationFailed
            }
            let count = Int(outLen)
            let data = Data(bytes: p, count: count)
            synx_free_bytes(p, outLen)
            return data
        }
    }

    /// `.synxb` → SYNX text.
    public static func decompile(_ data: Data) throws -> String {
        try data.withUnsafeBytes { raw in
            guard let base = raw.bindMemory(to: UInt8.self).baseAddress else {
                throw SynxEngineError.operationFailed
            }
            return try takeString(synx_decompile(base, data.count))
        }
    }

    /// True if data begins with `.synxb` magic.
    public static func isSynxb(_ data: Data) -> Bool {
        data.withUnsafeBytes { raw in
            guard let base = raw.bindMemory(to: UInt8.self).baseAddress else { return false }
            return synx_is_synxb(base, data.count) != 0
        }
    }
}
