package com.aperturesyndicate.synx;

import java.io.ByteArrayOutputStream;
import java.nio.ByteBuffer;
import java.nio.charset.CharacterCodingException;
import java.nio.charset.CodingErrorAction;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.zip.DataFormatException;
import java.util.zip.Deflater;
import java.util.zip.Inflater;

/**
 * {@code .synxb} compact binary format, wire-compatible with {@code crates/synx-core/src/binary.rs}.
 *
 * <p>The string table is ordered by use count (ties keep first-seen order), as in synx-core, so
 * frequent strings get one-byte indexes and one document always compiles to the same payload.
 * Raw DEFLATE comes from {@link Deflater} with {@code nowrap=true} at level 9; its compressed
 * bytes differ from {@code miniz_oxide}'s, the decoded payload does not.
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

    /** Upper bound on the inflated payload of an untrusted file. */
    private static final int MAX_DECODED_BYTES = 256 * 1024 * 1024;
    /** Upper bound on ARRAY/OBJECT nesting while decoding. */
    private static final int MAX_DECODE_DEPTH = 256;
    /** What one decoded value costs against {@link #MAX_DECODED_BYTES}, text aside. */
    private static final int DECODED_VALUE_COST = 64;
    /** Elements reserved up front for a count read from the file. */
    private static final int MAX_PREALLOC_ITEMS = 1024;

    /**
     * What decoding has built so far, in bytes.
     *
     * <p>safety: a value is one byte on the wire and tens of bytes in memory, a string is
     * stored once and referenced any number of times, and deflate shrinks a run of either a
     * thousandfold: unbudgeted, a file of a few kilobytes decoded to gigabytes (and to as
     * much text once written out). The tree shares the payload's limit — every value at a
     * fixed cost, plus the bytes of each key and string it refers to. Same accounting as
     * synx-core, so both refuse the same files.
     */
    private static final class Budget {
        long spent;

        void charge(long bytes) {
            spent += bytes;
            if (spent > MAX_DECODED_BYTES) {
                throw new BinaryException("decoded document exceeds " + MAX_DECODED_BYTES + " bytes — refusing to decode");
            }
        }

        void value() { charge(DECODED_VALUE_COST); }

        String text(String s) {
            charge(SynxParser.utf8Len(s, 0, s.length()));
            return s;
        }
    }

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
        boolean hasMeta = !resolved && result.metadata != null && !result.metadata.isEmpty();
        if (hasMeta) {
            collectMetadataStrings(result.metadata, table);
            collectIncludeStrings(result.includes, table);
        }
        table.orderByUse();

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
        if (hasMeta)                        flags |= FLAG_HAS_META;
        if (resolved)                       flags |= FLAG_RESOLVED;
        if (result.tool)                    flags |= FLAG_TOOL;
        if (result.schema)                  flags |= FLAG_SCHEMA;
        if (result.llm)                     flags |= FLAG_LLM;
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
        if (data[5] != VERSION) {
            return Outcome.failure("unsupported .synxb version: " + (data[5] & 0xFF) + " (expected " + VERSION + ")");
        }
        int flags = data[6] & 0xFF;
        long uncomp = (data[7] & 0xFFL)
                    | ((data[8] & 0xFFL) << 8)
                    | ((data[9] & 0xFFL) << 16)
                    | ((data[10] & 0xFFL) << 24);
        if (uncomp > MAX_DECODED_BYTES) {
            return Outcome.failure("declared size " + uncomp + " exceeds limit " + MAX_DECODED_BYTES);
        }
        byte[] payload;
        try {
            payload = inflateRaw(data, 11, data.length - 11, (int) uncomp);
        } catch (DataFormatException e) {
            return Outcome.failure("decompression failed: " + e.getMessage());
        }
        if (payload.length != uncomp) {
            return Outcome.failure("size mismatch: expected " + uncomp + ", got " + payload.length);
        }
        try {
            Cursor cur = new Cursor(payload);
            Budget budget = new Budget();
            StringTableReader reader = StringTableReader.decode(cur);
            SynxValue root = decodeValue(cur, reader, budget, 0);
            SynxParseResult pr = new SynxParseResult();
            pr.root = root;
            pr.mode = (flags & FLAG_ACTIVE) != 0 ? SynxMode.ACTIVE : SynxMode.STATIC;
            pr.locked = (flags & FLAG_LOCKED) != 0;
            pr.tool = (flags & FLAG_TOOL) != 0;
            pr.schema = (flags & FLAG_SCHEMA) != 0;
            pr.llm = (flags & FLAG_LLM) != 0;
            if ((flags & FLAG_HAS_META) != 0) {
                pr.metadata = decodeMetadata(cur, reader, budget);
                pr.includes = decodeIncludes(cur, reader, budget);
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
        ByteArrayOutputStream out = new ByteArrayOutputStream(input.length / 2 + 64);
        byte[] buf = new byte[16384];
        while (!deflater.finished()) {
            int n = deflater.deflate(buf);
            if (n > 0) out.write(buf, 0, n);
        }
        deflater.end();
        return out.toByteArray();
    }

    /**
     * Inflates at most {@code expected + 1} bytes: the stream may produce the declared size and
     * not a byte more.
     *
     * <p>safety: bounded only by the 256 MiB cap, a 260 KB file declaring 1 KiB still inflated
     * to the full cap before the size check rejected it; and the buffer was reserved from the
     * declared size (up to 64 MiB) before a byte was inflated.
     */
    private static byte[] inflateRaw(byte[] data, int offset, int length, int expected)
            throws DataFormatException {
        Inflater inflater = new Inflater(true /* nowrap = raw deflate */);
        inflater.setInput(data, offset, length);
        ByteArrayOutputStream out = new ByteArrayOutputStream(Math.max(Math.min(expected, 1 << 20), 64));
        byte[] buf = new byte[16384];
        try {
            while (!inflater.finished() && out.size() <= expected) {
                int n = inflater.inflate(buf);
                if (n == 0) {
                    if (inflater.needsInput() || inflater.needsDictionary()) break;
                } else {
                    out.write(buf, 0, n);
                }
            }
        } finally {
            inflater.end();
        }
        return out.toByteArray();
    }

    // ─── Cursor ──────────────────────────────────────────────────────────────

    private static final class Cursor {
        final byte[] data;
        int pos;
        Cursor(byte[] data) { this.data = data; }
        int remaining() { return data.length - pos; }
    }

    private static final class BinaryException extends RuntimeException {
        BinaryException(String msg) { super(msg, null, false, false); }
    }

    /**
     * Bounds a capacity read from the file. Every element costs at least one byte on the wire,
     * but several in memory, so the reservation is a small hint and the list grows as elements
     * actually arrive.
     */
    private static int safeCapacity(long count, Cursor cur) {
        return (int) Math.min(Math.min(Math.max(count, 0), cur.remaining()), MAX_PREALLOC_ITEMS);
    }

    /**
     * Whether {@code i} is below a count read as an unsigned varint.
     *
     * <p>safety: a count of 2^63 or more is a negative {@code long}; compared signed, the loop
     * did not run and the file decoded as an empty list where synx-core refuses it.
     */
    private static boolean below(long i, long count) {
        return Long.compareUnsigned(i, count) < 0;
    }

    // ─── varint / zigzag ─────────────────────────────────────────────────────

    private static void encodeVarint(ByteArrayOutputStream out, long value) {
        long v = value;
        while (true) {
            int b = (int) (v & 0x7F);
            v >>>= 7;
            if (v == 0) { out.write(b); return; }
            out.write(b | 0x80);
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

    private static long zigzagEncode(long n) { return (n << 1) ^ (n >> 63); }

    private static long zigzagDecode(long n) { return (n >>> 1) ^ -(n & 1); }

    private static void encodeF64LE(ByteArrayOutputStream out, double f) {
        long bits = Double.doubleToRawLongBits(f);
        for (int i = 0; i < 8; i++) {
            out.write((int) (bits & 0xFF));
            bits >>>= 8;
        }
    }

    private static double decodeF64LE(Cursor cur, String what) {
        if (cur.pos + 8 > cur.data.length) throw new BinaryException(what);
        long bits = 0;
        for (int i = 0; i < 8; i++) bits |= ((long) (cur.data[cur.pos + i] & 0xFF)) << (8 * i);
        cur.pos += 8;
        return Double.longBitsToDouble(bits);
    }

    // ─── string table ────────────────────────────────────────────────────────

    private static final class StringTable {
        List<String> strings = new ArrayList<>();
        final Map<String, Integer> index = new HashMap<>();
        final List<int[]> uses = new ArrayList<>();

        void intern(String s) {
            Integer idx = index.get(s);
            if (idx != null) {
                uses.get(idx)[0]++;
                return;
            }
            index.put(s, strings.size());
            strings.add(s);
            uses.add(new int[] { 1 });
        }

        /** Renumbers by descending use count; ties keep first-seen order (a stable sort). */
        void orderByUse() {
            List<Integer> order = new ArrayList<>(strings.size());
            for (int i = 0; i < strings.size(); i++) order.add(i);
            order.sort((a, b) -> Integer.compare(uses.get(b)[0], uses.get(a)[0]));
            List<String> ordered = new ArrayList<>(strings.size());
            for (int newIdx = 0; newIdx < order.size(); newIdx++) {
                String s = strings.get(order.get(newIdx));
                index.put(s, newIdx);
                ordered.add(s);
            }
            strings = ordered;
        }

        int indexOf(String s) {
            Integer idx = index.get(s);
            return idx == null ? 0 : idx;
        }

        void encode(ByteArrayOutputStream out) {
            encodeVarint(out, strings.size());
            for (String s : strings) {
                byte[] bytes = s.getBytes(StandardCharsets.UTF_8);
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
            List<String> s = new ArrayList<>(safeCapacity(count, cur));
            var decoder = StandardCharsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT)
                .onUnmappableCharacter(CodingErrorAction.REPORT);
            for (long i = 0; below(i, count); i++) {
                long len = decodeVarint(cur);
                if (len < 0 || len > cur.remaining()) throw new BinaryException("unexpected end of data in string table");
                int n = (int) len;
                try {
                    s.add(decoder.decode(ByteBuffer.wrap(cur.data, cur.pos, n)).toString());
                } catch (CharacterCodingException e) {
                    throw new BinaryException("invalid UTF-8 in string table");
                }
                cur.pos += n;
            }
            return new StringTableReader(s);
        }

        String get(long idx) {
            // An index is a u32 on the wire of synx-core; anything larger is not an index.
            if (idx < 0 || idx > 0xFFFF_FFFFL) throw new BinaryException("string index out of range");
            if (idx >= strings.size()) {
                throw new BinaryException("string index " + idx + " out of bounds (size " + strings.size() + ")");
            }
            return strings.get((int) idx);
        }
    }

    // ─── value encode / decode ───────────────────────────────────────────────

    private static void collectStrings(SynxValue v, StringTable t) {
        if (v instanceof SynxValue.Str s) t.intern(s.value());
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
        if (incs == null) return;
        for (var inc : incs) {
            t.intern(inc.path());
            t.intern(inc.alias());
        }
    }

    private static void encodeValue(SynxValue v, StringTable t, ByteArrayOutputStream out) {
        if (v instanceof SynxValue.Null) {
            out.write(TAG_NULL);
        } else if (v instanceof SynxValue.Bool b) {
            out.write(b.value() ? TAG_TRUE : TAG_FALSE);
        } else if (v instanceof SynxValue.Int i) {
            out.write(TAG_INT);
            encodeVarint(out, zigzagEncode(i.value()));
        } else if (v instanceof SynxValue.Float f) {
            out.write(TAG_FLOAT);
            encodeF64LE(out, f.value());
        } else if (v instanceof SynxValue.Str s) {
            out.write(TAG_STRING);
            encodeVarint(out, t.indexOf(s.value()));
        } else if (v instanceof SynxValue.Secret s) {
            out.write(TAG_SECRET);
            encodeVarint(out, t.indexOf(s.value()));
        } else if (v instanceof SynxValue.Arr a) {
            out.write(TAG_ARRAY);
            encodeVarint(out, a.values().size());
            for (SynxValue item : a.values()) encodeValue(item, t, out);
        } else if (v instanceof SynxValue.Obj o) {
            out.write(TAG_OBJECT);
            var entries = SynxStringify.sortedEntries(o.map());
            encodeVarint(out, entries.size());
            for (var e : entries) {
                encodeVarint(out, t.indexOf(e.getKey()));
                encodeValue(e.getValue() == null ? SynxValue.ofNull() : e.getValue(), t, out);
            }
        } else {
            out.write(TAG_NULL);
        }
    }

    private static SynxValue decodeValue(Cursor cur, StringTableReader t, Budget budget, int depth) {
        if (depth > MAX_DECODE_DEPTH) throw new BinaryException("nesting exceeds " + MAX_DECODE_DEPTH + " — refusing to decode");
        budget.value();
        if (cur.pos >= cur.data.length) throw new BinaryException("unexpected end of data");
        byte tag = cur.data[cur.pos++];
        switch (tag) {
            case TAG_NULL:   return SynxValue.ofNull();
            case TAG_FALSE:  return SynxValue.ofBool(false);
            case TAG_TRUE:   return SynxValue.ofBool(true);
            case TAG_INT:    return SynxValue.ofInt(zigzagDecode(decodeVarint(cur)));
            case TAG_FLOAT:  return SynxValue.ofFloat(decodeF64LE(cur, "unexpected end of data in float"));
            case TAG_STRING: return SynxValue.ofString(budget.text(t.get(decodeVarint(cur))));
            case TAG_SECRET: return SynxValue.ofSecret(budget.text(t.get(decodeVarint(cur))));
            case TAG_ARRAY: {
                long count = decodeVarint(cur);
                List<SynxValue> arr = new ArrayList<>(safeCapacity(count, cur));
                for (long i = 0; below(i, count); i++) arr.add(decodeValue(cur, t, budget, depth + 1));
                return SynxValue.ofArray(arr);
            }
            case TAG_OBJECT: {
                long count = decodeVarint(cur);
                SynxObject obj = new SynxObject();
                for (long i = 0; below(i, count); i++) {
                    String key = budget.text(t.get(decodeVarint(cur)));
                    obj.set(key, decodeValue(cur, t, budget, depth + 1));
                }
                return SynxValue.ofObject(obj);
            }
            default:
                throw new BinaryException(String.format("unknown type tag: 0x%02x", tag));
        }
    }

    // ─── metadata encode / decode ────────────────────────────────────────────

    private static void encodeConstraints(SynxConstraints c, StringTable t, ByteArrayOutputStream out) {
        int bits = 0;
        if (c.min != null)        bits |= 0x01;
        if (c.max != null)        bits |= 0x02;
        if (c.typeName != null)   bits |= 0x04;
        if (c.required)           bits |= 0x08;
        if (c.readonly)           bits |= 0x10;
        if (c.pattern != null)    bits |= 0x20;
        if (c.enumValues != null) bits |= 0x40;
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

    private static SynxConstraints decodeConstraints(Cursor cur, StringTableReader t, Budget budget) {
        if (cur.pos >= cur.data.length) throw new BinaryException("unexpected end of data in constraints");
        int bits = cur.data[cur.pos++] & 0xFF;
        SynxConstraints c = new SynxConstraints();
        if ((bits & 0x01) != 0) c.min = decodeF64LE(cur, "truncated min");
        if ((bits & 0x02) != 0) c.max = decodeF64LE(cur, "truncated max");
        if ((bits & 0x04) != 0) c.typeName = budget.text(t.get(decodeVarint(cur)));
        if ((bits & 0x08) != 0) c.required = true;
        if ((bits & 0x10) != 0) c.readonly = true;
        if ((bits & 0x20) != 0) c.pattern = budget.text(t.get(decodeVarint(cur)));
        if ((bits & 0x40) != 0) {
            long count = decodeVarint(cur);
            List<String> vals = new ArrayList<>(safeCapacity(count, cur));
            for (long i = 0; below(i, count); i++) {
                budget.value();
                vals.add(budget.text(t.get(decodeVarint(cur))));
            }
            c.enumValues = vals;
        }
        return c;
    }

    private static void encodeMetadata(Map<String, Map<String, SynxMeta>> tree, StringTable t,
                                       ByteArrayOutputStream out) {
        List<String> outerKeys = new ArrayList<>(tree.keySet());
        outerKeys.sort(SynxParser::compareCodePoints);
        encodeVarint(out, outerKeys.size());
        for (String path : outerKeys) {
            encodeVarint(out, t.indexOf(path));
            Map<String, SynxMeta> map = tree.get(path);
            List<String> innerKeys = new ArrayList<>(map.keySet());
            innerKeys.sort(SynxParser::compareCodePoints);
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
                } else {
                    out.write(0);
                }
                if (m.constraints != null) {
                    out.write(1);
                    encodeConstraints(m.constraints, t, out);
                } else {
                    out.write(0);
                }
            }
        }
    }

    private static Map<String, Map<String, SynxMeta>> decodeMetadata(Cursor cur, StringTableReader t, Budget budget) {
        long outer = decodeVarint(cur);
        Map<String, Map<String, SynxMeta>> tree = new LinkedHashMap<>();
        for (long i = 0; below(i, outer); i++) {
            budget.value();
            String path = budget.text(t.get(decodeVarint(cur)));
            long inner = decodeVarint(cur);
            Map<String, SynxMeta> map = new LinkedHashMap<>();
            for (long j = 0; below(j, inner); j++) {
                String fk = budget.text(t.get(decodeVarint(cur)));
                budget.value();
                SynxMeta m = new SynxMeta();
                long mc = decodeVarint(cur);
                for (long k = 0; below(k, mc); k++) {
                    budget.value();
                    m.markers.add(budget.text(t.get(decodeVarint(cur))));
                }
                long ac = decodeVarint(cur);
                for (long k = 0; below(k, ac); k++) {
                    budget.value();
                    m.args.add(budget.text(t.get(decodeVarint(cur))));
                }
                if (cur.pos >= cur.data.length) throw new BinaryException("unexpected end in meta");
                int hasTh = cur.data[cur.pos++] & 0xFF;
                if (hasTh != 0) m.typeHint = budget.text(t.get(decodeVarint(cur)));
                if (cur.pos >= cur.data.length) throw new BinaryException("unexpected end in meta");
                int hasC = cur.data[cur.pos++] & 0xFF;
                if (hasC != 0) m.constraints = decodeConstraints(cur, t, budget);
                map.put(fk, m);
            }
            tree.put(path, map);
        }
        return tree;
    }

    private static void encodeIncludes(List<SynxIncludeDirective> incs, StringTable t, ByteArrayOutputStream out) {
        if (incs == null) incs = List.of();
        encodeVarint(out, incs.size());
        for (var inc : incs) {
            encodeVarint(out, t.indexOf(inc.path()));
            encodeVarint(out, t.indexOf(inc.alias()));
        }
    }

    private static List<SynxIncludeDirective> decodeIncludes(Cursor cur, StringTableReader t, Budget budget) {
        long count = decodeVarint(cur);
        List<SynxIncludeDirective> out = new ArrayList<>(safeCapacity(count, cur));
        for (long i = 0; below(i, count); i++) {
            budget.value();
            long pathIdx = decodeVarint(cur);
            long aliasIdx = decodeVarint(cur);
            out.add(new SynxIncludeDirective(budget.text(t.get(pathIdx)), budget.text(t.get(aliasIdx))));
        }
        return out;
    }
}
