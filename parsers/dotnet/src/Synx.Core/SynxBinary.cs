using System.Buffers.Binary;
using System.IO.Compression;
using System.Text;

namespace Synx;

/// <summary>
/// SYNX Binary (.synxb) compiler and decompiler — the <c>synx_core::binary</c> format:
/// header, string table, value tree, and (unless resolved) metadata and includes.
/// </summary>
internal static class SynxBinary
{
    private static readonly byte[] Magic = "SYNXB"u8.ToArray();
    private const byte FormatVersion = 1;

    /// <summary>safety: the decoded payload is bounded — a small deflate stream can expand into gigabytes.</summary>
    private const int MaxDecodedBytes = 256 * 1024 * 1024;
    /// <summary>Depth limit for the recursive value decoder (twice the parser's nesting cap).</summary>
    private const int MaxDecodeDepth = 256;
    /// <summary>What one decoded value costs against <see cref="MaxDecodedBytes"/>, text aside.</summary>
    private const int DecodedValueCost = 64;
    /// <summary>Elements reserved up front for a count read from the file.</summary>
    private const int MaxPreallocItems = 1024;

    /// <summary>
    /// What decoding has built so far, in bytes.
    /// safety: a value is one byte on the wire and tens of bytes in memory, a string is stored
    /// once and referenced any number of times, and deflate shrinks a run of either a
    /// thousandfold: unbudgeted, 171 bytes of file decoded to 11 GiB. The tree shares the
    /// payload's limit — every value at a fixed cost, plus the bytes of each key and string it
    /// refers to. Same accounting as synx-core, so both refuse the same files.
    /// </summary>
    private sealed class Budget
    {
        private long _spent;

        internal void Charge(long bytes)
        {
            _spent += bytes;
            if (_spent > MaxDecodedBytes)
                throw new InvalidDataException($"decoded document exceeds {MaxDecodedBytes} bytes — refusing to decode");
        }

        internal void Value() => Charge(DecodedValueCost);

        internal string Text(string s)
        {
            Charge(Encoding.UTF8.GetByteCount(s));
            return s;
        }
    }

    private const byte FlagActive   = 0b0000_0001;
    private const byte FlagLocked   = 0b0000_0010;
    private const byte FlagHasMeta  = 0b0000_0100;
    private const byte FlagResolved = 0b0000_1000;
    private const byte FlagTool     = 0b0001_0000;
    private const byte FlagSchema   = 0b0010_0000;
    private const byte FlagLlm      = 0b0100_0000;

    private const byte TagNull   = 0x00;
    private const byte TagFalse  = 0x01;
    private const byte TagTrue   = 0x02;
    private const byte TagInt    = 0x03;
    private const byte TagFloat  = 0x04;
    private const byte TagString = 0x05;
    private const byte TagArray  = 0x06;
    private const byte TagObject = 0x07;
    private const byte TagSecret = 0x08;

    // ── Public API ──

    internal static bool IsSynxb(ReadOnlySpan<byte> data)
        => data.Length >= 5 && data[..5].SequenceEqual(Magic);

    internal static byte[] Compile(SynxParseResult result, bool resolved)
    {
        var st = new StringTable();
        st.CollectValue(result.Root);
        var hasMeta = !resolved && result.Metadata.Count > 0;
        if (hasMeta)
        {
            st.CollectMetadata(result.Metadata);
            st.CollectIncludes(result.Includes);
        }
        st.OrderByUse();

        var payload = new Writer(1024);
        st.Encode(payload);
        EncodeValue(payload, result.Root, st);
        if (hasMeta)
        {
            EncodeMetadata(payload, result.Metadata, st);
            EncodeIncludes(payload, result.Includes, st);
        }

        var uncompressed = payload.AsSpan();
        var compressed = Deflate(uncompressed);

        var output = new byte[11 + compressed.Length];
        Magic.CopyTo(output, 0);
        output[5] = FormatVersion;
        byte flags = 0;
        if (result.Mode == SynxMode.Active) flags |= FlagActive;
        if (result.Locked) flags |= FlagLocked;
        if (hasMeta) flags |= FlagHasMeta;
        if (resolved) flags |= FlagResolved;
        if (result.Tool) flags |= FlagTool;
        if (result.Schema) flags |= FlagSchema;
        if (result.Llm) flags |= FlagLlm;
        output[6] = flags;
        BinaryPrimitives.WriteUInt32LittleEndian(output.AsSpan(7, 4), (uint)uncompressed.Length);
        compressed.CopyTo(output, 11);
        return output;
    }

    internal static SynxParseResult Decompile(ReadOnlySpan<byte> data)
    {
        if (data.Length < 11)
            throw new InvalidDataException("file too small for .synxb header");
        if (!data[..5].SequenceEqual(Magic))
            throw new InvalidDataException("invalid .synxb magic (expected SYNXB)");
        if (data[5] != FormatVersion)
            throw new InvalidDataException($"unsupported .synxb version: {data[5]} (expected {FormatVersion})");

        var flags = data[6];
        var uncompSize = BinaryPrimitives.ReadUInt32LittleEndian(data[7..11]);
        if (uncompSize > MaxDecodedBytes)
            throw new InvalidDataException($"declared size {uncompSize} exceeds limit {MaxDecodedBytes}");

        byte[] payload;
        try { payload = Inflate(data[11..].ToArray(), (int)uncompSize); }
        catch (InvalidDataException e) { throw new InvalidDataException($"decompression failed: {e.Message}"); }
        if (payload.Length != (int)uncompSize)
            throw new InvalidDataException($"size mismatch: expected {uncompSize}, got {payload.Length}");

        var pos = 0;
        var budget = new Budget();
        var st = StringTableReader.Decode(payload, ref pos);
        var root = DecodeValue(payload, ref pos, st, budget, 0);

        var result = new SynxParseResult
        {
            Root = root,
            Mode = (flags & FlagActive) != 0 ? SynxMode.Active : SynxMode.Static,
            Locked = (flags & FlagLocked) != 0,
            Tool = (flags & FlagTool) != 0,
            Schema = (flags & FlagSchema) != 0,
            Llm = (flags & FlagLlm) != 0,
        };
        if ((flags & FlagHasMeta) != 0)
        {
            result.Metadata = DecodeMetadata(payload, ref pos, st, budget);
            result.Includes = DecodeIncludes(payload, ref pos, st, budget);
        }
        return result;
    }

    // ── Compression ──

    private static byte[] Deflate(ReadOnlySpan<byte> data)
    {
        using var output = new MemoryStream();
        using (var deflate = new DeflateStream(output, CompressionLevel.SmallestSize, leaveOpen: true))
            deflate.Write(data);
        return output.ToArray();
    }

    /// <summary>
    /// Inflates at most <paramref name="expectedSize"/> + 1 bytes: one more than declared, to
    /// notice a stream that inflates past it.
    /// safety: the buffer was allocated from the declared size before a byte was inflated —
    /// 13 bytes of file declaring 256 MiB cost 256 MiB each (20 such files: 5 GiB). It now
    /// grows with what the stream actually produces.
    /// </summary>
    private static byte[] Inflate(byte[] compressed, int expectedSize)
    {
        using var input = new MemoryStream(compressed);
        using var deflate = new DeflateStream(input, CompressionMode.Decompress);
        var limit = (long)expectedSize + 1;
        using var output = new MemoryStream((int)Math.Min(limit, 64 * 1024));
        var chunk = new byte[64 * 1024];
        while (output.Length < limit)
        {
            var read = deflate.Read(chunk, 0, (int)Math.Min(chunk.Length, limit - output.Length));
            if (read == 0) break;
            output.Write(chunk, 0, read);
        }
        return output.ToArray();
    }

    // ── Byte writer ──

    private sealed class Writer(int capacity)
    {
        private byte[] _buf = new byte[capacity];
        private int _len;

        public ReadOnlySpan<byte> AsSpan() => _buf.AsSpan(0, _len);

        private void Ensure(int extra)
        {
            if (_len + extra <= _buf.Length) return;
            Array.Resize(ref _buf, Math.Max(_buf.Length * 2, _len + extra));
        }

        public void Byte(byte b)
        {
            Ensure(1);
            _buf[_len++] = b;
        }

        public void Varint(ulong value)
        {
            Ensure(10);
            do
            {
                var b = (byte)(value & 0x7F);
                value >>= 7;
                if (value != 0) b |= 0x80;
                _buf[_len++] = b;
            } while (value != 0);
        }

        public void F64(double d)
        {
            Ensure(8);
            BinaryPrimitives.WriteDoubleLittleEndian(_buf.AsSpan(_len, 8), d);
            _len += 8;
        }

        public void Utf8(string s)
        {
            var n = Encoding.UTF8.GetByteCount(s);
            Varint((ulong)n);
            Ensure(n);
            Encoding.UTF8.GetBytes(s, _buf.AsSpan(_len, n));
            _len += n;
        }
    }

    private static ulong ReadVarint(byte[] data, ref int pos)
    {
        ulong result = 0;
        var shift = 0;
        while (true)
        {
            if (pos >= data.Length) throw new InvalidDataException("unexpected end of data in varint");
            var b = data[pos++];
            result |= (ulong)(b & 0x7F) << shift;
            if ((b & 0x80) == 0) return result;
            shift += 7;
            if (shift >= 64) throw new InvalidDataException("varint overflow");
        }
    }

    /// <summary>A count from the file, bounded by the bytes left (every element costs at least one).</summary>
    private static int ReadCount(byte[] data, ref int pos)
    {
        var n = ReadVarint(data, ref pos);
        var remaining = (ulong)(data.Length - pos);
        if (n > remaining) throw new InvalidDataException("unexpected end of data");
        return (int)n;
    }

    private static double ReadF64(byte[] data, ref int pos, string what)
    {
        if (pos + 8 > data.Length) throw new InvalidDataException(what);
        var v = BinaryPrimitives.ReadDoubleLittleEndian(data.AsSpan(pos, 8));
        pos += 8;
        return v;
    }

    private static ulong ZigzagEncode(long n) => (ulong)((n << 1) ^ (n >> 63));
    private static long ZigzagDecode(ulong n) => (long)(n >> 1) ^ -(long)(n & 1);

    // ── String Table ──

    private sealed class StringTable
    {
        private readonly Dictionary<string, int> _index = new(StringComparer.Ordinal);
        private List<string> _strings = [];
        /// <summary>How many times each string is referenced, by first-seen index.</summary>
        private readonly List<int> _uses = [];

        private void Intern(string s)
        {
            if (_index.TryGetValue(s, out var idx))
            {
                _uses[idx]++;
                return;
            }
            _index[s] = _strings.Count;
            _strings.Add(s);
            _uses.Add(1);
        }

        /// <summary>
        /// Renumber by descending use count (ties keep first-seen order): frequent strings get
        /// one-byte indexes, and the same document always compiles to the same bytes.
        /// </summary>
        internal void OrderByUse()
        {
            var order = Enumerable.Range(0, _strings.Count).ToArray();
            Array.Sort(order, (a, b) =>
            {
                var c = _uses[b].CompareTo(_uses[a]);
                return c != 0 ? c : a.CompareTo(b);
            });
            var strings = new List<string>(_strings.Count);
            for (var newIdx = 0; newIdx < order.Length; newIdx++)
            {
                var s = _strings[order[newIdx]];
                _index[s] = newIdx;
                strings.Add(s);
            }
            _strings = strings;
        }

        internal void CollectValue(SynxValue val)
        {
            switch (val)
            {
                case SynxValue.Str s: Intern(s.Value); break;
                case SynxValue.Secret s: Intern(s.Value); break;
                case SynxValue.Arr a:
                    foreach (var item in a.Items) CollectValue(item);
                    break;
                case SynxValue.Obj o:
                    foreach (var (k, v) in o.Map) { Intern(k); CollectValue(v); }
                    break;
            }
        }

        internal void CollectMetadata(Dictionary<string, Dictionary<string, SynxMeta>> metadata)
        {
            foreach (var (path, metaMap) in metadata)
            {
                Intern(path);
                foreach (var (key, meta) in metaMap)
                {
                    Intern(key);
                    foreach (var m in meta.Markers) Intern(m);
                    foreach (var a in meta.Args) Intern(a);
                    if (meta.TypeHint is { } th) Intern(th);
                    if (meta.Constraints is { } c)
                    {
                        if (c.TypeName is { } tn) Intern(tn);
                        if (c.Pattern is { } pat) Intern(pat);
                        if (c.EnumValues is { } ev)
                            foreach (var v in ev) Intern(v);
                    }
                }
            }
        }

        internal void CollectIncludes(List<SynxIncludeDirective> includes)
        {
            foreach (var inc in includes)
            {
                Intern(inc.Path);
                Intern(inc.Alias);
            }
        }

        internal void Encode(Writer w)
        {
            w.Varint((ulong)_strings.Count);
            foreach (var s in _strings) w.Utf8(s);
        }

        internal ulong this[string s] => (ulong)_index[s];
    }

    private sealed class StringTableReader(string[] strings)
    {
        internal static StringTableReader Decode(byte[] data, ref int pos)
        {
            var count = ReadCount(data, ref pos);
            var strings = new List<string>(Math.Min(count, MaxPreallocItems));
            var utf8 = new UTF8Encoding(false, true);
            for (var i = 0; i < count; i++)
            {
                var len = ReadVarint(data, ref pos);
                // safety: `pos + len` wraps for a length near 2^64; the check passed and the
                // cast to int below threw ArgumentOutOfRangeException instead of the format error.
                if (len > (ulong)(data.Length - pos))
                    throw new InvalidDataException("unexpected end of data in string table");
                try { strings.Add(utf8.GetString(data, pos, (int)len)); }
                catch (DecoderFallbackException) { throw new InvalidDataException("invalid UTF-8 in string table"); }
                pos += (int)len;
            }
            return new StringTableReader(strings.ToArray());
        }

        internal string Get(ulong idx)
        {
            // An index is a u32 on the wire of synx-core; anything larger is not an index.
            if (idx > uint.MaxValue) throw new InvalidDataException("string index out of range");
            return idx < (ulong)strings.Length
                ? strings[idx]
                : throw new InvalidDataException($"string index {idx} out of bounds (size {strings.Length})");
        }
    }

    // ── Values ──

    private static void EncodeValue(Writer w, SynxValue val, StringTable st)
    {
        switch (val)
        {
            case SynxValue.Null:
                w.Byte(TagNull);
                break;
            case SynxValue.Bool b:
                w.Byte(b.Value ? TagTrue : TagFalse);
                break;
            case SynxValue.Int i:
                w.Byte(TagInt);
                w.Varint(ZigzagEncode(i.Value));
                break;
            case SynxValue.Float f:
                w.Byte(TagFloat);
                w.F64(f.Value);
                break;
            case SynxValue.Str str:
                w.Byte(TagString);
                w.Varint(st[str.Value]);
                break;
            case SynxValue.Secret sec:
                w.Byte(TagSecret);
                w.Varint(st[sec.Value]);
                break;
            case SynxValue.Arr a:
                w.Byte(TagArray);
                w.Varint((ulong)a.Items.Count);
                foreach (var item in a.Items) EncodeValue(w, item, st);
                break;
            case SynxValue.Obj o:
                w.Byte(TagObject);
                var keys = new string[o.Map.Count];
                o.Map.Keys.CopyTo(keys, 0);
                SynxSort.Keys(keys);
                w.Varint((ulong)keys.Length);
                foreach (var k in keys)
                {
                    w.Varint(st[k]);
                    EncodeValue(w, o.Map[k], st);
                }
                break;
        }
    }

    private static SynxValue DecodeValue(byte[] data, ref int pos, StringTableReader st, Budget budget, int depth)
    {
        if (depth > MaxDecodeDepth)
            throw new InvalidDataException($"nesting exceeds {MaxDecodeDepth} — refusing to decode");
        budget.Value();
        if (pos >= data.Length) throw new InvalidDataException("unexpected end of data");
        var tag = data[pos++];
        switch (tag)
        {
            case TagNull: return new SynxValue.Null();
            case TagFalse: return new SynxValue.Bool(false);
            case TagTrue: return new SynxValue.Bool(true);
            case TagInt: return new SynxValue.Int(ZigzagDecode(ReadVarint(data, ref pos)));
            case TagFloat: return new SynxValue.Float(ReadF64(data, ref pos, "unexpected end of data in float"));
            case TagString: return new SynxValue.Str(budget.Text(st.Get(ReadVarint(data, ref pos))));
            case TagSecret: return new SynxValue.Secret(budget.Text(st.Get(ReadVarint(data, ref pos))));
            case TagArray:
            {
                var count = ReadCount(data, ref pos);
                var items = new List<SynxValue>(Math.Min(count, MaxPreallocItems));
                for (var i = 0; i < count; i++)
                    items.Add(DecodeValue(data, ref pos, st, budget, depth + 1));
                return new SynxValue.Arr(items);
            }
            case TagObject:
            {
                var count = ReadCount(data, ref pos);
                var map = new Dictionary<string, SynxValue>(Math.Min(count, MaxPreallocItems), StringComparer.Ordinal);
                for (var i = 0; i < count; i++)
                {
                    var key = budget.Text(st.Get(ReadVarint(data, ref pos)));
                    map[key] = DecodeValue(data, ref pos, st, budget, depth + 1);
                }
                return new SynxValue.Obj(map);
            }
            default:
                throw new InvalidDataException($"unknown type tag: 0x{tag:x2}");
        }
    }

    // ── Metadata ──

    private static void EncodeConstraints(Writer w, SynxConstraints c, StringTable st)
    {
        byte bits = 0;
        if (c.Min.HasValue) bits |= 0x01;
        if (c.Max.HasValue) bits |= 0x02;
        if (c.TypeName != null) bits |= 0x04;
        if (c.Required) bits |= 0x08;
        if (c.Readonly) bits |= 0x10;
        if (c.Pattern != null) bits |= 0x20;
        if (c.EnumValues != null) bits |= 0x40;
        w.Byte(bits);
        if (c.Min is { } min) w.F64(min);
        if (c.Max is { } max) w.F64(max);
        if (c.TypeName is { } tn) w.Varint(st[tn]);
        if (c.Pattern is { } pat) w.Varint(st[pat]);
        if (c.EnumValues is { } ev)
        {
            w.Varint((ulong)ev.Count);
            foreach (var v in ev) w.Varint(st[v]);
        }
    }

    private static SynxConstraints DecodeConstraints(byte[] data, ref int pos, StringTableReader st, Budget budget)
    {
        if (pos >= data.Length) throw new InvalidDataException("unexpected end of data in constraints");
        var bits = data[pos++];
        var c = new SynxConstraints();
        if ((bits & 0x01) != 0) c.Min = ReadF64(data, ref pos, "truncated min");
        if ((bits & 0x02) != 0) c.Max = ReadF64(data, ref pos, "truncated max");
        if ((bits & 0x04) != 0) c.TypeName = budget.Text(st.Get(ReadVarint(data, ref pos)));
        if ((bits & 0x08) != 0) c.Required = true;
        if ((bits & 0x10) != 0) c.Readonly = true;
        if ((bits & 0x20) != 0) c.Pattern = budget.Text(st.Get(ReadVarint(data, ref pos)));
        if ((bits & 0x40) != 0)
        {
            var count = ReadCount(data, ref pos);
            var vals = new List<string>(Math.Min(count, MaxPreallocItems));
            for (var i = 0; i < count; i++)
            {
                budget.Value();
                vals.Add(budget.Text(st.Get(ReadVarint(data, ref pos))));
            }
            c.EnumValues = vals;
        }
        return c;
    }

    private static void EncodeMetadata(Writer w, Dictionary<string, Dictionary<string, SynxMeta>> metadata, StringTable st)
    {
        var paths = metadata.Keys.ToArray();
        SynxSort.Keys(paths);
        w.Varint((ulong)paths.Length);
        foreach (var path in paths)
        {
            w.Varint(st[path]);
            var metaMap = metadata[path];
            var keys = metaMap.Keys.ToArray();
            SynxSort.Keys(keys);
            w.Varint((ulong)keys.Length);
            foreach (var key in keys)
            {
                w.Varint(st[key]);
                var meta = metaMap[key];
                w.Varint((ulong)meta.Markers.Count);
                foreach (var m in meta.Markers) w.Varint(st[m]);
                w.Varint((ulong)meta.Args.Count);
                foreach (var a in meta.Args) w.Varint(st[a]);
                if (meta.TypeHint is { } th)
                {
                    w.Byte(1);
                    w.Varint(st[th]);
                }
                else w.Byte(0);
                if (meta.Constraints is { } c)
                {
                    w.Byte(1);
                    EncodeConstraints(w, c, st);
                }
                else w.Byte(0);
            }
        }
    }

    private static Dictionary<string, Dictionary<string, SynxMeta>> DecodeMetadata(byte[] data, ref int pos, StringTableReader st, Budget budget)
    {
        var outerCount = ReadCount(data, ref pos);
        var metadata = new Dictionary<string, Dictionary<string, SynxMeta>>(Math.Min(outerCount, MaxPreallocItems), StringComparer.Ordinal);
        for (var i = 0; i < outerCount; i++)
        {
            budget.Value();
            var path = budget.Text(st.Get(ReadVarint(data, ref pos)));
            var innerCount = ReadCount(data, ref pos);
            var metaMap = new Dictionary<string, SynxMeta>(Math.Min(innerCount, MaxPreallocItems), StringComparer.Ordinal);
            for (var j = 0; j < innerCount; j++)
            {
                var key = budget.Text(st.Get(ReadVarint(data, ref pos)));
                budget.Value();
                var meta = new SynxMeta();
                var mc = ReadCount(data, ref pos);
                for (var k = 0; k < mc; k++)
                {
                    budget.Value();
                    meta.Markers.Add(budget.Text(st.Get(ReadVarint(data, ref pos))));
                }
                var ac = ReadCount(data, ref pos);
                for (var k = 0; k < ac; k++)
                {
                    budget.Value();
                    meta.Args.Add(budget.Text(st.Get(ReadVarint(data, ref pos))));
                }
                if (pos >= data.Length) throw new InvalidDataException("unexpected end in meta");
                if (data[pos++] != 0) meta.TypeHint = budget.Text(st.Get(ReadVarint(data, ref pos)));
                if (pos >= data.Length) throw new InvalidDataException("unexpected end in meta");
                if (data[pos++] != 0) meta.Constraints = DecodeConstraints(data, ref pos, st, budget);
                metaMap[key] = meta;
            }
            metadata[path] = metaMap;
        }
        return metadata;
    }

    private static void EncodeIncludes(Writer w, List<SynxIncludeDirective> includes, StringTable st)
    {
        w.Varint((ulong)includes.Count);
        foreach (var inc in includes)
        {
            w.Varint(st[inc.Path]);
            w.Varint(st[inc.Alias]);
        }
    }

    private static List<SynxIncludeDirective> DecodeIncludes(byte[] data, ref int pos, StringTableReader st, Budget budget)
    {
        var count = ReadCount(data, ref pos);
        var includes = new List<SynxIncludeDirective>(Math.Min(count, MaxPreallocItems));
        for (var i = 0; i < count; i++)
        {
            budget.Value();
            var path = budget.Text(st.Get(ReadVarint(data, ref pos)));
            var alias = budget.Text(st.Get(ReadVarint(data, ref pos)));
            includes.Add(new SynxIncludeDirective { Path = path, Alias = alias });
        }
        return includes;
    }
}
