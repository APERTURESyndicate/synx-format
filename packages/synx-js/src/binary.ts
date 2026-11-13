/**
 * SYNX binary format (.synxb) — @aperturesyndicate/synx-format
 *
 * A port of `binary.rs` in synx-core: the same layout, the same order of
 * everything written, the same limits and the same refusals on the way back.
 *
 * ```text
 * HEADER       5 bytes magic "SYNXB"
 *              1 byte  version (currently 1)
 *              1 byte  flags   (bit 0: active, bit 1: locked,
 *                               bit 2: has_metadata, bit 3: resolved,
 *                               bit 4: tool, bit 5: schema, bit 6: llm)
 *              4 bytes size of the payload, little-endian
 * PAYLOAD      raw deflate of:
 *   STRING_TABLE varint count, then for each: varint length + UTF-8 bytes
 *   VALUE_TREE   the root value; strings and keys as table indices
 *   [METADATA]   if flag bit 2 is set
 *   [INCLUDES]   if flag bit 2 is set
 * ```
 */

import type { SynxObject, SynxValue, SynxParseResult, SynxMeta, SynxMetaMap, SynxConstraints, SynxInclude, SynxMode } from './types';
import { keysOf, metaMapOf, setMetaMap, newMetaMap, metaOrderOf, setKeyOrder, isIndexKey, utf8Length, SynxError } from './parser';
import { compareUnicodeScalar } from './json';
import { SynxSecret } from './engine';

const FORMAT_VERSION = 1;

/** Upper bound on the decompressed payload a decode will materialise. */
export const MAX_DECODED_BYTES = 256 * 1024 * 1024;
/** Depth limit of the value decoder: twice what the parser nests. */
const MAX_DECODE_DEPTH = 256;
/**
 * Most memory the tree decoded from one file may take, counted as synx-core
 * counts it: the index of its strings, every value, key and text it holds,
 * and its declarations.
 *
 * safety: a value is one byte on the wire, a string is stored once and named
 * by every reference, and deflate shrinks a run of either a thousandfold:
 * unbudgeted, a file of a few kilobytes decoded to gigabytes.
 */
const MAX_DECODED_TREE_BYTES = 192 * 1024 * 1024;

// What each thing occupies in the reference implementation (64-bit), so both
// refuse the same file.
const TABLE_SLOT_COST = 16;
const ARRAY_SLOT_COST = 64;
const OBJECT_ENTRY_COST = 112;
const TEXT_SLOT_COST = 24;
const META_ENTRY_COST = 232;
const META_PATH_COST = 112;
const INCLUDE_COST = 48;

const FLAG_ACTIVE = 0x01;
const FLAG_LOCKED = 0x02;
const FLAG_HAS_META = 0x04;
const FLAG_RESOLVED = 0x08;
const FLAG_TOOL = 0x10;
const FLAG_SCHEMA = 0x20;
const FLAG_LLM = 0x40;

const TAG_NULL = 0x00;
const TAG_FALSE = 0x01;
const TAG_TRUE = 0x02;
const TAG_INT = 0x03;
const TAG_FLOAT = 0x04;
const TAG_STRING = 0x05;
const TAG_ARRAY = 0x06;
const TAG_OBJECT = 0x07;
const TAG_SECRET = 0x08;

const U64_MASK = (BigInt(1) << BigInt(64)) - BigInt(1);

/** Lazy load Node's zlib once (browser callers don't compile/decompile binary). */
let zlibModule: typeof import('zlib') | undefined;
function zlib(): typeof import('zlib') {
  if (zlibModule) return zlibModule;
  try {
    zlibModule = require('zlib');
    return zlibModule!;
  } catch {
    throw new SynxError('.synxb compile/decompile requires Node.js zlib (not available in this environment)');
  }
}

// ─── Writing ──────────────────────────────────────────────

/** A byte buffer that grows as it is written. */
class Writer {
  bytes = new Uint8Array(1024);
  length = 0;
  private view = new DataView(this.bytes.buffer);

  private room(n: number): void {
    if (this.length + n <= this.bytes.length) return;
    let size = this.bytes.length * 2;
    while (size < this.length + n) size *= 2;
    const next = new Uint8Array(size);
    next.set(this.bytes.subarray(0, this.length));
    this.bytes = next;
    this.view = new DataView(next.buffer);
  }

  byte(b: number): void {
    this.room(1);
    this.bytes[this.length++] = b;
  }

  /** LEB128, unsigned. */
  varint(value: number): void {
    this.room(10);
    let v = value;
    while (v > 0x7f) {
      this.bytes[this.length++] = (v % 0x80) | 0x80;
      v = Math.floor(v / 0x80);
    }
    this.bytes[this.length++] = v;
  }

  varintBig(value: bigint): void {
    this.room(10);
    let v = value;
    while (v > BigInt(0x7f)) {
      this.bytes[this.length++] = Number(v & BigInt(0x7f)) | 0x80;
      v >>= BigInt(7);
    }
    this.bytes[this.length++] = Number(v);
  }

  /** An i64 as a zigzag varint. */
  zigzag(value: number | bigint): void {
    // perf: within ±2^52 the doubled magnitude is still exact as a number.
    if (typeof value === 'number' && Math.abs(value) <= 4503599627370496) {
      this.varint(value >= 0 ? value * 2 : -value * 2 - 1);
      return;
    }
    const n = typeof value === 'bigint' ? value : BigInt(value);
    this.varintBig(((n << BigInt(1)) ^ (n >> BigInt(63))) & U64_MASK);
  }

  float(value: number): void {
    this.room(8);
    this.view.setFloat64(this.length, value, true);
    this.length += 8;
  }

  /** A string as a varint byte length and its UTF-8 bytes. */
  text(s: string, encoder: { encodeInto(source: string, destination: Uint8Array): unknown }): void {
    const size = utf8Length(s);
    this.varint(size);
    this.room(size);
    encoder.encodeInto(s, this.bytes.subarray(this.length, this.length + size));
    this.length += size;
  }
}

/** Every string of a document, stored once; the most used first. */
class StringTable {
  strings: string[] = [];
  index = new Map<string, number>();
  /** How many times each string is referenced, by first-seen index. */
  private uses: number[] = [];

  intern(s: string): void {
    const known = this.index.get(s);
    if (known !== undefined) {
      this.uses[known]++;
      return;
    }
    this.index.set(s, this.strings.length);
    this.strings.push(s);
    this.uses.push(1);
  }

  /**
   * Renumber the table by descending use count (ties keep first-seen order),
   * so the most referenced strings get one-byte varints. The format is
   * unchanged — a decoder reads indices against whatever table it is given.
   */
  orderByUse(): void {
    const uses = this.uses;
    const order = this.strings.map((_, i) => i).sort((a, b) => (uses[b] - uses[a]) || (a - b));
    const strings = order.map(i => this.strings[i]);
    this.strings = strings;
    for (let i = 0; i < strings.length; i++) this.index.set(strings[i], i);
  }

  collectValue(val: SynxValue | undefined): void {
    if (typeof val === 'string') {
      this.intern(val);
      return;
    }
    if (val === null || typeof val !== 'object') return;
    if (val instanceof SynxSecret) {
      this.intern(val.reveal());
      return;
    }
    if (Array.isArray(val)) {
      for (let i = 0; i < val.length; i++) this.collectValue(val[i]);
      return;
    }
    const obj = val as SynxObject;
    const keys = keysOf(obj);
    for (let i = 0; i < keys.length; i++) {
      this.intern(keys[i]);
      this.collectValue(obj[keys[i]]);
    }
  }

  collectMetadata(table: MetaEntry[]): void {
    for (const entry of table) {
      this.intern(entry.path);
      for (const [key, meta] of entry.map) {
        this.intern(key);
        for (const m of meta.markers) this.intern(m);
        if (meta.args) for (const a of meta.args) this.intern(a);
        if (meta.typeHint !== undefined) this.intern(meta.typeHint);
        const c = meta.constraints;
        if (c) {
          if (c.type !== undefined) this.intern(c.type);
          if (c.pattern !== undefined) this.intern(c.pattern);
          if (c.enum) for (const v of c.enum) this.intern(v);
        }
      }
    }
  }

  of(s: string): number {
    return this.index.get(s) as number;
  }
}

/** The declarations of one object, under the path the format files them by. */
interface MetaEntry {
  path: string;
  map: SynxMetaMap;
  order: number;
}

/** `key` as one segment of a declarations path: `.` and `\` are escaped with `\`. */
function keySegment(key: string): string {
  if (key.indexOf('.') === -1 && key.indexOf('\\') === -1) return key;
  let out = '';
  for (let i = 0; i < key.length; i++) {
    const c = key.charCodeAt(i);
    if (c === 46 || c === 92) out += '\\';
    out += key[i];
  }
  return out;
}

/** The declarations of every object of a tree, by path, in the order the parser filed them. */
function metadataTable(root: SynxObject): MetaEntry[] {
  const table: MetaEntry[] = [];
  collectMeta(root, '', table, 0);
  // Stable, so maps the parser did not number keep the order of the walk.
  table.sort((a, b) => a.order - b.order);
  return table;
}

function collectMeta(obj: SynxObject, path: string, table: MetaEntry[], depth: number): void {
  if (depth > MAX_DECODE_DEPTH * 2) return;
  const map = metaMapOf(obj);
  if (map !== undefined && map.size > 0) table.push({ path, map, order: metaOrderOf(map) });
  const keys = keysOf(obj);
  for (let i = 0; i < keys.length; i++) {
    const child = obj[keys[i]];
    if (child === null || typeof child !== 'object' || child instanceof SynxSecret) continue;
    const childPath = path === '' ? keySegment(keys[i]) : `${path}.${keySegment(keys[i])}`;
    if (Array.isArray(child)) {
      for (let j = 0; j < child.length; j++) {
        const item = child[j];
        if (item !== null && typeof item === 'object' && !Array.isArray(item) && !(item instanceof SynxSecret)) {
          collectMeta(item as SynxObject, `${childPath}.#${j}`, table, depth + 1);
        }
      }
    } else {
      collectMeta(child as SynxObject, childPath, table, depth + 1);
    }
  }
}

/**
 * `inOrder` writes an object's entries as the document has them instead of
 * sorted by key.
 *
 * spec: a file that still carries its markers is resolved after it is read
 * back, and markers resolve in document order. A decoder reads the entries in
 * whatever order they arrive, so the layout is unchanged — and a static or
 * resolved file, where order decides nothing, is written sorted.
 */
function encodeValue(out: Writer, val: SynxValue | undefined, st: StringTable, inOrder: boolean): void {
  if (val === null || val === undefined) {
    out.byte(TAG_NULL);
    return;
  }
  switch (typeof val) {
    case 'boolean':
      out.byte(val ? TAG_TRUE : TAG_FALSE);
      return;
    case 'number':
      // An integer is one this number type holds exactly. A whole value past
      // 2^53 is a float here — the parser gives such an integer as a bigint —
      // and written as INT it came back as a bigint the document never had.
      if (Number.isSafeInteger(val) && !Object.is(val, -0)) {
        out.byte(TAG_INT);
        out.zigzag(val);
      } else {
        out.byte(TAG_FLOAT);
        out.float(val);
      }
      return;
    case 'bigint':
      out.byte(TAG_INT);
      out.zigzag(val);
      return;
    case 'string':
      out.byte(TAG_STRING);
      out.varint(st.of(val));
      return;
  }
  if (val instanceof SynxSecret) {
    out.byte(TAG_SECRET);
    out.varint(st.of(val.reveal()));
    return;
  }
  if (Array.isArray(val)) {
    out.byte(TAG_ARRAY);
    out.varint(val.length);
    for (let i = 0; i < val.length; i++) encodeValue(out, val[i], st, inOrder);
    return;
  }
  const obj = val as SynxObject;
  const keys = inOrder ? keysOf(obj) : Object.keys(obj).sort(compareUnicodeScalar);
  out.byte(TAG_OBJECT);
  out.varint(keys.length);
  for (let i = 0; i < keys.length; i++) {
    out.varint(st.of(keys[i]));
    encodeValue(out, obj[keys[i]], st, inOrder);
  }
}

function encodeConstraints(out: Writer, c: SynxConstraints, st: StringTable): void {
  let bits = 0;
  if (c.min !== undefined) bits |= 0x01;
  if (c.max !== undefined) bits |= 0x02;
  if (c.type !== undefined) bits |= 0x04;
  if (c.required) bits |= 0x08;
  if (c.readonly) bits |= 0x10;
  if (c.pattern !== undefined) bits |= 0x20;
  if (c.enum !== undefined) bits |= 0x40;
  out.byte(bits);
  if (c.min !== undefined) out.float(c.min);
  if (c.max !== undefined) out.float(c.max);
  if (c.type !== undefined) out.varint(st.of(c.type));
  if (c.pattern !== undefined) out.varint(st.of(c.pattern));
  if (c.enum !== undefined) {
    out.varint(c.enum.length);
    for (const v of c.enum) out.varint(st.of(v));
  }
}

function encodeMeta(out: Writer, meta: SynxMeta, st: StringTable): void {
  out.varint(meta.markers.length);
  for (const m of meta.markers) out.varint(st.of(m));
  const args = meta.args ?? [];
  out.varint(args.length);
  for (const a of args) out.varint(st.of(a));
  if (meta.typeHint !== undefined) {
    out.byte(1);
    out.varint(st.of(meta.typeHint));
  } else {
    out.byte(0);
  }
  if (meta.constraints) {
    out.byte(1);
    encodeConstraints(out, meta.constraints, st);
  } else {
    out.byte(0);
  }
}

/**
 * Compile a parsed document into `.synxb`. With `resolved` the tree is
 * written as it stands and declarations are left out; without it the tree is
 * written as parsed, with the declarations of its keys and its `!include`
 * lines, for the reader to resolve.
 */
export function compileSynxb(result: SynxParseResult, resolved: boolean): Uint8Array {
  const st = new StringTable();
  st.collectValue(result.root);
  const table = resolved ? [] : metadataTable(result.root);
  const includes = result.includes ?? [];
  const hasMeta = table.length > 0;
  if (hasMeta) {
    st.collectMetadata(table);
    for (const inc of includes) {
      st.intern(inc.path);
      st.intern(inc.alias);
    }
  }
  st.orderByUse();

  const out = new Writer();
  const encoder = new (TextEncoder as any)() as { encodeInto(source: string, destination: Uint8Array): unknown };
  out.varint(st.strings.length);
  for (const s of st.strings) out.text(s, encoder);
  encodeValue(out, result.root, st, hasMeta);
  if (hasMeta) {
    out.varint(table.length);
    for (const entry of table) {
      out.varint(st.of(entry.path));
      out.varint(entry.map.size);
      for (const [key, meta] of entry.map) {
        out.varint(st.of(key));
        encodeMeta(out, meta, st);
      }
    }
    out.varint(includes.length);
    for (const inc of includes) {
      out.varint(st.of(inc.path));
      out.varint(st.of(inc.alias));
    }
  }

  // A storage format: size over speed, and level 9 as synx-core uses.
  const payload = out.bytes.subarray(0, out.length);
  const compressed = zlib().deflateRawSync(payload, { level: 9 });

  let flags = 0;
  if (result.mode === 'active') flags |= FLAG_ACTIVE;
  if (result.locked) flags |= FLAG_LOCKED;
  if (hasMeta) flags |= FLAG_HAS_META;
  if (resolved) flags |= FLAG_RESOLVED;
  if (result.tool) flags |= FLAG_TOOL;
  if (result.schema) flags |= FLAG_SCHEMA;
  if (result.llm) flags |= FLAG_LLM;

  const file = new Uint8Array(11 + compressed.length);
  file[0] = 0x53; file[1] = 0x59; file[2] = 0x4e; file[3] = 0x58; file[4] = 0x42; // SYNXB
  file[5] = FORMAT_VERSION;
  file[6] = flags;
  const size = payload.length >>> 0;
  file[7] = size & 0xff;
  file[8] = (size >>> 8) & 0xff;
  file[9] = (size >>> 16) & 0xff;
  file[10] = (size >>> 24) & 0xff;
  file.set(compressed, 11);
  return file;
}

// ─── Reading ──────────────────────────────────────────────

/** A `.synxb` file as a document. */
export interface DecodedSynxb {
  root: SynxValue;
  mode: SynxMode;
  locked: boolean;
  tool: boolean;
  schema: boolean;
  llm: boolean;
  /** The file holds an already resolved document (header flag). */
  resolved: boolean;
  /** The file carries declarations; they are attached to the objects of `root`. */
  hasMeta: boolean;
  includes: SynxInclude[];
}

function refuse(message: string): never {
  throw new SynxError(message);
}

/** Reads a payload: position, limits and the memory budget of what it has built. */
class Reader {
  pos = 0;
  private spent = 0;
  private readonly view: DataView;
  /** The string table, and the UTF-8 size of each string for the budget. */
  private strings: string[] = [];
  private sizes: number[] = [];

  constructor(readonly data: Uint8Array) {
    this.view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  }

  private charge(bytes: number): void {
    this.spent += bytes;
    if (this.spent > MAX_DECODED_TREE_BYTES) {
      refuse(`decoded document exceeds ${MAX_DECODED_TREE_BYTES} bytes — refusing to decode`);
    }
  }

  /**
   * Storage for `count` items read from the file, paid for: each takes at
   * least `wire` of the bytes that remain and `cost` bytes in memory.
   *
   * safety: counts are varints straight from the file. More items than the
   * rest of the file can spell is a truncated file, said at once.
   */
  private reserve(count: number, wire: number, cost: number): void {
    if (count > Math.floor((this.data.length - this.pos) / wire)) refuse('unexpected end of data');
    this.charge(count * cost);
  }

  private byte(what: string): number {
    if (this.pos >= this.data.length) refuse(what);
    return this.data[this.pos++];
  }

  /**
   * LEB128, unsigned, at most 64 bits: ten bytes, the tenth without a
   * continuation bit.
   *
   * safety: a longer one was read to its end and its high bits dropped, and
   * one cut off by the end of the payload was taken for the value read so
   * far — both are malformed files, refused as synx-core refuses them.
   */
  private varintBig(): bigint {
    const data = this.data;
    let result = BigInt(0);
    let shift = 0;
    for (;;) {
      if (this.pos >= data.length) refuse('unexpected end of data in varint');
      const byte = data[this.pos++];
      result |= BigInt(byte & 0x7f) << BigInt(shift);
      if ((byte & 0x80) === 0) return result & U64_MASK;
      shift += 7;
      if (shift >= 64) refuse('varint overflow');
    }
  }

  /** A varint as a number: exact to 2^53, enough for every count, length and index. */
  private varint(): number {
    const data = this.data;
    const start = this.pos;
    let result = 0;
    let mul = 1;
    // perf: up to seven bytes (49 bits) in plain arithmetic.
    for (let i = 0; i < 7; i++) {
      if (this.pos >= data.length) refuse('unexpected end of data in varint');
      const byte = data[this.pos++];
      result += (byte & 0x7f) * mul;
      if ((byte & 0x80) === 0) return result;
      mul *= 128;
    }
    this.pos = start;
    return Number(this.varintBig());
  }

  private index(): number {
    const idx = this.varint();
    if (idx > 0xffffffff) refuse('string index out of range');
    return idx;
  }

  /** The table string at `idx`, charged as the copy it is in synx-core. */
  private text(idx: number): string {
    if (idx >= this.strings.length) refuse(`string index ${idx} out of bounds (size ${this.strings.length})`);
    this.charge(this.sizes[idx]);
    return this.strings[idx];
  }

  /** `count` references to table strings. */
  private texts(count: number): string[] {
    this.reserve(count, 1, TEXT_SLOT_COST);
    const out = new Array<string>(count);
    for (let i = 0; i < count; i++) out[i] = this.text(this.index());
    return out;
  }

  stringTable(): void {
    const count = this.varint();
    // safety: 64 MiB of zero bytes is sixty-four million empty strings.
    this.reserve(count, 1, TABLE_SLOT_COST);
    const data = this.data;
    let decoder: { decode(input: Uint8Array): string } | undefined;
    for (let i = 0; i < count; i++) {
      const size = this.varint();
      if (size > data.length - this.pos) refuse('unexpected end of data in string table');
      const end = this.pos + size;
      let s: string;
      if (size === 0) s = '';
      else {
        if (decoder === undefined) decoder = new (TextDecoder as any)('utf-8', { fatal: true, ignoreBOM: true }) as { decode(input: Uint8Array): string };
        try {
          s = decoder.decode(data.subarray(this.pos, end));
        } catch {
          refuse('invalid UTF-8 in string table');
        }
      }
      this.pos = end;
      this.strings.push(s);
      this.sizes.push(size);
    }
  }

  value(depth: number): SynxValue {
    if (depth > MAX_DECODE_DEPTH) refuse(`nesting exceeds ${MAX_DECODE_DEPTH} — refusing to decode`);
    const tag = this.byte('unexpected end of data');
    switch (tag) {
      case TAG_NULL: return null;
      case TAG_FALSE: return false;
      case TAG_TRUE: return true;
      case TAG_INT: {
        const raw = this.varintBig();
        const value = BigInt.asIntN(64, (raw >> BigInt(1)) ^ -(raw & BigInt(1)));
        // An integer beyond 2^53 stays exact as a bigint, as the parser returns it.
        const n = Number(value);
        return Number.isSafeInteger(n) ? n : value;
      }
      case TAG_FLOAT: {
        if (this.pos + 8 > this.data.length) refuse('unexpected end of data in float');
        const f = this.view.getFloat64(this.pos, true);
        this.pos += 8;
        return f;
      }
      case TAG_STRING: return this.text(this.index());
      case TAG_ARRAY: {
        const count = this.varint();
        this.reserve(count, 1, ARRAY_SLOT_COST);
        const arr = new Array<SynxValue>(count);
        for (let i = 0; i < count; i++) arr[i] = this.value(depth + 1);
        return arr;
      }
      case TAG_OBJECT: {
        const count = this.varint();
        this.reserve(count, 2, OBJECT_ENTRY_COST);
        const obj: SynxObject = {};
        // The order of the file, kept where an object would list it otherwise.
        let order: string[] | null = null;
        for (let i = 0; i < count; i++) {
          const key = this.text(this.index());
          const val = this.value(depth + 1);
          const known = Object.prototype.hasOwnProperty.call(obj, key);
          if (order === null && isIndexKey(key)) order = Object.keys(obj);
          if (order !== null && !known) order.push(key);
          // `obj.__proto__ = x` would hit the prototype setter; as an own
          // property a `__proto__` key from the file stays plain data.
          if (key === '__proto__') {
            Object.defineProperty(obj, key, { value: val, writable: true, enumerable: true, configurable: true });
          } else {
            obj[key] = val;
          }
        }
        if (order !== null) setKeyOrder(obj, order);
        return obj;
      }
      case TAG_SECRET: return new SynxSecret(this.text(this.index())) as unknown as SynxValue;
      default: return refuse(`unknown type tag: 0x${tag.toString(16).padStart(2, '0')}`);
    }
  }

  private constraints(): SynxConstraints {
    const bits = this.byte('unexpected end of data in constraints');
    const c: SynxConstraints = {};
    if (bits & 0x01) {
      if (this.pos + 8 > this.data.length) refuse('truncated min');
      c.min = this.view.getFloat64(this.pos, true);
      this.pos += 8;
    }
    if (bits & 0x02) {
      if (this.pos + 8 > this.data.length) refuse('truncated max');
      c.max = this.view.getFloat64(this.pos, true);
      this.pos += 8;
    }
    if (bits & 0x04) c.type = this.text(this.index());
    if (bits & 0x08) c.required = true;
    if (bits & 0x10) c.readonly = true;
    if (bits & 0x20) c.pattern = this.text(this.index());
    if (bits & 0x40) c.enum = this.texts(this.varint());
    return c;
  }

  private meta(): SynxMeta {
    const markers = this.texts(this.varint());
    const args = this.texts(this.varint());
    const typeHint = this.byte('unexpected end in meta') !== 0 ? this.text(this.index()) : undefined;
    const constraints = this.byte('unexpected end in meta') !== 0 ? this.constraints() : undefined;
    return { markers, args: args.length > 0 ? args : undefined, constraints, typeHint, bits: 0 };
  }

  /** The declarations of the document, by the path of the object they belong to. */
  metadata(): Map<string, SynxMetaMap> {
    const count = this.varint();
    this.reserve(count, 2, META_PATH_COST);
    const table = new Map<string, SynxMetaMap>();
    for (let i = 0; i < count; i++) {
      const path = this.text(this.index());
      const fields = this.varint();
      // An entry is a key and four bytes of declaration at the least.
      this.reserve(fields, 5, META_ENTRY_COST);
      const map = newMetaMap();
      for (let j = 0; j < fields; j++) {
        const key = this.text(this.index());
        map.set(key, this.meta());
      }
      table.set(path, map);
    }
    return table;
  }

  includes(): SynxInclude[] {
    const count = this.varint();
    this.reserve(count, 2, INCLUDE_COST);
    const out: SynxInclude[] = [];
    for (let i = 0; i < count; i++) {
      const path = this.text(this.index());
      const alias = this.text(this.index());
      out.push({ path, alias });
    }
    return out;
  }
}

/** Give each object of the tree the declarations the file holds for its path. */
function attachMetadata(obj: SynxObject, path: string, table: Map<string, SynxMetaMap>): void {
  const map = table.get(path);
  if (map !== undefined && map.size > 0) setMetaMap(obj, map);
  const keys = keysOf(obj);
  for (let i = 0; i < keys.length; i++) {
    const child = obj[keys[i]];
    if (child === null || typeof child !== 'object' || child instanceof SynxSecret) continue;
    const childPath = path === '' ? keySegment(keys[i]) : `${path}.${keySegment(keys[i])}`;
    if (Array.isArray(child)) {
      for (let j = 0; j < child.length; j++) {
        const item = child[j];
        if (item !== null && typeof item === 'object' && !Array.isArray(item) && !(item instanceof SynxSecret)) {
          attachMetadata(item as SynxObject, `${childPath}.#${j}`, table);
        }
      }
    } else {
      attachMetadata(child as SynxObject, childPath, table);
    }
  }
}

/** Whether data starts with the `.synxb` magic bytes. */
export function isSynxbData(data: Uint8Array): boolean {
  return data.length >= 5 && data[0] === 0x53 && data[1] === 0x59 && data[2] === 0x4e && data[3] === 0x58 && data[4] === 0x42;
}

/**
 * Read a `.synxb` file back into a document.
 *
 * safety: the bytes are untrusted. Every length, count and index is checked
 * against what is there, the inflate is bounded by the declared size, and
 * whatever is wrong with the file is a `SynxError` — an exception of zlib or
 * of the runtime (`RangeError`, `TypeError`) never leaves this function.
 */
export function decodeSynxb(data: Uint8Array): DecodedSynxb {
  if (!(data instanceof Uint8Array)) refuse('INPUT_ERR: Synx.decompile expects bytes');
  if (data.length < 11) refuse('file too small for .synxb header');
  if (!isSynxbData(data)) refuse('invalid .synxb magic (expected SYNXB)');
  const version = data[5];
  if (version !== FORMAT_VERSION) refuse(`unsupported .synxb version: ${version} (expected ${FORMAT_VERSION})`);
  const flags = data[6];
  // `>>> 0` keeps it unsigned; a shift by 24 on a high byte would sign-flip.
  const size = (data[7] | (data[8] << 8) | (data[9] << 16) | (data[10] << 24)) >>> 0;
  if (size > MAX_DECODED_BYTES) refuse(`declared size ${size} exceeds limit ${MAX_DECODED_BYTES}`);

  // safety: the stream may produce the declared size and not a byte more —
  // bounded only by the cap, a small file declaring 1 KiB still inflated to
  // 256 MiB before the size check. The output buffer grows as the stream
  // does; sized by the declared length, a 13-byte file reserved 256 MiB.
  let payload: Uint8Array;
  try {
    payload = zlib().inflateRawSync(data.subarray(11), { maxOutputLength: size + 1 });
  } catch (e) {
    if (e instanceof SynxError) throw e;
    return refuse('decompression failed');
  }
  if (payload.length !== size) refuse(`size mismatch: expected ${size}, got ${payload.length}`);

  const reader = new Reader(payload);
  let root: SynxValue;
  let table: Map<string, SynxMetaMap> | undefined;
  let includes: SynxInclude[] = [];
  const hasMeta = (flags & FLAG_HAS_META) !== 0;
  try {
    reader.stringTable();
    root = reader.value(0);
    if (hasMeta) {
      table = reader.metadata();
      includes = reader.includes();
    }
  } catch (e) {
    if (e instanceof SynxError) throw e;
    // Whatever else a crafted file provokes is still a refusal of the file.
    return refuse('malformed .synxb payload');
  }
  if (table !== undefined && root !== null && typeof root === 'object' && !Array.isArray(root) && !(root instanceof SynxSecret)) {
    attachMetadata(root as SynxObject, '', table);
  }
  return {
    root,
    mode: (flags & FLAG_ACTIVE) !== 0 ? 'active' : 'static',
    locked: (flags & FLAG_LOCKED) !== 0,
    tool: (flags & FLAG_TOOL) !== 0,
    schema: (flags & FLAG_SCHEMA) !== 0,
    llm: (flags & FLAG_LLM) !== 0,
    resolved: (flags & FLAG_RESOLVED) !== 0,
    hasMeta: table !== undefined && table.size > 0,
    includes,
  };
}
