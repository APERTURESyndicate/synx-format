// Canonical JSON serialiser. Sorted keys, secrets redacted to "[SECRET]",
// shortest round-trip floats in Rust `ryu` layout (mandatory decimal so the
// value stays a Float, not an Int).
import Foundation

public enum SynxJSON {

    public static let maxDepth = 128

    public static func encode(_ value: SynxValue) -> String {
        var out = ""
        out.reserveCapacity(2048)
        write(value, depth: 0, into: &out)
        return out
    }

    private static func write(_ v: SynxValue, depth: Int, into out: inout String) {
        if depth > maxDepth { out.append("null"); return }
        switch v {
        case .null: out.append("null")
        case .bool(let b): out.append(b ? "true" : "false")
        case .int(let n): out.append(String(n))
        case .float(let f):
            if f.isNaN || f.isInfinite { out.append("null"); return }
            let s = ryuString(f)
            out.append(s)
        case .string(let s):
            out.append("\"")
            escape(s, into: &out)
            out.append("\"")
        case .secret:
            out.append("\"[SECRET]\"")
        case .array(let a):
            out.append("[")
            for (i, item) in a.enumerated() {
                if i > 0 { out.append(",") }
                write(item, depth: depth + 1, into: &out)
            }
            out.append("]")
        case .object(let map):
            out.append("{")
            let sortedKeys = map.keys.sorted()
            for (i, key) in sortedKeys.enumerated() {
                if i > 0 { out.append(",") }
                out.append("\"")
                escape(key, into: &out)
                out.append("\":")
                if let v = map[key] {
                    write(v, depth: depth + 1, into: &out)
                } else {
                    out.append("null")
                }
            }
            out.append("}")
        }
    }

    /// Shortest round-trip decimal digits of a finite, non-zero `abs(f)` and the
    /// decimal exponent `kk` such that value = 0.<digits> * 10^kk.
    static func shortestDigits(_ f: Double) -> (digits: String, kk: Int) {
        // Swift's `description` already yields the shortest round-trip digits;
        // only the layout differs, so re-lay-out its digits and exponent.
        var desc = abs(f).description
        var exp10 = 0
        if let e = desc.firstIndex(where: { $0 == "e" || $0 == "E" }) {
            exp10 = Int(desc[desc.index(after: e)...]) ?? 0
            desc = String(desc[..<e])
        }
        var digits = ""
        var pointAt = desc.count
        for (i, ch) in desc.enumerated() {
            if ch == "." { pointAt = i } else { digits.append(ch) }
        }
        var kk = pointAt + exp10
        while digits.hasPrefix("0") { digits.removeFirst(); kk -= 1 }
        while digits.hasSuffix("0") { digits.removeLast() }
        return (digits, kk)
    }

    /// Plain positional notation of `digits`/`kk` (no exponent), always with a `.`.
    private static func plain(_ digits: String, _ kk: Int) -> String {
        let n = digits.count
        if kk <= 0 { return "0." + String(repeating: "0", count: -kk) + digits }
        if n <= kk { return digits + String(repeating: "0", count: kk - n) + ".0" }
        let idx = digits.index(digits.startIndex, offsetBy: kk)
        return digits[..<idx] + "." + digits[idx...]
    }

    /// Layout of Rust's `ryu::Buffer::format`: plain notation for
    /// 1e-5 <= |x| < 1e16 (always with a `.`), else `d.ddde<exp>` with no `+`
    /// and no exponent padding.
    static func ryuString(_ f: Double) -> String {
        if f == 0 { return f.sign == .minus ? "-0.0" : "0.0" }
        let (digits, kk) = shortestDigits(f)
        let sign = f < 0 ? "-" : ""
        if kk > -5 && kk <= 16 { return sign + plain(digits, kk) }
        var out = sign + String(digits.first!)
        if digits.count > 1 { out += "." + digits.dropFirst() }
        return out + "e" + String(kk - 1)
    }

    /// Rust `f64::to_string()` (Display): shortest digits, never an exponent,
    /// `.0` appended to whole numbers.
    static func displayString(_ f: Double) -> String {
        if f == 0 { return f.sign == .minus ? "-0.0" : "0.0" }
        let (digits, kk) = shortestDigits(f)
        return (f < 0 ? "-" : "") + plain(digits, kk)
    }

    /// Rust `f64` Display as used inside messages: like `displayString` but a
    /// whole number carries no `.0` (`5`, `0.25`, `70000`).
    static func displayNumber(_ f: Double) -> String {
        if f.isNaN { return "NaN" }
        if f.isInfinite { return f < 0 ? "-inf" : "inf" }
        let s = displayString(f)
        return s.hasSuffix(".0") ? String(s.dropLast(2)) : s
    }

    private static func escape(_ s: String, into out: inout String) {
        for scalar in s.unicodeScalars {
            switch scalar {
            case "\"":  out.append("\\\"")
            case "\\":  out.append("\\\\")
            case "\n":  out.append("\\n")
            case "\r":  out.append("\\r")
            case "\t":  out.append("\\t")
            default:
                if scalar.value < 0x20 {
                    out.append(String(format: "\\u%04x", scalar.value))
                } else {
                    out.unicodeScalars.append(scalar)
                }
            }
        }
    }
}
