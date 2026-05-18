// SYNX active-mode resolver options. Parity with crates/synx-core Options.
import Foundation

/// User-supplied custom marker.
///
/// Closure receives (key, args, currentValue) and returns the resolved value.
/// Throwing is not supported — return `.null` for "no result". Builtin markers
/// always win over custom ones with the same name.
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
    /// When `true`, missing required keys / failed constraints log to stderr.
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
