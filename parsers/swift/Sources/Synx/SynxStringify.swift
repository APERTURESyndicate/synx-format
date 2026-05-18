// Value → SYNX text. Mirrors crates/synx-core/src/lib.rs `serialize`.
import Foundation

public enum SynxStringify {

    public static let maxDepth = 128

    public static func stringify(_ value: SynxValue) -> String {
        var out = ""
        out.reserveCapacity(2048)
        serialize(value, depth: 0, into: &out)
        return out
    }

    private static func serialize(_ v: SynxValue, depth: Int, into out: inout String) {
        if depth > maxDepth { out.append("[synx:max-depth]\n"); return }
        guard case .object(let map) = v else {
            out.append(formatPrimitive(v))
            return
        }
        let indent = String(repeating: " ", count: depth * 2)
        for key in map.keys.sorted() {
            guard let val = map[key] else { continue }
            switch val {
            case .array(let arr):
                out.append(indent); out.append(key); out.append("\n")
                for item in arr {
                    if case .object(let inner) = item {
                        let keys = inner.keys
                        if let first = keys.first, let firstVal = inner[first] {
                            out.append(indent); out.append("  - "); out.append(first)
                            out.append(" "); out.append(formatPrimitive(firstVal)); out.append("\n")
                            for k in keys.dropFirst() {
                                if let v = inner[k] {
                                    out.append(indent); out.append("    "); out.append(k)
                                    out.append(" "); out.append(formatPrimitive(v)); out.append("\n")
                                }
                            }
                        }
                    } else {
                        out.append(indent); out.append("  - ")
                        out.append(formatPrimitive(item)); out.append("\n")
                    }
                }
            case .object:
                out.append(indent); out.append(key); out.append("\n")
                serialize(val, depth: depth + 1, into: &out)
            case .string(let s) where s.contains("\n"):
                out.append(indent); out.append(key); out.append(" |\n")
                for line in s.split(separator: "\n", omittingEmptySubsequences: false) {
                    out.append(indent); out.append("  "); out.append(String(line)); out.append("\n")
                }
            default:
                out.append(indent); out.append(key); out.append(" ")
                out.append(formatPrimitive(val)); out.append("\n")
            }
        }
    }

    public static func formatPrimitive(_ v: SynxValue) -> String {
        switch v {
        case .string(let s): return s
        case .int(let n): return String(n)
        case .float(let f):
            if f.isNaN || f.isInfinite { return "null" }
            var s = String(format: "%.17g", f)
            if !s.contains(".") && !s.contains("e") && !s.contains("E") {
                s.append(".0")
            }
            return s
        case .bool(let b): return b ? "true" : "false"
        case .null: return "null"
        case .array(let arr):
            let parts = arr.map { formatPrimitive($0) }
            return "[" + parts.joined(separator: ", ") + "]"
        case .object: return "[Object]"
        case .secret: return "[SECRET]"
        }
    }
}
