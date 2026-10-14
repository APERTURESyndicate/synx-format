// SYNX `!active` engine — resolves markers, includes, packages, interpolation,
// constraints. Mirrors crates/synx-core/src/engine.rs.
import Foundation

public enum SynxEngine {

    public static let maxResolveDepth = 512

    /// Apply markers and constraints to `result` in-place. No-op if mode != .active.
    public static func resolve(_ result: inout SynxParseResult, options: SynxOptions) {
        guard result.mode == .active else { return }
        guard case .object = result.root else { return }
        var r = Resolver(result: result, options: options)
        r.run()
        result = r.result
    }
}

// MARK: - Resolver

final class Resolver {
    var result: SynxParseResult
    var options: SynxOptions
    var namespaces: [String: SynxObject] = [:]
    var typeRegistry: [String: SynxConstraints] = [:]
    var onceLoaded = false
    var onceKeys: Set<String> = []
    var onceNewKeys: Set<String> = []
    /// Aliases of `!include`d files (not `!use` packages) — `{key:include}` scope.
    var includeAliases: [String] = []
    var rng: SystemRandomNumberGenerator

    init(result: SynxParseResult, options: SynxOptions) {
        self.result = result
        self.options = options
        self.rng = SystemRandomNumberGenerator()
    }

    func run() {
        loadPackages()
        loadIncludes()
        applyInheritPass()
        stripUnderscoreKeys()
        maskSecretLiterals()
        walk(path: "", depth: 0)
        validateFieldConstraints()
        validateFieldTypes()
        flushOnce()
    }

    // MARK: - top-level passes

    private func loadPackages() {
        guard !result.uses.isEmpty else { return }
        let base = options.packagesPath ?? "./synx_packages"
        for use in result.uses {
            if use.package.contains("..") { continue }
            let path = (base as NSString).appendingPathComponent(use.package + "/synx.synx")
            guard let text = try? String(contentsOfFile: path, encoding: .utf8) else { continue }
            let sub = SynxParser.parse(text)
            guard case .object(let obj) = sub.root else { continue }
            namespaces[use.alias] = obj
            // Expose under alias key in root so interpolation `{alias.key}` works.
            if case .object(var root) = result.root {
                root.set(use.alias, to: .object(obj))
                result.root = .object(root)
            }
        }
    }

    private func loadIncludes() {
        guard !result.includes.isEmpty else { return }
        let maxDepth = options.maxIncludeDepth ?? 16
        if options.includeDepth >= maxDepth { return }

        let base = options.basePath ?? "."
        for inc in result.includes {
            guard let safe = jailPath(base: base, rel: inc.path) else { continue }
            guard let text = try? String(contentsOfFile: safe, encoding: .utf8) else { continue }
            var sub = SynxParser.parse(text)
            if sub.mode == .active {
                var subOpts = options
                subOpts.basePath = (safe as NSString).deletingLastPathComponent
                subOpts.includeDepth = options.includeDepth + 1
                SynxEngine.resolve(&sub, options: subOpts)
            }
            guard case .object(let obj) = sub.root else { continue }
            namespaces[inc.alias] = obj
            includeAliases.append(inc.alias)
            if case .object(var root) = result.root {
                root.set(inc.alias, to: .object(obj))
                result.root = .object(root)
            }
        }
    }

    private func applyInheritPass() {
        for (path, fields) in result.metadata {
            for (key, meta) in fields where meta.hasMarker("inherit") && !meta.args.isEmpty {
                inheritMerge(path: path, key: key, parentNames: meta.args)
            }
        }
    }

    private func inheritMerge(path: String, key: String, parentNames: [String]) {
        guard case .object(var root) = result.root else { return }
        let parentObj = getObjectAt(path: path, in: &root)
        guard var parent = parentObj.value else { return }
        guard case .object(var target) = (parent[key] ?? .null) else { return }

        for parentName in parentNames {
            if let pv = parent[parentName], case .object(let src) = pv {
                mergeMissing(into: &target, from: src)
            }
        }
        parent.set(key, to: .object(target))
        setObjectAt(path: path, in: &root, value: parent)
        result.root = .object(root)
    }

    private func mergeMissing(into dst: inout SynxObject, from src: SynxObject) {
        for entry in src.entries {
            if let existing = dst[entry.key] {
                if case .object(var existingObj) = existing,
                   case .object(let srcObj) = entry.value {
                    mergeMissing(into: &existingObj, from: srcObj)
                    dst.set(entry.key, to: .object(existingObj))
                }
                // Child wins — don't overwrite.
            } else {
                dst.set(entry.key, to: entry.value)
            }
        }
    }

    private func stripUnderscoreKeys() {
        guard case .object(var root) = result.root else { return }
        let keys = root.keys
        for k in keys where k.first == "_" {
            _ = root.remove(k)
        }
        result.root = .object(root)
    }

    private func walk(path: String, depth: Int) {
        guard depth <= SynxEngine.maxResolveDepth else { return }
        // Children first, so a parent's markers and interpolation see resolved values.
        if case .object(let root) = result.root,
           case .object(let obj) = getValueAt(path: path, in: root) ?? .null {
            for entry in obj.entries {
                if case .object = entry.value {
                    let sub = path.isEmpty ? entry.key : path + "." + entry.key
                    walk(path: sub, depth: depth + 1)
                } else if case .array(let arr) = entry.value {
                    let sub = path.isEmpty ? entry.key : path + "." + entry.key
                    for item in arr {
                        if case .object = item { walk(path: sub, depth: depth + 1) }
                    }
                }
            }
        }
        // For every metadata entry at this path level apply markers.
        if let fields = result.metadata[path] {
            for (key, meta) in fields {
                applyMarkers(meta: meta, key: key, atPath: path)
            }
        }
        interpolateLevel(path: path)
    }

    // MARK: - :secret pre-pass

    private static let resolvingBeforeSecret: Set<String> = [
        "env", "include", "import", "watch", "ref", "alias", "calc", "map", "i18n", "default",
        "fallback", "once", "split", "join", "format", "replace",
    ]

    /// Wrap every literal `:secret` value in `.secret` before any resolution so
    /// an `{interpolation}` can never read the plaintext. A secret that itself
    /// needs resolving (`:secret:env`) is masked by its own marker afterwards.
    private func maskSecretLiterals() {
        guard case .object(var root) = result.root else { return }
        var changed = false
        for (path, fields) in result.metadata {
            for (key, meta) in fields where meta.hasMarker("secret")
                && !meta.markers.contains(where: { Resolver.resolvingBeforeSecret.contains($0) }) {
                guard var parent = getObjectAt(path: path, in: &root).value,
                      let v = parent[key] else { continue }
                switch v {
                case .secret, .object, .array: continue
                default: break
                }
                parent.set(key, to: .secret(valueToString(v)))
                setObjectAt(path: path, in: &root, value: parent)
                changed = true
            }
        }
        if changed { result.root = .object(root) }
    }

    // MARK: - auto-{} interpolation

    /// Resolve `{key}` / `{a.b}` / `{key:alias}` / `{key:include}` in every string
    /// value of the object at `path`. Mirrors the Rust engine's third pass.
    private func interpolateLevel(path: String) {
        guard case .object(var root) = result.root else { return }
        guard var parent = getObjectAt(path: path, in: &root).value else { return }
        var changed = false
        for entry in parent.entries {
            guard case .string(let s) = entry.value, s.contains("{") else { continue }
            let out = resolveInterpolation(s, root: root, local: parent)
            if out != s {
                parent.set(entry.key, to: .string(out))
                changed = true
            }
        }
        if changed {
            setObjectAt(path: path, in: &root, value: parent)
            result.root = .object(root)
        }
    }

    private static let maxScratchBytes = 4 * 1024 * 1024

    private func isRefName(_ scalars: ArraySlice<Unicode.Scalar>) -> Bool {
        scalars.allSatisfy { c in
            c == "_" || c == "." || c.properties.isAlphabetic || c.properties.numericType != nil
        }
    }

    func resolveInterpolation(_ tpl: String, root: SynxObject, local: SynxObject) -> String {
        let sc = Array(tpl.unicodeScalars)
        var out = String.UnicodeScalarView()
        var outBytes = 0
        func push<S: Sequence>(_ seq: S) where S.Element == Unicode.Scalar {
            for c in seq {
                let w = UTF8.width(c)
                if outBytes + w > Resolver.maxScratchBytes { return }
                out.append(c)
                outBytes += w
            }
        }
        var i = 0
        while i < sc.count {
            if outBytes >= Resolver.maxScratchBytes { break }
            if sc[i] == "{", let close = sc[(i + 1)...].firstIndex(of: "}") {
                let inner = sc[(i + 1)..<close]
                var refName = inner
                var scope: String? = nil
                if let colon = inner.firstIndex(of: ":") {
                    refName = inner[inner.startIndex..<colon]
                    var sv = String.UnicodeScalarView()
                    sv.append(contentsOf: inner[(colon + 1)...])
                    scope = String(sv)
                }
                if isRefName(refName) {
                    var nameView = String.UnicodeScalarView()
                    nameView.append(contentsOf: refName)
                    let name = String(nameView)
                    let resolved: SynxValue?
                    if let scope = scope {
                        if scope == "include" {
                            if includeAliases.count == 1, let ns = namespaces[includeAliases[0]] {
                                resolved = deepGetValue(ns, name)
                            } else {
                                resolved = nil
                            }
                        } else {
                            resolved = namespaces[scope].flatMap { deepGetValue($0, name) }
                        }
                    } else {
                        resolved = deepGetValue(root, name) ?? local[name]
                    }
                    if let val = resolved {
                        push(valueToString(val).unicodeScalars)
                    } else {
                        push(["{"]); push(inner); push(["}"])
                    }
                    i = close + 1
                    continue
                }
            }
            push([sc[i]])
            i += 1
        }
        return String(out)
    }

    /// Direct key first, then dot-path traversal through objects.
    private func deepGetValue(_ root: SynxObject, _ path: String) -> SynxValue? {
        if let direct = root[path] { return direct }
        var current: SynxValue = .object(root)
        for seg in path.split(separator: ".", omittingEmptySubsequences: false) {
            guard case .object(let obj) = current, let next = obj[String(seg)] else { return nil }
            current = next
        }
        return current
    }

    private func applyMarkers(meta: SynxMeta, key: String, atPath path: String) {
        guard case .object(var root) = result.root else { return }
        let parentOpt = getObjectAt(path: path, in: &root)
        guard var parent = parentOpt.value else { return }
        var value = parent[key] ?? .null

        // `:secret` masks the *resolved* value, so it runs after every other marker
        // regardless of where it sits in the chain (`:secret:env`, `:env:secret`).
        for marker in meta.markers where marker != "secret" {
            switch marker {
            case "env":      value = SynxMarkers.applyEnv(value, meta: meta, options: options)
            case "default":  value = SynxMarkers.applyDefault(value, meta: meta)
            case "calc":     value = SynxMarkers.applyCalc(value, key: key, root: root, local: parent)
            case "ref":      value = SynxMarkers.applyRef(value, root: root, namespaces: namespaces)
            case "alias":    value = SynxMarkers.applyRef(value, root: root, namespaces: namespaces)
            case "random":   value = SynxMarkers.applyRandom(value, meta: meta, rng: &rng)
            case "unique":   value = SynxMarkers.applyUnique(value)
            case "geo":      value = SynxMarkers.applyGeo(value, meta: meta, options: options)
            case "i18n":     value = SynxMarkers.applyI18n(value, meta: meta, options: options)
            case "split":    value = SynxMarkers.applySplit(value, meta: meta)
            case "join":     value = SynxMarkers.applyJoin(value, meta: meta)
            case "clamp":    value = SynxMarkers.applyClamp(value, meta: meta)
            case "round":    value = SynxMarkers.applyRound(value, meta: meta)
            case "map":      value = SynxMarkers.applyMap(value, meta: meta)
            case "format":   value = SynxMarkers.applyFormat(value, meta: meta,
                                                              interpolate: { self.interpolate($0, root: root) })
            case "replace":  value = SynxMarkers.applyReplace(value, meta: meta)
            case "sort":     value = SynxMarkers.applySort(value, meta: meta)
            case "sum":      value = SynxMarkers.applySum(value)
            case "fallback": value = SynxMarkers.applyFallback(value, meta: meta)
            case "once":     value = applyOnce(value, path: path, key: key)
            case "version":  value = SynxMarkers.applyVersion(value)
            case "watch":    value = SynxMarkers.applyWatch(value, basePath: options.basePath ?? ".")
            case "prompt":   value = SynxMarkers.applyPrompt(value,
                                                              interpolate: { self.interpolate($0, root: root) })
            case "vision", "audio":
                break // passthrough envelopes
            case "spam":     value = SynxMarkers.applySpam(value, key: key)
            case "inherit", "include", "import":
                break // handled in pre-pass / directives
            default:
                if !SynxMarkers.isBuiltin(marker), let fn = options.markerFns[marker] {
                    value = fn(key, meta.args, value)
                }
            }
        }

        if meta.hasMarker("secret") { value = SynxMarkers.applySecret(value) }

        // Apply runtime type cast for type hints (when no marker rewrote the value already).
        if let th = meta.typeHint {
            switch th {
            case "int":
                if case .int = value {} else if case .secret = value {} else {
                    if case .string(let str) = value, let n = Int64(str) {
                        value = .int(n)
                    } else if let d = valueToNumber(value) {
                        value = .int(Int64(d))
                    }
                }
            case "float":
                if case .float = value {} else {
                    if let d = valueToNumber(value) { value = .float(d) }
                }
            case "string":
                if case .string = value {} else if case .secret = value {} else {
                    value = .string(valueToString(value))
                }
            case "bool":
                if case .bool = value {} else {
                    let s = valueToString(value)
                    value = .bool(s == "true" || s == "1")
                }
            default: break
            }
        }

        // Constraint validation always runs last, after every marker has resolved.
        if let c = meta.constraints,
           let err = SynxValidate.constraintError(key: key, value: value, c: c) {
            reportViolation(err)
            value = .string(err)
        }

        parent.set(key, to: value)
        setObjectAt(path: path, in: &root, value: parent)
        result.root = .object(root)
    }

    // MARK: - once persistence

    private func applyOnce(_ v: SynxValue, path: String, key: String) -> SynxValue {
        if !onceLoaded {
            onceLoaded = true
            let base = options.basePath ?? "."
            let lockPath = (base as NSString).appendingPathComponent(".synx.lock")
            if let text = try? String(contentsOfFile: lockPath, encoding: .utf8) {
                for line in text.split(separator: "\n", omittingEmptySubsequences: true) {
                    let s = line.trimmingCharacters(in: .whitespaces)
                    if !s.isEmpty { onceKeys.insert(s) }
                }
            }
        }
        let lockKey = path.isEmpty ? key : path + "." + key
        if onceKeys.contains(lockKey) { return .null }
        onceNewKeys.insert(lockKey)
        return v
    }

    private func flushOnce() {
        guard !onceNewKeys.isEmpty else { return }
        let base = options.basePath ?? "."
        let lockPath = (base as NSString).appendingPathComponent(".synx.lock")
        let all = onceKeys.union(onceNewKeys)
        let joined = all.sorted().joined(separator: "\n") + "\n"
        try? joined.write(toFile: lockPath, atomically: true, encoding: .utf8)
    }

    // MARK: - path helpers

    func getValueAt(path: String, in root: SynxObject) -> SynxValue? {
        if path.isEmpty { return .object(root) }
        var current: SynxValue = .object(root)
        for seg in path.split(separator: ".") {
            guard case .object(let obj) = current, let next = obj[String(seg)] else { return nil }
            current = next
        }
        return current
    }

    func getObjectAt(path: String, in root: inout SynxObject) -> (value: SynxObject?, path: [String]) {
        if path.isEmpty { return (root, []) }
        let parts = path.split(separator: ".").map(String.init)
        var current = root
        for p in parts {
            guard let v = current[p], case .object(let obj) = v else { return (nil, parts) }
            current = obj
        }
        return (current, parts)
    }

    func setObjectAt(path: String, in root: inout SynxObject, value: SynxObject) {
        if path.isEmpty { root = value; return }
        let parts = path.split(separator: ".").map(String.init)
        // Walk down rebuilding upward (immutable-style with COW).
        func write(_ obj: inout SynxObject, _ idx: Int) {
            if idx == parts.count {
                obj = value
                return
            }
            let key = parts[idx]
            guard case .object(var child) = (obj[key] ?? .null) else { return }
            write(&child, idx + 1)
            obj.set(key, to: .object(child))
        }
        write(&root, 0)
    }

    // MARK: - jail / interpolate / helpers

    func jailPath(base: String, rel: String) -> String? {
        if rel.isEmpty { return nil }
        if rel.first == "/" || rel.first == "\\" { return nil }
        if rel.count >= 2 {
            let second = rel[rel.index(after: rel.startIndex)]
            if second == ":" { return nil } // Windows drive letter
        }
        if rel.hasPrefix("res://") || rel.hasPrefix("user://") { return nil }
        let normalized = rel.replacingOccurrences(of: "\\", with: "/")
        for seg in normalized.split(separator: "/", omittingEmptySubsequences: false) {
            if seg == ".." || seg == "..." { return nil }
        }
        return (base as NSString).appendingPathComponent(normalized)
    }

    func interpolate(_ s: String, root: SynxObject) -> String {
        var out = ""
        out.reserveCapacity(s.count)
        var i = s.startIndex
        while i < s.endIndex {
            let c = s[i]
            if c == "{" {
                if let end = s[s.index(after: i)...].firstIndex(of: "}") {
                    let inner = String(s[s.index(after: i)..<end])
                        .trimmingCharacters(in: .whitespaces)
                    if let v = lookup(path: inner, in: root) {
                        out.append(valueToString(v))
                    } else {
                        out.append("{")
                        out.append(inner)
                        out.append("}")
                    }
                    i = s.index(after: end)
                    continue
                } else {
                    out.append("{")
                    i = s.index(after: i)
                    continue
                }
            }
            out.append(c)
            i = s.index(after: i)
        }
        return out
    }

    private func lookup(path: String, in root: SynxObject) -> SynxValue? {
        // namespace.key.path form
        if let dot = path.firstIndex(of: ".") {
            let ns = String(path[..<dot])
            let rest = String(path[path.index(after: dot)...])
            if let nsRoot = namespaces[ns] {
                if let v = deepGet(rest, in: nsRoot) { return v }
            }
        }
        return deepGet(path, in: root)
    }

    private func deepGet(_ path: String, in root: SynxObject) -> SynxValue? {
        var current: SynxValue = .object(root)
        for seg in path.split(separator: ".") {
            guard case .object(let obj) = current, let next = obj[String(seg)] else { return nil }
            current = next
        }
        return current
    }
}

// MARK: - top-level helpers

func valueToString(_ v: SynxValue) -> String {
    switch v {
    case .null: return "null"
    case .bool(let b): return b ? "true" : "false"
    case .int(let n): return String(n)
    case .float(let f):
        return formatNumber(f)
    case .string(let s): return s
    case .secret: return "[SECRET]" // safety: plaintext must never reach a string sink
    case .array(let a):
        let parts = a.map { valueToString($0) }
        return "[" + parts.joined(separator: ", ") + "]"
    case .object: return "[Object]"
    }
}

/// Rust `format_number`: whole numbers print without a fraction, others as shortest `Display`.
func formatNumber(_ n: Double) -> String {
    if n.isFinite && n.truncatingRemainder(dividingBy: 1) == 0 && abs(n) < 9.223372036854775807e18 {
        return String(Int64(n))
    }
    return SynxJSON.displayNumber(n)
}

/// Does `meta` ask for the text to stay text (`(string)` or `[type:string]`)?
func wantsString(_ meta: SynxMeta) -> Bool {
    return meta.typeHint == "string" || meta.constraints?.typeName == "string"
}

/// Rust `cast_primitive`: quotes strip, `true`/`false`/`null`, exact `Int64`, then `Double`.
func castPrimitive(_ val: String) -> SynxValue {
    let scalars = Array(val.unicodeScalars)
    if scalars.count >= 2, let f = scalars.first, let l = scalars.last,
       (f == "\"" && l == "\"") || (f == "'" && l == "'") {
        var inner = String.UnicodeScalarView()
        inner.append(contentsOf: scalars[1..<(scalars.count - 1)])
        return .string(String(inner))
    }
    switch val {
    case "true":  return .bool(true)
    case "false": return .bool(false)
    case "null":  return .null
    default: break
    }
    if let n = Int64(val) { return .int(n) }
    // Hex floats (`0x1p3`) are not decimal numbers; Rust's parser rejects them.
    if !val.contains(where: { $0 == "x" || $0 == "X" }), let d = Double(val) { return .float(d) }
    return .string(val)
}

func valueToNumber(_ v: SynxValue) -> Double? {
    return v.asDouble
}

func regexMatches(_ value: String, pattern: String) -> Bool {
    guard let re = try? NSRegularExpression(pattern: pattern) else {
        return true // Invalid pattern — do not reject.
    }
    let range = NSRange(value.startIndex..<value.endIndex, in: value)
    return re.firstMatch(in: value, options: [], range: range) != nil
}
