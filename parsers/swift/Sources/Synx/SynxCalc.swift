// Safe arithmetic evaluator for `:calc`. Byte-for-byte port of crates/synx-core/src/calc.rs,
// including its error messages (`SYNX :calc — …`, with the em dash).
import Foundation

public enum SynxCalc {

    public struct Result {
        public let ok: Bool
        public let value: Double
        public let error: String
    }

    private static let prefix = "SYNX :calc \u{2014} "

    public static func evaluate(_ expr: String) -> Result {
        let trimmed = trimUnicodeWhitespace(expr)
        if trimmed.isEmpty { return Result(ok: true, value: 0, error: "") }
        var tokens: [Token] = []
        if let err = tokenize(Array(trimmed.utf8), into: &tokens) {
            return Result(ok: false, value: 0, error: err)
        }
        if tokens.isEmpty { return Result(ok: true, value: 0, error: "") }
        var parser = ExprParser(tokens: tokens)
        return parser.parse()
    }

    private static func trimUnicodeWhitespace(_ s: String) -> String {
        var scalars = Substring(s).unicodeScalars
        while let f = scalars.first, f.properties.isWhitespace { scalars.removeFirst() }
        while let l = scalars.last, l.properties.isWhitespace { scalars.removeLast() }
        return String(scalars)
    }

    // MARK: - tokens

    private enum Token {
        case number(Double)
        case op(UInt8)
        case lparen
        case rparen
    }

    private static func isDigit(_ b: UInt8) -> Bool { b >= 0x30 && b <= 0x39 }

    /// Rust `str::parse::<f64>` for the only shapes the tokenizer can produce:
    /// optional `-`, digits and dots. Needs at least one digit and at most one dot.
    private static func parseNumber(_ s: [UInt8]) -> Double? {
        var i = 0
        var neg = false
        if i < s.count && s[i] == 0x2D { neg = true; i += 1 }
        var intPart = ""
        var frac = ""
        var seenDot = false
        while i < s.count {
            let b = s[i]
            if b == 0x2E {
                if seenDot { return nil }
                seenDot = true
            } else if seenDot {
                frac.append(Character(UnicodeScalar(b)))
            } else {
                intPart.append(Character(UnicodeScalar(b)))
            }
            i += 1
        }
        if intPart.isEmpty && frac.isEmpty { return nil }
        guard let d = Double((intPart.isEmpty ? "0" : intPart) + "." + (frac.isEmpty ? "0" : frac)) else {
            return nil
        }
        return neg ? -d : d
    }

    private static func tokenize(_ b: [UInt8], into tokens: inout [Token]) -> String? {
        var i = 0
        let len = b.count
        while i < len {
            let ch = b[i]
            if ch == 0x20 || ch == 0x09 { i += 1; continue }

            let afterOperator: Bool = {
                if tokens.isEmpty { return true }
                switch tokens.last! {
                case .op, .lparen: return true
                default: return false
                }
            }()
            if isDigit(ch)
                || (ch == 0x2E && i + 1 < len && isDigit(b[i + 1]))
                || (ch == 0x2D && afterOperator) {
                let start = i
                if ch == 0x2D { i += 1 }
                while i < len && (isDigit(b[i]) || b[i] == 0x2E) { i += 1 }
                let numStr = Array(b[start..<i])
                guard let d = parseNumber(numStr) else {
                    return prefix + "invalid number: '\(String(decoding: numStr, as: UTF8.self))'"
                }
                tokens.append(.number(d))
                continue
            }
            if ch == 0x2B || ch == 0x2D || ch == 0x2A || ch == 0x2F || ch == 0x25 {
                tokens.append(.op(ch)); i += 1; continue
            }
            if ch == 0x28 { tokens.append(.lparen); i += 1; continue }
            if ch == 0x29 { tokens.append(.rparen); i += 1; continue }
            // Rust prints `ch as char`: a single byte widened to U+00xx.
            return prefix + "unexpected character: '\(Character(UnicodeScalar(ch)))' in expression"
        }
        return nil
    }

    private struct ExprParser {
        let tokens: [Token]
        var pos: Int = 0

        mutating func parse() -> Result {
            var v: Double = 0
            if let err = expr(&v) { return Result(ok: false, value: 0, error: err) }
            if pos < tokens.count {
                return Result(ok: false, value: 0,
                              error: SynxCalc.prefix + "unexpected token at position \(pos)")
            }
            return Result(ok: true, value: v, error: "")
        }

        mutating func expr(_ out: inout Double) -> String? {
            var left: Double = 0
            if let e = term(&left) { return e }
            while pos < tokens.count {
                if case .op(let c) = tokens[pos], c == 0x2B {
                    pos += 1
                    var r: Double = 0
                    if let e = term(&r) { return e }
                    left += r
                } else if case .op(let c) = tokens[pos], c == 0x2D {
                    pos += 1
                    var r: Double = 0
                    if let e = term(&r) { return e }
                    left -= r
                } else { break }
            }
            out = left
            return nil
        }

        mutating func term(_ out: inout Double) -> String? {
            var left: Double = 0
            if let e = factor(&left) { return e }
            while pos < tokens.count {
                guard case .op(let c) = tokens[pos] else { break }
                switch c {
                case 0x2A:
                    pos += 1
                    var r: Double = 0
                    if let e = factor(&r) { return e }
                    left *= r
                case 0x2F:
                    pos += 1
                    var r: Double = 0
                    if let e = factor(&r) { return e }
                    if r == 0 { return SynxCalc.prefix + "division by zero" }
                    left /= r
                case 0x25:
                    pos += 1
                    var r: Double = 0
                    if let e = factor(&r) { return e }
                    if r == 0 { return SynxCalc.prefix + "division by zero" }
                    left = left.truncatingRemainder(dividingBy: r)
                default:
                    out = left
                    return nil
                }
            }
            out = left
            return nil
        }

        mutating func factor(_ out: inout Double) -> String? {
            if pos >= tokens.count { return SynxCalc.prefix + "unexpected end of expression" }
            switch tokens[pos] {
            case .number(let n):
                pos += 1
                out = n
                return nil
            case .lparen:
                pos += 1
                var v: Double = 0
                if let e = expr(&v) { return e }
                if pos >= tokens.count { return SynxCalc.prefix + "missing closing parenthesis" }
                if case .rparen = tokens[pos] { pos += 1; out = v; return nil }
                return SynxCalc.prefix + "missing closing parenthesis"
            case .op(let c):
                return SynxCalc.prefix + "unexpected token: Op(\(c))"
            case .rparen:
                return SynxCalc.prefix + "unexpected token: RParen"
            }
        }
    }
}
