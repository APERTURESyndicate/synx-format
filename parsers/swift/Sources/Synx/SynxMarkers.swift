// All 27 SYNX active-mode markers as pure functions. Mirrors
// crates/synx-core/src/engine.rs (marker half).
import Foundation

public enum SynxMarkers {

    // MARK: - registry

    static let builtinSet: Set<String> = [
        "env","default","calc","ref","alias","secret","random","unique","geo","i18n",
        "split","join","clamp","round","map","format","replace","sort","sum","fallback",
        "once","version","watch","prompt","vision","audio","include","import","inherit","spam",
    ]

    public static func isBuiltin(_ name: String) -> Bool { builtinSet.contains(name) }

    // MARK: - simple markers

    public static func applyEnv(_ v: SynxValue, meta: SynxMeta, options: SynxOptions) -> SynxValue {
        guard case .string(let varName) = v else { return v }
        let envVal: String?
        if let env = options.env { envVal = env[varName] } else { envVal = ProcessInfo.processInfo.environment[varName] }
        // `(string)` / `[type:string]` keep the env text verbatim (`007`, 64-bit ids).
        let forceString = wantsString(meta)
        if let val = envVal, !val.isEmpty {
            return forceString ? .string(val) : castPrimitive(val)
        }
        // Fallback: every marker after `default`, rejoined with ':'.
        if let di = meta.markerIndex("default"), di + 1 < meta.markers.count {
            let fallback = meta.markers[(di + 1)...].joined(separator: ":")
            return forceString ? .string(fallback) : castPrimitive(fallback)
        }
        return .null
    }

    public static func applyDefault(_ v: SynxValue, meta: SynxMeta) -> SynxValue {
        if meta.hasMarker("env") { return v } // handled together with :env
        let isEmpty: Bool = {
            if case .null = v { return true }
            if case .string(let s) = v, s.isEmpty { return true }
            return false
        }()
        guard isEmpty else { return v }
        if let idx = meta.markerIndex("default"),
           idx + 1 < meta.markers.count {
            let fallback = meta.markers[(idx + 1)...].joined(separator: ":")
            return wantsString(meta) ? .string(fallback) : castPrimitive(fallback)
        }
        return v
    }

    static let maxCalcExprLen = 4096
    static let maxCalcResolvedLen = 64 * 1024

    /// `:calc` — port of the Rust engine handler: substitute numeric root keys,
    /// then numeric siblings, then dotted paths, then evaluate. A failure writes
    /// the exact `CALC_ERR: …` string into the value.
    public static func applyCalc(_ v: SynxValue, key: String,
                                  root: SynxObject, local: SynxObject) -> SynxValue {
        guard case .string(let expr) = v else { return v }
        let exprLen = expr.utf8.count
        if exprLen > maxCalcExprLen {
            return .string("CALC_ERR: expression too long (\(exprLen) chars, max \(maxCalcExprLen))")
        }
        let tooLong = SynxValue.string(
            "CALC_ERR: resolved expression too long (max \(maxCalcResolvedLen) bytes)")

        func number(_ val: SynxValue) -> Double? {
            switch val {
            case .int(let n):   return Double(n)
            case .float(let f): return f
            default:            return nil
            }
        }

        var resolved = Array(expr.utf8)
        for (k, val) in root.entries {
            guard let n = number(val) else { continue }
            resolved = replaceWord(in: resolved, word: Array(k.utf8), with: Array(formatNumber(n).utf8))
            if resolved.count > maxCalcResolvedLen { return tooLong }
        }
        for (k, val) in local.entries where k != key {
            guard let n = number(val) else { continue }
            resolved = replaceWord(in: resolved, word: Array(k.utf8), with: Array(formatNumber(n).utf8))
            if resolved.count > maxCalcResolvedLen { return tooLong }
        }

        // Dotted references (`base.hp`). Every other byte is widened to U+00xx as
        // Rust's `byte as char` does, which is what a later error message shows.
        func isWord(_ b: UInt8) -> Bool {
            return (b >= 0x30 && b <= 0x39) || (b >= 0x41 && b <= 0x5A) || (b >= 0x61 && b <= 0x7A) || b == 0x5F
        }
        var dotResolved: [UInt8] = []
        var i = 0
        while i < resolved.count {
            if isWord(resolved[i]) {
                let start = i
                var hasDot = false
                while i < resolved.count && (isWord(resolved[i]) || resolved[i] == 0x2E) {
                    if resolved[i] == 0x2E { hasDot = true }
                    i += 1
                }
                let token = Array(resolved[start..<i])
                if hasDot,
                   let n = number(deepGetCalc(root, String(decoding: token, as: UTF8.self)) ?? .null) {
                    dotResolved.append(contentsOf: Array(formatNumber(n).utf8))
                    if dotResolved.count > maxCalcResolvedLen { return tooLong }
                    continue
                }
                dotResolved.append(contentsOf: token)
            } else {
                let b = resolved[i]
                if b < 0x80 {
                    dotResolved.append(b)
                } else {
                    dotResolved.append(0xC0 | (b >> 6))
                    dotResolved.append(0x80 | (b & 0x3F))
                }
                i += 1
            }
            if dotResolved.count > maxCalcResolvedLen { return tooLong }
        }

        let r = SynxCalc.evaluate(String(decoding: dotResolved, as: UTF8.self))
        if !r.ok { return .string("CALC_ERR: \(r.error)") }
        if r.value.truncatingRemainder(dividingBy: 1) == 0 && abs(r.value) < 9.223372036854775807e18 {
            return .int(Int64(r.value))
        }
        return .float(r.value)
    }

    /// Direct key first, then dot-path traversal through objects (Rust `deep_get`).
    private static func deepGetCalc(_ root: SynxObject, _ path: String) -> SynxValue? {
        if let direct = root[path] { return direct }
        var current: SynxValue = .object(root)
        for seg in path.split(separator: ".", omittingEmptySubsequences: false) {
            guard case .object(let obj) = current, let next = obj[String(seg)] else { return nil }
            current = next
        }
        return current
    }

    public static func applyRef(_ v: SynxValue,
                                 root: SynxObject,
                                 namespaces: [String: SynxObject]) -> SynxValue {
        let path = valueToString(v)
        if path.isEmpty { return v }
        if let dot = path.firstIndex(of: ".") {
            let ns = String(path[..<dot])
            let rest = String(path[path.index(after: dot)...])
            if let nsRoot = namespaces[ns], let val = deepGet(rest, in: nsRoot) {
                return val
            }
        }
        return deepGet(path, in: root) ?? v
    }

    public static func applySecret(_ v: SynxValue) -> SynxValue {
        if case .secret = v { return v }
        if case .null = v { return v }
        if case .string(let s) = v { return .secret(s) }
        return .secret(valueToString(v))
    }

    // MARK: - randomness / collections

    public static func applyRandom(_ v: SynxValue, meta: SynxMeta,
                                    rng: inout SystemRandomNumberGenerator) -> SynxValue {
        var options: [String] = []
        if case .array(let arr) = v {
            for item in arr { options.append(valueToString(item)) }
        } else if case .string(let s) = v {
            for part in s.split(separator: ",") {
                options.append(part.trimmingCharacters(in: .whitespaces))
            }
        }
        guard !options.isEmpty else { return v }

        var weights: [Double] = meta.args.map { Double($0) ?? 1.0 }
        while weights.count < options.count { weights.append(1.0) }
        let total = weights.reduce(0, +)
        guard total > 0 else { return v }
        let pick = Double.random(in: 0...total, using: &rng)
        var acc = 0.0
        for (i, w) in weights.enumerated() {
            acc += w
            if pick <= acc { return .string(options[i]) }
        }
        return .string(options.last!)
    }

    public static func applyUnique(_ v: SynxValue) -> SynxValue {
        guard case .array(let arr) = v else { return v }
        var out: [SynxValue] = []
        for item in arr where !out.contains(where: { $0 == item }) {
            out.append(item)
        }
        return .array(out)
    }

    public static func applyGeo(_ v: SynxValue, meta: SynxMeta, options: SynxOptions) -> SynxValue {
        guard let region = options.region, !region.isEmpty else { return v }
        for a in meta.args {
            guard let colon = a.firstIndex(of: ":") else { continue }
            let r = String(a[..<colon])
            let val = String(a[a.index(after: colon)...])
            if r == region { return .string(val) }
        }
        return v
    }

    public static func applyI18n(_ v: SynxValue, meta: SynxMeta, options: SynxOptions) -> SynxValue {
        let lang = options.lang ?? "en"
        let n = valueToNumber(v) ?? 0
        let isNumeric = (valueToNumber(v) != nil)
        let category = isNumeric ? pluralCategory(lang: lang, n: n) : "other"
        let lang2 = String(lang.prefix(2))
        let keys = [
            "\(lang).\(category)",
            "\(lang2).\(category)",
            lang,
            lang2,
            "other",
        ]
        for key in keys {
            for a in meta.args {
                if let colon = a.firstIndex(of: ":") {
                    if a[..<colon] == Substring(key) {
                        return .string(String(a[a.index(after: colon)...]))
                    }
                }
            }
        }
        return v
    }

    public static func applySplit(_ v: SynxValue, meta: SynxMeta) -> SynxValue {
        guard case .string(let s) = v else { return v }
        var sep = ","
        if let idx = meta.markerIndex("split"), idx + 1 < meta.markers.count {
            sep = meta.markers[idx + 1]
        }
        if sep.isEmpty {
            return .array(s.map { .string(String($0)) })
        }
        let parts = s.components(separatedBy: sep)
            .map { SynxValue.string($0.trimmingCharacters(in: .whitespaces)) }
        return .array(parts)
    }

    public static func applyJoin(_ v: SynxValue, meta: SynxMeta) -> SynxValue {
        guard case .array(let arr) = v else { return v }
        var sep = ","
        if let idx = meta.markerIndex("join"), idx + 1 < meta.markers.count {
            sep = meta.markers[idx + 1]
        }
        return .string(arr.map { valueToString($0) }.joined(separator: sep))
    }

    public static func applyClamp(_ v: SynxValue, meta: SynxMeta) -> SynxValue {
        guard var d = valueToNumber(v) else { return v }
        var lo = -Double.infinity
        var hi = Double.infinity
        if let idx = meta.markerIndex("clamp"), idx + 2 < meta.markers.count {
            lo = Double(meta.markers[idx + 1]) ?? lo
            hi = Double(meta.markers[idx + 2]) ?? hi
        }
        if d < lo { d = lo }
        if d > hi { d = hi }
        if case .int = v { return .int(Int64(d)) }
        return .float(d)
    }

    public static func applyRound(_ v: SynxValue, meta: SynxMeta) -> SynxValue {
        guard let d = valueToNumber(v) else { return v }
        var digits = 0
        if let idx = meta.markerIndex("round"), idx + 1 < meta.markers.count {
            digits = Int(meta.markers[idx + 1]) ?? 0
        }
        let factor = pow(10.0, Double(digits))
        let r = (d * factor).rounded() / factor
        if digits == 0 { return .int(Int64(r)) }
        return .float(r)
    }

    public static func applyMap(_ v: SynxValue, meta: SynxMeta) -> SynxValue {
        let key = valueToString(v)
        for a in meta.args {
            guard let colon = a.firstIndex(of: ":") else { continue }
            if a[..<colon] == Substring(key) {
                return .string(String(a[a.index(after: colon)...]))
            }
        }
        return v
    }

    public static func applyFormat(_ v: SynxValue, meta: SynxMeta,
                                    interpolate: (String) -> String) -> SynxValue {
        guard let idx = meta.markerIndex("format"), idx + 1 < meta.markers.count else { return v }
        let pattern = interpolate(meta.markers[idx + 1])
        let n = valueToNumber(v) ?? 0
        let sIn = valueToString(v)
        return .string(applyPrintfPattern(pattern, number: n, string: sIn))
    }

    public static func applyReplace(_ v: SynxValue, meta: SynxMeta) -> SynxValue {
        guard case .string(var s) = v else { return v }
        guard let idx = meta.markerIndex("replace"), idx + 2 < meta.markers.count else { return v }
        let from = meta.markers[idx + 1]
        let to = meta.markers[idx + 2]
        if from.isEmpty { return .string(s) }
        s = s.replacingOccurrences(of: from, with: to)
        return .string(s)
    }

    public static func applySort(_ v: SynxValue, meta: SynxMeta) -> SynxValue {
        guard case .array(var arr) = v else { return v }
        var descending = false
        if let idx = meta.markerIndex("sort"), idx + 1 < meta.markers.count {
            descending = (meta.markers[idx + 1] == "desc")
        }
        arr.sort { a, b in
            if let da = valueToNumber(a), let db = valueToNumber(b) {
                return descending ? da > db : da < db
            }
            let sa = valueToString(a)
            let sb = valueToString(b)
            return descending ? sa > sb : sa < sb
        }
        return .array(arr)
    }

    public static func applySum(_ v: SynxValue) -> SynxValue {
        guard case .array(let arr) = v else { return v }
        var total = 0.0
        var anyFloat = false
        for item in arr {
            if let d = valueToNumber(item) {
                total += d
                if case .float = item { anyFloat = true }
            }
        }
        if anyFloat { return .float(total) }
        return .int(Int64(total))
    }

    public static func applyFallback(_ v: SynxValue, meta: SynxMeta) -> SynxValue {
        let isEmpty: Bool = {
            if case .null = v { return true }
            if case .string(let s) = v, s.isEmpty { return true }
            return false
        }()
        guard isEmpty else { return v }
        if let idx = meta.markerIndex("fallback"), idx + 1 < meta.markers.count {
            return .string(meta.markers[idx + 1])
        }
        return v
    }

    public static func applyVersion(_ v: SynxValue) -> SynxValue {
        if case .string = v { return v }
        return .string(valueToString(v))
    }

    public static func applyWatch(_ v: SynxValue, basePath: String) -> SynxValue {
        guard case .string(let rel) = v else { return v }
        // Local jail check (matches engine.jailPath but inlined to keep this function pure).
        if rel.isEmpty || rel.first == "/" || rel.first == "\\"
            || rel.hasPrefix("res://") || rel.hasPrefix("user://") {
            return v
        }
        if rel.contains("..") { return v }
        let path = (basePath as NSString).appendingPathComponent(rel)
        if let text = try? String(contentsOfFile: path, encoding: .utf8) {
            return .string(text)
        }
        return v
    }

    public static func applyPrompt(_ v: SynxValue, interpolate: (String) -> String) -> SynxValue {
        guard case .string(let s) = v else { return v }
        return .string(interpolate(s))
    }

    // Process-wide rate limit bucket — at most one resolution per (process, key).
    private static let spamLock = NSLock()
    private static var spamBuckets: Set<String> = []

    public static func applySpam(_ v: SynxValue, key: String) -> SynxValue {
        spamLock.lock()
        defer { spamLock.unlock() }
        if spamBuckets.contains(key) { return .null }
        spamBuckets.insert(key)
        return v
    }
}

// MARK: - helpers shared with the engine

func deepGet(_ path: String, in root: SynxObject) -> SynxValue? {
    var current: SynxValue = .object(root)
    for seg in path.split(separator: ".") {
        guard case .object(let obj) = current, let next = obj[String(seg)] else { return nil }
        current = next
    }
    return current
}

/// Whole-word replace over bytes; word characters are ASCII alphanumerics and `_`.
func replaceWord(in hay: [UInt8], word: [UInt8], with replacement: [UInt8]) -> [UInt8] {
    guard !word.isEmpty, word.count <= hay.count else { return hay }
    func isWord(_ b: UInt8) -> Bool {
        return (b >= 0x30 && b <= 0x39) || (b >= 0x41 && b <= 0x5A) || (b >= 0x61 && b <= 0x7A) || b == 0x5F
    }
    var out: [UInt8] = []
    out.reserveCapacity(hay.count)
    var i = 0
    while i < hay.count {
        if i + word.count <= hay.count && Array(hay[i..<i + word.count]) == word {
            let beforeOK = i == 0 || !isWord(hay[i - 1])
            let afterOK = i + word.count >= hay.count || !isWord(hay[i + word.count])
            if beforeOK && afterOK {
                out.append(contentsOf: replacement)
                i += word.count
                continue
            }
        }
        out.append(hay[i])
        i += 1
    }
    return out
}

func applyPrintfPattern(_ pattern: String, number: Double, string: String) -> String {
    var out = ""
    out.reserveCapacity(pattern.count + 16)
    let chars = Array(pattern)
    var i = 0
    while i < chars.count {
        if chars[i] != "%" {
            out.append(chars[i]); i += 1; continue
        }
        if i + 1 < chars.count && chars[i + 1] == "%" {
            out.append("%"); i += 2; continue
        }
        var end = i + 1
        while end < chars.count {
            let k = chars[end]
            if k == "d" || k == "i" || k == "f" || k == "e" || k == "g" || k == "s" { break }
            end += 1
        }
        if end >= chars.count {
            out.append(String(chars[i...])); break
        }
        let spec = String(chars[i...end])
        let kind = chars[end]
        switch kind {
        case "d", "i":
            out.append(String(format: spec, Int(number)))
        case "f", "e", "g":
            out.append(String(format: spec, number))
        case "s":
            out.append(String(format: spec, string))
        default: break
        }
        i = end + 1
    }
    return out
}

// MARK: - CLDR plurals

func pluralCategory(lang: String, n: Double) -> String {
    let two = String(lang.prefix(2))
    let intN = Int64(abs(n).rounded(.down))
    let mod10 = intN % 10
    let mod100 = intN % 100
    let intLike = (n == n.rounded(.down))

    switch two {
    case "ru", "uk", "be":
        if intLike && mod10 == 1 && mod100 != 11 { return "one" }
        if intLike && (2...4).contains(mod10) && !(12...14).contains(mod100) { return "few" }
        if intLike && (mod10 == 0 || (5...9).contains(mod10) || (11...14).contains(mod100)) { return "many" }
        return "other"
    case "pl":
        if intLike && n == 1 { return "one" }
        if intLike && (2...4).contains(mod10) && !(12...14).contains(mod100) { return "few" }
        if intLike && n != 1 && (mod10 == 0 || mod10 == 1
            || (5...9).contains(mod10) || (12...14).contains(mod100)) { return "many" }
        return "other"
    case "cs", "sk":
        if intLike && n == 1 { return "one" }
        if intLike && (2...4).contains(intN) { return "few" }
        if !intLike { return "many" }
        return "other"
    case "ar":
        if n == 0 { return "zero" }
        if n == 1 { return "one" }
        if n == 2 { return "two" }
        if intLike && (3...10).contains(mod100) { return "few" }
        if intLike && mod100 >= 11 { return "many" }
        return "other"
    case "fr", "pt":
        if n >= 0 && n < 2 { return "one" }
        return "other"
    case "ja", "zh", "ko", "vi", "th":
        return "other"
    default:
        return n == 1 ? "one" : "other"
    }
}
