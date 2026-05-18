// Safe arithmetic evaluator for `:calc`. Mirrors crates/synx-core/src/calc.rs.
import Foundation

public enum SynxCalc {

    public struct Result {
        public let ok: Bool
        public let value: Double
        public let error: String
    }

    public static func evaluate(_ expr: String) -> Result {
        let trimmed = expr.trimmingCharacters(in: .whitespaces)
        if trimmed.isEmpty { return Result(ok: true, value: 0, error: "") }
        var tokens: [Token] = []
        if let err = tokenize(trimmed, into: &tokens) {
            return Result(ok: false, value: 0, error: err)
        }
        if tokens.isEmpty { return Result(ok: true, value: 0, error: "") }
        var parser = ExprParser(tokens: tokens)
        return parser.parse()
    }

    // MARK: - tokens

    private enum Token {
        case number(Double)
        case op(Character)
        case lparen
        case rparen
    }

    private static func tokenize(_ expr: String, into tokens: inout [Token]) -> String? {
        let chars = Array(expr)
        var i = 0
        while i < chars.count {
            let c = chars[i]
            if c == " " || c == "\t" { i += 1; continue }

            let isDigit = (c >= "0" && c <= "9")
            let isDotNumber = (c == "." && i + 1 < chars.count
                               && chars[i + 1] >= "0" && chars[i + 1] <= "9")
            let isUnaryMinus: Bool = {
                if c != "-" { return false }
                if tokens.isEmpty { return true }
                switch tokens.last! {
                case .op, .lparen: return true
                default: return false
                }
            }()
            if isDigit || isDotNumber || isUnaryMinus {
                let start = i
                if c == "-" { i += 1 }
                while i < chars.count && (chars[i] >= "0" && chars[i] <= "9" || chars[i] == ".") {
                    i += 1
                }
                let str = String(chars[start..<i])
                guard let d = Double(str) else {
                    return "SYNX :calc - invalid number: '\(str)'"
                }
                tokens.append(.number(d))
                continue
            }
            if c == "+" || c == "-" || c == "*" || c == "/" || c == "%" {
                tokens.append(.op(c)); i += 1; continue
            }
            if c == "(" { tokens.append(.lparen); i += 1; continue }
            if c == ")" { tokens.append(.rparen); i += 1; continue }
            return "SYNX :calc - unexpected character: '\(c)'"
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
                              error: "SYNX :calc - unexpected token at position \(pos)")
            }
            return Result(ok: true, value: v, error: "")
        }

        mutating func expr(_ out: inout Double) -> String? {
            var left: Double = 0
            if let e = term(&left) { return e }
            while pos < tokens.count {
                if case .op(let c) = tokens[pos], c == "+" {
                    pos += 1
                    var r: Double = 0
                    if let e = term(&r) { return e }
                    left += r
                } else if case .op(let c) = tokens[pos], c == "-" {
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
                if case .op(let c) = tokens[pos] {
                    switch c {
                    case "*":
                        pos += 1
                        var r: Double = 0
                        if let e = factor(&r) { return e }
                        left *= r
                    case "/":
                        pos += 1
                        var r: Double = 0
                        if let e = factor(&r) { return e }
                        if r == 0 { return "SYNX :calc - division by zero" }
                        left /= r
                    case "%":
                        pos += 1
                        var r: Double = 0
                        if let e = factor(&r) { return e }
                        if r == 0 { return "SYNX :calc - division by zero" }
                        left = left.truncatingRemainder(dividingBy: r)
                    default: out = left; return nil
                    }
                } else { break }
            }
            out = left
            return nil
        }

        mutating func factor(_ out: inout Double) -> String? {
            if pos >= tokens.count { return "SYNX :calc - unexpected end of expression" }
            switch tokens[pos] {
            case .number(let n):
                pos += 1
                out = n
                return nil
            case .lparen:
                pos += 1
                var v: Double = 0
                if let e = expr(&v) { return e }
                if pos >= tokens.count {
                    return "SYNX :calc - missing closing parenthesis"
                }
                if case .rparen = tokens[pos] { pos += 1; out = v; return nil }
                return "SYNX :calc - missing closing parenthesis"
            default:
                return "SYNX :calc - unexpected token"
            }
        }
    }
}
