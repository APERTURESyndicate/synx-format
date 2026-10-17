// SYNX active-mode resolver options. Parity with crates/synx-core Options.
import Foundation

/// User-supplied custom marker, run where synx-core runs WASM markers: after
/// the built-in markers of the key, once — the first registered non-built-in
/// marker of the chain.
///
/// Closure receives (key, args — the chain parts after the marker, currentValue)
/// and returns the resolved value. Throwing is not supported — return `.null`
/// for "no result". Built-in markers always win over custom ones of the same name.
public typealias SynxMarkerFn = @Sendable (_ key: String,
                                            _ args: [String],
                                            _ value: SynxValue) -> SynxValue

public struct SynxOptions: Sendable {
    public var env: [String: String]?
    public var region: String?
    public var lang: String?
    /// Base directory for `:include` / `:use` lookups. Defaults to the
    /// process current directory at resolve time.
    public var basePath: String?
    public var maxIncludeDepth: Int?
    public var packagesPath: String?
    /// When `true`, every constraint / type violation is also printed to stderr.
    public var strict: Bool
    /// Custom (non-builtin) markers, looked up by name.
    public var markerFns: [String: SynxMarkerFn]
    /// Internal include-recursion counter — do not set manually.
    public var includeDepth: Int

    public init(env: [String: String]? = nil,
                region: String? = nil,
                lang: String? = nil,
                basePath: String? = nil,
                maxIncludeDepth: Int? = nil,
                packagesPath: String? = nil,
                strict: Bool = false,
                markerFns: [String: SynxMarkerFn] = [:]) {
        self.env = env
        self.region = region
        self.lang = lang
        self.basePath = basePath
        self.maxIncludeDepth = maxIncludeDepth
        self.packagesPath = packagesPath
        self.strict = strict
        self.markerFns = markerFns
        self.includeDepth = 0
    }
}
