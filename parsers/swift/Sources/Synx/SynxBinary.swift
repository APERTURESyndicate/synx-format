// .synxb compact binary format. Wire-compatible with crates/synx-core 3.6.x.
//
// Apple targets use the `Compression` framework's `COMPRESSION_ZLIB` (raw
// DEFLATE, RFC 1951), which matches Rust miniz_oxide bytes byte-for-byte.
// On Linux, where `Compression` is unavailable, compile/decompile return
// `.failure(.unsupportedPlatform)`. Add a zlib bridge for Linux to enable.
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

    // Flag bits
    fileprivate static let flagActive:  UInt8 = 0x01
    fileprivate static let flagLocked:  UInt8 = 0x02
    fileprivate static let flagHasMeta: UInt8 = 0x04
    fileprivate static let flagResolved: UInt8 = 0x08
    fileprivate static let flagTool:    UInt8 = 0x10
    fileprivate static let flagSchema:  UInt8 = 0x20
    fileprivate static let flagLlm:     UInt8 = 0x40

    // Type tags
    fileprivate static let tagNull:   UInt8 = 0x00
    fileprivate static let tagFalse:  UInt8 = 0x01
    fileprivate static let tagTrue:   UInt8 = 0x02
    fileprivate static let tagInt:    UInt8 = 0x03
    fileprivate static let tagFloat:  UInt8 = 0x04
    fileprivate static let tagString: UInt8 = 0x05
    fileprivate static let tagArray:  UInt8 = 0x06
    fileprivate static let tagObject: UInt8 = 0x07
    fileprivate static let tagSecret: UInt8 = 0x08

    /// True if the bytes begin with `.synxb` magic.
    public static func isSynxb(_ data: Data) -> Bool {
        guard data.count >= 5 else { return false }
        let prefix = Array(data.prefix(5))
        return prefix == magic
    }

    /// Compile a ParseResult to `.synxb` bytes.
    public static func compile(_ result: SynxParseResult, resolved: Bool) -> Result<Data, SynxBinaryError> {
        var table = StringTable()
        collectStrings(in: result.root, into: &table)
        let hasMeta = !resolved && !result.metadata.isEmpty
        if hasMeta {
            collectMetadataStrings(result.metadata, into: &table)
            collectIncludeStrings(result.includes, into: &table)
        }

        var payload: [UInt8] = []
        payload.reserveCapacity(1024)
        table.encode(into: &payload)
        encodeValue(result.root, table: table, into: &payload)
        if hasMeta {
            encodeMetadata(result.metadata, table: table, into: &payload)
            encodeIncludes(result.includes, table: table, into: &payload)
        }

        switch rawDeflate(payload) {
        case .failure(let e): return .failure(e)
        case .success(let compressed):
            var out: [UInt8] = []
            out.reserveCapacity(11 + compressed.count)
            out.append(contentsOf: magic)
            out.append(version)
            var flags: UInt8 = 0
            if result.mode == .active { flags |= flagActive }
            if result.locked           { flags |= flagLocked }
            if hasMeta                  { flags |= flagHasMeta }
            if resolved                 { flags |= flagResolved }
            if result.tool              { flags |= flagTool }
            if result.schema            { flags |= flagSchema }
            if result.llm               { flags |= flagLlm }
            out.append(flags)

            let uncomp = UInt32(payload.count)
            out.append(UInt8(truncatingIfNeeded: uncomp & 0xFF))
            out.append(UInt8(truncatingIfNeeded: (uncomp >> 8) & 0xFF))
            out.append(UInt8(truncatingIfNeeded: (uncomp >> 16) & 0xFF))
            out.append(UInt8(truncatingIfNeeded: (uncomp >> 24) & 0xFF))
            out.append(contentsOf: compressed)
            return .success(Data(out))
        }
    }

    /// Decompile `.synxb` bytes into a ParseResult.
    public static func decompile(_ data: Data) -> Result<SynxParseResult, SynxBinaryError> {
        guard data.count >= 11 else { return .failure(.malformed("file too small for .synxb header")) }
        guard isSynxb(data) else { return .failure(.malformed("invalid .synxb magic (expected SYNXB)")) }
        guard data[5] == version else { return .failure(.malformed("unsupported .synxb version")) }
        let flags = data[6]

        let uncomp: UInt32 =
            UInt32(data[7])
            | (UInt32(data[8]) << 8)
            | (UInt32(data[9]) << 16)
            | (UInt32(data[10]) << 24)

        let compressed = Array(data.suffix(from: 11))
        switch rawInflate(compressed, expected: Int(uncomp)) {
        case .failure(let e): return .failure(e)
        case .success(let payload):
            if payload.count != Int(uncomp) {
                return .failure(.malformed("size mismatch in decompressed payload"))
            }
            var pos = 0
            var reader: StringTableReader
            do {
                reader = try StringTableReader(from: payload, pos: &pos)
            } catch let e as SynxBinaryError {
                return .failure(e)
            } catch {
                return .failure(.malformed("\(error)"))
            }
            let root: SynxValue
            do {
                root = try decodeValue(payload, pos: &pos, table: reader)
            } catch let e as SynxBinaryError {
                return .failure(e)
            } catch {
                return .failure(.malformed("\(error)"))
            }
            var pr = SynxParseResult(root: root)
            pr.mode = (flags & flagActive) != 0 ? .active : .static
            pr.locked = (flags & flagLocked) != 0
            pr.tool = (flags & flagTool) != 0
            pr.schema = (flags & flagSchema) != 0
            pr.llm = (flags & flagLlm) != 0
            if (flags & flagHasMeta) != 0 {
                do {
                    pr.metadata = try decodeMetadata(payload, pos: &pos, table: reader)
                    pr.includes = try decodeIncludes(payload, pos: &pos, table: reader)
                } catch let e as SynxBinaryError {
                    return .failure(e)
                } catch {
                    return .failure(.malformed("\(error)"))
                }
            }
            return .success(pr)
        }
    }
}

// MARK: - String table

private struct StringTable {
    var strings: [String] = []
    var index: [String: UInt32] = [:]

    @discardableResult
    mutating func intern(_ s: String) -> UInt32 {
        if let idx = index[s] { return idx }
        let idx = UInt32(strings.count)
        strings.append(s)
        index[s] = idx
        return idx
    }

    func indexOf(_ s: String) -> UInt32 { index[s] ?? 0 }

    func encode(into out: inout [UInt8]) {
        encodeVarint(UInt64(strings.count), into: &out)
        for s in strings {
            let bytes = Array(s.utf8)
            encodeVarint(UInt64(bytes.count), into: &out)
            out.append(contentsOf: bytes)
        }
    }
}

private struct StringTableReader {
    let strings: [String]

    init(from data: [UInt8], pos: inout Int) throws {
        let count = try decodeVarint(data, pos: &pos)
        var strs: [String] = []
        strs.reserveCapacity(Int(count))
        for _ in 0..<count {
            let len = try decodeVarint(data, pos: &pos)
            let n = Int(len)
            if pos + n > data.count {
                throw SynxBinaryError.malformed("unexpected end of data in string table")
            }
            let slice = data[pos..<pos + n]
            strs.append(String(decoding: slice, as: UTF8.self))
            pos += n
        }
        self.strings = strs
    }

    func get(_ idx: UInt32) throws -> String {
        if Int(idx) >= strings.count {
            throw SynxBinaryError.malformed("string index out of bounds")
        }
        return strings[Int(idx)]
    }
}

// MARK: - Varint / zigzag

private func encodeVarint(_ value: UInt64, into out: inout [UInt8]) {
    var v = value
    while true {
        let byte = UInt8(v & 0x7F)
        v >>= 7
        if v == 0 { out.append(byte); return }
        out.append(byte | 0x80)
    }
}

private func decodeVarint(_ data: [UInt8], pos: inout Int) throws -> UInt64 {
    var result: UInt64 = 0
    var shift: UInt64 = 0
    while true {
        if pos >= data.count {
            throw SynxBinaryError.malformed("unexpected end of data in varint")
        }
        let byte = data[pos]
        pos += 1
        result |= UInt64(byte & 0x7F) << shift
        if (byte & 0x80) == 0 { return result }
        shift += 7
        if shift >= 64 {
            throw SynxBinaryError.malformed("varint overflow")
        }
    }
}

private func zigzagEncode(_ n: Int64) -> UInt64 {
    let shifted = (n << 1) ^ (n >> 63)
    return UInt64(bitPattern: shifted)
}

private func zigzagDecode(_ n: UInt64) -> Int64 {
    return Int64(bitPattern: n >> 1) ^ -Int64(n & 1)
}

private func encodeF64LE(_ f: Double, into out: inout [UInt8]) {
    var v = f.bitPattern
    for _ in 0..<8 {
        out.append(UInt8(v & 0xFF))
        v >>= 8
    }
}

private func decodeF64LE(_ data: [UInt8], pos: inout Int) throws -> Double {
    if pos + 8 > data.count {
        throw SynxBinaryError.malformed("unexpected end of data in float")
    }
    var bits: UInt64 = 0
    for i in 0..<8 {
        bits |= UInt64(data[pos + i]) << (8 * i)
    }
    pos += 8
    return Double(bitPattern: bits)
}

// MARK: - Value encode / decode

private func collectStrings(in value: SynxValue, into table: inout StringTable) {
    switch value {
    case .string(let s): _ = table.intern(s)
    case .secret(let s): _ = table.intern(s)
    case .array(let a):
        for item in a { collectStrings(in: item, into: &table) }
    case .object(let o):
        for e in o.entries {
            _ = table.intern(e.key)
            collectStrings(in: e.value, into: &table)
        }
    default: break
    }
}

private func collectMetadataStrings(_ tree: SynxMetadataTree, into table: inout StringTable) {
    for (path, map) in tree {
        _ = table.intern(path)
        for (key, meta) in map {
            _ = table.intern(key)
            for m in meta.markers { _ = table.intern(m) }
            for a in meta.args    { _ = table.intern(a) }
            if let th = meta.typeHint { _ = table.intern(th) }
            if let c = meta.constraints {
                if let tn = c.typeName { _ = table.intern(tn) }
                if let p  = c.pattern  { _ = table.intern(p) }
                if let ev = c.enumValues { for e in ev { _ = table.intern(e) } }
            }
        }
    }
}

private func collectIncludeStrings(_ includes: [SynxIncludeDirective], into table: inout StringTable) {
    for inc in includes {
        _ = table.intern(inc.path)
        _ = table.intern(inc.alias)
    }
}

private func encodeValue(_ v: SynxValue, table: StringTable, into out: inout [UInt8]) {
    switch v {
    case .null:  out.append(SynxBinary.tagNull)
    case .bool(let b):
        out.append(b ? SynxBinary.tagTrue : SynxBinary.tagFalse)
    case .int(let n):
        out.append(SynxBinary.tagInt)
        encodeVarint(zigzagEncode(n), into: &out)
    case .float(let f):
        out.append(SynxBinary.tagFloat)
        encodeF64LE(f, into: &out)
    case .string(let s):
        out.append(SynxBinary.tagString)
        encodeVarint(UInt64(table.indexOf(s)), into: &out)
    case .secret(let s):
        out.append(SynxBinary.tagSecret)
        encodeVarint(UInt64(table.indexOf(s)), into: &out)
    case .array(let a):
        out.append(SynxBinary.tagArray)
        encodeVarint(UInt64(a.count), into: &out)
        for item in a { encodeValue(item, table: table, into: &out) }
    case .object(let map):
        out.append(SynxBinary.tagObject)
        let sortedKeys = map.keys.sorted()
        encodeVarint(UInt64(sortedKeys.count), into: &out)
        for k in sortedKeys {
            encodeVarint(UInt64(table.indexOf(k)), into: &out)
            encodeValue(map[k] ?? .null, table: table, into: &out)
        }
    }
}

private func decodeValue(_ data: [UInt8], pos: inout Int,
                          table: StringTableReader) throws -> SynxValue {
    if pos >= data.count {
        throw SynxBinaryError.malformed("unexpected end of data")
    }
    let tag = data[pos]
    pos += 1
    switch tag {
    case SynxBinary.tagNull:  return .null
    case SynxBinary.tagFalse: return .bool(false)
    case SynxBinary.tagTrue:  return .bool(true)
    case SynxBinary.tagInt:
        let raw = try decodeVarint(data, pos: &pos)
        return .int(zigzagDecode(raw))
    case SynxBinary.tagFloat:
        return .float(try decodeF64LE(data, pos: &pos))
    case SynxBinary.tagString:
        let idx = UInt32(try decodeVarint(data, pos: &pos))
        return .string(try table.get(idx))
    case SynxBinary.tagSecret:
        let idx = UInt32(try decodeVarint(data, pos: &pos))
        return .secret(try table.get(idx))
    case SynxBinary.tagArray:
        let count = try decodeVarint(data, pos: &pos)
        var arr: [SynxValue] = []
        arr.reserveCapacity(Int(count))
        for _ in 0..<count {
            arr.append(try decodeValue(data, pos: &pos, table: table))
        }
        return .array(arr)
    case SynxBinary.tagObject:
        let count = try decodeVarint(data, pos: &pos)
        var obj = SynxObject()
        for _ in 0..<count {
            let keyIdx = UInt32(try decodeVarint(data, pos: &pos))
            let key = try table.get(keyIdx)
            let v = try decodeValue(data, pos: &pos, table: table)
            obj.set(key, to: v)
        }
        return .object(obj)
    default:
        throw SynxBinaryError.malformed("unknown type tag 0x\(String(tag, radix: 16))")
    }
}

// MARK: - Metadata encode / decode

private func encodeConstraints(_ c: SynxConstraints, table: StringTable, into out: inout [UInt8]) {
    var bits: UInt8 = 0
    if c.min != nil        { bits |= 0x01 }
    if c.max != nil        { bits |= 0x02 }
    if c.typeName != nil   { bits |= 0x04 }
    if c.required           { bits |= 0x08 }
    if c.readonly           { bits |= 0x10 }
    if c.pattern != nil     { bits |= 0x20 }
    if c.enumValues != nil  { bits |= 0x40 }
    out.append(bits)

    if let m = c.min { encodeF64LE(m, into: &out) }
    if let m = c.max { encodeF64LE(m, into: &out) }
    if let tn = c.typeName { encodeVarint(UInt64(table.indexOf(tn)), into: &out) }
    if let p  = c.pattern  { encodeVarint(UInt64(table.indexOf(p)),  into: &out) }
    if let ev = c.enumValues {
        encodeVarint(UInt64(ev.count), into: &out)
        for v in ev { encodeVarint(UInt64(table.indexOf(v)), into: &out) }
    }
}

private func decodeConstraints(_ data: [UInt8], pos: inout Int,
                                table: StringTableReader) throws -> SynxConstraints {
    if pos >= data.count {
        throw SynxBinaryError.malformed("unexpected end in constraints")
    }
    let bits = data[pos]; pos += 1
    var c = SynxConstraints()
    if (bits & 0x01) != 0 { c.min = try decodeF64LE(data, pos: &pos) }
    if (bits & 0x02) != 0 { c.max = try decodeF64LE(data, pos: &pos) }
    if (bits & 0x04) != 0 {
        let idx = UInt32(try decodeVarint(data, pos: &pos))
        c.typeName = try table.get(idx)
    }
    if (bits & 0x08) != 0 { c.required = true }
    if (bits & 0x10) != 0 { c.readonly = true }
    if (bits & 0x20) != 0 {
        let idx = UInt32(try decodeVarint(data, pos: &pos))
        c.pattern = try table.get(idx)
    }
    if (bits & 0x40) != 0 {
        let count = try decodeVarint(data, pos: &pos)
        var vals: [String] = []
        for _ in 0..<count {
            let idx = UInt32(try decodeVarint(data, pos: &pos))
            vals.append(try table.get(idx))
        }
        c.enumValues = vals
    }
    return c
}

private func encodeMetadata(_ tree: SynxMetadataTree, table: StringTable, into out: inout [UInt8]) {
    let outerKeys = tree.keys.sorted()
    encodeVarint(UInt64(outerKeys.count), into: &out)
    for path in outerKeys {
        encodeVarint(UInt64(table.indexOf(path)), into: &out)
        let map = tree[path]!
        let innerKeys = map.keys.sorted()
        encodeVarint(UInt64(innerKeys.count), into: &out)
        for fk in innerKeys {
            let meta = map[fk]!
            encodeVarint(UInt64(table.indexOf(fk)), into: &out)
            encodeVarint(UInt64(meta.markers.count), into: &out)
            for m in meta.markers { encodeVarint(UInt64(table.indexOf(m)), into: &out) }
            encodeVarint(UInt64(meta.args.count), into: &out)
            for a in meta.args    { encodeVarint(UInt64(table.indexOf(a)), into: &out) }
            if let th = meta.typeHint {
                out.append(1)
                encodeVarint(UInt64(table.indexOf(th)), into: &out)
            } else { out.append(0) }
            if let c = meta.constraints {
                out.append(1)
                encodeConstraints(c, table: table, into: &out)
            } else { out.append(0) }
        }
    }
}

private func decodeMetadata(_ data: [UInt8], pos: inout Int,
                             table: StringTableReader) throws -> SynxMetadataTree {
    let outer = try decodeVarint(data, pos: &pos)
    var tree: SynxMetadataTree = [:]
    for _ in 0..<outer {
        let pathIdx = UInt32(try decodeVarint(data, pos: &pos))
        let path = try table.get(pathIdx)
        let inner = try decodeVarint(data, pos: &pos)
        var map: SynxMetaMap = [:]
        for _ in 0..<inner {
            let fkIdx = UInt32(try decodeVarint(data, pos: &pos))
            let fk = try table.get(fkIdx)
            var meta = SynxMeta()
            let mc = try decodeVarint(data, pos: &pos)
            for _ in 0..<mc {
                let i = UInt32(try decodeVarint(data, pos: &pos))
                meta.markers.append(try table.get(i))
            }
            let ac = try decodeVarint(data, pos: &pos)
            for _ in 0..<ac {
                let i = UInt32(try decodeVarint(data, pos: &pos))
                meta.args.append(try table.get(i))
            }
            if pos >= data.count {
                throw SynxBinaryError.malformed("unexpected end in meta (type_hint flag)")
            }
            let hasTh = data[pos]; pos += 1
            if hasTh != 0 {
                let i = UInt32(try decodeVarint(data, pos: &pos))
                meta.typeHint = try table.get(i)
            }
            if pos >= data.count {
                throw SynxBinaryError.malformed("unexpected end in meta (constraints flag)")
            }
            let hasC = data[pos]; pos += 1
            if hasC != 0 {
                meta.constraints = try decodeConstraints(data, pos: &pos, table: table)
            }
            map[fk] = meta
        }
        tree[path] = map
    }
    return tree
}

private func encodeIncludes(_ incs: [SynxIncludeDirective], table: StringTable, into out: inout [UInt8]) {
    encodeVarint(UInt64(incs.count), into: &out)
    for inc in incs {
        encodeVarint(UInt64(table.indexOf(inc.path)),  into: &out)
        encodeVarint(UInt64(table.indexOf(inc.alias)), into: &out)
    }
}

private func decodeIncludes(_ data: [UInt8], pos: inout Int,
                             table: StringTableReader) throws -> [SynxIncludeDirective] {
    let count = try decodeVarint(data, pos: &pos)
    var out: [SynxIncludeDirective] = []
    out.reserveCapacity(Int(count))
    for _ in 0..<count {
        let p = UInt32(try decodeVarint(data, pos: &pos))
        let a = UInt32(try decodeVarint(data, pos: &pos))
        let path  = try table.get(p)
        let alias = try table.get(a)
        out.append(SynxIncludeDirective(path: path, alias: alias))
    }
    return out
}

// MARK: - Compression (raw DEFLATE)

#if canImport(Compression)
private func rawDeflate(_ input: [UInt8]) -> Result<[UInt8], SynxBinaryError> {
    let srcSize = input.count
    // Upper bound: source + 1 KiB headroom for incompressible inputs.
    let dstSize = srcSize + 1024
    let dst = UnsafeMutablePointer<UInt8>.allocate(capacity: dstSize)
    defer { dst.deallocate() }
    let written = input.withUnsafeBufferPointer { src -> Int in
        guard let base = src.baseAddress else { return 0 }
        return compression_encode_buffer(dst, dstSize, base, srcSize, nil, COMPRESSION_ZLIB)
    }
    if written == 0 {
        return .failure(.ioError("compression_encode_buffer returned 0 (output too small or error)"))
    }
    return .success(Array(UnsafeBufferPointer(start: dst, count: written)))
}

private func rawInflate(_ input: [UInt8], expected: Int) -> Result<[UInt8], SynxBinaryError> {
    let dstSize = max(expected, 1)
    let dst = UnsafeMutablePointer<UInt8>.allocate(capacity: dstSize)
    defer { dst.deallocate() }
    let written = input.withUnsafeBufferPointer { src -> Int in
        guard let base = src.baseAddress else { return 0 }
        return compression_decode_buffer(dst, dstSize, base, input.count, nil, COMPRESSION_ZLIB)
    }
    if written == 0 {
        return .failure(.ioError("decompression failed"))
    }
    return .success(Array(UnsafeBufferPointer(start: dst, count: written)))
}
#else
private func rawDeflate(_ input: [UInt8]) -> Result<[UInt8], SynxBinaryError> {
    return .failure(.unsupportedPlatform)
}

private func rawInflate(_ input: [UInt8], expected: Int) -> Result<[UInt8], SynxBinaryError> {
    return .failure(.unsupportedPlatform)
}
#endif
