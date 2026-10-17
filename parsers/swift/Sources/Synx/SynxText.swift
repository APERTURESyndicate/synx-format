// Text helpers with Rust `char` / `str` semantics, over UTF-8 bytes: White_Space
// trimming and indentation (SYNX §4), `char::is_alphanumeric`,
// `str::to_lowercase`, plus the key index shared by the ordered maps.
import Foundation

/// Byte length of the Unicode whitespace character at `i` (Rust
/// `char::is_whitespace`: U+0009–000D, U+0020, U+0085, U+00A0, U+1680,
/// U+2000–200A, U+2028/2029, U+202F, U+205F, U+3000), or 0.
@inline(__always)
func wsLen(_ b: UnsafeBufferPointer<UInt8>, _ i: Int, _ end: Int) -> Int {
    let c = b[i]
    if c == 0x20 || (c >= 0x09 && c <= 0x0D) { return 1 }
    if c < 0x80 { return 0 }
    let rest = end - i
    if c == 0xC2 && rest >= 2 && (b[i + 1] == 0x85 || b[i + 1] == 0xA0) { return 2 }
    if c == 0xE1 && rest >= 3 && b[i + 1] == 0x9A && b[i + 2] == 0x80 { return 3 }
    if c == 0xE2 && rest >= 3 {
        let b1 = b[i + 1], b2 = b[i + 2]
        if b1 == 0x80 && ((b2 >= 0x80 && b2 <= 0x8A) || b2 == 0xA8 || b2 == 0xA9 || b2 == 0xAF) { return 3 }
        if b1 == 0x81 && b2 == 0x9F { return 3 }
    }
    if c == 0xE3 && rest >= 3 && b[i + 1] == 0x80 && b[i + 2] == 0x80 { return 3 }
    return 0
}

/// Byte length of the Unicode whitespace character ending at `end`, or 0.
@inline(__always)
func wsLenBefore(_ b: UnsafeBufferPointer<UInt8>, _ start: Int, _ end: Int) -> Int {
    if end <= start { return 0 }
    if b[end - 1] < 0x80 { return wsLen(b, end - 1, end) }
    var n = 2
    while n <= 3 && end - n >= start {
        if wsLen(b, end - n, end) == n { return n }
        n += 1
    }
    return 0
}

@inline(__always)
func isAsciiWS(_ c: UInt8) -> Bool { c == 0x20 || (c >= 0x09 && c <= 0x0D) }

/// Rust `str::trim` over `start..<end`, ASCII fast path at both edges.
@inline(__always)
func trimRange(_ b: UnsafeBufferPointer<UInt8>, _ start: Int, _ end: Int) -> Range<Int> {
    var s = start, e = end
    while s < e && isAsciiWS(b[s]) { s += 1 }
    if s < e && b[s] >= 0x80 {
        while s < e { let n = wsLen(b, s, e); if n == 0 { break }; s += n }
    }
    while e > s && isAsciiWS(b[e - 1]) { e -= 1 }
    if e > s && b[e - 1] >= 0x80 {
        while e > s { let n = wsLenBefore(b, s, e); if n == 0 { break }; e -= n }
    }
    return s..<e
}

/// Rust `str::trim_end`.
func trimEndRange(_ b: UnsafeBufferPointer<UInt8>, _ start: Int, _ end: Int) -> Int {
    var e = end
    while e > start { let n = wsLenBefore(b, start, e); if n == 0 { break }; e -= n }
    return e
}

/// Byte length of the leading Unicode whitespace — the SYNX indentation.
@inline(__always)
func indentLen(_ b: UnsafeBufferPointer<UInt8>, _ start: Int, _ end: Int) -> Int {
    var i = start
    while i < end && isAsciiWS(b[i]) { i += 1 }
    if i < end && b[i] >= 0x80 {
        while i < end { let n = wsLen(b, i, end); if n == 0 { break }; i += n }
    }
    return i - start
}

/// First offset at or past `strip` bytes of leading whitespace, never inside a
/// character (Rust `strip_boundary`).
func stripBoundary(_ b: UnsafeBufferPointer<UInt8>, _ start: Int, _ end: Int, _ strip: Int) -> Int {
    var off = start
    while off - start < strip && off < end {
        let n = wsLen(b, off, end)
        if n == 0 { break }
        off += n
    }
    return off
}

/// Byte length of the UTF-8 sequence led by `c` (1 for a stray byte).
@inline(__always)
func utf8Len(_ c: UInt8) -> Int {
    if c < 0x80 { return 1 }
    if c & 0xE0 == 0xC0 { return 2 }
    if c & 0xF0 == 0xE0 { return 3 }
    if c & 0xF8 == 0xF0 { return 4 }
    return 1
}

/// Longest prefix length of `count` bytes at `start` within `limit`, never
/// splitting a character.
func utf8PrefixLen(_ b: UnsafeBufferPointer<UInt8>, _ start: Int, _ count: Int, _ limit: Int) -> Int {
    if count <= limit { return count }
    var n = limit
    while n > 0 && (b[start + n] & 0xC0) == 0x80 { n -= 1 }
    return n
}

/// The same over a String's UTF-8.
func utf8Prefix(_ s: String, _ limit: Int) -> String {
    let u = Array(s.utf8)
    if u.count <= limit { return s }
    var n = limit
    while n > 0 && (u[n] & 0xC0) == 0x80 { n -= 1 }
    return String(decoding: u[0..<n], as: UTF8.self)
}

extension String {
    /// Rust `str::trim` (Unicode White_Space).
    var rustTrimmed: String {
        var s = self
        return s.withUTF8 { b -> String in
            let r = trimRange(b, 0, b.count)
            if r.lowerBound == 0 && r.upperBound == b.count { return self }
            return String(decoding: UnsafeBufferPointer(rebasing: b[r]), as: UTF8.self)
        }
    }

    /// Rust `str::trim_start`.
    var rustTrimmedStart: String {
        var s = self
        return s.withUTF8 { b -> String in
            var i = 0
            while i < b.count { let n = wsLen(b, i, b.count); if n == 0 { break }; i += n }
            return String(decoding: UnsafeBufferPointer(rebasing: b[i...]), as: UTF8.self)
        }
    }

    /// Byte-wise equality (Rust `==` on `str`).
    @inline(__always) func bytesEqual(_ other: String) -> Bool {
        if utf8.count != other.utf8.count { return false }
        if let r = utf8.withContiguousStorageIfAvailable({ a in
            other.utf8.withContiguousStorageIfAvailable { b in a.count == 0 || memcmp(a.baseAddress!, b.baseAddress!, a.count) == 0 }
        }), let eq = r {
            return eq
        }
        return utf8.elementsEqual(other.utf8)
    }

    /// Byte-wise order, which for UTF-8 is code-point order (Rust `Ord` on `str`).
    @inline(__always) func bytesLess(_ other: String) -> Bool {
        if let r = utf8.withContiguousStorageIfAvailable({ a in
            other.utf8.withContiguousStorageIfAvailable { b -> Bool in
                let n = Swift.min(a.count, b.count)
                let c = n == 0 ? 0 : memcmp(a.baseAddress!, b.baseAddress!, n)
                return c != 0 ? c < 0 : a.count < b.count
            }
        }), let less = r {
            return less
        }
        return utf8.lexicographicallyPrecedes(other.utf8)
    }

    @inline(__always) func bytesHasPrefix(_ p: String) -> Bool { utf8.starts(with: p.utf8) }

    /// Whether the bytes of `needle` occur in `self`.
    func bytesContains(_ needle: String) -> Bool { bytesFind(needle) != nil }

    /// Byte offset of the first occurrence of `needle`.
    func bytesFind(_ needle: String, from: Int = 0) -> Int? {
        let h = Array(utf8), n = Array(needle.utf8)
        return findBytes(h, n, from: from)
    }

    /// Rust `str::replace`, byte-exact.
    func bytesReplacing(_ from: String, with to: String) -> String {
        let h = Array(utf8), f = Array(from.utf8), t = Array(to.utf8)
        if f.isEmpty { return self }
        var out: [UInt8] = []
        out.reserveCapacity(h.count)
        var pos = 0
        while let hit = findBytes(h, f, from: pos) {
            out.append(contentsOf: h[pos..<hit])
            out.append(contentsOf: t)
            pos = hit + f.count
        }
        out.append(contentsOf: h[pos...])
        return String(decoding: out, as: UTF8.self)
    }
}

/// Rust `str::split(sep)` for an ASCII separator: always `count(sep) + 1` parts.
func splitBytes(_ s: String, _ sep: UInt8) -> [String] {
    var out: [String] = []
    var cur: [UInt8] = []
    for c in s.utf8 {
        if c == sep {
            out.append(String(decoding: cur, as: UTF8.self))
            cur.removeAll(keepingCapacity: true)
        } else {
            cur.append(c)
        }
    }
    out.append(String(decoding: cur, as: UTF8.self))
    return out
}

/// `s` split at the first `sep` byte: (before, after), or nil.
func splitOnce(_ s: String, _ sep: UInt8) -> (String, String)? {
    let u = Array(s.utf8)
    guard let i = u.firstIndex(of: sep) else { return nil }
    return (String(decoding: u[..<i], as: UTF8.self), String(decoding: u[(i + 1)...], as: UTF8.self))
}

/// The part after the last of `seps` (Rust `rsplit(...).next()`).
func afterLast(_ s: String, _ seps: [UInt8]) -> String {
    let u = Array(s.utf8)
    guard let i = u.lastIndex(where: { seps.contains($0) }) else { return s }
    return String(decoding: u[(i + 1)...], as: UTF8.self)
}

func findBytes(_ h: [UInt8], _ n: [UInt8], from: Int) -> Int? {
    if n.isEmpty { return from <= h.count ? from : nil }
    if n.count > h.count { return nil }
    var i = from
    let first = n[0]
    while i + n.count <= h.count {
        if h[i] == first {
            var k = 1
            while k < n.count && h[i + k] == n[k] { k += 1 }
            if k == n.count { return i }
        }
        i += 1
    }
    return nil
}

// MARK: - Unicode properties (Rust semantics)

/// Rust `char::is_alphanumeric`: Alphabetic, or a general category N*.
func isAlphanumeric(_ s: Unicode.Scalar) -> Bool {
    if s.value < 0x80 {
        let c = s.value
        return (c >= 0x30 && c <= 0x39) || (c >= 0x41 && c <= 0x5A) || (c >= 0x61 && c <= 0x7A)
    }
    let p = s.properties
    if p.isAlphabetic { return true }
    switch p.generalCategory {
    case .decimalNumber, .letterNumber, .otherNumber: return true
    default: return false
    }
}

/// Rust `str::to_lowercase`, with the Final_Sigma rule for U+03A3.
func rustLowercased(_ s: String) -> String {
    // ASCII fast path.
    if s.utf8.allSatisfy({ $0 < 0x80 }) {
        return String(decoding: s.utf8.map { $0 >= 0x41 && $0 <= 0x5A ? $0 + 32 : $0 }, as: UTF8.self)
    }
    let scalars = Array(s.unicodeScalars)
    var out = String.UnicodeScalarView()
    for (i, c) in scalars.enumerated() {
        if c.value < 0x80 {
            out.append(c.value >= 0x41 && c.value <= 0x5A ? Unicode.Scalar(c.value + 32)! : c)
        } else if c.value == 0x3A3 {
            out.append(isFinalSigma(scalars, i) ? "\u{3C2}" : "\u{3C3}")
        } else {
            out.append(contentsOf: c.properties.lowercaseMapping.unicodeScalars)
        }
    }
    return String(out)
}

private func isFinalSigma(_ s: [Unicode.Scalar], _ i: Int) -> Bool {
    // Preceded by a cased letter and not followed by one, skipping case-ignorables.
    var before = false
    var k = i - 1
    while k >= 0 {
        let p = s[k].properties
        if !p.isCaseIgnorable { before = p.isCased; break }
        k -= 1
    }
    if !before { return false }
    k = i + 1
    while k < s.count {
        let p = s[k].properties
        if !p.isCaseIgnorable { return !p.isCased }
        k += 1
    }
    return true
}

// MARK: - Key index

/// Hash index of positions over keys kept elsewhere: no copied keys, one
/// allocation per rehash. Open addressing, linear probing, load ≤ 1/2.
struct KeyIndex {
    private var slots: [UInt32] = []
    private var indexed = 0

    var isEmpty: Bool { slots.isEmpty }

    /// Position of `key` among the indexed entries, or nil.
    func find(_ key: String, in count: Int, keyAt: (Int) -> String) -> Int? {
        if slots.isEmpty { return nil }
        let mask = slots.count - 1
        var s = KeyIndex.hash(key) & mask
        while true {
            let v = slots[s]
            if v == 0 { return nil }
            if keyAt(Int(v) - 1).bytesEqual(key) { return Int(v) - 1 }
            s = (s + 1) & mask
        }
    }

    /// Index every entry past those already indexed (keys are distinct).
    mutating func sync(count n: Int, keyAt: (Int) -> String) {
        if n * 2 > slots.count {
            var cap = 32
            while cap < n * 2 { cap *= 2 }
            slots = [UInt32](repeating: 0, count: cap)
            indexed = 0
        }
        let mask = slots.count - 1
        while indexed < n {
            var s = KeyIndex.hash(keyAt(indexed)) & mask
            while slots[s] != 0 { s = (s + 1) & mask }
            slots[s] = UInt32(indexed + 1)
            indexed += 1
        }
    }

    // FNV-1a from a per-process random basis; nothing observable depends on it.
    private static let basis: UInt64 = UInt64.random(in: 1...UInt64.max)

    static func hash(_ s: String) -> Int {
        var h = basis
        for c in s.utf8 {
            h ^= UInt64(c)
            h = h &* 1099511628211
        }
        return Int(truncatingIfNeeded: h ^ (h >> 29)) & Int.max
    }
}
