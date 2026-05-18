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
        guard let env = options.env else { return v }
        let varName = valueToString(v)
        var fallback = ""
        if let idx = meta.markerIndex("env"),
           idx + 1 < meta.markers.count, meta.markers[idx + 1] == "default",
           idx + 2 < meta.markers.count {
            fallback = meta.markers[idx + 2]
        }
        if let val = env[varName] { return .string(val) }
        if !fallback.isEmpty { return .string(fallback) }
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
            return .string(meta.markers[idx + 1])
        }
        return v
    }

    public static func applyCalc(_ v: SynxValue, meta: SynxMeta,
                                  root: SynxObject,
                                  interpolate: (String) -> String) -> SynxValue {
        var expr = valueToString(v)
        if expr.isEmpty { return v }
        expr = interpolate(expr)

        // Word-replace sibling numeric identifiers — sort longest-first to avoid
        // partial overlaps (e.g. `price` vs `price2`).
        let pairs: [(String, Double)] = root.entries.compactMap { (k, val) in
            valueToNumber(val).map { (k, $0) }
        }
        let sorted = pairs.sorted { $0.0.count > $1.0.count }
        for (k, d) in sorted {
            expr = replaceWord(in: expr, word: k, with: String(format: "%.17g", d))
        }

        let r = SynxCalc.evaluate(expr)
        if !r.ok { return v }
        if r.value.truncatingRemainder(dividingBy: 1) == 0
            && abs(r.value) < 9.2233720368547758e18 {
            return .int(Int64(r.value))
        }
        return .float(r.value)
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

func replaceWord(in s: String, word: String, with replacement: String) -> String {
    guard !word.isEmpty else { return s }
    var out = ""
    out.reserveCapacity(s.count)
    let chars = Array(s)
    let wchars = Array(word)
    var i = 0
    while i < chars.count {
        if i + wchars.count <= chars.count
            && Array(chars[i..<i + wchars.count]) == wchars {
            let leftOK: Bool = {
                if i == 0 { return true }
                let prev = chars[i - 1]
                return !(prev.isLetter || prev.isNumber || prev == "_")
            }()
            let rightOK: Bool = {
                let after = i + wchars.count
                if after == chars.count { return true }
                let next = chars[after]
                return !(next.isLetter || next.isNumber || next == "_")
            }()
            if leftOK && rightOK {
                out.append(replacement)
                i += wchars.count
                continue
            }
        }
        out.append(chars[i])
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
