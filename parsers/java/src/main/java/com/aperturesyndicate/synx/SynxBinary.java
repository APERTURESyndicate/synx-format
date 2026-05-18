package com.aperturesyndicate.synx;

import java.io.ByteArrayOutputStream;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.zip.DataFormatException;
import java.util.zip.Deflater;
import java.util.zip.Inflater;

/**
 * {@code .synxb} compact binary format. Wire-compatible with
 * {@code crates/synx-core/binary.rs}.
 *
 * <p>Raw DEFLATE is produced via {@link Deflater} with {@code nowrap=true}
 * (no zlib wrapper) at compression level 9 to match Rust {@code miniz_oxide::deflate::compress_to_vec(_, 9)}.
 */
public final class SynxBinary {

    private SynxBinary() {}

    private static final byte[] MAGIC = { 'S', 'Y', 'N', 'X', 'B' };
    private static final byte VERSION = 1;

    private static final byte FLAG_ACTIVE   = 0x01;
    private static final byte FLAG_LOCKED   = 0x02;
    private static final byte FLAG_HAS_META = 0x04;
    private static final byte FLAG_RESOLVED = 0x08;
    private static final byte FLAG_TOOL     = 0x10;
    private static final byte FLAG_SCHEMA   = 0x20;
    private static final byte FLAG_LLM      = 0x40;

    private static final byte TAG_NULL   = 0x00;
    private static final byte TAG_FALSE  = 0x01;
    private static final byte TAG_TRUE   = 0x02;
    private static final byte TAG_INT    = 0x03;
    private static final byte TAG_FLOAT  = 0x04;
    private static final byte TAG_STRING = 0x05;
    private static final byte TAG_ARRAY  = 0x06;
    private static final byte TAG_OBJECT = 0x07;
    private static final byte TAG_SECRET = 0x08;

    public static boolean isSynxb(byte[] data) {
        if (data == null || data.length < 5) return false;
        for (int i = 0; i < 5; i++) if (data[i] != MAGIC[i]) return false;
        return true;
    }

    public static final class Outcome<T> {
        public final boolean ok;
        public final T value;
        public final String error;
        Outcome(boolean ok, T value, String error) {
            this.ok = ok; this.value = value; this.error = error;
        }
        public static <T> Outcome<T> success(T v) { return new Outcome<>(true, v, ""); }
        public static <T> Outcome<T> failure(String msg) { return new Outcome<>(false, null, msg); }
    }

    public static Outcome<byte[]> compile(SynxParseResult result, boolean resolved) {
        StringTable table = new StringTable();
        collectStrings(result.root, table);
        boolean hasMeta = !resolved && !result.metadata.isEmpty();
        if (hasMeta) {
            collectMetadataStrings(result.metadata, table);
            collectIncludeStrings(result.includes, table);
        }

        ByteArrayOutputStream payload = new ByteArrayOutputStream(1024);
        table.encode(payload);
        encodeValue(result.root, table, payload);
        if (hasMeta) {
            encodeMetadata(result.metadata, table, payload);
            encodeIncludes(result.includes, table, payload);
        }

        byte[] compressed = deflateRaw(payload.toByteArray());

        ByteArrayOutputStream out = new ByteArrayOutputStream(11 + compressed.length);
        out.write(MAGIC, 0, MAGIC.length);
        out.write(VERSION);
        int flags = 0;
        if (result.mode == SynxMode.ACTIVE) flags |= FLAG_ACTIVE;
        if (result.locked)                  flags |= FLAG_LOCKED;
        if (hasMeta)                         flags |= FLAG_HAS_META;
        if (resolved)                        flags |= FLAG_RESOLVED;
        if (result.tool)                     flags |= FLAG_TOOL;
        if (result.schema)                   flags |= FLAG_SCHEMA;
        if (result.llm)                      flags |= FLAG_LLM;
        out.write(flags);
        int uncomp = payload.size();
        out.write(uncomp & 0xFF);
        out.write((uncomp >> 8) & 0xFF);
        out.write((uncomp >> 16) & 0xFF);
        out.write((uncomp >> 24) & 0xFF);
        out.write(compressed, 0, compressed.length);
        return Outcome.success(out.toByteArray());
    }

    public static Outcome<SynxParseResult> decompile(byte[] data) {
        if (data == null || data.length < 11) return Outcome.failure("file too small for .synxb header");
        if (!isSynxb(data)) return Outcome.failure("invalid .synxb magic (expected SYNXB)");
        if (data[5] != VERSION) return Outcome.failure("unsupported .synxb version");
        int flags = data[6] & 0xFF;
        int uncomp = (data[7] & 0xFF)
                   | ((data[8] & 0xFF) << 8)
                   | ((data[9] & 0xFF) << 16)
                   | ((data[10] & 0xFF) << 24);
        byte[] payload;
        try {
            payload = inflateRaw(data, 11, data.length - 11, uncomp);
        } catch (DataFormatException e) {
            return Outcome.failure("decompression failed: " + e.getMessage());
        }
        if (payload.length != uncomp) {
            return Outcome.failure("size mismatch in decompressed payload");
        }
        try {
            Cursor cur = new Cursor(payload);
            StringTableReader reader = StringTableReader.decode(cur);
            SynxValue root = decodeValue(cur, reader);
            SynxParseResult pr = new SynxParseResult();
            pr.root = root;
            pr.mode = (flags & FLAG_ACTIVE) != 0 ? SynxMode.ACTIVE : SynxMode.STATIC;
            pr.locked = (flags & FLAG_LOCKED) != 0;
            pr.tool = (flags & FLAG_TOOL) != 0;
            pr.schema = (flags & FLAG_SCHEMA) != 0;
            pr.llm = (flags & FLAG_LLM) != 0;
            if ((flags & FLAG_HAS_META) != 0) {
                pr.metadata = decodeMetadata(cur, reader);
                pr.includes = decodeIncludes(cur, reader);
            }
            return Outcome.success(pr);
        } catch (BinaryException e) {
            return Outcome.failure(e.getMessage());
        }
    }

    // ─── DEFLATE helpers ─────────────────────────────────────────────────────
    private static byte[] deflateRaw(byte[] input) {
        Deflater deflater = new Deflater(9, true /* nowrap = raw deflate */);
        deflater.setInput(input);
        deflater.finish();
        ByteArrayOutputStream out = new ByteArrayOutputStream(input.length + 64);
        byte[] buf = new byte[4096];
        while (!deflater.finished()) {
            int n = deflater.deflate(buf);
            if (n > 0) out.write(buf, 0, n);
        }
        deflater.end();
        return out.toByteArray();
    }

    private static byte[] inflateRaw(byte[] data, int offset, int length, int expected)
            throws DataFormatException {
        Inflater inflater = new Inflater(true /* nowrap = raw deflate */);
        inflater.setInput(data, offset, length);
        ByteArrayOutputStream out = new ByteArrayOutputStream(Math.max(expected, 64));
        byte[] buf = new byte[4096];
        while (!inflater.finished()) {
            int n = inflater.inflate(buf);
            if (n == 0) {
                if (inflater.needsInput() || inflater.needsDictionary()) break;
            } else {
                out.write(buf, 0, n);
            }
        }
        inflater.end();
        return out.toByteArray();
    }

    // ─── Cursor ──────────────────────────────────────────────────────────────
    private static final class Cursor {
        final byte[] data;
        int pos;
        Cursor(byte[] data) { this.data = data; }
    }

    private static final class BinaryException extends RuntimeException {
        BinaryException(String msg) { super(msg); }
    }

    // ─── varint / zigzag ─────────────────────────────────────────────────────
    private static void encodeVarint(ByteArrayOutputStream out, long value) {
        long v = value;
        while (true) {
            byte b = (byte) (v & 0x7F);
            v >>>= 7;
            if (v == 0) { out.write(b & 0xFF); return; }
            out.write((b | 0x80) & 0xFF);
        }
    }

    private static long decodeVarint(Cursor cur) {
        long result = 0;
        int shift = 0;
        while (true) {
            if (cur.pos >= cur.data.length) throw new BinaryException("unexpected end of data in varint");
            int b = cur.data[cur.pos++] & 0xFF;
            result |= ((long) (b & 0x7F)) << shift;
            if ((b & 0x80) == 0) return result;
            shift += 7;
            if (shift >= 64) throw new BinaryException("varint overflow");
        }
    }

    private static long zigzagEncode(long n) {
        return (n << 1) ^ (n >> 63);
    }

    private static long zigzagDecode(long n) {
        return (n >>> 1) ^ -(n & 1);
    }

    private static void encodeF64LE(ByteArrayOutputStream out, double f) {
        long bits = Double.doubleToRawLongBits(f);
        for (int i = 0; i < 8; i++) {
            out.write((int) (bits & 0xFF));
            bits >>>= 8;
        }
    }

    private static double decodeF64LE(Cursor cur) {
        if (cur.pos + 8 > cur.data.length) throw new BinaryException("unexpected end of data in float");
        long bits = 0;
        for (int i = 0; i < 8; i++) {
            bits |= ((long) (cur.data[cur.pos + i] & 0xFF)) << (8 * i);
        }
        cur.pos += 8;
        return Double.longBitsToDouble(bits);
    }

    // ─── string table ────────────────────────────────────────────────────────
    private static final class StringTable {
        final List<String> strings = new ArrayList<>();
        final Map<String, Integer> index = new LinkedHashMap<>();

        int intern(String s) {
            Integer idx = index.get(s);
            if (idx != null) return idx;
            int newIdx = strings.size();
            strings.add(s);
            index.put(s, newIdx);
            return newIdx;
        }

        int indexOf(String s) {
            Integer idx = index.get(s);
            return idx == null ? 0 : idx;
        }

        void encode(ByteArrayOutputStream out) {
            encodeVarint(out, strings.size());
            for (String s : strings) {
                byte[] bytes = s.getBytes(java.nio.charset.StandardCharsets.UTF_8);
                encodeVarint(out, bytes.length);
                out.write(bytes, 0, bytes.length);
            }
        }
    }

    private static final class StringTableReader {
        final List<String> strings;
        StringTableReader(List<String> s) { this.strings = s; }

        static StringTableReader decode(Cursor cur) {
            long count = decodeVarint(cur);
            List<String> s = new ArrayList<>((int) count);
            for (long i = 0; i < count; i++) {
                long len = decodeVarint(cur);
                int n = (int) len;
                if (cur.pos + n > cur.data.length) {
                    throw new BinaryException("unexpected end of data in string table");
                }
                s.add(new String(cur.data, cur.pos, n, java.nio.charset.StandardCharsets.UTF_8));
                cur.pos += n;
            }
            return new StringTableReader(s);
        }

        String get(int idx) {
            if (idx < 0 || idx >= strings.size()) {
                throw new BinaryException("string index out of bounds");
            }
            return strings.get(idx);
        }
    }

    // ─── value encode / decode ───────────────────────────────────────────────
    private static void collectStrings(SynxValue v, StringTable t) {
        if (v instanceof SynxValue.Str s)        t.intern(s.value());
        else if (v instanceof SynxValue.Secret s) t.intern(s.value());
        else if (v instanceof SynxValue.Arr a) {
            for (SynxValue item : a.values()) collectStrings(item, t);
        } else if (v instanceof SynxValue.Obj o) {
            for (var e : o.map()) {
                t.intern(e.getKey());
                collectStrings(e.getValue(), t);
            }
        }
    }

    private static void collectMetadataStrings(Map<String, Map<String, SynxMeta>> tree, StringTable t) {
        for (var path : tree.entrySet()) {
            t.intern(path.getKey());
            for (var field : path.getValue().entrySet()) {
                t.intern(field.getKey());
                SynxMeta m = field.getValue();
                for (String mk : m.markers) t.intern(mk);
                for (String a : m.args) t.intern(a);
                if (m.typeHint != null) t.intern(m.typeHint);
                if (m.constraints != null) {
                    SynxConstraints c = m.constraints;
                    if (c.typeName != null) t.intern(c.typeName);
                    if (c.pattern != null)  t.intern(c.pattern);
                    if (c.enumValues != null) for (String e : c.enumValues) t.intern(e);
                }
            }
        }
    }

    private static void collectIncludeStrings(List<SynxIncludeDirective> incs, StringTable t) {
        for (var inc : incs) {
            t.intern(inc.path());
            t.intern(inc.alias());
        }
    }

    private static void encodeValue(SynxValue v, StringTable t, ByteArrayOutputStream out) {
        if (v instanceof SynxValue.Null) { out.write(TAG_NULL); }
        else if (v instanceof SynxValue.Bool b) { out.write(b.value() ? TAG_TRUE : TAG_FALSE); }
        else if (v instanceof SynxValue.Int i) {
            out.write(TAG_INT);
            encodeVarint(out, zigzagEncode(i.value()));
        }
        else if (v instanceof SynxValue.Float f) {
            out.write(TAG_FLOAT);
            encodeF64LE(out, f.value());
        }
        else if (v instanceof SynxValue.Str s) {
            out.write(TAG_STRING);
            encodeVarint(out, t.indexOf(s.value()));
        }
        else if (v instanceof SynxValue.Secret s) {
            out.write(TAG_SECRET);
            encodeVarint(out, t.indexOf(s.value()));
        }
        else if (v instanceof SynxValue.Arr a) {
            out.write(TAG_ARRAY);
            encodeVarint(out, a.values().size());
            for (SynxValue item : a.values()) encodeValue(item, t, out);
        }
        else if (v instanceof SynxValue.Obj o) {
            out.write(TAG_OBJECT);
            List<String> keys = new ArrayList<>(o.map().keys());
            Collections.sort(keys);
            encodeVarint(out, keys.size());
            for (String k : keys) {
                encodeVarint(out, t.indexOf(k));
                SynxValue cv = o.map().get(k);
                encodeValue(cv == null ? SynxValue.ofNull() : cv, t, out);
            }
        }
    }

    private static SynxValue decodeValue(Cursor cur, StringTableReader t) {
        if (cur.pos >= cur.data.length) throw new BinaryException("unexpected end of data");
        byte tag = cur.data[cur.pos++];
        switch (tag) {
            case TAG_NULL:   return SynxValue.ofNull();
            case TAG_FALSE:  return SynxValue.ofBool(false);
            case TAG_TRUE:   return SynxValue.ofBool(true);
            case TAG_INT:    return SynxValue.ofInt(zigzagDecode(decodeVarint(cur)));
            case TAG_FLOAT:  return SynxValue.ofFloat(decodeF64LE(cur));
            case TAG_STRING: return SynxValue.ofString(t.get((int) decodeVarint(cur)));
            case TAG_SECRET: return SynxValue.ofSecret(t.get((int) decodeVarint(cur)));
            case TAG_ARRAY: {
                long count = decodeVarint(cur);
                List<SynxValue> arr = new ArrayList<>((int) count);
                for (long i = 0; i < count; i++) arr.add(decodeValue(cur, t));
                return SynxValue.ofArray(arr);
            }
            case TAG_OBJECT: {
                long count = decodeVarint(cur);
                SynxObject obj = new SynxObject();
                for (long i = 0; i < count; i++) {
                    int keyIdx = (int) decodeVarint(cur);
                    String key = t.get(keyIdx);
                    obj.set(key, decodeValue(cur, t));
                }
                return SynxValue.ofObject(obj);
            }
            default:
                throw new BinaryException(String.format("unknown type tag 0x%02x", tag));
        }
    }

    // ─── metadata encode / decode ────────────────────────────────────────────
    private static void encodeConstraints(SynxConstraints c, StringTable t, ByteArrayOutputStream out) {
        int bits = 0;
        if (c.min != null)        bits |= 0x01;
        if (c.max != null)        bits |= 0x02;
        if (c.typeName != null)   bits |= 0x04;
        if (c.required)            bits |= 0x08;
        if (c.readonly)            bits |= 0x10;
        if (c.pattern != null)     bits |= 0x20;
        if (c.enumValues != null)  bits |= 0x40;
        out.write(bits);
        if (c.min != null) encodeF64LE(out, c.min);
        if (c.max != null) encodeF64LE(out, c.max);
        if (c.typeName != null) encodeVarint(out, t.indexOf(c.typeName));
        if (c.pattern != null) encodeVarint(out, t.indexOf(c.pattern));
        if (c.enumValues != null) {
            encodeVarint(out, c.enumValues.size());
            for (String v : c.enumValues) encodeVarint(out, t.indexOf(v));
        }
    }

    private static SynxConstraints decodeConstraints(Cursor cur, StringTableReader t) {
        if (cur.pos >= cur.data.length) throw new BinaryException("unexpected end in constraints");
        int bits = cur.data[cur.pos++] & 0xFF;
        SynxConstraints c = new SynxConstraints();
        if ((bits & 0x01) != 0) c.min = decodeF64LE(cur);
        if ((bits & 0x02) != 0) c.max = decodeF64LE(cur);
        if ((bits & 0x04) != 0) c.typeName = t.get((int) decodeVarint(cur));
        if ((bits & 0x08) != 0) c.required = true;
        if ((bits & 0x10) != 0) c.readonly = true;
        if ((bits & 0x20) != 0) c.pattern = t.get((int) decodeVarint(cur));
        if ((bits & 0x40) != 0) {
            long count = decodeVarint(cur);
            List<String> vals = new ArrayList<>((int) count);
            for (long i = 0; i < count; i++) vals.add(t.get((int) decodeVarint(cur)));
            c.enumValues = vals;
        }
        return c;
    }

    private static void encodeMetadata(Map<String, Map<String, SynxMeta>> tree, StringTable t,
                                        ByteArrayOutputStream out) {
        List<String> outerKeys = new ArrayList<>(tree.keySet());
        Collections.sort(outerKeys);
        encodeVarint(out, outerKeys.size());
        for (String path : outerKeys) {
            encodeVarint(out, t.indexOf(path));
            Map<String, SynxMeta> map = tree.get(path);
            List<String> innerKeys = new ArrayList<>(map.keySet());
            Collections.sort(innerKeys);
            encodeVarint(out, innerKeys.size());
            for (String fk : innerKeys) {
                SynxMeta m = map.get(fk);
                encodeVarint(out, t.indexOf(fk));
                encodeVarint(out, m.markers.size());
                for (String mk : m.markers) encodeVarint(out, t.indexOf(mk));
                encodeVarint(out, m.args.size());
                for (String a : m.args) encodeVarint(out, t.indexOf(a));
                if (m.typeHint != null) {
                    out.write(1);
                    encodeVarint(out, t.indexOf(m.typeHint));
                } else { out.write(0); }
                if (m.constraints != null) {
                    out.write(1);
                    encodeConstraints(m.constraints, t, out);
                } else { out.write(0); }
            }
        }
    }

    private static Map<String, Map<String, SynxMeta>> decodeMetadata(Cursor cur, StringTableReader t) {
        long outer = decodeVarint(cur);
        Map<String, Map<String, SynxMeta>> tree = new LinkedHashMap<>();
        for (long i = 0; i < outer; i++) {
            String path = t.get((int) decodeVarint(cur));
            long inner = decodeVarint(cur);
            Map<String, SynxMeta> map = new LinkedHashMap<>();
            for (long j = 0; j < inner; j++) {
                String fk = t.get((int) decodeVarint(cur));
                SynxMeta m = new SynxMeta();
                long mc = decodeVarint(cur);
                for (long k = 0; k < mc; k++) m.markers.add(t.get((int) decodeVarint(cur)));
                long ac = decodeVarint(cur);
                for (long k = 0; k < ac; k++) m.args.add(t.get((int) decodeVarint(cur)));
                if (cur.pos >= cur.data.length) throw new BinaryException("unexpected end in meta");
                int hasTh = cur.data[cur.pos++] & 0xFF;
                if (hasTh != 0) m.typeHint = t.get((int) decodeVarint(cur));
                if (cur.pos >= cur.data.length) throw new BinaryException("unexpected end in meta");
                int hasC = cur.data[cur.pos++] & 0xFF;
                if (hasC != 0) m.constraints = decodeConstraints(cur, t);
                map.put(fk, m);
            }
            tree.put(path, map);
        }
        return tree;
    }

    private static void encodeIncludes(List<SynxIncludeDirective> incs, StringTable t,
                                        ByteArrayOutputStream out) {
        encodeVarint(out, incs.size());
        for (var inc : incs) {
            encodeVarint(out, t.indexOf(inc.path()));
            encodeVarint(out, t.indexOf(inc.alias()));
        }
    }

    private static List<SynxIncludeDirective> decodeIncludes(Cursor cur, StringTableReader t) {
        long count = decodeVarint(cur);
        List<SynxIncludeDirective> out = new ArrayList<>((int) count);
        for (long i = 0; i < count; i++) {
            String path = t.get((int) decodeVarint(cur));
            String alias = t.get((int) decodeVarint(cur));
            out.add(new SynxIncludeDirective(path, alias));
        }
        return out;
    }
}
