// Numbers with Rust semantics: `str::parse` for i64 / u64 / f64, and the text
// Rust writes for f64 — Display (`{}`), `{:.N}`, `{:e}`, `{:.Ne}` — and the
// `ryu` layout of the JSON projection.

enum RustNumber {

    // MARK: - parsing

    /// Rust `i64::from_str`: optional `+`/`-`, ASCII digits, no overflow.
    static func parseI64<C: Collection>(_ s: C) -> Int64? where C.Element == UInt8 {
        var it = s.makeIterator()
        guard var c = it.next() else { return nil }
        var neg = false
        if c == 0x2D || c == 0x2B {
            neg = c == 0x2D
            guard let n = it.next() else { return nil }
            c = n
        }
        var v: UInt64 = 0
        let limit: UInt64 = neg ? 9223372036854775808 : 9223372036854775807
        while true {
            guard c >= 0x30 && c <= 0x39 else { return nil }
            let d = UInt64(c - 0x30)
            if v > (limit - d) / 10 { return nil }
            v = v * 10 + d
            guard let n = it.next() else { break }
            c = n
        }
        return neg ? Int64(bitPattern: 0 &- v) : Int64(v)
    }

    static func parseI64(_ s: String) -> Int64? { parseI64(s.utf8) }

    /// Rust `u64::from_str` (an optional `+`, ASCII digits).
    static func parseU64(_ s: String) -> UInt64? {
        var u = Substring(s).utf8
        if u.first == 0x2B { u = u.dropFirst() }
        if u.isEmpty { return nil }
        var v: UInt64 = 0
        for c in u {
            guard c >= 0x30 && c <= 0x39 else { return nil }
            let d = UInt64(c - 0x30)
            if v > (UInt64.max - d) / 10 { return nil }
            v = v * 10 + d
        }
        return v
    }

    /// Rust `f64::from_str`: `[+-]? (inf | infinity | nan | digits[.digits][e[+-]digits])`,
    /// case-insensitive words, no whitespace, no hex. Overflow is ±inf, underflow ±0.
    static func parseF64<C: Collection>(_ s: C) -> Double? where C.Element == UInt8 {
        let b = Array(s)
        var i = 0
        var neg = false
        if i < b.count && (b[i] == 0x2D || b[i] == 0x2B) { neg = b[i] == 0x2D; i += 1 }
        let word = String(decoding: b[i...], as: UTF8.self).lowercased()
        if word == "inf" || word == "infinity" { return neg ? -.infinity : .infinity }
        if word == "nan" { return .nan }
        let intStart = i
        while i < b.count && b[i] >= 0x30 && b[i] <= 0x39 { i += 1 }
        let intEnd = i
        var fracStart = i, fracEnd = i
        if i < b.count && b[i] == 0x2E {
            i += 1
            fracStart = i
            while i < b.count && b[i] >= 0x30 && b[i] <= 0x39 { i += 1 }
            fracEnd = i
        }
        if intEnd == intStart && fracEnd == fracStart { return nil }
        var expText: [UInt8] = []
        if i < b.count && (b[i] == 0x65 || b[i] == 0x45) {
            i += 1
            if i < b.count && (b[i] == 0x2B || b[i] == 0x2D) { if b[i] == 0x2D { expText.append(0x2D) }; i += 1 }
            let ds = i
            while i < b.count && b[i] >= 0x30 && b[i] <= 0x39 { i += 1 }
            if i == ds { return nil }
            expText.append(contentsOf: b[ds..<i])
        }
        if i != b.count { return nil }
        var norm: [UInt8] = intEnd > intStart ? Array(b[intStart..<intEnd]) : [0x30]
        if fracEnd > fracStart { norm.append(0x2E); norm.append(contentsOf: b[fracStart..<fracEnd]) }
        if !expText.isEmpty { norm.append(0x65); norm.append(contentsOf: expText) }
        // The grammar is checked; Swift's own reader rounds correctly.
        guard let v = Double(String(decoding: norm, as: UTF8.self)) else { return nil }
        return neg ? -v : v
    }

    static func parseF64(_ s: String) -> Double? { parseF64(s.utf8) }

    // MARK: - shortest digits

    /// Shortest round-trip digits of finite, non-zero `abs(f)`:
    /// value = 0.<digits> × 10^point.
    static func shortest(_ f: Double) -> (digits: [UInt8], point: Int) {
        // Swift's description is shortest and closest (SwiftDtoa); only its
        // layout differs from Rust's.
        let text = Array(abs(f).description.utf8)
        var digits: [UInt8] = []
        var point = 0
        var seenDot = false
        var i = 0
        while i < text.count && text[i] != 0x65 {
            let c = text[i]
            if c == 0x2E {
                seenDot = true
            } else {
                digits.append(c)
                if !seenDot { point += 1 }
            }
            i += 1
        }
        if i < text.count {
            point += Int(String(decoding: text[(i + 1)...], as: UTF8.self)) ?? 0
        }
        // Strip leading zeros (`0.001`) and trailing zeros (`100.0`).
        var lead = 0
        while lead < digits.count - 1 && digits[lead] == 0x30 { lead += 1; point -= 1 }
        digits.removeFirst(lead)
        while digits.count > 1 && digits.last == 0x30 { digits.removeLast() }
        return (digits, point)
    }

    // MARK: - Display / format_number

    /// Rust `f64` Display: plain decimal, never an exponent, a whole number has
    /// no fraction (`5`, `-0`, `1e21` → `1000000000000000000000`).
    static func display(_ f: Double) -> String {
        if f.isNaN { return "NaN" }
        if f.isInfinite { return f < 0 ? "-inf" : "inf" }
        if f == 0 { return f.sign == .minus ? "-0" : "0" }
        let (d, point) = shortest(f)
        var out: [UInt8] = f < 0 ? [0x2D] : []
        if point <= 0 {
            out.append(0x30); out.append(0x2E)
            out.append(contentsOf: [UInt8](repeating: 0x30, count: -point))
            out.append(contentsOf: d)
        } else if point < d.count {
            out.append(contentsOf: d[0..<point]); out.append(0x2E); out.append(contentsOf: d[point...])
        } else {
            out.append(contentsOf: d)
            out.append(contentsOf: [UInt8](repeating: 0x30, count: point - d.count))
        }
        return String(decoding: out, as: UTF8.self)
    }

    /// Rust `f64::fract() == 0.0` (false for NaN and ±inf).
    @inline(__always) static func isWhole(_ f: Double) -> Bool { f.isFinite && f.rounded(.towardZero) == f }

    /// Rust `format_number`: a whole float below 2^63 as an integer, else Display.
    static func formatNumber(_ n: Double) -> String {
        if isWhole(n) && abs(n) < 9223372036854775808.0 { return String(Int64(n)) }
        return display(n)
    }

    /// Rust `f64 as i64`: saturating, NaN is 0.
    static func saturatingI64(_ f: Double) -> Int64 {
        if f.isNaN { return 0 }
        if f >= 9223372036854775807.0 { return .max }
        if f <= -9223372036854775808.0 { return .min }
        return Int64(f)
    }

    /// Rust `f64::powi` (compiler-rt `__powidf2`: repeated squaring).
    static func powi(_ base: Double, _ exp: Int32) -> Double {
        var b = base, e = exp, r = 1.0
        let recip = e < 0
        while true {
            if e & 1 != 0 { r *= b }
            e /= 2
            if e == 0 { break }
            b *= b
        }
        return recip ? 1 / r : r
    }

    // MARK: - ryu (JSON)

    /// `ryu::Buffer::format` layout: `1.0`, `0.001`, `1e-7`, `1.5e16`. `f` finite.
    static func ryu(_ f: Double, into out: inout String) {
        if f == 0 { out.append(f.sign == .minus ? "-0.0" : "0.0"); return }
        let (d, kk) = shortest(f)
        let len = d.count
        var o: [UInt8] = f < 0 ? [0x2D] : []
        if kk >= len && kk <= 16 {
            o.append(contentsOf: d); o.append(contentsOf: [UInt8](repeating: 0x30, count: kk - len)); o.append(0x2E); o.append(0x30)
        } else if kk > 0 && kk <= 16 {
            o.append(contentsOf: d[0..<kk]); o.append(0x2E); o.append(contentsOf: d[kk...])
        } else if kk > -5 && kk <= 0 {
            o.append(0x30); o.append(0x2E); o.append(contentsOf: [UInt8](repeating: 0x30, count: -kk)); o.append(contentsOf: d)
        } else {
            o.append(d[0])
            if len > 1 { o.append(0x2E); o.append(contentsOf: d[1...]) }
            o.append(0x65)
            o.append(contentsOf: Array(String(kk - 1).utf8))
        }
        out.append(String(decoding: o, as: UTF8.self))
    }

    // MARK: - exact formatting ({:.N}, {:e}, {:.Ne})

    /// The exact decimal expansion of finite, non-zero `abs(f)`:
    /// value = 0.<digits> × 10^point (an f64 always terminates).
    static func exactDigits(_ f: Double) -> (digits: [UInt8], point: Int) {
        let m = abs(f).significandBitPattern | (abs(f).isNormal ? (1 << 52) : 0)
        let e = abs(f).isNormal ? Int(abs(f).exponentBitPattern) - 1075 : -1074
        var big = BigDecimal(m)
        var point: Int
        if e >= 0 {
            var k = e
            while k > 0 { let s = min(k, 28); big.multiply(by: 1 << UInt32(s)); k -= s }
            point = 0
        } else {
            var k = -e
            while k > 0 { let s = min(k, 13); var p: UInt32 = 1; for _ in 0..<s { p *= 5 }; big.multiply(by: p); k -= s }
            point = e  // × 10^e
        }
        var digits = big.decimalDigits()
        point += digits.count
        while digits.count > 1 && digits.last == 0x30 { digits.removeLast() }
        return (digits, point)
    }

    /// Round `digits` to `keep` leading digits, half to even on the exact value.
    /// Returns the rounded digits (exactly `keep` long, may carry into one more)
    /// and whether a carry added a leading digit.
    private static func roundDigits(_ digits: [UInt8], keep: Int) -> ([UInt8], Bool) {
        if keep < 0 { return ([], false) }
        if keep >= digits.count {
            return (digits + [UInt8](repeating: 0x30, count: keep - digits.count), false)
        }
        var out = Array(digits[0..<keep])
        let next = digits[keep]
        let restNonZero = digits[(keep + 1)...].contains { $0 != 0x30 }
        var roundUp = next > 0x35 || (next == 0x35 && restNonZero)
        if next == 0x35 && !restNonZero {
            // A tie: up only when the kept last digit is odd (none kept: down).
            roundUp = keep > 0 && (out[keep - 1] - 0x30) % 2 == 1
        }
        if !roundUp { return (out, false) }
        var i = keep - 1
        while i >= 0 {
            if out[i] == 0x39 { out[i] = 0x30; i -= 1 } else { out[i] += 1; return (out, false) }
        }
        out.insert(0x31, at: 0)
        return (out, true)
    }

    /// Rust `{:.prec$}`.
    static func fixed(_ f: Double, _ prec: Int) -> String {
        if f.isNaN { return "NaN" }
        if f.isInfinite { return f < 0 ? "-inf" : "inf" }
        var out: [UInt8] = f.sign == .minus ? [0x2D] : []
        if f == 0 {
            out.append(0x30)
            if prec > 0 { out.append(0x2E); out.append(contentsOf: [UInt8](repeating: 0x30, count: prec)) }
            return String(decoding: out, as: UTF8.self)
        }
        let (d, point) = exactDigits(f)
        // Digits needed: everything before the point plus `prec` after it.
        var (r, carried) = roundDigits(d, keep: point + prec)
        var p = point
        if carried { p += 1 }
        if p + prec <= 0 { r = [] }
        // r holds the digits from the first significant one; lay them out.
        var intPart: [UInt8], fracPart: [UInt8]
        if p <= 0 {
            intPart = [0x30]
            fracPart = [UInt8](repeating: 0x30, count: min(-p, prec)) + r
        } else {
            intPart = Array(r.prefix(p))
            if intPart.count < p { intPart += [UInt8](repeating: 0x30, count: p - intPart.count) }
            fracPart = r.count > p ? Array(r[p...]) : []
        }
        if fracPart.count < prec { fracPart += [UInt8](repeating: 0x30, count: prec - fracPart.count) }
        fracPart = Array(fracPart.prefix(prec))
        out.append(contentsOf: intPart)
        if prec > 0 { out.append(0x2E); out.append(contentsOf: fracPart) }
        return String(decoding: out, as: UTF8.self)
    }

    /// Rust `{:e}`: shortest digits, `1.5e3`, `1e0`, `1e-7`.
    static func exp(_ f: Double) -> String {
        if f.isNaN { return "NaN" }
        if f.isInfinite { return f < 0 ? "-inf" : "inf" }
        if f == 0 { return f.sign == .minus ? "-0e0" : "0e0" }
        let (d, point) = shortest(f)
        return layoutExp(d, point - 1, negative: f < 0)
    }

    /// Rust `{:.prec$e}`.
    static func expPrec(_ f: Double, _ prec: Int) -> String {
        if f.isNaN { return "NaN" }
        if f.isInfinite { return f < 0 ? "-inf" : "inf" }
        if f == 0 {
            var s = f.sign == .minus ? "-0" : "0"
            if prec > 0 { s += "." + String(repeating: "0", count: prec) }
            return s + "e0"
        }
        let (d, point) = exactDigits(f)
        let (r, carried) = roundDigits(d, keep: prec + 1)
        return layoutExp(Array(r.prefix(prec + 1)), point - 1 + (carried ? 1 : 0), negative: f < 0)
    }

    private static func layoutExp(_ d: [UInt8], _ e: Int, negative: Bool) -> String {
        var o: [UInt8] = negative ? [0x2D] : []
        o.append(d[0])
        if d.count > 1 { o.append(0x2E); o.append(contentsOf: d[1...]) }
        o.append(0x65)
        o.append(contentsOf: Array(String(e).utf8))
        return String(decoding: o, as: UTF8.self)
    }
}

/// Unsigned big integer in base 10^9, enough for the exact expansion of an f64.
private struct BigDecimal {
    private var limbs: [UInt32]  // little-endian, base 1e9

    init(_ v: UInt64) {
        limbs = []
        var x = v
        repeat { limbs.append(UInt32(x % 1_000_000_000)); x /= 1_000_000_000 } while x > 0
    }

    mutating func multiply(by m: UInt32) {
        var carry: UInt64 = 0
        for i in 0..<limbs.count {
            let p = UInt64(limbs[i]) * UInt64(m) + carry
            limbs[i] = UInt32(p % 1_000_000_000)
            carry = p / 1_000_000_000
        }
        while carry > 0 { limbs.append(UInt32(carry % 1_000_000_000)); carry /= 1_000_000_000 }
    }

    func decimalDigits() -> [UInt8] {
        var out = Array(String(limbs.last!).utf8)
        for limb in limbs.dropLast().reversed() {
            let s = Array(String(limb).utf8)
            out += [UInt8](repeating: 0x30, count: 9 - s.count) + s
        }
        return out
    }
}
