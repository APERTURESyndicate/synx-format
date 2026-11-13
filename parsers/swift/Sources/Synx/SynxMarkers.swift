// SYNX `!active` markers. Port of `apply_markers` and its helpers in
// crates/synx-core/src/engine.rs — same order, same early returns, same texts.
import Foundation
#if os(Windows)
import WinSDK
#endif

public enum SynxMarkers {
    private static let builtinSet: Set<String> = [
        "spam", "include", "import", "env", "random", "ref", "i18n", "calc", "alias", "secret",
        "unique", "geo", "template", "split", "join", "default", "clamp", "round", "map", "format",
        "fallback", "once", "version", "watch", "prompt", "vision", "audio", "replace", "sort", "sum",
        "inherit",
    ]

    /// Markers the engine handles itself; a custom marker of the same name never runs.
    public static func isBuiltin(_ name: String) -> Bool { builtinSet.contains(name) }
}

// Bits for the built-in markers, classified once per chain.
private struct MK: OptionSet {
    let rawValue: UInt32
    static let alias = MK(rawValue: 1 << 0), calc = MK(rawValue: 1 << 1), clamp = MK(rawValue: 1 << 2)
    static let `default` = MK(rawValue: 1 << 3), env = MK(rawValue: 1 << 4), fallback = MK(rawValue: 1 << 5)
    static let format = MK(rawValue: 1 << 6), geo = MK(rawValue: 1 << 7), i18n = MK(rawValue: 1 << 8)
    static let `import` = MK(rawValue: 1 << 9), include = MK(rawValue: 1 << 10), join = MK(rawValue: 1 << 11)
    static let map = MK(rawValue: 1 << 12), once = MK(rawValue: 1 << 13), prompt = MK(rawValue: 1 << 14)
    static let random = MK(rawValue: 1 << 15), ref = MK(rawValue: 1 << 16), replace = MK(rawValue: 1 << 17)
    static let round = MK(rawValue: 1 << 18), secret = MK(rawValue: 1 << 19), sort = MK(rawValue: 1 << 20)
    static let spam = MK(rawValue: 1 << 21), split = MK(rawValue: 1 << 22), sum = MK(rawValue: 1 << 23)
    static let unique = MK(rawValue: 1 << 24), version = MK(rawValue: 1 << 25), watch = MK(rawValue: 1 << 26)

    static let names: [String: MK] = [
        "alias": .alias, "calc": .calc, "clamp": .clamp, "default": .default, "env": .env,
        "fallback": .fallback, "format": .format, "geo": .geo, "i18n": .i18n, "import": .import,
        "include": .include, "join": .join, "map": .map, "once": .once, "prompt": .prompt,
        "random": .random, "ref": .ref, "replace": .replace, "round": .round, "secret": .secret,
        "sort": .sort, "spam": .spam, "split": .split, "sum": .sum, "unique": .unique,
        "version": .version, "watch": .watch,
    ]

    init(rawValue: UInt32) { self.rawValue = rawValue }
    init(_ markers: [String]) {
        var bits = MK()
        for m in markers { if let b = MK.names[m] { bits.insert(b) } }
        self = bits
    }
}

extension Resolver {

    private func after(_ markers: [String], _ name: String, _ offset: Int = 1) -> String? {
        guard let i = markers.firstIndex(of: name), i + offset < markers.count else { return nil }
        return markers[i + offset]
    }

    private func numberValue(_ r: Double) -> MValue {
        RustNumber.isWhole(r) && abs(r) < 9223372036854775807.0 ? .int(Int64(r)) : .float(r)
    }

    /// `depth`: how many levels below the root `map` sits.
    func applyMarkers(_ map: MObject, _ index: Int, _ meta: SynxMeta, _ path: String, _ depth: Int) {
        let markers = meta.markers
        let mb = MK(markers)
        let secret = mb.contains(.secret)
        let key = map.keys[index]
        func cur() -> MValue { map.values[index] }
        func set(_ v: MValue) { map.values[index] = v }
        let rootValue = MValue.object(root)

        // ── :spam — key:spam:MAX_CALLS[:WINDOW_SEC] target ──
        if mb.contains(.spam) {
            let maxCalls = after(markers, "spam").flatMap(RustNumber.parseU64) ?? 0
            let windowSec = after(markers, "spam", 2).flatMap(RustNumber.parseU64) ?? 1
            if maxCalls == 0 {
                set(.string("SPAM_ERR: invalid limit, use :spam:MAX[:WINDOW_SEC]"))
                return
            }
            let target = cur().text
            if !Spam.allow(key + "::" + target, maxCalls, windowSec) {
                set(.string("SPAM_ERR: '\(target)' exceeded \(maxCalls) calls per \(windowSec)s"))
                return
            }
            if let resolved = deepGet(rootValue, target) ?? map.get(target) { set(budgetedCopy(resolved, depth)) }
        }

        // ── :include / :import ──
        if mb.contains(.include) || mb.contains(.import) {
            if let file = cur().string {
                let maxDepth = options.maxIncludeDepth ?? Files.defaultMaxIncludeDepth
                if options.includeDepth >= maxDepth {
                    set(.string("INCLUDE_ERR: max include depth (\(maxDepth)) exceeded"))
                    return
                }
                // A root key the pre-pass already resolved for the same path.
                if path.isEmpty, let pre = preresolved[key]?.array, pre.items.count == 2,
                   pre.items[0].string == file {
                    set(placedDocument(pre.items[1].deepCopy(), depth, secret: secret))
                    return
                }
                switch Files.jail(options.basePath ?? ".", file) {
                case .err(let e):
                    set(.string("INCLUDE_ERR: \(e)"))
                    return
                case .ok(let path):
                    if let e = Files.sizeError(path) {
                        set(.string("INCLUDE_ERR: \(e)"))
                        return
                    }
                    switch Files.include(path, file, options) {
                    case .ok(let included): set(placedDocument(included, depth, secret: secret))
                    case .err(let e): set(.string("INCLUDE_ERR: \(e)"))
                    }
                }
            }
            // safety: the chain ends here, so `:secret` is applied here — a value
            // that was not a path (`k:secret:include 5`) stayed in the clear.
            if secret { set(maskSecretValue(cur())) }
            return
        }

        // ── :env ──
        if mb.contains(.env), let name = cur().string {
            let envVal: String? = options.env != nil ? options.env![name] : processEnv(name)
            let forceString = wantsString(meta)
            let di = markers.firstIndex(of: "default")
            if let v = envVal, !v.isEmpty {
                set(externalValue(v, asString: forceString, secret: secret))
            } else if let d = di, markers.count > d + 1 {
                // Every part after `default`, rejoined with ':' (IPs, compound values).
                let fallback = markers[(d + 1)...].joined(separator: ":")
                set(externalValue(fallback, asString: forceString, secret: secret))
            } else {
                set(.null)
            }
        }

        // ── :random ──
        if mb.contains(.random), let arr = cur().array {
            if arr.items.isEmpty {
                set(.null)
                return
            }
            if !meta.args.isEmpty {
                let weights = meta.args.compactMap { RustNumber.parseF64($0) }
                set(Resolver.weightedRandom(arr.items, weights).deepCopy())
            } else {
                set(arr.items[SynxRng.randomIndex(arr.items.count)].deepCopy())
            }
        }

        // ── :ref — like :alias, feeding the value into the next markers;
        //    `:ref:calc:*2 base` applies `VALUE * 2` ──
        if mb.contains(.ref), let target = cur().string {
            let resolved = (deepGet(rootValue, target) ?? map.get(target)).map { budgetedCopy($0, depth) } ?? .null
            if mb.contains(.calc), let n = resolved.number, let expr = after(markers, "calc") {
                let first = expr.unicodeScalars.first ?? " "
                if "+-*/%".unicodeScalars.contains(first) {
                    let r = SynxCalc.evaluate(RustNumber.formatNumber(n) + " " + expr)
                    set(r.ok ? numberValue(r.value) : .string("CALC_ERR: " + r.error))
                } else {
                    set(resolved)
                }
            } else {
                set(resolved)
            }
        }

        // ── :i18n[:COUNT_FIELD] — a localized value, with plural forms ──
        if mb.contains(.i18n), let translations = cur().object {
            let lang = options.lang ?? "en"
            let pick = translations.get(lang) ?? translations.get("en") ?? translations.values.first
            let val = pick?.deepCopy() ?? .null
            if let countField = after(markers, "i18n"), let forms = val.object {
                let n = map.get(countField)?.number ?? deepGet(rootValue, countField)?.number ?? 0
                let count = RustNumber.saturatingI64(n)
                let chosen = forms.get(Resolver.pluralCategory(lang, count)) ?? forms.get("other") ?? forms.values.first ?? .null
                if let s = chosen.string {
                    set(.string(s.bytesReplacing("{count}", with: String(count))))
                } else {
                    set(chosen)
                }
            } else {
                set(val)
            }
        }

        // ── :calc ──
        if mb.contains(.calc), let expr = cur().string {
            let tooLong = MValue.string("CALC_ERR: resolved expression too long (max \(Calc.maxResolvedLen) bytes)")
            if expr.utf8.count > Calc.maxExprLen {
                set(.string("CALC_ERR: expression too long (\(expr.utf8.count) chars, max \(Calc.maxExprLen))"))
                return
            }
            guard let sub = Calc.substitute(expr, local: map, selfKey: key, root: root, rootSpecial: rootSpecial) else {
                set(tooLong)
                return
            }
            // Dotted references (`base.hp`, `server.port`).
            let r = Array(sub.utf8)
            var out: [UInt8] = []
            out.reserveCapacity(r.count)
            var i = 0
            while i < r.count {
                if Calc.isWord(r[i]) {
                    let start = i
                    var hasDot = false
                    while i < r.count && (Calc.isWord(r[i]) || r[i] == 0x2E) {
                        if r[i] == 0x2E { hasDot = true }
                        i += 1
                    }
                    let token = r[start..<i]
                    if hasDot, let v = deepGet(rootValue, String(decoding: token, as: UTF8.self)), let n = v.number {
                        out.append(contentsOf: RustNumber.formatNumber(n).utf8)
                        if out.count > Calc.maxResolvedLen { set(tooLong); return }
                        continue
                    }
                    out.append(contentsOf: token)
                } else {
                    let n = utf8Len(r[i])
                    out.append(contentsOf: r[i..<min(i + n, r.count)])
                    i += n
                }
                if out.count > Calc.maxResolvedLen { set(tooLong); return }
            }
            let res = SynxCalc.evaluate(String(decoding: out, as: UTF8.self))
            set(res.ok ? numberValue(res.value) : .string("CALC_ERR: " + res.error))
        }

        // ── :alias ──
        if mb.contains(.alias), let target = cur().string {
            // The key's full dot-path; a list item's `#<index>` is not part of it.
            var currentPath = ""
            for seg in MetaPath.segments(path) where !seg.bytesHasPrefix("#") {
                currentPath += seg + "."
            }
            currentPath += key
            if target.bytesEqual(key) || target.bytesEqual(currentPath) {
                set(.string("ALIAS_ERR: self-referential alias: \(currentPath) \u{2192} \(target)"))
            } else {
                let tv = deepGet(rootValue, target)
                let tb = Array(target.utf8)
                let dot = tb.lastIndex(of: 0x2E)
                let targetParent = dot.map { String(decoding: tb[..<$0], as: UTF8.self) } ?? ""
                let targetKey = dot.map { String(decoding: tb[($0 + 1)...], as: UTF8.self) } ?? target
                let targetHasAlias = metadata[targetParent]?[targetKey]?.hasMarker("alias") ?? false
                let ts = tv?.string
                let isCycle = targetHasAlias && ts != nil && (ts!.bytesEqual(key) || ts!.bytesEqual(currentPath))
                if isCycle {
                    // Order-independent: both participants produce the same text.
                    let (a, b) = !target.bytesLess(currentPath) ? (currentPath, target) : (target, currentPath)
                    set(.string("ALIAS_ERR: circular alias detected: \(a) \u{2192} \(b)"))
                } else {
                    set(tv.map { budgetedCopy($0, depth) } ?? .null)
                }
            }
        }

        // ── :unique ──
        if mb.contains(.unique), let arr = cur().array {
            var seen = Set<[UInt8]>()
            var out: [MValue] = []
            for item in arr.items where seen.insert(Array(item.text.utf8)).inserted { out.append(item) }
            set(.array(MArray(out)))
        }

        // ── :geo — `- REGION value` items, by options.region ──
        if mb.contains(.geo), let arr = cur().array {
            let prefix = (options.region ?? "US") + " "
            if let found = arr.items.first(where: { $0.string?.bytesHasPrefix(prefix) ?? false })?.string {
                set(.string(String(decoding: Array(found.utf8).dropFirst(prefix.utf8.count), as: UTF8.self).rustTrimmed))
            } else if let first = arr.items.first {
                if let s = first.string, let sp = Array(s.utf8).firstIndex(of: 0x20) {
                    set(.string(String(decoding: Array(s.utf8)[(sp + 1)...], as: UTF8.self).rustTrimmed))
                } else {
                    set(first.deepCopy())
                }
            } else {
                set(.null)
            }
        }

        // ── :split[:SEP] ──
        if mb.contains(.split), let s = cur().string {
            let sep = after(markers, "split").map(Resolver.delimiter) ?? ","
            var items: [MValue] = []
            let parts: [String]
            if sep.isEmpty {
                // Rust `str::split("")` yields every character.
                parts = s.unicodeScalars.map { String($0) }
            } else {
                var ps: [String] = []
                let h = Array(s.utf8), sp = Array(sep.utf8)
                var pos = 0
                while let hit = findBytes(h, sp, from: pos) {
                    ps.append(String(decoding: h[pos..<hit], as: UTF8.self))
                    pos = hit + sp.count
                }
                ps.append(String(decoding: h[pos...], as: UTF8.self))
                parts = ps
            }
            for p in parts {
                let t = p.rustTrimmed
                if !t.isEmpty { items.append(castPrimitive(t)) }
            }
            set(.array(MArray(items)))
        }

        // ── :join[:SEP] ──
        if mb.contains(.join), let arr = cur().array {
            let sep = after(markers, "join").map(Resolver.delimiter) ?? ","
            // safety: the separator is as long as the document likes and is
            // written once per item.
            let n = arr.items.count
            let separators = sep.utf8.count * max(0, n - 1)
            if separators > n && !state.charge(separators) {
                set(.string(ResolveState.limitErr))
            } else {
                set(.string(arr.items.map { $0.text }.joined(separator: sep)))
            }
        }

        // ── :default (without :env) ──
        if mb.contains(.default) && !mb.contains(.env) {
            var empty = false
            switch cur() {
            case .null: empty = true
            case .string(let s): empty = s.isEmpty
            default: break
            }
            if empty, let d = markers.firstIndex(of: "default"), markers.count > d + 1 {
                let fallback = markers[(d + 1)...].joined(separator: ":")
                set(externalValue(fallback, asString: wantsString(meta), secret: secret))
            }
        }

        // ── :clamp:MIN:MAX ── a bound must be a finite number: `nan`, `inf`,
        // `1e400`, `5abc` skip the marker.
        if mb.contains(.clamp), let lo = RustNumber.parseF64(after(markers, "clamp") ?? ""),
           let hi = RustNumber.parseF64(after(markers, "clamp", 2) ?? ""), lo.isFinite, hi.isFinite {
            if lo > hi {
                set(.string("CONSTRAINT_ERR: clamp min (\(RustNumber.display(lo))) > max (\(RustNumber.display(hi)))"))
            } else if let n = cur().number {
                set(numberValue(n.isNaN ? n : Swift.min(Swift.max(n, lo), hi)))
            }
        }

        // ── :round[:N] ──
        if mb.contains(.round) {
            var decimals: UInt64 = 0
            if let d = after(markers, "round"), let v = RustNumber.parseU64(d), v <= UInt64(UInt32.max) { decimals = v }
            if let n = cur().number {
                // A scale past the range of f64 rounds nothing: the number stays.
                let factor = RustNumber.powi(10, Int32(min(decimals, 400)))
                let scaled = n * factor
                let rounded = scaled.isFinite ? scaled.rounded(.toNearestOrAwayFromZero) / factor : n
                // safety: only a whole number that fits is an integer. The cast
                // alone saturated: `:round` of 1e30 was 9223372036854775807 and of
                // NaN was 0 — invented values.
                if decimals == 0 && rounded.isFinite && RustNumber.isWhole(rounded) && abs(rounded) < 9223372036854775807.0 {
                    set(.int(Int64(rounded)))
                } else {
                    set(.float(rounded))
                }
            }
        }

        // ── :map[:SOURCE_KEY] — `- lookup result` items ──
        if mb.contains(.map), let arr = cur().array {
            var lookup = ""
            if let source = after(markers, "map"), !source.isEmpty {
                lookup = (deepGet(rootValue, source) ?? map.get(source))?.text ?? ""
            }
            var result: MValue = .null
            for item in arr.items {
                guard let s = item.string else { continue }
                let u = Array(s.utf8)
                guard let sp = u.firstIndex(of: 0x20) else { continue }
                if String(decoding: u[..<sp], as: UTF8.self).rustTrimmed.bytesEqual(lookup) {
                    result = castPrimitive(String(decoding: u[(sp + 1)...], as: UTF8.self).rustTrimmed)
                    break
                }
            }
            set(result)
        }

        // ── :format:PATTERN — printf-style %.2f, %d, %05d, %e ──
        if mb.contains(.format) {
            var formatted = Resolver.applyFormat(after(markers, "format") ?? "%s", cur())
            // safety: a width of 4096 on a twenty-byte line is a 200-fold growth.
            if cur().number != nil && !state.charge(formatted.utf8.count) { formatted = ResolveState.limitErr }
            set(.string(formatted))
        }

        // ── :replace:FROM:TO ──
        if mb.contains(.replace), let s = cur().string, let from = after(markers, "replace"), !from.isEmpty {
            let to = after(markers, "replace", 2) ?? ""
            // safety: `:replace:a:<long>` on a run of `a` multiplies the two
            // lengths; the growth is known before a byte is written.
            let grown = to.utf8.count > from.utf8.count ? countMatches(s, from) * (to.utf8.count - from.utf8.count) : 0
            if grown > 0 && !state.charge(grown) {
                set(.string(ResolveState.limitErr))
            } else {
                set(.string(s.bytesReplacing(from, with: to)))
            }
        }

        // ── :sort[:desc] ── a total order: numbers first, by value (NaN after the
        // rest of them), then everything else by its text, by code point; stable.
        // Comparing a number with text as text made `10 < 1a < 9 < 10` a cycle.
        if mb.contains(.sort), let arr = cur().array {
            let desc = after(markers, "sort") == "desc"
            let keyed = arr.items.map { (v: $0, n: $0.number, t: Array($0.text.utf8)) }
            var sorted = Resolver.mergeSort(keyed) { a, b in
                switch (a.n, b.n) {
                case let (x?, y?):
                    if x.isNaN || y.isNaN { return !x.isNaN && y.isNaN }
                    return x < y
                case (.some, nil): return true
                case (nil, .some): return false
                default: return a.t.lexicographicallyPrecedes(b.t)
                }
            }.map { $0.v }
            if desc { sorted.reverse() }
            set(.array(MArray(sorted)))
        }

        // ── :sum — Int when every summand is whole, Float otherwise ──
        if mb.contains(.sum), let arr = cur().array {
            var total = 0.0
            var allInt = true
            for v in arr.items {
                switch v {
                case .int(let n): total += Double(n)
                case .float(let f):
                    total += f
                    if !RustNumber.isWhole(f) { allInt = false }
                case .string(let s):
                    // `inf` and `nan` are text here as everywhere (see castPrimitive).
                    if let f = RustNumber.parseF64(s), f.isFinite {
                        total += f
                        if !RustNumber.isWhole(f) { allInt = false }
                    }
                default: break
                }
            }
            set(allInt && RustNumber.isWhole(total) && abs(total) < 9223372036854775807.0 ? .int(Int64(total)) : .float(total))
        }

        // ── :fallback:DEFAULT — a missing file path takes the default ──
        if mb.contains(.fallback) {
            var use = false
            switch cur() {
            case .null: use = true
            case .string(let s):
                if s.isEmpty {
                    use = true
                } else if case .ok(let p) = Files.jail(options.basePath ?? ".", s) {
                    use = !FileManager.default.fileExists(atPath: p.path)
                } else {
                    use = true
                }
            default: break
            }
            if use, let d = after(markers, "fallback"), !d.isEmpty { set(.string(d)) }
        }

        // ── :once[:uuid|timestamp|random] — generated once, kept in `.synx.lock` ──
        if mb.contains(.once) {
            let genType = after(markers, "once") ?? "uuid"
            let lock = options.basePath.map { URL(fileURLWithPath: $0).appendingPathComponent(".synx.lock") }
                ?? URL(fileURLWithPath: ".synx.lock")
            if let locked = Once.read(lock, key) {
                set(.string(locked))
            } else {
                let generated: String
                switch genType {
                case "timestamp": generated = String(Int64(Date().timeIntervalSince1970))
                case "random": generated = String(SynxRng.randomIndex(Int(UInt32.max)))
                default: generated = SynxRng.uuid()
                }
                Once.write(lock, key, generated)
                set(.string(generated))
            }
        }

        // ── :version:OP:REQUIRED ──
        if mb.contains(.version), let current = cur().string {
            set(.bool(Resolver.compareVersions(current, after(markers, "version") ?? ">=", after(markers, "version", 2) ?? "")))
        }

        // ── :watch[:KEY_PATH] — a file read at resolution time ──
        if mb.contains(.watch), let file = cur().string {
            let maxDepth = options.maxIncludeDepth ?? Files.defaultMaxIncludeDepth
            if options.includeDepth >= maxDepth {
                set(.string("WATCH_ERR: max include depth (\(maxDepth)) exceeded"))
                return
            }
            switch Files.jail(options.basePath ?? ".", file) {
            case .err(let e):
                set(.string("WATCH_ERR: \(e)"))
                return
            case .ok(let p):
                if let e = Files.sizeError(p) {
                    set(.string("WATCH_ERR: \(e)"))
                    return
                }
                let keyPath = after(markers, "watch")
                switch Files.readBudgeted(p, ctx: file, state) {
                case .ok(let content):
                    if let kp = keyPath {
                        set(Resolver.extract(content, kp, ext: p.pathExtension).map { placedDocument($0, depth, secret: false) } ?? .null)
                    } else {
                        set(.string(content.rustTrimmed))
                    }
                case .err(let e):
                    set(.string("WATCH_ERR: \(e)"))
                }
            }
        }

        // ── :secret ──
        // Last of the markers that produce the value, so whatever they resolved
        // is what gets masked. It used to sit after `:alias`: `t:secret:once` came
        // out as plain text and `:secret:format` formatted the mask. `:prompt`
        // and custom markers below still see only the mask.
        if secret { set(maskSecretValue(cur())) }

        // ── :prompt[:LABEL] — the subtree as SYNX in a labelled code fence ──
        if mb.contains(.prompt) {
            let label = after(markers, "prompt") ?? key
            let block = "\(label) (SYNX):\n```synx\n\(Resolver.promptText(cur(), 0))```"
            set(.string(state.charge(block.utf8.count) ? block : ResolveState.limitErr))
        }

        // ── custom markers: the first registered non-builtin one, with the parts after it ──
        if !options.markerFns.isEmpty {
            for (i, m) in markers.enumerated() where !SynxMarkers.isBuiltin(m) {
                guard let fn = options.markerFns[m] else { continue }
                set(MValue(fn(key, Array(markers[(i + 1)...]), cur().toValue())))
                break
            }
        }

        // ── constraints, always last ──
        if let c = meta.constraints, let err = Constraints.violation(key, cur(), c) {
            SynxEngine.report(options, err)
            set(.string(err))
        }
    }

    // MARK: - helpers

    static func delimiter(_ k: String) -> String {
        switch k {
        case "space": return " "
        case "pipe": return "|"
        case "dash": return "-"
        case "dot": return "."
        case "semi": return ";"
        case "tab": return "\t"
        case "slash": return "/"
        default: return k
        }
    }

    static func weightedRandom(_ items: [MValue], _ weights: [Double]) -> MValue {
        var w = weights
        if w.count < items.count {
            let assigned = w.reduce(0, +)
            // Weights above 100 give the rest their average; room below 100 is shared.
            let per = assigned < 100 ? (100 - assigned) / Double(items.count - w.count) : assigned / Double(w.count)
            while w.count < items.count { w.append(per) }
        }
        let total = w.reduce(0, +)
        if total <= 0 { return items[SynxRng.randomIndex(items.count)] }
        let r = SynxRng.randomF64()
        var cumulative = 0.0
        for (i, item) in items.enumerated() {
            cumulative += w[i] / total
            if r <= cumulative { return item }
        }
        return items.last ?? .null
    }

    static func pluralCategory(_ lang: String, _ n: Int64) -> String {
        let a = n.magnitude
        let n10 = a % 10, n100 = a % 100
        switch lang {
        case "ru", "uk", "be", "pl":
            if n10 == 1 && n100 != 11 { return "one" }
            if (2...4).contains(n10) && !(12...14).contains(n100) { return "few" }
            return "many"
        case "cs", "sk":
            if a == 1 { return "one" }
            if (2...4).contains(a) { return "few" }
            return "other"
        case "ar":
            if a == 0 { return "zero" }
            if a == 1 { return "one" }
            if a == 2 { return "two" }
            if (3...10).contains(n100) { return "few" }
            if (11...99).contains(n100) { return "many" }
            return "other"
        case "fr", "pt": return a <= 1 ? "one" : "other"
        case "ja", "zh", "ko", "vi", "th": return "other"
        default: return a == 1 ? "one" : "other"
        }
    }

    /// The integer at the start of `s` (after whitespace, with a sign), or 0.
    static func leadingInt(_ s: String) -> Int64 {
        var u = Array(s.rustTrimmedStart.utf8)[...]
        var neg = false
        if let f = u.first, f == 0x2D || f == 0x2B { neg = f == 0x2D; u = u.dropFirst() }
        var n: Int64 = 0
        for c in u {
            guard c >= 0x30 && c <= 0x39 else { break }
            let (m, o1) = n.multipliedReportingOverflow(by: 10)
            let (r, o2) = m.addingReportingOverflow(Int64(c - 0x30))
            n = (o1 || o2) ? .max : r
        }
        return neg ? -n : n
    }

    /// Positional parts, each read for its leading integer: `1.2.3-beta` is 1.2.3.
    static func compareVersions(_ current: String, _ op: String, _ required: String) -> Bool {
        let cv = splitBytes(current, 0x2E).map(leadingInt)
        let rv = splitBytes(required, 0x2E).map(leadingInt)
        var ord = 0
        for i in 0..<Swift.max(cv.count, rv.count) {
            let a = i < cv.count ? cv[i] : 0
            let b = i < rv.count ? rv[i] : 0
            if a != b { ord = a < b ? -1 : 1; break }
        }
        switch op {
        case ">=": return ord >= 0
        case "<=": return ord <= 0
        case ">": return ord > 0
        case "<": return ord < 0
        case "==", "=": return ord == 0
        case "!=": return ord != 0
        default: return false
        }
    }

    static func padLeft(_ s: String, _ width: Int, _ fill: Character) -> String {
        let n = s.unicodeScalars.count
        return n < width ? String(repeating: fill, count: width - n) + s : s
    }

    static func formatInt(_ pattern: String, _ n: Int64) -> String {
        // Guardrail: a user-controlled width can be enormous.
        let maxWidth: UInt64 = 4096
        let p = Array(pattern.utf8)
        if p.first == 0x25, let last = p.last, p.count >= 2, last == 0x64 || last == 0x69 {
            let inner = String(decoding: p[1..<(p.count - 1)], as: UTF8.self)
            if inner.bytesHasPrefix("0"), let w = RustNumber.parseU64(String(inner.dropFirst())) {
                return padLeft(String(n), Int(Swift.min(w, maxWidth)), "0")
            }
            if let w = RustNumber.parseU64(inner) {
                return padLeft(String(n), Int(Swift.min(w, maxWidth)), " ")
            }
        }
        return String(n)
    }

    static func formatFloat(_ pattern: String, _ f: Double) -> String {
        let maxPrec: UInt64 = 1024
        let p = Array(pattern.utf8)
        if p.first == 0x25 {
            let s = String(decoding: p[1...], as: UTF8.self)
            // `%e` — the JS `toExponential()` shape: `1.23456789e+5`.
            if s == "e" {
                if f == 0 { return "0e+0" }
                let raw = RustNumber.exp(f)
                guard let e = raw.utf8.lastIndex(of: 0x65) else { return raw }
                var mantissa = String(raw[..<e])
                var exp = String(raw[raw.index(after: e)...])
                var sign = "+"
                if exp.hasPrefix("+") || exp.hasPrefix("-") { sign = String(exp.removeFirst()) }
                if mantissa.hasSuffix(".0") { mantissa.removeLast(2) }
                return mantissa + "e" + sign + exp
            }
            if let last = p.last, p.count >= 2, last == 0x66 || last == 0x65 {
                let inner = Array(p[1..<(p.count - 1)])
                if inner.first == 0x2E, let prec = RustNumber.parseU64(String(decoding: inner.dropFirst(), as: UTF8.self)) {
                    let n = Int(Swift.min(prec, maxPrec))
                    return last == 0x65 ? RustNumber.expPrec(f, n) : RustNumber.fixed(f, n)
                }
            }
        }
        return RustNumber.display(f)
    }

    static func applyFormat(_ pattern: String, _ v: MValue) -> String {
        let has = { (c: UInt8) in pattern.utf8.contains(c) }
        switch v {
        case .int(let n):
            if has(0x64) || has(0x69) { return formatInt(pattern, n) }
            if has(0x66) || has(0x65) { return formatFloat(pattern, Double(n)) }
            return String(n)
        case .float(let f):
            if has(0x66) || has(0x65) { return formatFloat(pattern, f) }
            // A whole float takes an integer pattern: `12.0` under `%05d` is `00012`.
            if (has(0x64) || has(0x69)) && RustNumber.isWhole(f) && abs(f) < 9.0e15 { return formatInt(pattern, Int64(f)) }
            return RustNumber.formatNumber(f)
        case .string(let s): return s
        default: return v.text
        }
    }

    static func extract(_ content: String, _ keyPath: String, ext: String) -> MValue? {
        var current: MValue
        if ext == "json" {
            guard let v = JSONReader.parse(content) else { return nil }
            current = v
        } else {
            current = .object(SynxParser.parseTree(content).0)
        }
        for part in splitBytes(keyPath, 0x2E) {
            guard let o = current.object, let next = o.get(part) else { return nil }
            current = next
        }
        return current
    }

    /// A value as SYNX for `:prompt` (Rust `stringify_value`).
    static func promptText(_ v: MValue, _ indent: Int) -> String {
        let sp = String(repeating: " ", count: indent)
        var out = ""
        if let o = v.object {
            let order = (0..<o.count).sorted { o.keys[$0].bytesLess(o.keys[$1]) }
            for i in order {
                let val = o.values[i]
                if val.object != nil {
                    out += sp + o.keys[i] + "\n" + promptText(val, indent + 2)
                } else if let a = val.array {
                    out += sp + o.keys[i] + "\n"
                    for item in a.items { out += sp + "  - " + item.text + "\n" }
                } else {
                    out += sp + o.keys[i] + " " + val.text + "\n"
                }
            }
            return out
        }
        if let a = v.array {
            for item in a.items { out += sp + "  - " + item.text + "\n" }
            return out
        }
        return sp + v.text + "\n"
    }

    /// Stable merge sort: the right element goes first only when strictly smaller.
    static func mergeSort<T>(_ a: [T], _ less: (T, T) -> Bool) -> [T] {
        if a.count < 2 { return a }
        let mid = a.count / 2
        let l = mergeSort(Array(a[..<mid]), less), r = mergeSort(Array(a[mid...]), less)
        var out: [T] = []
        out.reserveCapacity(a.count)
        var i = 0, j = 0
        while i < l.count && j < r.count {
            if less(r[j], l[i]) { out.append(r[j]); j += 1 } else { out.append(l[i]); i += 1 }
        }
        out.append(contentsOf: l[i...])
        out.append(contentsOf: r[j...])
        return out
    }
}

// MARK: - casting

/// Rust `engine::cast_primitive`.
func castPrimitive(_ s: String) -> MValue {
    let u = Array(s.utf8)
    if u.count >= 2, (u.first == 0x22 && u.last == 0x22) || (u.first == 0x27 && u.last == 0x27) {
        return .string(String(decoding: u[1..<(u.count - 1)], as: UTF8.self))
    }
    switch s {
    case "true": return .bool(true)
    case "false": return .bool(false)
    case "null": return .null
    default:
        if let i = RustNumber.parseI64(u) { return .int(i) }
        // spec: `inf`, `NaN` and `1e400` are text. Read as floats they became
        // `null` in JSON: a host named `inf` lost its value.
        if let f = RustNumber.parseF64(u), f.isFinite { return .float(f) }
        return .string(s)
    }
}

/// Text from outside the document as a value. Under `:secret` a cast that gave
/// a number or a bool keeps the text instead: `00123` stays `00123`.
func externalValue(_ text: String, asString: Bool, secret: Bool) -> MValue {
    if asString { return .string(text) }
    let cast = castPrimitive(text)
    if secret {
        switch cast {
        case .int, .float, .bool: return .string(text)
        default: break
        }
    }
    return cast
}

/// Non-overlapping occurrences of `needle` in `hay`, left to right.
func countMatches(_ hay: String, _ needle: String) -> Int {
    let h = Array(hay.utf8), n = Array(needle.utf8)
    var count = 0, pos = 0
    while let hit = findBytes(h, n, from: pos) {
        count += 1
        pos = hit + n.count
    }
    return count
}

/// One environment variable as UTF-8 (Rust `std::env::var`; case-insensitive on
/// Windows). perf: `ProcessInfo.environment` rebuilt the whole table per lookup.
func processEnv(_ name: String) -> String? {
    #if os(Windows)
    var wname = Array(name.utf16)
    wname.append(0)
    let n = GetEnvironmentVariableW(wname, nil, 0)
    if n == 0 { return nil }
    var buf = [UInt16](repeating: 0, count: Int(n))
    let got = GetEnvironmentVariableW(wname, &buf, n)
    if got == 0 && n > 1 { return nil }
    return String(decoding: buf.prefix(Int(got)), as: UTF16.self)
    #else
    guard let raw = getenv(name) else { return nil }
    return String(cString: raw)
    #endif
}

/// `(string)` / `[type:string]` keep outside text as text.
func wantsString(_ m: SynxMeta) -> Bool { m.typeHint == "string" || m.constraints?.typeName == "string" }

// MARK: - :calc substitution

enum Calc {
    static let maxExprLen = 4096
    static let maxResolvedLen = 64 * 1024

    @inline(__always) static func isWord(_ b: UInt8) -> Bool {
        (b >= 0x30 && b <= 0x39) || (b >= 0x61 && b <= 0x7A) || (b >= 0x41 && b <= 0x5A) || b == 0x5F
    }

    /// A key a token lookup cannot see: a non-word byte (`max-hp`, `a.b`), or one a
    /// substituted number can spell (`2`, `NaN`, `inf`).
    static func isSpecialKey(_ k: String) -> Bool {
        let u = k.utf8
        return !u.allSatisfy(isWord) || u.allSatisfy { $0 >= 0x30 && $0 <= 0x39 } || k == "NaN" || k == "inf"
    }

    /// Whole-word replacement (Rust `replace_word`, with the scratch cap).
    static func replaceWord(_ hay: [UInt8], _ word: [UInt8], _ repl: [UInt8]) -> [UInt8] {
        if word.isEmpty || word.count > hay.count { return hay }
        let cap = Files.maxScratch
        var out: [UInt8] = []
        out.reserveCapacity(Swift.min(hay.count, cap))
        var i = 0
        while i <= hay.count - word.count {
            if out.count >= cap { break }
            if hay[i] == word[0] && hay[i..<(i + word.count)].elementsEqual(word) {
                let beforeOk = i == 0 || !isWord(hay[i - 1])
                let afterOk = i + word.count >= hay.count || !isWord(hay[i + word.count])
                if beforeOk && afterOk {
                    out.append(contentsOf: repl.prefix(utf8Cut(repl, cap - out.count)))
                    i += word.count
                    continue
                }
            }
            let n = utf8Len(hay[i])
            if out.count < cap { out.append(contentsOf: hay[i..<Swift.min(i + n, hay.count)]) }
            i += n
        }
        if i < hay.count {
            let tail = Array(hay[i...])
            let room = cap > out.count ? cap - out.count : 0
            out.append(contentsOf: tail.prefix(utf8Cut(tail, Swift.min(room, tail.count))))
        }
        return out
    }

    /// Names in a `:calc` expression: a key of the same object first (it shadows
    /// the root), then a root key. nil past `maxResolvedLen`.
    static func substitute(_ expr: String, local: MObject, selfKey: String, root: MObject, rootSpecial: [String]) -> String? {
        let localIsRoot = local === root
        var special = rootSpecial.contains { root.get($0)?.number != nil }
        if !special && !localIsRoot {
            for i in 0..<local.count where !local.keys[i].bytesEqual(selfKey) && local.values[i].number != nil && isSpecialKey(local.keys[i]) {
                special = true
                break
            }
        }
        if special {
            // Exact sequential whole-word replacement, object before root.
            var resolved = Array(expr.utf8)
            let maps = localIsRoot ? [local] : [local, root]
            for m in maps {
                for i in 0..<m.count {
                    if m === local && m.keys[i].bytesEqual(selfKey) { continue }
                    guard let n = m.values[i].number else { continue }
                    resolved = replaceWord(resolved, Array(m.keys[i].utf8), Array(RustNumber.formatNumber(n).utf8))
                    if resolved.count > maxResolvedLen { return nil }
                }
            }
            return String(decoding: resolved, as: UTF8.self)
        }
        let e = Array(expr.utf8)
        var out: [UInt8] = []
        out.reserveCapacity(e.count + 16)
        var i = 0
        while i < e.count {
            if isWord(e[i]) {
                let start = i
                while i < e.count && isWord(e[i]) { i += 1 }
                let token = String(decoding: e[start..<i], as: UTF8.self)
                var n: Double? = nil
                if !token.bytesEqual(selfKey) { n = local.get(token)?.number }
                if n == nil { n = root.get(token)?.number }
                if let v = n { out.append(contentsOf: RustNumber.formatNumber(v).utf8) } else { out.append(contentsOf: e[start..<i]) }
            } else {
                let n = utf8Len(e[i])
                out.append(contentsOf: e[i..<Swift.min(i + n, e.count)])
                i += n
            }
            if out.count > maxResolvedLen { return nil }
        }
        return String(decoding: out, as: UTF8.self)
    }
}

// MARK: - state: :spam buckets, :once lock file, rng

enum Spam {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var buckets: [String: [Date]] = [:]

    static func allow(_ bucket: String, _ maxCalls: UInt64, _ windowSec: UInt64) -> Bool {
        let now = Date()
        let window = Double(Swift.max(windowSec, 1))
        lock.lock()
        defer { lock.unlock() }
        var calls = (buckets[bucket] ?? []).filter { now.timeIntervalSince($0) <= window }
        if UInt64(calls.count) >= maxCalls {
            buckets[bucket] = calls
            return false
        }
        calls.append(now)
        buckets[bucket] = calls
        return true
    }
}

enum Once {
    static func lines(_ text: String) -> [String] {
        var out = splitBytes(text, 0x0A).map { $0.utf8.last == 0x0D ? String(decoding: Array($0.utf8).dropLast(), as: UTF8.self) : $0 }
        if text.utf8.last == 0x0A || text.isEmpty { out.removeLast() }
        return out
    }

    static func read(_ lock: URL, _ key: String) -> String? {
        guard case .ok(let content) = Files.read(lock, ctx: "") else { return nil }
        for line in lines(content) where line.bytesHasPrefix(key) {
            let rest = String(decoding: Array(line.utf8).dropFirst(key.utf8.count), as: UTF8.self)
            if rest.bytesHasPrefix(" ") { return rest.rustTrimmedStart }
        }
        return nil
    }

    static func write(_ lock: URL, _ key: String, _ value: String) {
        var content = ""
        if case .ok(let c) = Files.read(lock, ctx: "") { content = c }
        var ls = lines(content)
        let newLine = key + " " + value
        if let i = ls.firstIndex(where: { $0.bytesHasPrefix(key) && String(decoding: Array($0.utf8).dropFirst(key.utf8.count), as: UTF8.self).bytesHasPrefix(" ") }) {
            ls[i] = newLine
        } else {
            ls.append(newLine)
        }
        try? (ls.joined(separator: "\n") + "\n").write(to: lock, atomically: false, encoding: .utf8)
    }
}

/// Port of synx-core's rng.rs: xorshift64. Not cryptographically secure.
enum SynxRng {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var state: UInt64 = {
        let nanos = UInt64(Date().timeIntervalSince1970 * 1_000_000_000)
        return nanos == 0 ? 0xcafe_dead_beef_1234 : nanos
    }()

    static func seed(_ s: UInt64) {
        lock.lock()
        state = s == 0 ? 0xcafe_dead_beef_1234 : s
        lock.unlock()
    }

    static func next() -> UInt64 {
        lock.lock()
        defer { lock.unlock() }
        var x = state
        x ^= x << 13
        x ^= x >> 7
        x ^= x << 17
        state = x
        return x
    }

    /// `[0, bound)`; 0 when `bound` is 0.
    static func randomIndex(_ bound: Int) -> Int { bound <= 0 ? 0 : Int(next() % UInt64(bound)) }
    /// `[0, 1)` with 1/10 000 granularity.
    static func randomF64() -> Double { Double(next() % 10_000) / 10_000 }
    /// `[0, 2 147 483 647)`.
    static func randomI64() -> Int64 { Int64(next() % 2_147_483_647) }
    static func randomBool() -> Bool { next() % 2 == 0 }

    static func uuid() -> String {
        let hi = next(), lo = next()
        let timeHi = ((hi >> 16) & 0x0FFF) | 0x4000
        let clk = ((lo >> 48) & 0x3FFF) | 0x8000
        func hex(_ v: UInt64, _ w: Int) -> String {
            let s = String(v, radix: 16)
            return String(repeating: "0", count: Swift.max(0, w - s.count)) + s
        }
        return "\(hex(hi >> 32, 8))-\(hex((hi >> 16) & 0xFFFF, 4))-\(hex(timeHi & 0xFFFF, 4))-\(hex(clk & 0xFFFF, 4))-\(hex(lo & 0x0000_FFFF_FFFF_FFFF, 12))"
    }
}
