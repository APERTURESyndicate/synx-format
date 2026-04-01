using System.IO.Compression;
using System.Text;

namespace Synx;

/// <summary>
/// SYNX Binary (.synxb) compiler and decompiler.
/// Matches Rust <c>synx_core::binary</c> format byte-for-byte.
/// </summary>
internal static class SynxBinary
{
    private static readonly byte[] Magic = "SYNXB"u8.ToArray();
    private const byte FormatVersion = 1;

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
        st.Collect(result.Root);

        using var payload = new MemoryStream(1024);
        st.Encode(payload);
        EncodeValue(payload, result.Root, st);

        var uncompressed = payload.ToArray();
        var compressed = Deflate(uncompressed);

        using var outStream = new MemoryStream(11 + compressed.Length);
        outStream.Write(Magic);                      // 5 bytes
        outStream.WriteByte(FormatVersion);           // 1 byte
        byte flags = 0;
        if (result.Mode == SynxMode.Active) flags |= FlagActive;
        if (result.Locked) flags |= FlagLocked;
        if (resolved) flags |= FlagResolved;
        if (result.Tool) flags |= FlagTool;
        if (result.Schema) flags |= FlagSchema;
        if (result.Llm) flags |= FlagLlm;
        outStream.WriteByte(flags);                   // 1 byte
        outStream.Write(BitConverter.GetBytes((uint)uncompressed.Length)); // 4 bytes LE
        outStream.Write(compressed);

        return outStream.ToArray();
    }

    internal static SynxParseResult Decompile(ReadOnlySpan<byte> data)
    {
        if (data.Length < 11)
            throw new InvalidDataException("File too small for .synxb header.");
        if (!data[..5].SequenceEqual(Magic))
            throw new InvalidDataException("Invalid .synxb magic (expected SYNXB).");
        if (data[5] != FormatVersion)
            throw new InvalidDataException($"Unsupported .synxb version: {data[5]} (expected {FormatVersion}).");

        byte flags = data[6];
        uint uncompSize = BitConverter.ToUInt32(data[7..11]);

        var compressed = data[11..].ToArray();
        var payload = Inflate(compressed, (int)uncompSize);
        if (payload.Length != (int)uncompSize)
            throw new InvalidDataException($"Size mismatch: expected {uncompSize}, got {payload.Length}.");

        int pos = 0;
        var st = StringTableReader.Decode(payload, ref pos);
        var root = DecodeValue(payload, ref pos, st);

        return new SynxParseResult
        {
            Root = root,
            Mode = (flags & FlagActive) != 0 ? SynxMode.Active : SynxMode.Static,
            Locked = (flags & FlagLocked) != 0,
            Tool = (flags & FlagTool) != 0,
            Schema = (flags & FlagSchema) != 0,
            Llm = (flags & FlagLlm) != 0,
        };
    }

    // ── Compression ──

    private static byte[] Deflate(byte[] data)
    {
        using var output = new MemoryStream();
        using (var deflate = new DeflateStream(output, CompressionLevel.SmallestSize, leaveOpen: true))
            deflate.Write(data);
        return output.ToArray();
    }

    private static byte[] Inflate(byte[] compressed, int expectedSize)
    {
        using var input = new MemoryStream(compressed);
        using var deflate = new DeflateStream(input, CompressionMode.Decompress);
        var buffer = new byte[expectedSize];
        int totalRead = 0;
        while (totalRead < expectedSize)
        {
            int read = deflate.Read(buffer, totalRead, expectedSize - totalRead);
            if (read == 0) break;
            totalRead += read;
        }
        return buffer[..totalRead];
    }

    // ── Varint (LEB128) ──

    private static void WriteVarint(Stream s, ulong value)
    {
        do
        {
            byte b = (byte)(value & 0x7F);
            value >>= 7;
            if (value != 0) b |= 0x80;
            s.WriteByte(b);
        } while (value != 0);
    }

    private static ulong ReadVarint(byte[] data, ref int pos)
    {
        ulong result = 0;
        int shift = 0;
        while (pos < data.Length)
        {
            byte b = data[pos++];
            result |= (ulong)(b & 0x7F) << shift;
            if ((b & 0x80) == 0) break;
            shift += 7;
            if (shift > 63) throw new InvalidDataException("Varint overflow.");
        }
        return result;
    }

    // ── Zigzag encoding for signed integers ──

    private static ulong ZigzagEncode(long n) => (ulong)((n << 1) ^ (n >> 63));
    private static long ZigzagDecode(ulong n) => (long)(n >> 1) ^ -(long)(n & 1);

    // ── String Table ──

    private sealed class StringTable
    {
        private readonly Dictionary<string, int> _index = new(StringComparer.Ordinal);
        private readonly List<string> _strings = [];

        internal int Intern(string s)
        {
            if (_index.TryGetValue(s, out int idx)) return idx;
            idx = _strings.Count;
            _strings.Add(s);
            _index[s] = idx;
            return idx;
        }

        internal void Collect(SynxValue val)
        {
            switch (val)
            {
                case SynxValue.Str s: Intern(s.Value); break;
                case SynxValue.Secret s: Intern(s.Value); break;
                case SynxValue.Arr a:
                    foreach (var item in a.Items) Collect(item);
                    break;
                case SynxValue.Obj o:
                    foreach (var (k, v) in o.Map) { Intern(k); Collect(v); }
                    break;
            }
        }

        internal void Encode(Stream s)
        {
            WriteVarint(s, (ulong)_strings.Count);
            foreach (var str in _strings)
            {
                var bytes = Encoding.UTF8.GetBytes(str);
                WriteVarint(s, (ulong)bytes.Length);
                s.Write(bytes);
            }
        }

        internal int Get(string s) => _index[s];
    }

    private sealed class StringTableReader
    {
        private readonly string[] _strings;
        private StringTableReader(string[] strings) => _strings = strings;

        internal static StringTableReader Decode(byte[] data, ref int pos)
        {
            int count = (int)ReadVarint(data, ref pos);
            var strings = new string[count];
            for (int i = 0; i < count; i++)
            {
                int len = (int)ReadVarint(data, ref pos);
                strings[i] = Encoding.UTF8.GetString(data, pos, len);
                pos += len;
            }
            return new StringTableReader(strings);
        }

        internal string Get(int idx) => _strings[idx];
    }

    // ── Value encoding ──

    private static void EncodeValue(Stream s, SynxValue val, StringTable st)
    {
        switch (val)
        {
            case SynxValue.Null:
                s.WriteByte(TagNull);
                break;
            case SynxValue.Bool b:
                s.WriteByte(b.Value ? TagTrue : TagFalse);
                break;
            case SynxValue.Int i:
                s.WriteByte(TagInt);
                WriteVarint(s, ZigzagEncode(i.Value));
                break;
            case SynxValue.Float f:
                s.WriteByte(TagFloat);
                s.Write(BitConverter.GetBytes(f.Value));
                break;
            case SynxValue.Str str:
                s.WriteByte(TagString);
                WriteVarint(s, (ulong)st.Get(str.Value));
                break;
            case SynxValue.Secret sec:
                s.WriteByte(TagSecret);
                WriteVarint(s, (ulong)st.Get(sec.Value));
                break;
            case SynxValue.Arr a:
                s.WriteByte(TagArray);
                WriteVarint(s, (ulong)a.Items.Count);
                foreach (var item in a.Items) EncodeValue(s, item, st);
                break;
            case SynxValue.Obj o:
                s.WriteByte(TagObject);
                var sorted = o.Map.OrderBy(kv => kv.Key, StringComparer.Ordinal).ToList();
                WriteVarint(s, (ulong)sorted.Count);
                foreach (var (k, v) in sorted)
                {
                    WriteVarint(s, (ulong)st.Get(k));
                    EncodeValue(s, v, st);
                }
                break;
        }
    }

    // ── Value decoding ──

    private static SynxValue DecodeValue(byte[] data, ref int pos, StringTableReader st)
    {
        if (pos >= data.Length) throw new InvalidDataException("Unexpected end of .synxb payload.");
        byte tag = data[pos++];
        return tag switch
        {
            TagNull => new SynxValue.Null(),
            TagFalse => new SynxValue.Bool(false),
            TagTrue => new SynxValue.Bool(true),
            TagInt => new SynxValue.Int(ZigzagDecode(ReadVarint(data, ref pos))),
            TagFloat => DecodeFloat(data, ref pos),
            TagString => new SynxValue.Str(st.Get((int)ReadVarint(data, ref pos))),
            TagSecret => new SynxValue.Secret(st.Get((int)ReadVarint(data, ref pos))),
            TagArray => DecodeArray(data, ref pos, st),
            TagObject => DecodeObject(data, ref pos, st),
            _ => throw new InvalidDataException($"Unknown .synxb tag: 0x{tag:X2}")
        };
    }

    private static SynxValue.Float DecodeFloat(byte[] data, ref int pos)
    {
        double v = BitConverter.ToDouble(data, pos);
        pos += 8;
        return new SynxValue.Float(v);
    }

    private static SynxValue.Arr DecodeArray(byte[] data, ref int pos, StringTableReader st)
    {
        int count = (int)ReadVarint(data, ref pos);
        var items = new List<SynxValue>(count);
        for (int i = 0; i < count; i++)
            items.Add(DecodeValue(data, ref pos, st));
        return new SynxValue.Arr(items);
    }

    private static SynxValue.Obj DecodeObject(byte[] data, ref int pos, StringTableReader st)
    {
        int count = (int)ReadVarint(data, ref pos);
        var map = new Dictionary<string, SynxValue>(count, StringComparer.Ordinal);
        for (int i = 0; i < count; i++)
        {
            var key = st.Get((int)ReadVarint(data, ref pos));
            var val = DecodeValue(data, ref pos, st);
            map[key] = val;
        }
        return new SynxValue.Obj(map);
    }
}
