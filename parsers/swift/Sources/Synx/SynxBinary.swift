// .synxb compact binary format. Port of crates/synx-core/src/binary.rs.
//
// The payload (string table + tree + metadata) is byte-identical to synx-core's.
// Compression is raw DEFLATE: Apple targets use the `Compression` framework;
// elsewhere compile/decompile return `.failure(.unsupportedPlatform)`.
import Foundation
#if canImport(Compression)
import Compression
#endif

public enum SynxBinaryError: Error, Sendable {
    case unsupportedPlatform
    case ioError(String)
    case malformed(String)
}

public enum SynxBinary {

    fileprivate static let magic: [UInt8] = [0x53, 0x59, 0x4E, 0x58, 0x42] // "SYNXB"
    fileprivate static let version: UInt8 = 1
    /// safety: an untrusted header may declare up to 4 GiB; decoding is bounded.
    fileprivate static let maxDecodedBytes = 256 * 1024 * 1024
    /// A hostile file can nest arrays/objects arbitrarily; text caps at 128.
    fileprivate static let maxDecodeDepth = 256

    fileprivate static let flagActive: UInt8 = 0x01
    fileprivate static let flagLocked: UInt8 = 0x02
    fileprivate static let flagHasMeta: UInt8 = 0x04
    fileprivate static let flagResolved: UInt8 = 0x08
    fileprivate static let flagTool: UInt8 = 0x10
    fileprivate static let flagSchema: UInt8 = 0x20
    fileprivate static let flagLlm: UInt8 = 0x40

    fileprivate static let tagNull: UInt8 = 0x00
    fileprivate static let tagFalse: UInt8 = 0x01
    fileprivate static let tagTrue: UInt8 = 0x02
    fileprivate static let tagInt: UInt8 = 0x03
    fileprivate static let tagFloat: UInt8 = 0x04
    fileprivate static let tagString: UInt8 = 0x05
    fileprivate static let tagArray: UInt8 = 0x06
    fileprivate static let tagObject: UInt8 = 0x07
    fileprivate static let tagSecret: UInt8 = 0x08

    /// True if the bytes begin with `.synxb` magic.
    public static func isSynxb(_ data: Data) -> Bool {
        data.count >= 5 && Array(data.prefix(5)) == magic
    }

    /// Compile a ParseResult to `.synxb` bytes.
    public static func compile(_ result: SynxParseResult, resolved: Bool) -> Result<Data, SynxBinaryError> {
        let payload = encodePayload(result, resolved: resolved)
        switch rawDeflate(payload) {
        case .failure(let e): return .failure(e)
        case .success(let compressed):
            var out = magic
            out.append(version)
            out.append(flags(result, resolved: resolved))
            let size = UInt32(payload.count)
            for i in 0..<4 { out.append(UInt8(truncatingIfNeeded: size >> (8 * UInt32(i)))) }
            out.append(contentsOf: compressed)
            return .success(Data(out))
        }
    }

    /// Decompile `.synxb` bytes into a ParseResult.
    public static func decompile(_ data: Data) -> Result<SynxParseResult, SynxBinaryError> {
        let d = Array(data)
        guard d.count >= 11 else { return .failure(.malformed("file too small for .synxb header")) }
        guard Array(d[0..<5]) == magic else { return .failure(.malformed("invalid .synxb magic (expected SYNXB)")) }
        guard d[5] == version else {
            return .failure(.malformed("unsupported .synxb version: \(d[5]) (expected \(version))"))
        }
        let declared = Int(d[7]) | (Int(d[8]) << 8) | (Int(d[9]) << 16) | (Int(d[10]) << 24)
        if declared > maxDecodedBytes {
            return .failure(.malformed("declared size \(declared) exceeds limit \(maxDecodedBytes)"))
        }
        switch rawInflate(Array(d[11...]), expected: declared) {
        case .failure(let e): return .failure(e)
        case .success(let payload):
            if payload.count != declared {
                return .failure(.malformed("size mismatch: expected \(declared), got \(payload.count)"))
            }
            return decodePayload(flags: d[6], payload)
        }
    }

    // MARK: - payload (before / after DEFLATE)

    static func flags(_ r: SynxParseResult, resolved: Bool) -> UInt8 {
        var f: UInt8 = 0
        if r.mode == .active { f |= flagActive }
        if r.locked { f |= flagLocked }
        if !resolved && !r.metadata.isEmpty { f |= flagHasMeta }
        if resolved { f |= flagResolved }
        if r.tool { f |= flagTool }
        if r.schema { f |= flagSchema }
        if r.llm { f |= flagLlm }
        return f
    }

    static func encodePayload(_ result: SynxParseResult, resolved: Bool) -> [UInt8] {
        var table = StringTable()
        table.collect(result.root)
        let hasMeta = !resolved && !result.metadata.isEmpty
        if hasMeta {
            table.collect(result.metadata)
            for inc in result.includes {
                table.intern(inc.path)
                table.intern(inc.alias)
            }
        }
        table.orderByUse()
        var out: [UInt8] = []
        out.reserveCapacity(1024)
        table.encode(&out)
        encodeValue(result.root, table, &out)
        if hasMeta {
            encodeMetadata(result.metadata, table, &out)
            varint(UInt64(result.includes.count), &out)
            for inc in result.includes {
                varint(UInt64(table.index(inc.path)), &out)
                varint(UInt64(table.index(inc.alias)), &out)
            }
        }
        return out
    }

    static func decodePayload(flags: UInt8, _ payload: [UInt8]) -> Result<SynxParseResult, SynxBinaryError> {
        var r = Reader(payload)
        do {
            let table = try r.table()
            var pr = SynxParseResult(root: try r.value(table, 0))
            pr.mode = (flags & flagActive) != 0 ? .active : .static
            pr.locked = (flags & flagLocked) != 0
            pr.tool = (flags & flagTool) != 0
            pr.schema = (flags & flagSchema) != 0
            pr.llm = (flags & flagLlm) != 0
            if (flags & flagHasMeta) != 0 {
                pr.metadata = try r.metadata(table)
                pr.includes = try r.includes(table)
            }
            return .success(pr)
        } catch let e as SynxBinaryError {
            return .failure(e)
        } catch {
            return .failure(.malformed("\(error)"))
        }
    }
}

// MARK: - String table

private struct StringTable {
    var strings: [[UInt8]] = []
    var positions: [[UInt8]: UInt32] = [:]   // byte keys: no canonical equivalence
    var uses: [UInt32] = []

    mutating func intern(_ s: String) {
        let k = Array(s.utf8)
        if let i = positions[k] {
            uses[Int(i)] += 1
            return
        }
        positions[k] = UInt32(strings.count)
        strings.append(k)
        uses.append(1)
    }

    /// Renumber by descending use count, ties in first-seen order: frequent
    /// strings get one-byte indexes and the output never depends on hash order.
    mutating func orderByUse() {
        let order = Resolver.mergeSort(Array(0..<strings.count)) { uses[$0] > uses[$1] }
        var sorted: [[UInt8]] = []
        sorted.reserveCapacity(strings.count)
        for (n, old) in order.enumerated() {
            positions[strings[old]] = UInt32(n)
            sorted.append(strings[old])
        }
        strings = sorted
    }

    func index(_ s: String) -> UInt32 { positions[Array(s.utf8)] ?? 0 }

    mutating func collect(_ v: SynxValue) {
        switch v {
        case .string(let s), .secret(let s): intern(s)
        case .array(let a): for item in a { collect(item) }
        case .object(let o):
            for e in o.entries {
                intern(e.key)
                collect(e.value)
            }
        default: break
        }
    }

    mutating func collect(_ tree: SynxMetadataTree) {
        for (path, map) in tree {
            intern(path)
            for (key, meta) in map {
                intern(key)
                for m in meta.markers { intern(m) }
                for a in meta.args { intern(a) }
                if let th = meta.typeHint { intern(th) }
                if let c = meta.constraints {
                    if let tn = c.typeName { intern(tn) }
                    if let p = c.pattern { intern(p) }
                    for e in c.enumValues ?? [] { intern(e) }
                }
            }
        }
    }

    func encode(_ out: inout [UInt8]) {
        varint(UInt64(strings.count), &out)
        for s in strings {
            varint(UInt64(s.count), &out)
            out.append(contentsOf: s)
        }
    }
}

// MARK: - encoding

private func varint(_ value: UInt64, _ out: inout [UInt8]) {
    var v = value
    while true {
        let byte = UInt8(v & 0x7F)
        v >>= 7
        if v == 0 { out.append(byte); return }
        out.append(byte | 0x80)
    }
}

private func f64LE(_ f: Double, _ out: inout [UInt8]) {
    var v = f.bitPattern
    for _ in 0..<8 {
        out.append(UInt8(v & 0xFF))
        v >>= 8
    }
}

private func encodeValue(_ v: SynxValue, _ t: StringTable, _ out: inout [UInt8]) {
    switch v {
    case .null: out.append(SynxBinary.tagNull)
    case .bool(let b): out.append(b ? SynxBinary.tagTrue : SynxBinary.tagFalse)
    case .int(let n):
        out.append(SynxBinary.tagInt)
        varint(UInt64(bitPattern: (n << 1) ^ (n >> 63)), &out)
    case .float(let f):
        out.append(SynxBinary.tagFloat)
        f64LE(f, &out)
    case .string(let s):
        out.append(SynxBinary.tagString)
        varint(UInt64(t.index(s)), &out)
    case .secret(let s):
        out.append(SynxBinary.tagSecret)
        varint(UInt64(t.index(s)), &out)
    case .array(let a):
        out.append(SynxBinary.tagArray)
        varint(UInt64(a.count), &out)
        for item in a { encodeValue(item, t, &out) }
    case .object(let map):
        out.append(SynxBinary.tagObject)
        let entries = SynxStringify.sortedEntries(map)
        varint(UInt64(entries.count), &out)
        for e in entries {
            varint(UInt64(t.index(e.key)), &out)
            encodeValue(e.value, t, &out)
        }
    }
}

private func encodeConstraints(_ c: SynxConstraints, _ t: StringTable, _ out: inout [UInt8]) {
    var bits: UInt8 = 0
    if c.min != nil { bits |= 0x01 }
    if c.max != nil { bits |= 0x02 }
    if c.typeName != nil { bits |= 0x04 }
    if c.required { bits |= 0x08 }
    if c.readonly { bits |= 0x10 }
    if c.pattern != nil { bits |= 0x20 }
    if c.enumValues != nil { bits |= 0x40 }
    out.append(bits)
    if let m = c.min { f64LE(m, &out) }
    if let m = c.max { f64LE(m, &out) }
    if let tn = c.typeName { varint(UInt64(t.index(tn)), &out) }
    if let p = c.pattern { varint(UInt64(t.index(p)), &out) }
    if let ev = c.enumValues {
        varint(UInt64(ev.count), &out)
        for v in ev { varint(UInt64(t.index(v)), &out) }
    }
}

private func encodeMetadata(_ tree: SynxMetadataTree, _ t: StringTable, _ out: inout [UInt8]) {
    let outer = tree.entries.sorted { $0.key.bytesLess($1.key) }
    varint(UInt64(outer.count), &out)
    for (path, map) in outer {
        varint(UInt64(t.index(path)), &out)
        let inner = map.entries.sorted { $0.key.bytesLess($1.key) }
        varint(UInt64(inner.count), &out)
        for (key, meta) in inner {
            varint(UInt64(t.index(key)), &out)
            varint(UInt64(meta.markers.count), &out)
            for m in meta.markers { varint(UInt64(t.index(m)), &out) }
            varint(UInt64(meta.args.count), &out)
            for a in meta.args { varint(UInt64(t.index(a)), &out) }
            if let th = meta.typeHint {
                out.append(1)
                varint(UInt64(t.index(th)), &out)
            } else {
                out.append(0)
            }
            if let c = meta.constraints {
                out.append(1)
                encodeConstraints(c, t, &out)
            } else {
                out.append(0)
            }
        }
    }
}

// MARK: - decoding

private struct Reader {
    let d: [UInt8]
    var pos = 0
    init(_ d: [UInt8]) { self.d = d }

    /// A count read from the file never exceeds the bytes left; reserve no more.
    func capacity(_ n: UInt64) -> Int { Int(min(n, UInt64(d.count - pos))) }

    mutating func varint() throws -> UInt64 {
        var result: UInt64 = 0
        var shift: UInt64 = 0
        while true {
            guard pos < d.count else { throw SynxBinaryError.malformed("unexpected end of data in varint") }
            let byte = d[pos]
            pos += 1
            result |= UInt64(byte & 0x7F) << shift
            if byte & 0x80 == 0 { return result }
            shift += 7
            if shift >= 64 { throw SynxBinaryError.malformed("varint overflow") }
        }
    }

    mutating func f64(_ what: String) throws -> Double {
        guard d.count - pos >= 8 else { throw SynxBinaryError.malformed(what) }
        var bits: UInt64 = 0
        for i in 0..<8 { bits |= UInt64(d[pos + i]) << (8 * UInt64(i)) }
        pos += 8
        return Double(bitPattern: bits)
    }

    mutating func byte(_ what: String) throws -> UInt8 {
        guard pos < d.count else { throw SynxBinaryError.malformed(what) }
        pos += 1
        return d[pos - 1]
    }

    mutating func table() throws -> [String] {
        let count = try varint()
        var out: [String] = []
        out.reserveCapacity(capacity(count))
        for _ in 0..<count {
            let len = try varint()
            guard len <= UInt64(d.count - pos) else { throw SynxBinaryError.malformed("unexpected end of data in string table") }
            let bytes = d[pos..<(pos + Int(len))]
            guard let s = String(validatingUTF8Bytes: bytes) else { throw SynxBinaryError.malformed("invalid UTF-8 in string table") }
            out.append(s)
            pos += Int(len)
        }
        return out
    }

    mutating func string(_ t: [String]) throws -> String {
        let idx = try varint()
        guard idx < UInt64(t.count) else {
            throw SynxBinaryError.malformed("string index \(UInt32(truncatingIfNeeded: idx)) out of bounds (size \(t.count))")
        }
        return t[Int(idx)]
    }

    mutating func value(_ t: [String], _ depth: Int) throws -> SynxValue {
        if depth > SynxBinary.maxDecodeDepth {
            throw SynxBinaryError.malformed("nesting exceeds \(SynxBinary.maxDecodeDepth) \u{2014} refusing to decode")
        }
        let tag = try byte("unexpected end of data")
        switch tag {
        case SynxBinary.tagNull: return .null
        case SynxBinary.tagFalse: return .bool(false)
        case SynxBinary.tagTrue: return .bool(true)
        case SynxBinary.tagInt:
            let raw = try varint()
            return .int(Int64(bitPattern: raw >> 1) ^ -Int64(raw & 1))
        case SynxBinary.tagFloat: return .float(try f64("unexpected end of data in float"))
        case SynxBinary.tagString: return .string(try string(t))
        case SynxBinary.tagSecret: return .secret(try string(t))
        case SynxBinary.tagArray:
            let count = try varint()
            var arr: [SynxValue] = []
            arr.reserveCapacity(capacity(count))
            for _ in 0..<count { arr.append(try value(t, depth + 1)) }
            return .array(arr)
        case SynxBinary.tagObject:
            let count = try varint()
            // A repeated key keeps its first position and its last value.
            let obj = MObject()
            for _ in 0..<count {
                let key = try string(t)
                obj.set(key, MValue(try value(t, depth + 1)))
            }
            return .object(obj.toObject())
        default:
            let hex = String(tag, radix: 16)
            throw SynxBinaryError.malformed("unknown type tag: 0x\(hex.count < 2 ? "0" + hex : hex)")
        }
    }

    mutating func constraints(_ t: [String]) throws -> SynxConstraints {
        let bits = try byte("unexpected end of data in constraints")
        var c = SynxConstraints()
        if bits & 0x01 != 0 { c.min = try f64("truncated min") }
        if bits & 0x02 != 0 { c.max = try f64("truncated max") }
        if bits & 0x04 != 0 { c.typeName = try string(t) }
        if bits & 0x08 != 0 { c.required = true }
        if bits & 0x10 != 0 { c.readonly = true }
        if bits & 0x20 != 0 { c.pattern = try string(t) }
        if bits & 0x40 != 0 {
            let count = try varint()
            var vals: [String] = []
            vals.reserveCapacity(capacity(count))
            for _ in 0..<count { vals.append(try string(t)) }
            c.enumValues = vals
        }
        return c
    }

    mutating func strings(_ t: [String]) throws -> [String] {
        let count = try varint()
        var out: [String] = []
        out.reserveCapacity(capacity(count))
        for _ in 0..<count { out.append(try string(t)) }
        return out
    }

    mutating func metadata(_ t: [String]) throws -> SynxMetadataTree {
        var tree = SynxMetadataTree()
        let outer = try varint()
        for _ in 0..<outer {
            let path = try string(t)
            let inner = try varint()
            var map = SynxMetaMap()
            for _ in 0..<inner {
                let key = try string(t)
                var meta = SynxMeta()
                meta.markers = try strings(t)
                meta.args = try strings(t)
                if try byte("unexpected end in meta") != 0 { meta.typeHint = try string(t) }
                if try byte("unexpected end in meta") != 0 { meta.constraints = try constraints(t) }
                map[key] = meta
            }
            tree[path] = map
        }
        return tree
    }

    mutating func includes(_ t: [String]) throws -> [SynxIncludeDirective] {
        let count = try varint()
        var out: [SynxIncludeDirective] = []
        out.reserveCapacity(capacity(count))
        for _ in 0..<count {
            let path = try string(t)
            let alias = try string(t)
            out.append(SynxIncludeDirective(path: path, alias: alias))
        }
        return out
    }
}

private extension String {
    /// The bytes as a String if they are well-formed UTF-8.
    init?(validatingUTF8Bytes bytes: ArraySlice<UInt8>) {
        var it = bytes.makeIterator()
        var utf8 = UTF8()
        var scalars = String.UnicodeScalarView()
        while true {
            switch utf8.decode(&it) {
            case .scalarValue(let s): scalars.append(s)
            case .emptyInput: self = String(scalars); return
            case .error: return nil
            }
        }
    }
}

// MARK: - Compression (raw DEFLATE)

#if canImport(Compression)
private func rawDeflate(_ input: [UInt8]) -> Result<[UInt8], SynxBinaryError> {
    let dstSize = input.count + 1024
    let dst = UnsafeMutablePointer<UInt8>.allocate(capacity: dstSize)
    defer { dst.deallocate() }
    let written = input.withUnsafeBufferPointer { src -> Int in
        guard let base = src.baseAddress else { return 0 }
        return compression_encode_buffer(dst, dstSize, base, input.count, nil, COMPRESSION_ZLIB)
    }
    if written == 0 { return .failure(.ioError("compression_encode_buffer returned 0 (output too small or error)")) }
    return .success(Array(UnsafeBufferPointer(start: dst, count: written)))
}

private func rawInflate(_ input: [UInt8], expected: Int) -> Result<[UInt8], SynxBinaryError> {
    // One byte past the declared size, so a longer stream is caught as a mismatch.
    let dstSize = expected + 1
    let dst = UnsafeMutablePointer<UInt8>.allocate(capacity: dstSize)
    defer { dst.deallocate() }
    let written = input.withUnsafeBufferPointer { src -> Int in
        guard let base = src.baseAddress else { return 0 }
        return compression_decode_buffer(dst, dstSize, base, input.count, nil, COMPRESSION_ZLIB)
    }
    if written == 0 && expected != 0 { return .failure(.ioError("decompression failed")) }
    return .success(Array(UnsafeBufferPointer(start: dst, count: written)))
}
#else
private func rawDeflate(_ input: [UInt8]) -> Result<[UInt8], SynxBinaryError> { .failure(.unsupportedPlatform) }

private func rawInflate(_ input: [UInt8], expected: Int) -> Result<[UInt8], SynxBinaryError> { .failure(.unsupportedPlatform) }
#endif
