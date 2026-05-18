// Canonical JSON serialiser. Sorted keys, secrets redacted to "[SECRET]",
// %.17g floats with mandatory decimal so round-trip stays Float (not Int).
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
            var s = String(format: "%.17g", f)
            // Round-trip parity with Rust ryu: keep a decimal marker.
            if !s.contains(".") && !s.contains("e") && !s.contains("E") {
                s.append(".0")
            }
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
