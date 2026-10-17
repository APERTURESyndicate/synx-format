// Canonical JSON serialiser. Port of `write_json` in synx-core/src/lib.rs:
// keys sorted by code point, secrets redacted to "[SECRET]", floats in the
// `ryu` layout, non-finite floats as null.

public enum SynxJSON {

    public static let maxDepth = 128

    public static func encode(_ value: SynxValue) -> String {
        var out: [UInt8] = []
        out.reserveCapacity(2048)
        write(value, 0, &out)
        return String(decoding: out, as: UTF8.self)
    }

    /// A float as Rust's `Value` Display writes it: `f64` Display with `.0`
    /// appended when there is no `.` (`5.0`, `0.25`, `1000000000000000000000.0`).
    public static func displayString(_ f: Double) -> String {
        let s = RustNumber.display(f)
        return s.utf8.contains(0x2E) ? s : s + ".0"
    }

    /// Rust `f64` Display: no `.0` on whole numbers (`5`, `0.25`, `70000`).
    public static func displayNumber(_ f: Double) -> String { RustNumber.display(f) }

    private static let hex: [UInt8] = Array("0123456789abcdef".utf8)

    static func writeString(_ s: String, _ out: inout [UInt8]) {
        out.append(0x22)
        var s = s
        s.withUTF8 { b in
            var start = 0
            for i in 0..<b.count {
                let c = b[i]
                guard c < 0x20 || c == 0x22 || c == 0x5C else { continue }
                out.append(contentsOf: UnsafeBufferPointer(rebasing: b[start..<i]))
                switch c {
                case 0x22: out.append(0x5C); out.append(0x22)
                case 0x5C: out.append(0x5C); out.append(0x5C)
                case 0x0A: out.append(0x5C); out.append(0x6E)
                case 0x0D: out.append(0x5C); out.append(0x72)
                case 0x09: out.append(0x5C); out.append(0x74)
                default:
                    out.append(contentsOf: [0x5C, 0x75, 0x30, 0x30, hex[Int(c >> 4)], hex[Int(c & 0xF)]])
                }
                start = i + 1
            }
            out.append(contentsOf: UnsafeBufferPointer(rebasing: b[start..<b.count]))
        }
        out.append(0x22)
    }

    private static func write(_ v: SynxValue, _ depth: Int, _ out: inout [UInt8]) {
        if depth > maxDepth { out.append(contentsOf: "null".utf8); return }
        switch v {
        case .null: out.append(contentsOf: "null".utf8)
        case .bool(let b): out.append(contentsOf: (b ? "true" : "false").utf8)
        case .int(let n): out.append(contentsOf: String(n).utf8)
        case .float(let f):
            if !f.isFinite { out.append(contentsOf: "null".utf8); return }
            var s = ""
            RustNumber.ryu(f, into: &s)
            out.append(contentsOf: s.utf8)
        case .string(let s): writeString(s, &out)
        case .secret: out.append(contentsOf: "\"[SECRET]\"".utf8)
        case .array(let a):
            out.append(0x5B)
            for (i, item) in a.enumerated() {
                if i > 0 { out.append(0x2C) }
                write(item, depth + 1, &out)
            }
            out.append(0x5D)
        case .object(let map):
            out.append(0x7B)
            let entries = map.entries
            let order = entries.indices.sorted { entries[$0].key.bytesLess(entries[$1].key) }
            for (n, i) in order.enumerated() {
                if n > 0 { out.append(0x2C) }
                writeString(entries[i].key, &out)
                out.append(0x3A)
                write(entries[i].value, depth + 1, &out)
            }
            out.append(0x7D)
        }
    }
}
