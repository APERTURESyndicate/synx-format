// SYNX `!active` engine — resolution order, pre-passes, registries, validation.
// Port of crates/synx-core/src/engine.rs (control-flow half).
import Foundation

public enum SynxEngine {

    public static let maxResolveDepth = 512

    /// Apply markers and constraints to `result` in place. No-op unless `.active`.
    public static func resolve(_ result: inout SynxParseResult, options: SynxOptions) {
        guard result.mode == .active else { return }
        guard case .object(let o) = result.root else { return }
        let root = MObject(o)
        resolveTree(root, &result, options: options)
        result.root = .object(root.toObject())
    }

    /// Resolve the working tree of `result` (its `root` field is left alone).
    static func resolveTree(_ root: MObject, _ result: inout SynxParseResult, options: SynxOptions) {
        guard result.mode == .active else { return }
        var metadata = result.metadata
        result.metadata = [:]
        let includeDirectives = result.includes
        result.includes = []
        let useDirectives = result.uses
        result.uses = []

        // `options.env["SYNX_SEED"]` makes `:random` and friends reproducible (Swift extension).
        if let s = options.env?["SYNX_SEED"], let seed = RustNumber.parseU64(s) { SynxRng.seed(seed) }

        let packages = Files.loadPackages(useDirectives, options)
        var includes = Files.loadIncludes(includeDirectives, options)
        // Root `:include` keys the pre-pass already resolved, as `[path, root]`;
        // kept apart from `includes`, whose size `{key:include}` reads.
        var preresolved = SynxIndexMap<MValue>()

        // `:include` / `:import` root keys are include aliases too, so
        // `{leaf:<key>}` interpolation works for `db:include ./common.synx`.
        if let mm = metadata[""] {
            let base = options.basePath ?? "."
            for (i, key) in root.keys.enumerated() {
                guard let meta = mm[key], meta.hasMarker("include") || meta.hasMarker("import") else { continue }
                // safety: the included document's own pre-pass includes again;
                // without the depth cap a file that includes itself never ends.
                if options.includeDepth >= (options.maxIncludeDepth ?? Files.defaultMaxIncludeDepth) { continue }
                guard let file = root.values[i].string else { continue }
                guard case .ok(let path) = Files.jail(base, file), Files.sizeError(path) == nil,
                      case .ok(let text) = Files.read(path, ctx: file) else { continue }
                let included = Files.loadSynx(text, path, options)
                // perf: the key's `:include` marker takes this result instead of
                // resolving the file again (each level twice cost 2^depth).
                preresolved[key] = .array(MArray([.string(file), included]))
                if includes.position(of: key) == nil { includes[key] = included }
            }
        }
        for (alias, value) in packages where root.find(alias) == nil {
            root.keys.append(alias)
            root.values.append(value.deepCopy())
        }

        var types = SynxIndexMap<String>()
        var constraints = SynxIndexMap<SynxConstraints>()
        collectRegistries(.object(root), metadata, "", &types, &constraints, 0)

        applyInheritance(root, &metadata)
        root.retain { !$0.utf8.starts(with: [0x5F]) }
        maskSecretLiterals(.object(root), metadata, "")

        // Resolution replaces values but never adds or removes keys: the root's
        // key set is fixed from here on.
        var special: [String] = []
        for k in root.keys where Calc.isSpecialKey(k) { special.append(k) }
        let r = Resolver(options: options, root: root, metadata: metadata, includes: includes,
                         preresolved: preresolved, rootSpecial: special)
        r.resolve(.object(root), "", 0)

        if !constraints.isEmpty || !types.isEmpty {
            validateFields(root, constraints, types, options)
        }
        result.metadata = metadata
        result.includes = includeDirectives
    }

    // MARK: - registries

    private static func mergeConstraints(_ base: inout SynxConstraints, _ inc: SynxConstraints) {
        if inc.required { base.required = true }
        if inc.readonly { base.readonly = true }
        if let b = inc.min { base.min = base.min.map { Swift.max($0, b) } ?? b }
        if let b = inc.max { base.max = base.max.map { Swift.min($0, b) } ?? b }
        if base.typeName == nil { base.typeName = inc.typeName }
        if base.pattern == nil { base.pattern = inc.pattern }
        if base.enumValues == nil { base.enumValues = inc.enumValues }
    }

    /// Registries from the live tree: each object's own declarations first, then
    /// its children in document order. The first type for a name wins.
    private static func collectRegistries(_ value: MValue, _ metadata: SynxMetadataTree, _ path: String,
                                          _ types: inout SynxIndexMap<String>,
                                          _ constraints: inout SynxIndexMap<SynxConstraints>, _ depth: Int) {
        if depth > SynxEngine.maxResolveDepth { return }
        guard let map = value.object else { return }
        if let mm = metadata[path] {
            for (key, meta) in mm {
                if let hint = meta.typeHint, types[key] == nil { types[key] = hint }
                if let c = meta.constraints {
                    if constraints[key] != nil {
                        constraints.update(key, default: c) { mergeConstraints(&$0, c) }
                    } else {
                        constraints[key] = c
                    }
                }
            }
        }
        for i in 0..<map.count {
            let child = map.values[i]
            guard child.isObjectOrArray else { continue }
            let childPath = Resolver.childPath(path, map.keys[i])
            if let arr = child.array {
                for (idx, item) in arr.items.enumerated() where item.object != nil {
                    collectRegistries(item, metadata, Resolver.itemPath(childPath, idx), &types, &constraints, depth + 1)
                }
            } else {
                collectRegistries(child, metadata, childPath, &types, &constraints, depth + 1)
            }
        }
    }

    // MARK: - :inherit

    /// Several parents merged left to right, the child overrides; each field keeps
    /// the declaration of the object its value came from; inherited nested
    /// subtrees are copies carrying their own declarations.
    private static func applyInheritance(_ root: MObject, _ metadata: inout SynxMetadataTree) {
        guard let rootMeta = metadata[""] else { return }
        var inherits: [(String, [String])] = []
        for (key, meta) in rootMeta {
            guard let idx = meta.markerIndex("inherit") else { continue }
            var parents = Array(meta.markers[(idx + 1)...])
            if parents.isEmpty && !meta.args.isEmpty { parents = meta.args }
            if !parents.isEmpty { inherits.append((key, parents)) }
        }
        for (childKey, parents) in inherits {
            // Only an object inherits; a list or a scalar is left as written.
            guard let childObj = root.get(childKey)?.object else { continue }
            let merged = MObject()
            var source: [(key: String, from: String?)] = []
            func note(_ k: String, _ from: String?) {
                if let i = source.firstIndex(where: { $0.key.bytesEqual(k) }) { source[i].from = from } else { source.append((k, from)) }
            }
            for parentName in parents {
                guard let p = root.get(parentName)?.object else { continue }
                for i in 0..<p.count {
                    merged.set(p.keys[i], p.values[i].deepCopy())
                    note(p.keys[i], parentName)
                }
            }
            for i in 0..<childObj.count {
                merged.set(childObj.keys[i], childObj.values[i].deepCopy())
                note(childObj.keys[i], nil)
            }
            var top = SynxMetaMap()
            var nested: [(String, SynxMetaMap)] = []
            for (k, src) in source {
                let from = src ?? childKey
                if let m = metadata[MetaPath.segment(from)]?[k] { top[k] = m }
                if let parent = src {
                    // The inherited subtree is a copy; its nested markers move with it.
                    let prefix = MetaPath.segment(parent) + "." + MetaPath.segment(k)
                    for (p, mm) in metadata {
                        guard p.bytesHasPrefix(prefix) else { continue }
                        let rest = String(decoding: Array(p.utf8).dropFirst(prefix.utf8.count), as: UTF8.self)
                        if !rest.isEmpty && !rest.bytesHasPrefix(".") { continue }
                        nested.append((MetaPath.segment(childKey) + "." + MetaPath.segment(k) + rest, mm))
                    }
                }
            }
            root.set(childKey, .object(merged))
            if top.isEmpty {
                metadata.removeValue(forKey: MetaPath.segment(childKey))
            } else {
                metadata[MetaPath.segment(childKey)] = top
            }
            for (p, mm) in nested { metadata[p] = mm }
        }
    }

    // MARK: - :secret pre-pass

    private static let resolvingBeforeSecret: Set<String> = [
        "env", "include", "import", "watch", "ref", "alias", "calc", "map", "i18n", "default",
        "fallback", "once", "split", "join", "format", "replace",
    ]

    /// safety: literal `:secret` values are masked before any resolution, so no
    /// `{interpolation}` can read the plaintext.
    private static func maskSecretLiterals(_ value: MValue, _ metadata: SynxMetadataTree, _ path: String) {
        guard let map = value.object else { return }
        if let mm = metadata[path] {
            for (key, meta) in mm where meta.hasMarker("secret") && !meta.markers.contains(where: { resolvingBeforeSecret.contains($0) }) {
                guard let i = map.find(key) else { continue }
                let v = map.values[i]
                if !v.isSecret && !v.isObjectOrArray { map.values[i] = .secret(v.text) }
            }
        }
        for i in 0..<map.count {
            let child = map.values[i]
            guard child.isObjectOrArray else { continue }
            let childPath = Resolver.childPath(path, map.keys[i])
            if let arr = child.array {
                for (idx, item) in arr.items.enumerated() where item.object != nil {
                    maskSecretLiterals(item, metadata, Resolver.itemPath(childPath, idx))
                }
            } else {
                maskSecretLiterals(child, metadata, childPath)
            }
        }
    }

    // MARK: - validation

    private static func matchesType(_ v: MValue, _ t: String) -> Bool {
        switch (t, v) {
        case ("int", .int), ("float", .float), ("float", .int), ("bool", .bool),
             ("string", .string), ("string", .secret), ("array", .array), ("object", .object):
            return true
        case ("int", _), ("float", _), ("bool", _), ("string", _), ("array", _), ("object", _):
            return false
        default:
            return true  // unknown types are accepted
        }
    }

    /// Constraints then type for every field. A value already holding a
    /// CONSTRAINT_ERR / TYPE_ERR is not measured again against constraints.
    private static func validateFields(_ map: MObject, _ constraints: SynxIndexMap<SynxConstraints>,
                                       _ types: SynxIndexMap<String>, _ options: SynxOptions) {
        for i in 0..<map.count {
            let key = map.keys[i]
            if let c = constraints[key] {
                var already = false
                if let s = map.values[i].string {
                    already = s.bytesHasPrefix("CONSTRAINT_ERR:") || s.bytesHasPrefix("TYPE_ERR:")
                }
                if !already, let err = Constraints.violation(key, map.values[i], c) {
                    report(options, err)
                    map.values[i] = .string(err)
                }
            }
            if let expected = types[key], !matchesType(map.values[i], expected) {
                let err = "TYPE_ERR: '\(key)' expected \(expected) but got \(map.values[i].typeName)"
                report(options, err)
                map.values[i] = .string(err)
            }
            switch map.values[i] {
            case .object(let child): validateFields(child, constraints, types, options)
            case .array(let arr):
                for item in arr.items { if let o = item.object { validateFields(o, constraints, types, options) } }
            default: break
            }
        }
    }

    static func report(_ options: SynxOptions, _ message: String) {
        guard options.strict else { return }
        FileHandle.standardError.write(Data("synx: \(message)\n".utf8))
    }
}

// MARK: - lookups

/// `root.key` directly, then a dotted path through objects (Rust `deep_get`).
func deepGet(_ root: MValue, _ path: String) -> MValue? {
    guard let map = root.object else { return nil }
    if let v = map.get(path) { return v }
    var current = root
    for part in splitBytes(path, 0x2E) {
        guard let o = current.object, let next = o.get(part) else { return nil }
        current = next
    }
    return current
}

// MARK: - the resolver

final class Resolver {
    let options: SynxOptions
    let root: MObject
    let metadata: SynxMetadataTree
    let includes: SynxIndexMap<MValue>
    let preresolved: SynxIndexMap<MValue>
    let rootSpecial: [String]

    init(options: SynxOptions, root: MObject, metadata: SynxMetadataTree, includes: SynxIndexMap<MValue>,
         preresolved: SynxIndexMap<MValue>, rootSpecial: [String]) {
        self.options = options
        self.root = root
        self.metadata = metadata
        self.includes = includes
        self.preresolved = preresolved
        self.rootSpecial = rootSpecial
    }

    /// One pass in document order: each key's subtree, then the key's own markers
    /// — a marker sees every key before it resolved, at any depth — then `{…}`.
    func resolve(_ value: MValue, _ path: String, _ depth: Int) {
        guard let map = value.object else { return }
        if depth >= SynxEngine.maxResolveDepth {
            for i in 0..<map.count { map.values[i] = .string("NESTING_ERR: maximum object nesting depth exceeded") }
            return
        }
        let mm = metadata[path]
        let count = map.count
        for i in 0..<count {
            let child = map.values[i]
            if child.isObjectOrArray {
                let childPath = Resolver.childPath(path, map.keys[i])
                if let arr = child.array {
                    for (idx, item) in arr.items.enumerated() where item.object != nil {
                        resolve(item, Resolver.itemPath(childPath, idx), depth + 1)
                    }
                } else {
                    resolve(child, childPath, depth + 1)
                }
            }
            guard let meta = mm?[map.keys[i]] else { continue }
            applyMarkers(map, i, meta, path)
        }
        for i in 0..<count {
            guard let s = map.values[i].string, s.utf8.contains(0x7B) else { continue }
            let out = interpolate(s, map)
            if !out.bytesEqual(s) { map.values[i] = .string(out) }
        }
    }

    static func childPath(_ path: String, _ key: String) -> String {
        var p = path
        if !p.isEmpty { p.append(".") }
        MetaPath.pushKey(&p, key)
        return p
    }

    static func itemPath(_ listPath: String, _ idx: Int) -> String {
        var p = listPath
        MetaPath.pushItem(&p, idx)
        return p
    }

    /// `{name}`, `{a.b}`, `{name:alias}`, `{name:include}`.
    func interpolate(_ tpl: String, _ local: MObject) -> String {
        let cap = Files.maxScratch
        let t = Array(tpl.utf8)
        var out: [UInt8] = []
        out.reserveCapacity(min(t.count, cap))
        func pushCapped(_ s: String) {
            let u = Array(s.utf8)
            let room = cap - min(out.count, cap)
            if room > 0 { out.append(contentsOf: u.prefix(utf8Cut(u, room))) }
        }
        func pushLiteral(_ inner: ArraySlice<UInt8>) {
            out.append(0x7B)
            let rem = cap > out.count + 1 ? cap - (out.count + 1) : 0
            if rem > 0 { let a = Array(inner); out.append(contentsOf: a.prefix(utf8Cut(a, rem))) }
            if out.count < cap { out.append(0x7D) }
        }
        var i = 0
        while i < t.count {
            if out.count >= cap { break }
            if t[i] == 0x7B, let close = t[(i + 1)...].firstIndex(of: 0x7D) {
                let inner = t[(i + 1)..<close]
                let colon = inner.firstIndex(of: 0x3A)
                let ref = colon.map { t[(i + 1)..<$0] } ?? inner
                if Resolver.isRefName(ref) {
                    let name = String(decoding: ref, as: UTF8.self)
                    var val: MValue? = nil
                    if let c = colon {
                        let scope = String(decoding: t[(c + 1)..<close], as: UTF8.self)
                        if scope == "include" {
                            if includes.count == 1 { val = deepGet(includes.entries[0].value, name) }
                        } else if let inc = includes[scope] {
                            val = deepGet(inc, name)
                        }
                        if let v = val { pushCapped(v.text) } else { pushLiteral(inner) }
                    } else {
                        val = deepGet(.object(root), name) ?? local.get(name)
                        if let v = val { pushCapped(v.text) } else { pushLiteral(ref) }
                    }
                    i = close + 1
                    continue
                }
            }
            let n = utf8Len(t[i])
            if out.count < cap { out.append(contentsOf: t[i..<min(i + n, t.count)]) }
            i += n
        }
        return String(decoding: out, as: UTF8.self)
    }

    /// Reference names: Unicode alphanumerics, `_` and `.`.
    static func isRefName(_ s: ArraySlice<UInt8>) -> Bool {
        for u in String(decoding: s, as: UTF8.self).unicodeScalars where !(isAlphanumeric(u) || u == "_" || u == ".") {
            return false
        }
        return true
    }
}

/// Longest prefix of `u` within `limit` bytes that does not split a character.
func utf8Cut(_ u: [UInt8], _ limit: Int) -> Int {
    if u.count <= limit { return u.count }
    var n = limit
    while n > 0 && (u[n] & 0xC0) == 0x80 { n -= 1 }
    return n
}

// MARK: - files

enum Files {
    static let maxFileSize = 10 * 1024 * 1024
    static let defaultMaxIncludeDepth = 16
    static let maxScratch = 4 * 1024 * 1024

    enum Outcome<T> {
        case ok(T)
        case err(String)
    }

    /// Path sandbox (Rust `jail_path`).
    static func jail(_ base: String, _ filePath: String) -> Outcome<URL> {
        let quoted = "'\(filePath)'"
        if filePath.utf8.first == 0x2F || filePath.utf8.first == 0x5C {
            return .err("SECURITY: rooted paths are not allowed: \(quoted)")
        }
        if isAbsolute(filePath) { return .err("SECURITY: absolute paths are not allowed: \(quoted)") }
        let baseURL = URL(fileURLWithPath: base)
        let canonicalBase = canonical(baseURL)
        let baseCanonical = canonicalBase ?? baseURL
        var full = baseCanonical.appendingPathComponent(filePath)
        #if os(Windows)
        // Rust's canonical base on Windows is verbatim (`\\?\C:\…`), and pushing
        // onto it resolves `.` and `..` lexically.
        if canonicalBase != nil { full = full.standardizedFileURL }
        #endif
        guard let fullCanonical = canonical(full) else {
            if full.path.contains("..") { return .err("SECURITY: path traversal detected: \(quoted)") }
            if !startsWith(full, baseCanonical) { return .err("SECURITY: path escapes base directory: \(quoted)") }
            return .ok(full)
        }
        if !startsWith(fullCanonical, baseCanonical) { return .err("SECURITY: path escapes base directory: \(quoted)") }
        return .ok(fullCanonical)
    }

    private static func isAbsolute(_ p: String) -> Bool {
        #if os(Windows)
        let u = Array(p.utf8)
        // `C:\x` / `C:/x` and UNC `\\server` (a leading separator is rooted, above).
        if u.count >= 3 && ((u[0] | 0x20) >= 0x61 && (u[0] | 0x20) <= 0x7A) && u[1] == 0x3A && (u[2] == 0x5C || u[2] == 0x2F) {
            return true
        }
        return false
        #else
        return p.hasPrefix("/")
        #endif
    }

    /// The canonical path of an existing file or directory.
    private static func canonical(_ url: URL) -> URL? {
        guard FileManager.default.fileExists(atPath: url.path) else { return nil }
        return url.resolvingSymlinksInPath().standardizedFileURL
    }

    /// Rust `Path::starts_with`: whole components.
    private static func startsWith(_ full: URL, _ base: URL) -> Bool {
        let f = full.standardizedFileURL.pathComponents
        let b = base.standardizedFileURL.pathComponents
        if b.count > f.count { return false }
        for (x, y) in zip(b, f) where !x.bytesEqual(y) {
            #if os(Windows)
            if x.lowercased() == y.lowercased() { continue }
            #endif
            return false
        }
        return true
    }

    /// `SECURITY: file too large …`, or nil.
    static func sizeError(_ url: URL) -> String? {
        guard let attrs = try? FileManager.default.attributesOfItem(atPath: url.path),
              let size = (attrs[.size] as? NSNumber)?.intValue else { return nil }
        if size > maxFileSize { return "SECURITY: file too large (\(size) bytes, max \(maxFileSize))" }
        return nil
    }

    /// Read a UTF-8 text file; errors read as Rust normalises them.
    static func read(_ url: URL, ctx: String) -> Outcome<String> {
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir) else {
            return .err("file not found: \(ctx)")
        }
        if isDir.boolValue { return .err("permission denied: \(ctx)") }
        guard let data = FileManager.default.contents(atPath: url.path) else { return .err("permission denied: \(ctx)") }
        guard let text = String(data: data, encoding: .utf8), Array(text.utf8).count == data.count else {
            return .err("stream did not contain valid UTF-8")
        }
        return .ok(text)
    }

    /// Parse, and resolve an `!active` file with the include depth bumped.
    static func loadSynx(_ text: String, _ file: URL, _ options: SynxOptions) -> MValue {
        let (root, parsed) = SynxParser.parseTree(text)
        var result = parsed
        if result.mode == .active {
            var child = options
            child.includeDepth += 1
            child.basePath = file.deletingLastPathComponent().path
            SynxEngine.resolveTree(root, &result, options: child)
        }
        return .object(root)
    }

    static func loadIncludes(_ directives: [SynxIncludeDirective], _ options: SynxOptions) -> SynxIndexMap<MValue> {
        var map = SynxIndexMap<MValue>()
        if options.includeDepth >= (options.maxIncludeDepth ?? defaultMaxIncludeDepth) { return map }
        let base = options.basePath ?? "."
        for inc in directives {
            guard case .ok(let path) = jail(base, inc.path), sizeError(path) == nil,
                  case .ok(let text) = read(path, ctx: inc.path) else { continue }
            map[inc.alias] = loadSynx(text, path, options)
        }
        return map
    }

    /// Does `path` name something inside its base directory: relative, with no
    /// root, drive or `..` segment? Judged by its text, so a package directory
    /// that is itself a symlink keeps working (Rust `is_contained_relative`;
    /// its `Path::is_absolute` adds nothing past the root and `:` checks).
    /// safety: `!use @a/../../secret` and a manifest `main ../../x` read files
    /// from anywhere — the escape `:include` refuses through `jail`.
    static func isContainedRelative(_ path: String) -> Bool {
        let b = Array(path.utf8)
        guard let first = b.first, first != 0x2F, first != 0x5C, !b.contains(0x3A) else { return false }
        return !b.split(omittingEmptySubsequences: false, whereSeparator: { $0 == 0x2F || $0 == 0x5C })
            .contains { $0.elementsEqual([0x2E, 0x2E]) }
    }

    /// `main` / `entry` of `synx-pkg.synx`, if the manifest names a contained one.
    private static func manifestMain(_ pkgDir: URL) -> URL? {
        guard case .ok(let text) = read(pkgDir.appendingPathComponent("synx-pkg.synx"), ctx: "") else { return nil }
        for line in splitBytes(text, 0x0A) {
            let t = line.rustTrimmed
            for prefix in ["main ", "entry "] where t.bytesHasPrefix(prefix) {
                let entry = String(decoding: Array(t.utf8).dropFirst(prefix.utf8.count), as: UTF8.self).rustTrimmed
                if isContainedRelative(entry) { return pkgDir.appendingPathComponent(entry) }
            }
        }
        return nil
    }

    /// `<base>/<packages_path>/@scope/name` → its manifest's entry or `src/main.synx`.
    static func loadPackages(_ directives: [SynxUseDirective], _ options: SynxOptions) -> SynxIndexMap<MValue> {
        var map = SynxIndexMap<MValue>()
        let pkgRoot = URL(fileURLWithPath: options.basePath ?? ".")
            .appendingPathComponent(options.packagesPath ?? "./synx_packages")
        for ud in directives where isContainedRelative(ud.package) {
            let pkgDir = pkgRoot.appendingPathComponent(ud.package)
            let entry = manifestMain(pkgDir) ?? pkgDir.appendingPathComponent("src").appendingPathComponent("main.synx")
            var isDir: ObjCBool = false
            guard FileManager.default.fileExists(atPath: entry.path, isDirectory: &isDir), !isDir.boolValue,
                  sizeError(entry) == nil, case .ok(let text) = read(entry, ctx: "") else { continue }
            let (root, parsed) = SynxParser.parseTree(text)
            var result = parsed
            if result.mode == .active {
                var child = options
                child.includeDepth += 1
                child.basePath = entry.deletingLastPathComponent().path
                SynxEngine.resolveTree(root, &result, options: child)
            }
            map[ud.alias] = .object(root)
        }
        return map
    }
}
