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
    static func resolveTree(_ root: MObject, _ result: inout SynxParseResult, options inOptions: SynxOptions) {
        guard result.mode == .active else { return }
        var options = inOptions
        if options.state == nil { options.state = ResolveState() }
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
                      case .ok(let included) = Files.include(path, file, options) else { continue }
                // perf: the key's `:include` marker takes this result instead of
                // resolving the file again (each level twice cost 2^depth).
                preresolved[key] = .array(MArray([.string(file), included]))
                // safety: the tree is also what `{leaf:key}` reads. Under `:secret`
                // that scope gets the masked tree, or `x {password:db}` printed what
                // `db:secret:include` hid.
                if includes.position(of: key) == nil {
                    includes[key] = meta.hasMarker("secret") ? maskSecretValue(included.deepCopy()) : included
                }
            }
        }
        for (alias, value) in packages where root.find(alias) == nil {
            root.keys.append(alias)
            root.values.append(value.deepCopy())
        }

        var types = SynxIndexMap<String>()
        var constraints = SynxIndexMap<SynxConstraints>()
        collectRegistries(.object(root), metadata, "", &types, &constraints, 0)

        applyInheritance(root, &metadata, options.state!)
        root.retain { !$0.utf8.starts(with: [0x5F]) }
        // safety: every value under `:secret` is masked before any resolution, so
        // no field that names it — `{that_key}`, `:alias`, `:map` — reads the
        // plain text, wherever in the document it stands.
        var originals: [String: [String: MValue]] = [:]
        maskSecretLiterals(.object(root), metadata, "", &originals)

        // Resolution replaces values but never adds or removes keys: the root's
        // key set is fixed from here on.
        var special: [String] = []
        for k in root.keys where Calc.isSpecialKey(k) { special.append(k) }
        let r = Resolver(options: options, root: root, metadata: metadata, includes: includes,
                         preresolved: preresolved, rootSpecial: special)
        r.secretOriginals = originals
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
    ///
    /// safety: every inherited field and declaration is a copy, charged to the
    /// document's budget — 3 000 children of a 3 000-field parent are nine
    /// million fields from a 130 KB file.
    private static func applyInheritance(_ root: MObject, _ metadata: inout SynxMetadataTree, _ state: ResolveState) {
        guard let rootMeta = metadata[""] else { return }
        var inherits: [(String, [String])] = []
        for (key, meta) in rootMeta {
            guard let idx = meta.markerIndex("inherit") else { continue }
            var parents = Array(meta.markers[(idx + 1)...])
            if parents.isEmpty && !meta.args.isEmpty { parents = meta.args }
            if !parents.isEmpty { inherits.append((key, parents)) }
        }
        if inherits.isEmpty { return }
        // perf: the declarations nested under each `object.field`, found once.
        // Every inherited field walked the whole table: a chain of 2 000 took
        // more than 40 s.
        var nestedIndex: [String: [String]] = [:]
        for (p, _) in metadata { if let head = twoSegmentHead(p) { nestedIndex[head, default: []].append(p) } }
        for (childKey, parents) in inherits {
            // Only an object inherits; a list or a scalar is left as written.
            guard let childObj = root.get(childKey)?.object else { continue }
            let merged = MObject()
            var sourceKeys: [String] = []
            var sourceFrom: [String: String?] = [:]
            func note(_ k: String, _ from: String?) {
                if sourceFrom[k] == nil { sourceKeys.append(k) }
                sourceFrom[k] = .some(from)
            }
            var withinBudget = true
            parentsLoop: for parentName in parents {
                guard let p = root.get(parentName)?.object else { continue }
                for i in 0..<p.count {
                    if !state.charge(valueCost(p.values[i]).cost + p.keys[i].utf8.count) {
                        withinBudget = false
                        break parentsLoop
                    }
                    merged.set(p.keys[i], p.values[i].deepCopy())
                    note(p.keys[i], parentName)
                }
            }
            if !withinBudget {
                root.set(childKey, .string(ResolveState.limitErr))
                continue
            }
            for i in 0..<childObj.count {
                merged.set(childObj.keys[i], childObj.values[i].deepCopy())
                note(childObj.keys[i], nil)
            }
            var top = SynxMetaMap()
            var nested: [(String, SynxMetaMap)] = []
            sourceLoop: for k in sourceKeys {
                let src = sourceFrom[k] ?? nil
                let from = src ?? childKey
                if let m = metadata[MetaPath.segment(from)]?[k] {
                    if src != nil && !state.charge(metaCost(k, m)) {
                        withinBudget = false
                        break sourceLoop
                    }
                    top[k] = m
                }
                if let parent = src {
                    // The inherited subtree is a copy; its nested markers move with it.
                    let prefix = MetaPath.segment(parent) + "." + MetaPath.segment(k)
                    for p in nestedIndex[prefix] ?? [] {
                        guard let mm = metadata[p] else { continue }
                        if !state.charge(metaMapCost(mm) + p.utf8.count) {
                            withinBudget = false
                            break sourceLoop
                        }
                        let rest = String(decoding: Array(p.utf8).dropFirst(prefix.utf8.count), as: UTF8.self)
                        nested.append((MetaPath.segment(childKey) + "." + MetaPath.segment(k) + rest, mm))
                    }
                }
            }
            if !withinBudget {
                root.set(childKey, .string(ResolveState.limitErr))
                continue
            }
            root.set(childKey, .object(merged))
            if top.isEmpty {
                metadata.removeValue(forKey: MetaPath.segment(childKey))
            } else {
                metadata[MetaPath.segment(childKey)] = top
            }
            for (p, mm) in nested {
                // A later child may inherit from this one: its copies are indexed too.
                if metadata[p] == nil, let head = twoSegmentHead(p) { nestedIndex[head, default: []].append(p) }
                metadata[p] = mm
            }
        }
    }

    /// The first two segments of a metadata path (`parent.field`), or nil for a
    /// path of one segment. A `.` escaped with `\` is part of its segment.
    private static func twoSegmentHead(_ path: String) -> String? {
        let u = Array(path.utf8)
        var dots = 0
        var i = 0
        while i < u.count {
            if u[i] == 0x5C {
                i += 1
            } else if u[i] == 0x2E {
                dots += 1
                if dots == 2 { return String(decoding: u[..<i], as: UTF8.self) }
            }
            i += 1
        }
        return dots == 1 ? path : nil
    }

    /// What copying one key's declarations costs against the budget (synx-core `meta_cost`).
    private static func metaCost(_ key: String, _ meta: SynxMeta) -> Int {
        var cost = 2 * ResolveState.valueCost + key.utf8.count
        for s in meta.markers { cost += 24 + s.utf8.count }
        for s in meta.args { cost += 24 + s.utf8.count }
        if let h = meta.typeHint { cost += h.utf8.count }
        if let c = meta.constraints {
            if let t = c.typeName { cost += 24 + t.utf8.count }
            if let p = c.pattern { cost += 24 + p.utf8.count }
            for e in c.enumValues ?? [] { cost += 24 + e.utf8.count }
        }
        return cost
    }

    private static func metaMapCost(_ mm: SynxMetaMap) -> Int {
        var cost = ResolveState.valueCost
        for (k, m) in mm { cost += metaCost(k, m) }
        return cost
    }

    // MARK: - :secret pre-pass

    /// The `:secret` pre-pass: every value under `:secret` is masked before any
    /// resolution. What the key held is kept in `originals` and comes back for
    /// the key's own turn (its subtree and its marker chain); what that produces
    /// is masked again by the `:secret` marker.
    ///
    /// safety: a literal next to `:default`, `:replace`, `:format`, `:split`, …
    /// was left in the clear until its own turn, and the fields of a group under
    /// `:secret` were never masked here: `leak:alias password` above
    /// `password:secret:default:x hunter2` read `hunter2`.
    static func maskSecretLiterals(_ value: MValue, _ metadata: SynxMetadataTree, _ path: String,
                                   _ originals: inout [String: [String: MValue]]) {
        guard let map = value.object else { return }
        if let mm = metadata[path] {
            for (key, meta) in mm where meta.hasMarker("secret") {
                guard let i = map.find(key) else { continue }
                let v = map.values[i]
                if v.isSecret || v.isNull { continue }
                originals[path, default: [:]][key] = v
                map.values[i] = maskSecretValue(v.deepCopy())
            }
        }
        for i in 0..<map.count {
            let child = map.values[i]
            guard child.isObjectOrArray else { continue }
            maskSecretSubtree(child, metadata, Resolver.childPath(path, map.keys[i]), &originals)
        }
    }

    /// `maskSecretLiterals` for the object or list at `path`.
    static func maskSecretSubtree(_ child: MValue, _ metadata: SynxMetadataTree, _ path: String,
                                  _ originals: inout [String: [String: MValue]]) {
        if let arr = child.array {
            for (idx, item) in arr.items.enumerated() where item.object != nil {
                maskSecretLiterals(item, metadata, Resolver.itemPath(path, idx), &originals)
            }
        } else {
            maskSecretLiterals(child, metadata, path, &originals)
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

// MARK: - budgets and secrets shared by a top-level resolve

/// What a top-level resolve shares with the resolves it starts for included
/// documents and packages (synx-core keeps the same state per thread).
final class ResolveState: @unchecked Sendable {
    /// Included documents one top-level resolve may read.
    static let maxIncludeReads = 1024
    /// Bytes one top-level resolve may read through `:include`, `!include`,
    /// `!use` and `:watch`.
    static let maxReadBytes = 64 * 1024 * 1024
    /// Bytes the markers of one top-level resolve may add to the document:
    /// copies by `:alias`, `:ref`, `:spam`, `:inherit`, text by `{…}`,
    /// `:replace`, `:join`, `:format`, `:prompt`. synx-core's costs, so a
    /// document runs out at the same key in every implementation.
    static let maxExpansionBytes = 64 * 1024 * 1024
    /// What one copied value costs, text aside.
    static let valueCost = 128
    static let limitErr = "LIMIT_ERR: resolution budget exceeded"
    static let nestingErr = "NESTING_ERR: maximum object nesting depth exceeded"

    /// Files whose resolution is in progress, outermost first.
    var chain: [String] = []
    var reads = 0
    var readBytes = 0
    var expansion = 0

    /// Take `bytes` from the expansion budget; false once it is spent (and it stays spent).
    func charge(_ bytes: Int) -> Bool {
        if bytes >= 0 && bytes <= ResolveState.maxExpansionBytes - expansion {
            expansion += bytes
            return true
        }
        expansion = ResolveState.maxExpansionBytes
        return false
    }

    var room: Int { ResolveState.maxExpansionBytes - expansion }
}

/// What copying `v` costs, and how many levels of objects and lists it nests.
func valueCost(_ v: MValue) -> (cost: Int, nesting: Int) {
    switch v {
    case .string(let s), .secret(let s): return (ResolveState.valueCost + s.utf8.count, 0)
    case .array(let a):
        var cost = ResolveState.valueCost, depth = 0
        for item in a.items {
            let (c, d) = valueCost(item)
            cost += c
            depth = max(depth, d)
        }
        return (cost, depth + 1)
    case .object(let o):
        var cost = ResolveState.valueCost, depth = 0
        for i in 0..<o.count {
            let (c, d) = valueCost(o.values[i])
            cost += c + o.keys[i].utf8.count
            depth = max(depth, d)
        }
        return (cost, depth + 1)
    default: return (ResolveState.valueCost, 0)
    }
}

/// The prefixes of the strings the engine leaves in place of a value it could not produce.
private let runtimeErrorPrefixes = [
    "INCLUDE_ERR:", "WATCH_ERR:", "CALC_ERR:", "SPAM_ERR:", "CONSTRAINT_ERR:",
    "ALIAS_ERR:", "NESTING_ERR:", "LIMIT_ERR:", "TYPE_ERR:", "WASM_ERR:",
]

func isEngineError(_ s: String) -> Bool {
    guard let f = s.utf8.first, f >= 0x41 && f <= 0x5A else { return false }
    return runtimeErrorPrefixes.contains { s.bytesHasPrefix($0) }
}

/// Make a resolved value a secret. A scalar becomes the secret of its text; a
/// list or a group keeps its shape and every scalar in it is masked (in
/// place). Null stays null — wrapping it made the secret the text "null",
/// which `[required]` read as a present value — an existing secret is kept,
/// and an engine error stays readable.
func maskSecretValue(_ v: MValue) -> MValue {
    switch v {
    case .secret, .null: return v
    case .array(let a):
        for i in 0..<a.items.count { a.items[i] = maskSecretValue(a.items[i]) }
        return v
    case .object(let o):
        for i in 0..<o.count { o.values[i] = maskSecretValue(o.values[i]) }
        return v
    case .string(let s) where isEngineError(s): return v
    default: return .secret(v.text)
    }
}

final class Resolver {
    let options: SynxOptions
    let root: MObject
    let metadata: SynxMetadataTree
    let includes: SynxIndexMap<MValue>
    let preresolved: SynxIndexMap<MValue>
    let rootSpecial: [String]
    /// What the keys under `:secret` held before the pre-pass, by object path and key.
    var secretOriginals: [String: [String: MValue]] = [:]
    var state: ResolveState { options.state! }

    /// A copy of `v` for an object `depth` levels below the root, or the error left instead.
    func budgetedCopy(_ v: MValue, _ depth: Int) -> MValue {
        let (cost, nesting) = valueCost(v)
        if depth + nesting >= SynxEngine.maxResolveDepth { return .string(ResolveState.nestingErr) }
        if !state.charge(cost) { return .string(ResolveState.limitErr) }
        return v.deepCopy()
    }

    /// A value read from a file for an object `depth` levels below the root.
    func placedDocument(_ v: MValue, _ depth: Int, secret: Bool) -> MValue {
        if depth + valueCost(v).nesting >= SynxEngine.maxResolveDepth { return .string(ResolveState.nestingErr) }
        return secret ? maskSecretValue(v) : v
    }

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
            for i in 0..<map.count { map.values[i] = .string(ResolveState.nestingErr) }
            return
        }
        let mm = metadata[path]
        let count = map.count
        // What this object's `:secret` keys held before the pre-pass.
        var originals: [String: MValue]? = nil
        if !secretOriginals.isEmpty, let o = secretOriginals.removeValue(forKey: path) { originals = o }
        for i in 0..<count {
            if originals != nil, let original = originals!.removeValue(forKey: map.keys[i]) {
                map.values[i] = original
                // Secrets nested in it stay masked until their own turn.
                if original.isObjectOrArray {
                    SynxEngine.maskSecretSubtree(original, metadata, Resolver.childPath(path, map.keys[i]), &secretOriginals)
                }
            }
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
            applyMarkers(map, i, meta, path, depth)
        }
        for i in 0..<count {
            guard let s = map.values[i].string, s.utf8.contains(0x7B) else { continue }
            // safety: the text may grow by what is left of the budget and no
            // further — one 4 MiB value named from 600 keys was 2.4 GiB.
            let before = s.utf8.count
            guard let out = interpolate(s, map, cap: before + state.room) else {
                _ = state.charge(-1)
                map.values[i] = .string(ResolveState.limitErr)
                continue
            }
            if !out.bytesEqual(s) {
                _ = state.charge(max(0, out.utf8.count - before))
                map.values[i] = .string(out)
            }
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

    /// `{name}`, `{a.b}`, `{name:alias}`, `{name:include}`; nil when the text
    /// would pass `limit` bytes.
    func interpolate(_ tpl: String, _ local: MObject, cap limit: Int) -> String? {
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
                    if out.count > limit { return nil }
                    i = close + 1
                    continue
                }
            }
            let n = utf8Len(t[i])
            if out.count < cap { out.append(contentsOf: t[i..<min(i + n, t.count)]) }
            i += n
        }
        if out.count > limit { return nil }
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
        #if os(Windows)
        // A drive-relative path (`C:x`) leaves the base.
        let fp = Array(filePath.utf8)
        if fp.count >= 2 && fp[1] == 0x3A && ((fp[0] | 0x20) >= 0x61 && (fp[0] | 0x20) <= 0x7A) {
            return .err("SECURITY: path escapes base directory: \(quoted)")
        }
        // safety: `NUL`, `con.txt` or `COM1` in any directory opens a device;
        // synx-core reaches the disk through a verbatim path, where these are
        // ordinary missing files.
        if hasDeviceName(filePath) { return .err("file not found: \(filePath)") }
        #endif
        let segments = Array(filePath.utf8).split(omittingEmptySubsequences: false, whereSeparator: { $0 == 0x2F || $0 == 0x5C })
        // A base that cannot be resolved refuses instead of jailing nothing.
        let b = base.isEmpty ? "." : base
        let baseURL = URL(fileURLWithPath: b)
        guard let canonicalBase = canonical(baseURL) else { return .err("SECURITY: base directory not found: '\(b)'") }
        let baseCanonical = canonicalBase
        var full = baseCanonical.appendingPathComponent(filePath)
        #if os(Windows)
        // Rust's canonical base on Windows is verbatim (`\\?\C:\…`), and pushing
        // onto it resolves `.` and `..` lexically.
        full = full.standardizedFileURL
        #endif
        guard let fullCanonical = canonical(full) else {
            // The file may not exist yet — at least verify no `..` escapes,
            // judged by the path as the document wrote it.
            if segments.contains(where: { $0.elementsEqual([0x2E, 0x2E]) }) {
                return .err("SECURITY: path traversal detected: \(quoted)")
            }
            if !startsWith(full, baseCanonical) { return .err("SECURITY: path escapes base directory: \(quoted)") }
            return .ok(full)
        }
        if !startsWith(fullCanonical, baseCanonical) { return .err("SECURITY: path escapes base directory: \(quoted)") }
        return .ok(fullCanonical)
    }

    /// A path component that names a DOS device, with or without an extension
    /// or a stream: `NUL`, `con.txt`, `sub/COM1`, `aux:x`.
    static func hasDeviceName(_ p: String) -> Bool {
        for part in Array(p.utf8).split(omittingEmptySubsequences: false, whereSeparator: { $0 == 0x2F || $0 == 0x5C }) {
            var stem = Array(part.prefix { $0 != 0x2E && $0 != 0x3A })
            while stem.last == 0x20 { stem.removeLast() }
            let name = String(decoding: stem, as: UTF8.self).uppercased()
            if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(name) { return true }
            if (name.hasPrefix("COM") || name.hasPrefix("LPT")) && name.unicodeScalars.count == 4 {
                let last = name.unicodeScalars.last!
                if (last >= "0" && last <= "9") || last == "\u{B9}" || last == "\u{B2}" || last == "\u{B3}" { return true }
            }
        }
        return false
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

    /// Read a UTF-8 text file; the error text never carries the operating
    /// system's wording or the absolute path (synx-core `fmt_io_err`).
    static func read(_ url: URL, ctx: String) -> Outcome<String> {
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir) else {
            return .err("file not found: \(ctx)")
        }
        // A directory reads as "permission denied" on Windows; it is neither.
        if isDir.boolValue { return .err("not a file: \(ctx)") }
        guard let data = FileManager.default.contents(atPath: url.path) else { return .err("permission denied: \(ctx)") }
        guard let text = String(data: data, encoding: .utf8), Array(text.utf8).count == data.count else {
            return .err("file is not valid UTF-8: \(ctx)")
        }
        return .ok(text)
    }

    /// `read` within the resolve's read budget.
    static func readBudgeted(_ url: URL, ctx: String, _ state: ResolveState) -> Outcome<String> {
        var size = 0
        if let attrs = try? FileManager.default.attributesOfItem(atPath: url.path),
           (attrs[.type] as? FileAttributeType) == .typeRegular,
           let n = (attrs[.size] as? NSNumber)?.intValue {
            size = n
        }
        if state.readBytes + size > ResolveState.maxReadBytes {
            return .err("read budget (\(ResolveState.maxReadBytes) bytes) exceeded")
        }
        let r = read(url, ctx: ctx)
        if case .ok(let text) = r { state.readBytes += text.utf8.count }
        return r
    }

    /// Read and resolve the document at `url` (`shown`: as the document wrote it).
    ///
    /// safety: the depth cap alone let `a:include self.synx` / `b:include
    /// self.synx` resolve 2^16 times — minutes for a three-line file. A cycle
    /// now stops at its first repeat, and the budget bounds an acyclic fan-out.
    static func include(_ url: URL, _ shown: String, _ options: SynxOptions) -> Outcome<MValue> {
        let state = options.state!
        #if os(Windows)
        let id = url.standardizedFileURL.path.lowercased()
        #else
        let id = url.standardizedFileURL.path
        #endif
        if state.chain.contains(id) { return .err("circular include: '\(shown)'") }
        if state.reads >= ResolveState.maxIncludeReads {
            return .err("include budget (\(ResolveState.maxIncludeReads) files) exceeded")
        }
        state.reads += 1
        state.chain.append(id)
        defer { state.chain.removeLast() }
        switch readBudgeted(url, ctx: shown, state) {
        case .ok(let text): return .ok(loadSynx(text, url, options))
        case .err(let e): return .err(e)
        }
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
                  case .ok(let included) = include(path, inc.path, options) else { continue }
            map[inc.alias] = included
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
        // safety: a package may `!use` another; packages using one another
        // recursed until the process ran out of stack.
        if options.includeDepth >= (options.maxIncludeDepth ?? defaultMaxIncludeDepth) { return map }
        let pkgRoot = URL(fileURLWithPath: options.basePath ?? ".")
            .appendingPathComponent(options.packagesPath ?? "./synx_packages")
        for ud in directives where isContainedRelative(ud.package) {
            let pkgDir = pkgRoot.appendingPathComponent(ud.package)
            let entry = manifestMain(pkgDir) ?? pkgDir.appendingPathComponent("src").appendingPathComponent("main.synx")
            var isDir: ObjCBool = false
            guard FileManager.default.fileExists(atPath: entry.path, isDirectory: &isDir), !isDir.boolValue,
                  sizeError(entry) == nil, case .ok(let included) = include(entry.resolvingSymlinksInPath(), ud.package, options)
            else { continue }
            map[ud.alias] = included
        }
        return map
    }
}
