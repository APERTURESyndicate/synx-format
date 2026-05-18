// SYNX top-level facade. Mirrors the Rust `Synx` struct in synx-core/src/lib.rs.
import Foundation

public enum Synx {

    /// Parse SYNX text and return its top-level object (static mode only).
    public static func parse(_ text: String) -> SynxObject {
        let r = SynxParser.parse(text)
        if case .object(let o) = r.root { return o }
        return SynxObject()
    }

    /// Parse and resolve `!active` markers. Returns top-level object.
    public static func parseActive(_ text: String, options: SynxOptions = SynxOptions()) -> SynxObject {
        var r = SynxParser.parse(text)
        if r.mode == .active {
            SynxEngine.resolve(&r, options: options)
        }
        if case .object(let o) = r.root { return o }
        return SynxObject()
    }

    /// Parse and return the full ParseResult (mode, metadata, includes, …).
    public static func parseFull(_ text: String) -> SynxParseResult {
        return SynxParser.parse(text)
    }

    /// Parse, resolve, and return the full ParseResult.
    public static func parseFullActive(_ text: String,
                                        options: SynxOptions = SynxOptions()) -> SynxParseResult {
        var r = SynxParser.parse(text)
        if r.mode == .active {
            SynxEngine.resolve(&r, options: options)
        }
        return r
    }

    /// Parse a `!tool` envelope into `{ tool, params }` or `{ tools: [...] }`.
    public static func parseTool(_ text: String,
                                  options: SynxOptions = SynxOptions()) -> SynxObject {
        var r = SynxParser.parse(text)
        if r.mode == .active {
            SynxEngine.resolve(&r, options: options)
        }
        let shaped = SynxParser.reshapeToolOutput(r.root, schema: r.schema)
        if case .object(let o) = shaped { return o }
        return SynxObject()
    }

    /// Canonical JSON.
    public static func toJSON(_ value: SynxValue) -> String { SynxJSON.encode(value) }
    public static func toJSON(_ object: SynxObject) -> String { SynxJSON.encode(.object(object)) }

    /// SYNX text serialiser.
    public static func stringify(_ value: SynxValue) -> String { SynxStringify.stringify(value) }
    public static func stringify(_ object: SynxObject) -> String { SynxStringify.stringify(.object(object)) }

    /// Canonical reformatter.
    public static func format(_ text: String) -> String { SynxFormatter.format(text) }

    /// Compile `.synx` text to `.synxb` bytes. Same semantics as Rust `Synx::compile`.
    public static func compile(_ text: String, resolved: Bool = false) -> Result<Data, SynxBinaryError> {
        var r = SynxParser.parse(text)
        if resolved && r.mode == .active {
            SynxEngine.resolve(&r, options: SynxOptions())
        }
        return SynxBinary.compile(r, resolved: resolved)
    }

    /// Decompile `.synxb` bytes back into a SYNX text string with directives.
    public static func decompile(_ data: Data) -> Result<String, SynxBinaryError> {
        switch SynxBinary.decompile(data) {
        case .failure(let e): return .failure(e)
        case .success(let pr):
            var out = ""
            if pr.tool   { out += "!tool\n" }
            if pr.schema { out += "!schema\n" }
            if pr.llm    { out += "!llm\n" }
            if pr.mode == .active { out += "!active\n" }
            if pr.locked { out += "!lock\n" }
            if !out.isEmpty { out += "\n" }
            out += SynxStringify.stringify(pr.root)
            return .success(out)
        }
    }

    /// True if `data` starts with the `.synxb` magic.
    public static func isSynxb(_ data: Data) -> Bool { SynxBinary.isSynxb(data) }

    /// Structural diff between two top-level objects.
    public static func diff(_ a: SynxObject, _ b: SynxObject) -> SynxDiffResult {
        SynxDiff.diff(a, b)
    }

    public static func diffToValue(_ d: SynxDiffResult) -> SynxValue {
        SynxDiff.toValue(d)
    }
}
