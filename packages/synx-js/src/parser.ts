/**
 * SYNX Parser — @aperturesyndicate/synx-format
 *
 * Converts raw .synx text into a structured object tree; the declarations of
 * its keys (markers, constraints, type hints) are kept beside the tree for the
 * engine to resolve.
 *
 * Written against character codes and offsets into the input: a line is never
 * copied, and of a key-value line only the key and the value become strings.
 * No regular expression runs over text from the document.
 *
 * perf: this module and the engine are all a service loads to read its config.
 * What both of them and the other modules share — the error type, a few text
 * helpers, the SYNXL limits — lives here, so that none of it pulls in a module
 * the first parse does not need.
 */

import type {
  SynxObject,
  SynxArray,
  SynxValue,
  SynxMode,
  SynxParseResult,
  SynxInclude,
  SynxMeta,
  SynxMetaMap,
  SynxConstraints,
} from './types';

/**
 * Typed error thrown by SYNX in strict mode.
 * The `code` field contains the error prefix (e.g. "CALC_ERR", "ALIAS_ERR").
 */
export class SynxError extends Error {
  readonly code: string;

  constructor(message: string) {
    super(message);
    this.name = 'SynxError';
    // Extract prefix up to first ':'
    const colonIdx = message.indexOf(':');
    this.code = colonIdx !== -1 ? message.slice(0, colonIdx) : 'SYNX_ERR';
  }
}

// ─── SYNXL identity and limits (SYNXL §1.3, §13) ──────────

/** The SYNXL format version implemented by this package. */
export const SYNXL_VERSION = 1;

/** Per-record byte budget (record line plus block). Exceeding it truncates. */
export const MAX_SYNXL_RECORD_BYTES = 16 * 1024 * 1024; // 16 MiB
/** Fields per field list. Exceeding it is a hard error. */
export const MAX_SYNXL_FIELDS = 4096;
/** Field-name length in UTF-8 bytes. Exceeding it is a hard error. */
export const MAX_SYNXL_FIELD_NAME_BYTES = 255;
/** Field lists per document. Exceeding it is a hard error. */
export const MAX_SYNXL_FIELD_LISTS = 65536;
/** Records per in-memory parse. The streaming reader has no record cap. */
export const MAX_SYNXL_RECORDS = 16777216;

const I64_MIN = -(2n ** 63n);
const I64_MAX = 2n ** 63n - 1n;

/**
 * An integer literal as the reference implementation represents it, or `null`
 * when it does not fit i64.
 *
 * Rust parses into `i64` and, on overflow, falls through §8.3 to the string
 * rule — SYNX floats need a dot, so there is nothing in between. Matching that
 * matters twice over: the JSON type of the value differs (`"92233…"` versus a
 * number), and `parseInt` on a 19-digit id returns a *different* id with no
 * diagnostic to say so.
 *
 * spec: the text is an optional sign and decimal digits, as `parse::<i64>()`
 * reads it. `BigInt()` alone also took `0x10`, `0b11`, `0o17` and surrounding
 * spaces, so `x(int) 0x10` was 16 here and 0 in synx-core.
 */
export function parseI64(text: string): number | bigint | null {
  const len = text.length;
  const c0 = text.charCodeAt(0);
  const start = c0 === 43 || c0 === 45 ? 1 : 0;
  if (start >= len) return null;
  for (let i = start; i < len; i++) {
    const c = text.charCodeAt(i);
    if (c < 48 || c > 57) return null;
  }
  // perf: up to 15 digits is always a safe integer — no BigInt round trip.
  if (len - start <= 15) {
    const n = +text;
    return n === 0 ? 0 : n;
  }
  const big = BigInt(text);
  if (big < I64_MIN || big > I64_MAX) return null;
  const asNumber = Number(big);
  return Number.isSafeInteger(asNumber) ? asNumber : big;
}

/**
 * The text of an input argument: a string as it is, bytes read as UTF-8.
 *
 * safety: anything else used to surface as a TypeError from deep inside the
 * parser (`text.charCodeAt is not a function`); it is refused here with the
 * package's own error type.
 */
export function inputText(input: unknown, api: string): string {
  if (typeof input === 'string') return input;
  if (input instanceof Uint8Array) return decodeUtf8(input, `the input of ${api}`);
  if (input instanceof ArrayBuffer) return decodeUtf8(new Uint8Array(input), `the input of ${api}`);
  const got = input === null ? 'null' : Array.isArray(input) ? 'an array' : typeof input;
  throw new SynxError(`INPUT_ERR: ${api} expects a string, got ${got}`);
}

let utf8Strict: { decode(input: Uint8Array): string } | undefined;

/**
 * Bytes as the text they encode. A leading byte-order mark stays in the text —
 * the parser drops it — and bytes that are not UTF-8 are refused.
 *
 * safety: SYNX is UTF-8 (§2.1). Read leniently, every byte that is not became
 * U+FFFD: a file in Latin-1 or UTF-16 loaded as a document nobody wrote, and
 * two different files loaded as the same one.
 */
export function decodeUtf8(bytes: Uint8Array, what: string): string {
  if (utf8Strict === undefined) {
    utf8Strict = new (TextDecoder as any)('utf-8', { fatal: true, ignoreBOM: true }) as { decode(input: Uint8Array): string };
  }
  try {
    return utf8Strict.decode(bytes);
  } catch {
    throw new SynxError(`INPUT_ERR: ${what} is not valid UTF-8`);
  }
}

// ─── Resource limits (per SYNX 3.6 §3) ────────────────────

/** Maximum UTF-8 bytes accepted per parse(); longer input is cut at a character boundary. */
const MAX_SYNX_INPUT_BYTES = 16 * 1024 * 1024; // 16 MiB
/** Maximum line count considered. */
const MAX_LINE_STARTS = 2_000_000;
/** Maximum nesting depth for the parser stack. */
const MAX_PARSE_NESTING_DEPTH = 128;
/** Maximum accumulated bytes per multiline `key |` block body. */
const MAX_MULTILINE_BLOCK_BYTES = 1024 * 1024; // 1 MiB
/** Maximum items per single list. */
const MAX_LIST_ITEMS = 1_048_576;
/** Maximum `!include` directives per file. */
const MAX_INCLUDE_DIRECTIVES = 4096;
/** Maximum comma-separated parts inside a single `enum:` constraint. */
const MAX_CONSTRAINT_ENUM_PARTS = 4096;
/** Maximum `:a:b:c` marker chain segments on one line. */
const MAX_MARKER_CHAIN_SEGMENTS = 512;

// ─── Whitespace ───────────────────────────────────────────

/**
 * Unicode `White_Space` — the set SYNX §4's `ltrim` removes when measuring
 * indentation, and the set Rust's `str::trim` and `split_whitespace` use.
 *
 * It is not JavaScript's: `String.prototype.trim` and `\s` also take U+FEFF
 * and leave U+0085. Every trim in this parser goes through this function, so a
 * stray U+FEFF (two files with a BOM joined together) is data here as it is in
 * synx-core, not a blank that opens a block comment or vanishes from a value.
 */
export function isUnicodeSpaceCode(c: number): boolean {
  return (c >= 0x09 && c <= 0x0d) || c === 0x20 || c === 0x85 || c === 0xa0 ||
    c === 0x1680 || (c >= 0x2000 && c <= 0x200a) ||
    c === 0x2028 || c === 0x2029 || c === 0x202f || c === 0x205f || c === 0x3000;
}

/** UTF-8 width of a whitespace code unit: every Unicode space is in the BMP. */
export function wsUtf8Width(c: number): number {
  return c < 0x80 ? 1 : c < 0x800 ? 2 : 3;
}

/** `s` without leading and trailing Unicode whitespace — Rust's `str::trim`. */
export function trimSpace(s: string): string {
  let a = 0;
  let b = s.length;
  while (a < b && isUnicodeSpaceCode(s.charCodeAt(a))) a++;
  while (b > a && isUnicodeSpaceCode(s.charCodeAt(b - 1))) b--;
  return a === 0 && b === s.length ? s : s.substring(a, b);
}

/** The runs of `s` between Unicode whitespace — Rust's `split_whitespace`. */
export function splitSpace(s: string): string[] {
  const out: string[] = [];
  const n = s.length;
  let i = 0;
  while (i < n) {
    while (i < n && isUnicodeSpaceCode(s.charCodeAt(i))) i++;
    if (i >= n) break;
    const start = i;
    while (i < n && !isUnicodeSpaceCode(s.charCodeAt(i))) i++;
    out.push(s.substring(start, i));
  }
  return out;
}

/** UTF-8 byte length of a UTF-16 string — the unit every §3/§13 cap is in. */
export function utf8Length(s: string): number {
  let bytes = 0;
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c < 0x80) bytes += 1;
    else if (c < 0x800) bytes += 2;
    else if (c >= 0xd800 && c <= 0xdbff && i + 1 < s.length) {
      const lo = s.charCodeAt(i + 1);
      if (lo >= 0xdc00 && lo <= 0xdfff) { bytes += 4; i++; } else { bytes += 3; }
    } else bytes += 3;
  }
  return bytes;
}

/** Length in Unicode scalar values, without materialising `[...s]`. */
export function scalarLength(s: string): number {
  let n = s.length;
  for (let i = 0; i < s.length - 1; i++) {
    const c = s.charCodeAt(i);
    if (c >= 0xd800 && c <= 0xdbff) {
      const d = s.charCodeAt(i + 1);
      if (d >= 0xdc00 && d <= 0xdfff) { n--; i++; }
    }
  }
  return n;
}

/**
 * Compare by Unicode **scalar** order, which is what SYNX §10 / SYNXL §12.1
 * specify — not JavaScript's default UTF-16 code-unit order. The two disagree
 * only for astral characters, which a naive `<` sorts below U+E000..U+FFFF.
 * (Rust compares `String`s by UTF-8 bytes, i.e. scalar order.)
 */
export function compareUnicodeScalar(a: string, b: string): number {
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) {
    const ca = a.charCodeAt(i);
    const cb = b.charCodeAt(i);
    if (ca === cb) continue;
    const sa = ca >= 0xd800 && ca <= 0xdbff ? (a.codePointAt(i) as number) : ca;
    const sb = cb >= 0xd800 && cb <= 0xdbff ? (b.codePointAt(i) as number) : cb;
    return sa < sb ? -1 : 1;
  }
  return a.length - b.length;
}

/** Longest prefix of `s` fitting `maxBytes` of UTF-8, never splitting a pair. */
export function truncateUtf8Prefix(s: string, maxBytes: number): string {
  if (maxBytes <= 0) return '';
  // perf: a UTF-16 unit is at most three bytes, so most strings fit unmeasured.
  if (s.length * 3 <= maxBytes) return s;
  let bytes = 0;
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    let size = 3;
    let step = 1;
    if (c < 0x80) size = 1;
    else if (c < 0x800) size = 2;
    else if (c >= 0xd800 && c <= 0xdbff && i + 1 < s.length) {
      const lo = s.charCodeAt(i + 1);
      if (lo >= 0xdc00 && lo <= 0xdfff) { size = 4; step = 2; }
    }
    if (bytes + size > maxBytes) return s.substring(0, i);
    bytes += size;
    i += step - 1;
  }
  return s;
}

// ─── Numbers ──────────────────────────────────────────────

/** Not a number, a decimal, an infinity, a NaN — see {@link floatKind}. */
export const FLOAT_NONE = 0;
export const FLOAT_DECIMAL = 1;
export const FLOAT_INF = 2;
export const FLOAT_NAN = 3;

/**
 * What Rust's `f64::from_str` makes of `s`: a sign, then a decimal with an
 * optional exponent (`5`, `5.`, `.5`, `1e5`), or `inf` / `infinity` / `nan`
 * in any ASCII case.
 *
 * safety: a scanner, not a pattern. The expression this replaces
 * (`\d+\.?\d*`) could split one run of digits in two at every position, so a
 * long run followed by a stray character cost the square of its length.
 */
export function floatKind(s: string): number {
  const n = s.length;
  let i = 0;
  const c0 = s.charCodeAt(0);
  if (c0 === 43 || c0 === 45) i = 1;
  if (i >= n) return FLOAT_NONE;
  const rest = n - i;
  if (rest === 3 || rest === 8) {
    const a = s.charCodeAt(i) | 0x20;
    if (a === 105 /* i */ && matchesWord(s, i, rest === 3 ? 'inf' : 'infinity')) return FLOAT_INF;
    if (a === 110 /* n */ && rest === 3 && matchesWord(s, i, 'nan')) return FLOAT_NAN;
  }
  let digits = 0;
  while (i < n) {
    const c = s.charCodeAt(i);
    if (c < 48 || c > 57) break;
    i++;
    digits++;
  }
  if (i < n && s.charCodeAt(i) === 46) {
    i++;
    while (i < n) {
      const c = s.charCodeAt(i);
      if (c < 48 || c > 57) break;
      i++;
      digits++;
    }
  }
  if (digits === 0) return FLOAT_NONE;
  if (i < n && (s.charCodeAt(i) | 0x20) === 101 /* e */) {
    i++;
    const sign = s.charCodeAt(i);
    if (sign === 43 || sign === 45) i++;
    const expStart = i;
    while (i < n) {
      const c = s.charCodeAt(i);
      if (c < 48 || c > 57) break;
      i++;
    }
    if (i === expStart) return FLOAT_NONE;
  }
  return i === n ? FLOAT_DECIMAL : FLOAT_NONE;
}

/** ASCII letters of `s` from `at`, ignoring case, against a lowercase word. */
function matchesWord(s: string, at: number, word: string): boolean {
  for (let k = 0; k < word.length; k++) {
    const c = s.charCodeAt(at + k);
    // Only an ASCII letter folds: `| 0x20` on anything else could invent one.
    if (((c >= 65 && c <= 90) || (c >= 97 && c <= 122) ? c | 0x20 : c) !== word.charCodeAt(k)) return false;
  }
  return true;
}

/** A finite decimal number (`5`, `-0.5`, `1e3`), or `null` for anything else. */
export function finiteDecimal(s: string): number | null {
  if (floatKind(s) !== FLOAT_DECIMAL) return null;
  const n = Number(s);
  return Number.isFinite(n) ? n : null;
}

/** 10^0 … 10^22 — every one of them an exact double. */
const POW10 = [
  1, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11,
  1e12, 1e13, 1e14, 1e15, 1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

/** Spell an exponent-form number out in full: SYNX §8.3 reads only `-?d.d`. */
export function expandExponent(s: string): string {
  // perf: almost every number has no exponent; skip the regex for those.
  if (s.indexOf('e') === -1 && s.indexOf('E') === -1) return s;
  const m = /^([+-]?)(\d+)(?:\.(\d+))?[eE]([+-]?\d+)$/.exec(s);
  if (!m) return s;

  // Expand the exponent in place, so the shortest round-tripping digits are
  // preserved exactly (toFixed would emit the full binary expansion instead).
  const sign = m[1] === '-' ? '-' : '';
  const digits = m[2] + (m[3] ?? '');
  const point = m[2].length + parseInt(m[4], 10);
  let out: string;
  if (point <= 0) {
    out = `0.${'0'.repeat(-point)}${digits}`;
  } else if (point >= digits.length) {
    out = digits + '0'.repeat(point - digits.length);
  } else {
    out = `${digits.slice(0, point)}.${digits.slice(point)}`;
  }
  if (out.indexOf('.') !== -1) {
    out = out.replace(/0+$/, '');
    if (out.endsWith('.')) out = out.slice(0, -1);
  }
  return sign + out;
}

// ─── Casting ──────────────────────────────────────────────

/**
 * Casts `raw` under an explicit `(hint)` from the key line — `cast_typed_with`
 * in synx-core. Until 3.7.4 the hint was glued onto the value as `(hint)raw`
 * and `castType` re-detected it, so any value that merely *began* with an
 * identifier in parentheses was taken for a cast: `key (aa)` read as `""`,
 * `key (random:bool)` as a random boolean, and `key(int)` with children or a
 * `|` body lost them. A value is always plain text now; only the key line can
 * carry a hint.
 */
export function castTyped(raw: string, hint: string, nondeterministic = true): SynxValue {
  switch (hint) {
    case 'int': {
      const exact = parseI64(raw);
      if (exact !== null) return exact;
      // spec: the reference `parse::<i64>().unwrap_or(0)` yields 0 for
      // anything that is not an in-range integer literal — `42.99` too.
      return 0;
    }
    case 'float': {
      // spec: the whole value must be a float, as Rust's `parse::<f64>()`
      // reads it; anything else is 0. `parseFloat` took the longest numeric
      // prefix, so `x(float) 2 + 2` was 2 here and 0 in synx-core.
      castFloat = true;
      switch (floatKind(raw)) {
        case FLOAT_DECIMAL: return Number(raw);
        case FLOAT_INF: return raw.charCodeAt(0) === 45 ? -Infinity : Infinity;
        case FLOAT_NAN: return NaN;
        default: return 0;
      }
    }
    // spec: Unicode whitespace, as Rust's `trim` — `true` followed by U+FEFF
    // is not `true`.
    case 'bool': return trimSpace(raw) === 'true';
    case 'string': return raw;
    // Match Rust `rng::random_i64()` as closely as JS can — full safe-integer
    // range including negatives. JS Number caps at 2^53-1 precisely; values
    // beyond that lose precision in any roundtrip with .synxb.
    case 'random':
    case 'random:int':
      if (!nondeterministic) return castType(raw);
      return Math.floor(Math.random() * Number.MAX_SAFE_INTEGER * 2) - Number.MAX_SAFE_INTEGER;
    case 'random:float':
      if (!nondeterministic) return castType(raw);
      return Math.random();
    case 'random:bool':
      if (!nondeterministic) return castType(raw);
      return Math.random() < 0.5;
    // Unknown hint — the bare value is cast as usual (Rust falls back to `cast`).
    default: return castType(raw);
  }
}

/**
 * Cast a raw string value to a JS primitive (SYNX §8.3 automatic casting).
 *
 * Exported (additively) so that the SYNXL reader can reuse the exact same
 * numeric/boolean/quote semantics instead of growing a second caster —
 * SYNXL §8.1 defines inline casting by reference to this function.
 */
export function castType(val: string, _nondeterministic = true): SynxValue {
  return castRange(val, 0, val.length);
}

/**
 * {@link castType} of `text[s, e)` without materialising it first: only a
 * value that stays a string is copied out of the input.
 *
 * SYNX §8.3: a quoted value is its inner text; then `true` / `false` / `null`;
 * then an integer, a decimal float, and a string for everything else.
 */
function castRange(text: string, s: number, e: number): SynxValue {
  const len = e - s;
  if (len === 0) return '';
  const c0 = text.charCodeAt(s);

  // Quoted strings preserve literal value (bypass auto-casting).
  // Per SYNX 3.6 §8.3.1: "<inner>" or '<inner>' with length >= 2 → string of inner text.
  if (len >= 2 && (c0 === 34 || c0 === 39) && text.charCodeAt(e - 1) === c0) {
    return text.substring(s + 1, e - 1);
  }

  if (len === 4) {
    if (c0 === 116 && text.charCodeAt(s + 1) === 114 && text.charCodeAt(s + 2) === 117 && text.charCodeAt(s + 3) === 101) return true;
    if (c0 === 110 && text.charCodeAt(s + 1) === 117 && text.charCodeAt(s + 2) === 108 && text.charCodeAt(s + 3) === 108) return null;
  } else if (len === 5 && c0 === 102 && text.charCodeAt(s + 1) === 97 && text.charCodeAt(s + 2) === 108 &&
             text.charCodeAt(s + 3) === 115 && text.charCodeAt(s + 4) === 101) {
    return false;
  }

  // Number: `-?digits[.digits]`.
  let i = s;
  let negative = false;
  let fc = c0;
  if (fc === 45) { // '-'
    if (len === 1) return '-';
    negative = true;
    i = s + 1;
    fc = text.charCodeAt(i);
  }
  if (fc >= 48 && fc <= 57) {
    // perf: the value is accumulated while it is scanned. Up to 15 digits fit
    // a double exactly, so no string is cut out of the input to convert.
    let mantissa = fc - 48;
    let dotPos = -1;
    let numeric = true;
    for (let j = i + 1; j < e; j++) {
      const ch = text.charCodeAt(j);
      if (ch >= 48 && ch <= 57) {
        mantissa = mantissa * 10 + (ch - 48);
      } else if (ch === 46 && dotPos === -1) {
        dotPos = j;
      } else {
        numeric = false;
        break;
      }
    }
    if (numeric) {
      if (dotPos === -1) {
        // SYNX §8.3: integer, then decimal float, then string. The reference
        // implementation parses into `i64`, so an integer that overflows it is
        // a string here too, and one that merely leaves JavaScript's safe range
        // is kept exact as a `bigint` — `parseInt` would round a 19-digit id to
        // a different id with nothing to signal it.
        if (e - i <= 15) return mantissa === 0 ? 0 : negative ? -mantissa : mantissa;
        const literal = text.substring(s, e);
        return parseI64(literal) ?? literal;
      }
      if (dotPos < e - 1) {
        castFloat = true;
        // The digits before and after the dot as one integer over a power of
        // ten: both exact, so the division rounds once — to the same double
        // `parseFloat` gives.
        if (e - i <= 16) {
          const value = mantissa / POW10[e - 1 - dotPos];
          return negative ? -value : value;
        }
        return parseFloat(text.substring(s, e));
      }
    }
  }

  return text.substring(s, e);
}

// ─── Constraints ──────────────────────────────────────────

/**
 * Parse constraint string like "min:3, max:30, required, type:int".
 *
 * Exported (additively) so that the SYNXL field-list parser can reuse the
 * existing SYNX constraint parser as mandated by SYNXL §5.2.
 */
export function parseConstraints(raw: string): SynxConstraints {
  const constraints: SynxConstraints = {};
  let start = 0;
  while (start < raw.length) {
    let end = raw.indexOf(',', start);
    if (end === -1) end = raw.length;
    const part = trimSpace(raw.substring(start, end));
    start = end + 1;
    if (!part) continue;

    if (part === 'required') {
      constraints.required = true;
    } else if (part === 'readonly') {
      constraints.readonly = true;
    } else {
      const colonIdx = part.indexOf(':');
      if (colonIdx !== -1) {
        const key = trimSpace(part.substring(0, colonIdx));
        const value = trimSpace(part.substring(colonIdx + 1));
        switch (key) {
          // spec: a bound is a finite decimal number or it is absent.
          // `Number()` read `0x10` as 16 and an empty bound as 0, where
          // synx-core has no bound at all.
          case 'min': { const n = finiteDecimal(value); if (n !== null) constraints.min = n; break; }
          case 'max': { const n = finiteDecimal(value); if (n !== null) constraints.max = n; break; }
          case 'type': constraints.type = value; break;
          case 'pattern': constraints.pattern = value; break;
          case 'enum': constraints.enum = value.split('|').slice(0, MAX_CONSTRAINT_ENUM_PARTS); break;
        }
      }
    }
  }
  return constraints;
}

// ─── Declarations ─────────────────────────────────────────

/**
 * Declarations of each object's keys, held beside the tree.
 *
 * safety: they used to live on the object itself, in a hidden `__synx`
 * property. A document key named `__synx` then replaced them — the key
 * vanished from static output, an active parse threw a TypeError, and a group
 * named `__synx` could declare markers the text does not contain. Nothing a
 * document says can reach this map.
 */
const META = new WeakMap<object, SynxMetaMap>();

/** The declarations of an object's keys, or `undefined` when it has none. */
export function metaMapOf(obj: object): SynxMetaMap | undefined {
  return META.get(obj);
}

/**
 * Keys of an object whose value is a float with a whole value (`8080.0`,
 * `(float) 5`).
 *
 * spec: synx-core keeps an int and a float apart, and `8080.0` is not an
 * `int` — `[type:int]` refuses it and `TYPE_ERR` names it `float` (E19). A JS
 * number cannot tell `8080.0` from `8080`, so this port let it through. Only
 * active documents are noted, and documents read on behalf of one: nothing
 * else checks a type.
 */
const INTEGRAL_FLOATS = new WeakMap<object, Set<string>>();

/**
 * Whether a note was ever made.
 *
 * perf: few documents write a whole number as a float, and until one does
 * every question about a note is answered without looking anything up.
 */
let integralFloatsNoted = false;

/** Note (or forget) that `obj[key]` holds a float whose value is whole. */
export function noteIntegralFloat(obj: object, key: string, isFloat: boolean): void {
  if (isFloat) {
    integralFloatsNoted = true;
    let keys = INTEGRAL_FLOATS.get(obj);
    if (keys === undefined) INTEGRAL_FLOATS.set(obj, keys = new Set<string>());
    keys.add(key);
  } else if (integralFloatsNoted) {
    const keys = INTEGRAL_FLOATS.get(obj);
    if (keys !== undefined) keys.delete(key);
  }
}

/** Whether `obj[key]`, a whole number, was read as a float. */
export function isIntegralFloat(obj: object, key: string): boolean {
  if (!integralFloatsNoted) return false;
  const keys = INTEGRAL_FLOATS.get(obj);
  return keys !== undefined && keys.has(key);
}

/** Give `to`, a copy of the object `from`, the notes of `from`. */
export function copyIntegralFloats(from: object, to: object): void {
  if (!integralFloatsNoted) return;
  const keys = INTEGRAL_FLOATS.get(from);
  if (keys !== undefined && keys.size > 0) INTEGRAL_FLOATS.set(to, new Set(keys));
}

/** Whether the last `castTyped` / `castType` returned a float (see INTEGRAL_FLOATS). */
let castFloat = false;

/** Attach declarations to an object the engine built (as `:inherit` does). */
export function setMetaMap(obj: object, map: SynxMetaMap): void {
  META.set(obj, map);
}

/**
 * An empty declarations map. A `Map`, so a key named `toString` finds nothing
 * and the declarations stay in document order — an object would list `10`
 * before `n` whatever the document said.
 */
export function newMetaMap(): SynxMetaMap {
  return new Map<string, SynxMeta>();
}

/**
 * File the declaration of `key` for the object of frame `sp`.
 *
 * perf: the frame remembers its object's map. Looking the map up beside the
 * object for every declared key was the largest single cost of an active parse.
 */
function saveMeta(
  maps: (OrderedMetaMap | null)[],
  sp: number,
  obj: SynxObject,
  key: string,
  markers: string[],
  args: string[],
  constraints: SynxConstraints | undefined,
  typeHint: string | undefined,
): void {
  let metaMap = maps[sp];
  if (metaMap === null) {
    metaMap = newMetaMap() as OrderedMetaMap;
    metaMap.filed = ++metaCount;
    META.set(obj, metaMap);
    maps[sp] = metaMap;
  }
  // The shared empty list must never escape into metadata a caller could mutate.
  // perf: one shape for every declaration — the engine reads these in a loop.
  metaMap.set(key, {
    markers: markers === NO_MARKERS ? [] : markers,
    args: args.length > 0 ? args : undefined,
    constraints,
    typeHint,
    bits: 0,
  });
}

/** A declarations map the parser made, numbered in the order it made them. */
type OrderedMetaMap = SynxMetaMap & { filed?: number };

let metaCount = 0;

/**
 * When the parser first filed a declaration in this map, among all the maps
 * it made — the order `.synxb` lists the declarations of a document in.
 */
export function metaOrderOf(map: SynxMetaMap): number {
  return (map as OrderedMetaMap).filed ?? 0;
}

// ─── Key order ────────────────────────────────────────────

/**
 * The keys of an object in the order the document declares them, kept only
 * for the objects where JavaScript's own order differs.
 *
 * spec: an object lists its integer-like keys first, in numeric order, then
 * the rest (`n 1` / `10 a` / `2 b` reads back as `2, 10, n`). The data is the
 * same either way, but active mode resolves keys in document order:
 * `x:calc 2 + 3` / `1:calc x * 2` computed `1` before `x` existed.
 */
const KEY_ORDER = new WeakMap<object, string[]>();

/** Document order of an object's keys, or `undefined` when it is `Object.keys` order. */
export function keyOrderOf(obj: object): string[] | undefined {
  return KEY_ORDER.get(obj);
}

/** Record the document order of an object the engine built. */
export function setKeyOrder(obj: object, order: string[]): void {
  KEY_ORDER.set(obj, order);
}

/** The keys of an object in document order. */
export function keysOf(obj: SynxObject): string[] {
  return KEY_ORDER.get(obj) ?? Object.keys(obj);
}

/**
 * Whether `key` is one JavaScript moves to the front of an object: a
 * canonical array index, `0` … `4294967294`.
 */
export function isIndexKey(key: string): boolean {
  const n = key.length;
  if (n === 0 || n > 10) return false;
  const c0 = key.charCodeAt(0);
  if (c0 < 48 || c0 > 57) return false;
  if (c0 === 48) return n === 1;
  for (let i = 1; i < n; i++) {
    const c = key.charCodeAt(i);
    if (c < 48 || c > 57) return false;
  }
  return n < 10 || key <= '4294967294';
}

/**
 * Note that `key` is about to be stored in `obj`, the object of frame `sp`,
 * once that object needs its order kept: from its first index key on.
 */
function noteKeyOrder(orders: (string[] | null)[], sp: number, obj: SynxObject, key: string): void {
  let order = orders[sp];
  if (order === null) {
    if (!isIndexKey(key)) return;
    // Until now the object's own order was the document's.
    order = Object.keys(obj);
    orders[sp] = order;
    KEY_ORDER.set(obj, order);
  }
  if (!Object.prototype.hasOwnProperty.call(obj, key)) order.push(key);
}

/** Shared empty marker list — never mutated, saves an array per key line. */
const NO_MARKERS: string[] = [];

/** Roots of documents that declare `!lock`. */
const LOCKED = new WeakSet<object>();

/**
 * Record that a parsed document is locked (`!lock`).
 *
 * The hidden `__synx_locked` property is kept for code that reads it, unless
 * the document has a key of that name: the property used to replace it, and
 * the key disappeared from the data.
 */
export function markLocked(root: SynxObject): void {
  LOCKED.add(root);
  if (!Object.prototype.hasOwnProperty.call(root, '__synx_locked')) {
    Object.defineProperty(root, '__synx_locked', {
      value: true,
      enumerable: false,
      writable: false,
      configurable: false,
    });
  }
}

/** Whether `obj` is the root of a document parsed with `!lock`. */
export function isLockedRoot(obj: object): boolean {
  if (LOCKED.has(obj)) return true;
  // A document parsed by another copy of this package carries only the property.
  const flag = Object.getOwnPropertyDescriptor(obj, '__synx_locked');
  return flag !== undefined && flag.enumerable === false && flag.value === true;
}

// ─── Keys ─────────────────────────────────────────────────

/** Keys that would reach a JS consumer's prototype chain instead of its data. */
function isReservedKey(key: string): boolean {
  return key === '__proto__' || key === 'constructor' || key === 'prototype';
}

/**
 * Store `value` under `key`.
 *
 * perf: every write of a document key goes through this one small function.
 * Written in place, `parent[key] = …` sat in the middle of the line loop, and
 * JavaScriptCore compiled it for the one key it had last seen: the next key
 * failed the check, the whole loop fell back to baseline code, and after a
 * hundred such exits it was compiled again the same way — in Bun the parser
 * never reached the optimising tiers. Here the guess can only cost this
 * function.
 */
function setOwn(obj: SynxObject, key: string, value: SynxValue): void {
  obj[key] = value;
}

const KEY_SLOTS = 2048;
/** Longest key kept in the table; a longer one is cut from the input each time. */
const KEY_MAX_LENGTH = 40;
/** New keys one parse may add: a document of all-different keys stops paying for the table. */
const KEY_MISS_BUDGET = 512;

/** Keys seen before, by hash; a collision simply replaces the entry. */
const KEY_TABLE: string[] = new Array(KEY_SLOTS).fill('');
let keyMisses = 0;

/**
 * The key `text[a, b)` — the same string object for the same key every time.
 *
 * perf: a document says the same keys over and over (`host`, `port`, the
 * fields of every list item), and so does the next document. A key cut fresh
 * from the input has to be hashed and looked up in the engine's own string
 * table on every write; the string already in this table is that entry.
 *
 * Memory: an entry is built character by character, never cut from the input,
 * so the table holds at most `KEY_SLOTS` short strings and no document.
 */
function internKey(text: string, a: number, b: number, hash: number): string {
  const len = b - a;
  if (len > KEY_MAX_LENGTH) return text.substring(a, b);
  const slot = (hash ^ (hash >>> 11)) & (KEY_SLOTS - 1);
  const known = KEY_TABLE[slot];
  if (known.length === len && text.startsWith(known, a)) return known;
  if (keyMisses >= KEY_MISS_BUDGET) return text.substring(a, b);
  keyMisses++;
  let key = '';
  for (let i = a; i < b; i++) key += text[i];
  KEY_TABLE[slot] = key;
  return key;
}

function hashRange(text: string, a: number, b: number): number {
  let h = 0;
  for (let i = a; i < b; i++) h = (Math.imul(h, 31) + text.charCodeAt(i)) | 0;
  return h;
}

// ─── Line scanners ────────────────────────────────────────

// The key line found by the last `scanKeyLine`.
let kKeyEnd = 0;
let kTypeHint: string | undefined;
let kConstraints: string | undefined;
let kMarkers: string | undefined;
let kValueStart = 0;

/**
 * Scan the key line `text[a, e)`, supporting balanced brackets in
 * `[constraints]`: key, optional `(type)`, optional `[constraints]`, optional
 * `:markers`, then the value. The parts are left in the `k…` variables;
 * `false` when the text cannot start a key (§7).
 */
function scanKeyLine(text: string, a: number, e: number): boolean {
  if (a >= e) return false;
  const first = text.charCodeAt(a);
  // First char cannot be [, :, -, #, /, ( per §7
  if (first === 91 || first === 58 || first === 45 || first === 35 || first === 47 || first === 40) return false;

  // Extract key — stop at space, tab, [, :, (
  let pos = a;
  while (pos < e) {
    const ch = text.charCodeAt(pos);
    if (ch === 32 || ch === 9 || ch === 91 || ch === 58 || ch === 40) break;
    pos++;
  }
  if (pos === a) return false;
  kKeyEnd = pos;
  kTypeHint = undefined;
  kConstraints = undefined;
  kMarkers = undefined;

  // Optional (type)
  if (pos < e && text.charCodeAt(pos) === 40) { // '('
    let close = pos + 1;
    while (close < e && text.charCodeAt(close) !== 41) close++;
    if (close < e) {
      kTypeHint = text.substring(pos + 1, close);
      pos = close + 1;
    } else {
      pos++;
    }
  }

  // Optional [constraints] — balanced bracket scan to support patterns like ^[A-Z]{2}$
  if (pos < e && text.charCodeAt(pos) === 91) { // '['
    pos++;
    const cStart = pos;
    let depth = 1;
    while (pos < e && depth > 0) {
      const ch = text.charCodeAt(pos);
      if (ch === 91) depth++;
      else if (ch === 93) depth--;
      if (depth > 0) pos++;
    }
    if (depth === 0) {
      kConstraints = text.substring(cStart, pos);
      pos++; // skip closing ']'
    } else {
      // Unbalanced — fallback to first ']' if any
      let fb = cStart;
      while (fb < e && text.charCodeAt(fb) !== 93) fb++;
      if (fb < e) {
        kConstraints = text.substring(cStart, fb);
        pos = fb + 1;
      } else {
        kConstraints = text.substring(cStart, e);
        pos = e;
      }
    }
  }

  // Optional :markers
  if (pos < e && text.charCodeAt(pos) === 58) { // ':'
    const mStart = pos + 1;
    let mEnd = mStart;
    while (mEnd < e) {
      const ch = text.charCodeAt(mEnd);
      if (ch === 32 || ch === 9) break;
      mEnd++;
    }
    kMarkers = text.substring(mStart, mEnd);
    pos = mEnd;
  }

  // Skip whitespace; the rest is the value
  while (pos < e) {
    const ch = text.charCodeAt(pos);
    if (ch !== 32 && ch !== 9) break;
    pos++;
  }
  kValueStart = pos;
  return true;
}

// What `scanDeclaration` adds to the `k…` variables.
let dMarkers: string[] = NO_MARKERS;
let dMarkerArgs: string[] = NO_MARKERS;
let dValueStop = 0;

/**
 * Scan the key line `text[a, e)` as a whole declaration: the key and its
 * `(type)`, `[constraints]` and `:markers` through {@link scanKeyLine}, then
 * the value, less an inline comment and less what a marker takes for itself.
 *
 * One routine for a key on its own line and for the key of a `- ` line: the
 * second used to be read by hand, so its markers were dropped and
 * `- token:secret abc` stayed in the clear.
 */
function scanDeclaration(text: string, a: number, e: number): boolean {
  if (!scanKeyLine(text, a, e)) return false;
  let markers = NO_MARKERS;
  let args = NO_MARKERS;
  // Parse markers chain (with segment cap)
  if (kMarkers) {
    markers = kMarkers.split(':');
    if (markers.length > MAX_MARKER_CHAIN_SEGMENTS) markers.length = MAX_MARKER_CHAIN_SEGMENTS;
  }
  let stop = kValueStart < e ? valueEnd(text, kValueStart, e) : kValueStart;
  if (markers !== NO_MARKERS && kValueStart < stop) {
    // For :random — the value may hold weight percentages
    if (markers.includes('random')) {
      const nums = splitSpace(text.substring(kValueStart, stop)).filter(isWeight);
      if (nums.length > 0) {
        args = nums;
        stop = kValueStart;
      }
    }
    // For :inherit — a non-empty value is the parent-key name, not a scalar value.
    // Promote it into the arguments and clear the value so the line opens a group.
    if (kValueStart < stop && markers.includes('inherit')) {
      args = [trimSpace(text.substring(kValueStart, stop))];
      stop = kValueStart;
    }
  }
  dMarkers = markers;
  dMarkerArgs = args;
  dValueStop = stop;
  return true;
}

/**
 * Where the value `text[s, e)` ends once an inline comment is cut off: at the
 * first ` #` or ` //`, less the whitespace before it.
 *
 * perf: one pass over the value, inside the line. Searching the whole input
 * for ` #` from every value read the rest of the document each time.
 */
function valueEnd(text: string, s: number, e: number): number {
  let cut = e;
  for (let i = s; i < e - 1; i++) {
    if (text.charCodeAt(i) === 32) {
      const next = text.charCodeAt(i + 1);
      if (next === 35 || (next === 47 && i + 2 < e && text.charCodeAt(i + 2) === 47)) {
        cut = i;
        break;
      }
    }
  }
  while (cut > s) {
    const c = text.charCodeAt(cut - 1);
    if (c === 32 || c === 9 || ((c < 32 || c > 127) && isUnicodeSpaceCode(c))) cut--;
    else break;
  }
  return cut;
}

/** `keyLineShape` of a line that is not a key line, or whose key is refused. */
export const SHAPE_NOT_A_KEY = -1;
/** The line has no value: it opens a group or a list. */
export const SHAPE_EMPTY = 1;
/** The value is `|`: the line opens a block. */
export const SHAPE_BLOCK = 2;
/** The value is `|+`: a block that keeps its indentation. */
export const SHAPE_BLOCK_KEEP = 4;
/** A marker of the line makes its key a list (`random`, `unique`, `geo`, `join`). */
export const SHAPE_LIST = 8;

/**
 * What the key line `text[a, e)` opens, as this parser reads it — for the
 * formatter, which must see every line the way the parser does.
 */
export function keyLineShape(text: string, a: number, e: number): number {
  if (!scanDeclaration(text, a, e)) return SHAPE_NOT_A_KEY;
  const keyLen = kKeyEnd - a;
  if ((keyLen === 9 || keyLen === 11) && isReservedKey(text.substring(a, kKeyEnd))) return SHAPE_NOT_A_KEY;
  let shape = 0;
  const valueLen = dValueStop - kValueStart;
  if (valueLen === 0) shape = SHAPE_EMPTY;
  else if (text.charCodeAt(kValueStart) === 124) {
    if (valueLen === 1) shape = SHAPE_BLOCK;
    else if (valueLen === 2 && text.charCodeAt(kValueStart + 1) === 43) shape = SHAPE_BLOCK_KEEP;
  }
  if (dMarkers !== NO_MARKERS &&
      (dMarkers.includes('random') || dMarkers.includes('unique') || dMarkers.includes('geo') || dMarkers.includes('join'))) {
    shape |= SHAPE_LIST;
  }
  return shape;
}

/** A `:random` weight: digits, optionally a dot and more digits. */
function isWeight(s: string): boolean {
  const n = s.length;
  let i = 0;
  while (i < n && s.charCodeAt(i) >= 48 && s.charCodeAt(i) <= 57) i++;
  if (i === 0) return false;
  if (i === n) return true;
  if (s.charCodeAt(i) !== 46) return false;
  const frac = ++i;
  while (i < n && s.charCodeAt(i) >= 48 && s.charCodeAt(i) <= 57) i++;
  return i === n && i > frac;
}

// The line found by the last `peekNextContent` call.
/** UTF-8 indent width of the line. */
let peekIndent = 0;
/** Whether the line, trimmed, starts with `- ` — a list item. */
let peekIsItem = false;
/** Set by `rootKeyLines`: receives the line of each key that lands at the root. */
let rootLineSink: Map<string, number> | null = null;

/**
 * The 0-based line of `text` that declares each key landing at the root of the
 * parsed document; the first declaration of a key wins.
 *
 * SYNXL §11.2 reports a block diagnostic on the line inside the block, and
 * which keys reach the root is the parser's own decision (stack repair, list
 * items, multiline bodies) — re-deriving it from indentation got it wrong.
 */
export function rootKeyLines(text: string, options?: ParseDataOptions): Map<string, number> {
  const sink = new Map<string, number>();
  rootLineSink = sink;
  try {
    parseData(text, options);
  } finally {
    rootLineSink = null;
  }
  return sink;
}

/**
 * Find the next line of content — not blank and not a comment — starting at
 * offset `from` (line number `lineNo`). Returns the offset of its first
 * character, or -1; its indent and whether it is a list item are left in
 * `peekIndent` and `peekIsItem`.
 * Lines past MAX_LINE_STARTS are not considered, as if the text ended there.
 *
 * spec: a comment line is ignored (§5), so it cannot stand between a key and
 * its list or between a `- ` line and the keys of its item. Only blank lines
 * were skipped here: `items` / `# note` / `- a` read as an empty object and
 * lost every item, and a comment after `- name a` turned the item into the
 * text `name a` and sent its keys to the parent. As `next_content_line` in
 * synx-core.
 */
function peekNextContent(text: string, from: number, lineNo: number): number {
  const len = text.length;
  let p = from;
  let n = lineNo;
  let inBlockComment = false;
  while (p <= len && n < MAX_LINE_STARTS) {
    // spec: the same whitespace the main loop measures indent with. Space,
    // tab and CR alone took a line of U+00A0 for content (Rust: blank) and
    // measured a U+3000-indented line shallower than the loop then did.
    let q = p;
    let width = 0;
    let c = 0;
    while (q < len) {
      c = text.charCodeAt(q);
      if (c === 32 || c === 9) width++;
      else if (c === 10) break;
      else if ((c < 32 || c > 127) && isUnicodeSpaceCode(c)) width += wsUtf8Width(c);
      else break;
      q++;
    }
    if (q >= len) return -1;
    if (c === 10) {
      p = q + 1;
      n++;
      continue;
    }
    let e = text.indexOf('\n', q);
    if (e === -1) e = len;
    if (c === 35 || inBlockComment || (c === 47 && text.charCodeAt(q + 1) === 47)) {
      // A comment. `###` alone on its line opens or closes a block of them.
      if (c === 35 && text.charCodeAt(q + 1) === 35 && text.charCodeAt(q + 2) === 35) {
        let t = e;
        while (t > q && isUnicodeSpaceCode(text.charCodeAt(t - 1))) t--;
        if (t - q === 3) inBlockComment = !inBlockComment;
      }
      p = e + 1;
      n++;
      continue;
    }
    peekIndent = width;
    // A trimmed line cannot end in a space, so `- ` has an item after it.
    peekIsItem = false;
    if (c === 45 && text.charCodeAt(q + 1) === 32) {
      let t = e;
      while (t > q && isUnicodeSpaceCode(text.charCodeAt(t - 1))) t--;
      peekIsItem = t - q > 2;
    }
    return q;
  }
  return -1;
}

/**
 * `parent[key]` is being replaced: the open lists that key held now hold
 * `list` — the new value when it is a list, else `null`, and items have
 * nowhere to go.
 *
 * perf: kept beside the line loop. The loop used to read `parent[key]` for
 * every item; JavaScriptCore compiled that read for one key, and a list
 * inside a list — two keys in turn — sent the whole loop back to the
 * baseline tier on every other item.
 */
function unlinkLists(
  parents: SynxObject[], keys: string[], arrays: (SynxArray | null)[], open: number,
  parent: SynxObject, key: string, list: SynxArray | null,
): void {
  for (let i = 0; i < open; i++) {
    if (parents[i] === parent && keys[i] === key) arrays[i] = list;
  }
}

// ─── Parser ───────────────────────────────────────────────

/** Options for {@link parseData}. Additive — omitting them keeps 3.7 behaviour. */
export interface ParseDataOptions {
  /**
   * When `false`, `!`-prefixed lines are NOT interpreted as SYNX directives:
   * they set no mode flag and record no include. Outside a multiline block such
   * a line is discarded like a comment; **inside** a `|` / `|+` body it stays
   * ordinary content, because the multiline context is known at this point.
   *
   * Used by the SYNXL reader for block delegation (SYNXL §9.4) — a dataset row
   * must never become a file-read or mode-change primitive. Defaults to `true`.
   */
  directives?: boolean;

  /**
   * Honour the `random`, `random:int`, `random:float` and `random:bool` type
   * hints. Default `true`.
   *
   * SYNXL block delegation sets this to `false` (SYNXL §8.3): a hint carried by
   * dataset data would make the §12 projection differ between two reads of the
   * same bytes, which defeats hashing, de-duplication and the byte-identical
   * projection §1.4 is measured on.
   */
  nondeterministicHints?: boolean;

  /**
   * Note whole-valued floats (`8080.0`) even when the document is static.
   * The engine asks for it when it reads a document on behalf of an active
   * one — an include, a watched file — whose fields the types of the active
   * document then check.
   */
  floats?: boolean;
}

/** Text of an open `|` block: its lines, and how many bytes they come to. */
interface OpenBlock {
  indent: number;
  obj: SynxObject;
  key: string;
  /** SYNX 3.7 `|+`: keep each line's indent relative to the first. */
  preserveIndent: boolean;
  /** Indent of the first line of a `|+` body, -1 until it is seen. */
  baseIndent: number;
  parts: string[];
  /** UTF-8 bytes of `parts` joined with `\n`; valid once `counted`. */
  byteLen: number;
  /** Upper bound of `byteLen` while the exact count is not needed yet. */
  byteBound: number;
  counted: boolean;
}

/** Set when the parse in progress dropped input to a limit. */
let inputDropped = false;

/** Append one line to an open block, within MAX_MULTILINE_BLOCK_BYTES. */
function appendBlockLine(block: OpenBlock, line: string): void {
  if (!block.counted) {
    // perf: a UTF-16 unit is at most three bytes. While that bound stays
    // under the cap the line is taken whole and nothing is measured — the
    // exact count walked every character of every block line.
    const bound = block.byteBound + line.length * 3 + 1;
    if (bound <= MAX_MULTILINE_BLOCK_BYTES) {
      block.byteBound = bound;
      block.parts.push(line);
      return;
    }
    let bytes = block.parts.length > 0 ? block.parts.length - 1 : 0;
    for (const part of block.parts) bytes += utf8Length(part);
    block.byteLen = bytes;
    block.counted = true;
  }
  if (block.byteLen >= MAX_MULTILINE_BLOCK_BYTES) {
    inputDropped = true;
    return;
  }
  const sepBytes = block.parts.length ? 1 : 0; // the '\n' join adds
  const room = MAX_MULTILINE_BLOCK_BYTES - block.byteLen - sepBytes;
  if (room > 0) {
    // Byte budget, UTF-8 boundary: `substring(0, room)` counts UTF-16
    // units, so it both overshoots the cap and can split a surrogate
    // pair, leaving a lone surrogate no strict JSON consumer accepts.
    const taken = truncateUtf8Prefix(line, room);
    block.parts.push(taken);
    block.byteLen += sepBytes + utf8Length(taken);
    if (taken.length < line.length) inputDropped = true;
  } else {
    // The line break alone reached the cap, as in synx-core.
    block.parts.push('');
    block.byteLen += sepBytes;
  }
}

/**
 * The input, less a leading BOM, cut to MAX_SYNX_INPUT_BYTES of UTF-8 at a
 * character boundary — `clamp_synx_text` in synx-core.
 */
export function clampInput(text: string): string {
  // A single leading U+FEFF is dropped (SYNX §2.1), as in synx-core. `trim()`
  // already hid it in front of `!active`, but a static document kept it glued
  // to its first key.
  if (text.charCodeAt(0) === 0xfeff) text = text.slice(1);
  // spec: the cap is in bytes. Counted in UTF-16 units, a document of
  // three-byte characters was read up to 48 MiB here and 16 MiB in synx-core.
  if (text.length * 3 > MAX_SYNX_INPUT_BYTES) text = truncateUtf8Prefix(text, MAX_SYNX_INPUT_BYTES);
  return text;
}

export function parseData(text: string, options?: ParseDataOptions): SynxParseResult {
  const directivesEnabled = options === undefined || options.directives !== false;
  const nondeterministic = options === undefined || options.nondeterministicHints !== false;
  const floatsWanted = options !== undefined && options.floats === true;

  // A leading BOM is dropped, not lost content; anything else cut is.
  const givenLength = text.length - (text.charCodeAt(0) === 0xfeff ? 1 : 0);
  text = clampInput(text);
  inputDropped = text.length < givenLength;
  keyMisses = 0;

  const root: SynxObject = {};
  // The frame stack as columns: the indent of each open object, the object,
  // whether it is a list item, and the document order of its keys once that
  // has to be kept (see `noteKeyOrder`).
  const stackIndent: number[] = [-1];
  const stackObj: SynxObject[] = [root];
  const stackItem: boolean[] = [false];
  const stackOrder: (string[] | null)[] = [null];
  const stackMeta: (OrderedMetaMap | null)[] = [null];
  let sp = 0;

  let active = false;
  let locked = false;
  let llm = false;
  let tool = false;
  let schema = false;
  let includes: SynxInclude[] | undefined;
  let block: OpenBlock | null = null;
  // Open lists, outermost first, as three columns: the indent of the list's
  // key, and the object and key that hold it — a later line can replace the
  // key, and items after that must not go into the detached array.
  //
  // spec: a list inside a list item ends at the next item of the outer list
  // (§8.5). One open list was kept, so a nested `tags` replaced its parent:
  // `- name b` after it matched no list and was dropped without a word.
  const listIndent: number[] = [];
  const listParent: SynxObject[] = [];
  const listKey: string[] = [];
  // The list itself, or `null` once its key holds something else.
  const listArr: (SynxArray | null)[] = [];
  let lists = 0;
  let inBlockComment = false;
  // Indent of a rejected key whose block is being dropped; -1 when none.
  let droppedBelow = -1;
  // A whole-valued float on this line, and whether any was noted (see INTEGRAL_FLOATS).
  let lineFloat = false;
  let anyIntegralFloat = false;

  const textLen = text.length;
  let pos = 0;
  let lineNo = 0;
  while (pos <= textLen && lineNo < MAX_LINE_STARTS) {
    const ls = pos;
    lineNo++;

    // ── Indent ──
    //
    // Unicode-aware, matching SYNX §4's `length(raw) - length(ltrim(raw))`: a
    // line opening with U+00A0 or U+3000 is indented, so it stays block content
    // instead of turning into a spurious key.
    // spec: §4 measures `raw` as UTF-8, so U+00A0 is 2 and U+3000 is 3, as in
    // synx-core. Counting UTF-16 units made a tab and U+00A0 the same depth here
    // and different depths in Rust, and the two built different trees.
    let a = ls;
    let indent = 0;
    let fc = 10; // first character that is not whitespace; LF for a blank line
    while (a < textLen) {
      fc = text.charCodeAt(a);
      if (fc === 32 || fc === 9) indent++;
      else if (fc === 10) break;
      else if ((fc < 32 || fc > 127) && isUnicodeSpaceCode(fc)) indent += wsUtf8Width(fc);
      else break;
      a++;
    }

    // ── Empty line ──
    if (a >= textLen || fc === 10) {
      pos = a + 1;
      continue;
    }

    let le = text.indexOf('\n', a);
    if (le === -1) le = textLen;
    pos = le + 1;

    // ── Trimmed range [a, te) — trailing Unicode whitespace ──
    // spec: §4 trims both ends of Unicode whitespace; a trailing U+00A0 stayed
    // in the key or value here (`v` + U+00A0) and was trimmed in Rust.
    let te = le;
    while (te > a) {
      const ch = text.charCodeAt(te - 1);
      if (ch === 32 || ch === 9 || ch === 13 || ((ch < 32 || ch > 127) && isUnicodeSpaceCode(ch))) te--;
      else break;
    }

    // safety: the lines under a rejected key go with it, as in synx-core. Only
    // the key line was skipped, so its block fell to whatever was open above
    // it — `profile` / `__proto__` / `  role admin` gave `profile` the role.
    if (droppedBelow >= 0) {
      if (indent > droppedBelow) continue;
      droppedBelow = -1;
    }

    // ── Continue multiline block ──
    //
    // Ahead of every line-class rule below, and deliberately so: a `|` / `|+`
    // body is raw text (SYNX §8.4.1), so `#`, `//`, `###` and `!` lines inside
    // one are content. Classifying first deletes comments out of code samples,
    // lets a stray `###` swallow the rest of the document, and — for `!` —
    // turns a data row into a directive, which SYNXL §9.4 forbids outright.
    if (block !== null) {
      if (indent > block.indent) {
        let from = a;
        if (block.preserveIndent) {
          // SYNX 3.7 `|+`: lock the base indent to the FIRST non-empty
          // continuation line, then strip exactly that prefix from every line.
          // Anything indented further than the base is preserved verbatim.
          if (block.baseIndent < 0) block.baseIndent = indent;
          const strip = Math.min(indent, block.baseIndent);
          // `strip` is UTF-8 bytes; whole whitespace characters are cut until
          // it is reached, as synx-core's `strip_boundary` does.
          from = ls;
          for (let w = 0; w < strip && from < a;) {
            const c = text.charCodeAt(from);
            w += c === 32 || c === 9 ? 1 : wsUtf8Width(c);
            from++;
          }
        }
        appendBlockLine(block, text.substring(from, te));
        continue;
      }
      setOwn(block.obj, block.key, block.parts.join('\n'));
      block = null;
    }

    if (fc === 35) { // #
      // ── Block comment toggle: ### ──
      if (te - a === 3 && text.charCodeAt(a + 1) === 35 && text.charCodeAt(a + 2) === 35) {
        inBlockComment = !inBlockComment;
        continue;
      }
      if (inBlockComment) continue;
      // Legacy pragma: #!mode:active / #!mode:static — anything but `active`,
      // an empty value too, is static.
      if (directivesEnabled && te - a >= 7 && text.charCodeAt(a + 1) === 33 && text.startsWith('#!mode:', a)) {
        let d = a + 7;
        while (d < te && isUnicodeSpaceCode(text.charCodeAt(d))) d++;
        active = te - d === 6 && text.startsWith('active', d);
      }
      continue;
    }
    if (inBlockComment) continue;

    // ── Directives: !active / !lock / !llm / !tool / !schema / !include ──
    if (fc === 33) {
      if (!directivesEnabled) {
        // Directives disabled (SYNXL §9.4): a `!` line reaching here is not
        // multiline body — the branch above already consumed and preserved
        // those — so it is discarded exactly like a comment.
        continue;
      }
      const trimmed = text.substring(a, te);
      if (trimmed === '!active') { active = true; continue; }
      if (trimmed === '!lock') { locked = true; continue; }
      if (trimmed === '!llm') { llm = true; continue; }
      if (trimmed === '!tool') { tool = true; continue; }
      if (trimmed === '!schema') { schema = true; continue; }
      if (trimmed.startsWith('!include ')) {
        if (includes === undefined) includes = [];
        if (includes.length < MAX_INCLUDE_DIRECTIVES) {
          // The path runs to the first whitespace; whatever follows, trimmed,
          // is the alias — spaces and all, as in synx-core.
          const rest = trimSpace(trimmed.substring(9));
          let cut = 0;
          while (cut < rest.length && !isUnicodeSpaceCode(rest.charCodeAt(cut))) cut++;
          const inclPath = rest.substring(0, cut);
          includes.push({
            path: inclPath,
            alias: cut < rest.length ? trimSpace(rest.substring(cut + 1)) : includeAlias(inclPath),
          });
        }
        continue;
      }
      // `!use @scope/pkg [as alias]` is a directive, not data. The Rust
      // reference (parser.rs) consumes it; without this branch it fell through
      // to key-line parsing and surfaced as a literal `"!use"` key — the
      // divergence conformance case 012-use-directive catches.
      if (trimmed.startsWith('!use ')) continue;
      // Any other `!` line is a key line.
    }

    // ── Comment: // ──
    if (fc === 47 && a + 1 < le && text.charCodeAt(a + 1) === 47) continue;

    // ── List items: '- ' ──
    if (fc === 45 && te - a > 1 && text.charCodeAt(a + 1) === 32) {
      // The list this item belongs to: the innermost open one whose key sits
      // to the left of the dash. Lists nested deeper than the dash end here —
      // but only when a list does take the line; a dash no list accepts is
      // skipped and closes nothing.
      let accepting = lists;
      while (accepting > 0 && indent < listIndent[accepting - 1]) accepting--;
      if (accepting > 0 && indent > listIndent[accepting - 1]) lists = accepting;
      // Not in list context — skip (no key-line starting with '- ' is valid)
      if (lists === 0 || indent <= listIndent[lists - 1]) continue;
      // The previous item's frames end here, whatever this item turns out
      // to be. Without this a scalar item left an object item's frame on
      // the stack, and the next deeper key landed inside that old item.
      // spec: popped before the item is checked, as in synx-core — an item
      // that is dropped (its list key was replaced, or the list is full)
      // still closes the frames above it.
      while (sp > 0 && stackIndent[sp] >= indent) sp--;
      // The list as its key holds it now, as synx-core looks it up per item:
      // once the key was replaced by a non-list, items have nowhere to go.
      const arr = listArr[lists - 1];
      if (arr === null) continue;
      if (arr.length >= MAX_LIST_ITEMS) {
        // The cap dropped an item: say so, like every other cap.
        inputDropped = true;
        continue;
      }

      let itemStart = a + 2;
      while (itemStart < te && isUnicodeSpaceCode(text.charCodeAt(itemStart))) itemStart++;
      const itemEnd = valueEnd(text, itemStart, te);

      // An item is an object when the next line of content is deeper and is
      // not an item itself.
      if (peekNextContent(text, pos, lineNo) !== -1 && peekIndent > indent && !peekIsItem) {
        const itemObj: SynxObject = {};
        let declared = false;
        let head = '';
        if (scanDeclaration(text, itemStart, itemEnd)) {
          head = internKey(text, itemStart, kKeyEnd, hashRange(text, itemStart, kKeyEnd));
          // safety: the same guard as on a key line — `- __proto__` set the
          // prototype of the item, and `- constructor x` stayed in it.
          if (!isReservedKey(head)) {
            // A typed key casts even an empty value; an untyped one without a
            // value is an empty group — as synx-core reads the `- ` line.
            castFloat = false;
            const headValue = kTypeHint !== undefined ? castTyped(text.substring(kValueStart, dValueStop), kTypeHint, nondeterministic)
              : kValueStart < dValueStop ? castRange(text, kValueStart, dValueStop)
              : {};
            setOwn(itemObj, head, headValue);
            if (castFloat && (active || floatsWanted) && typeof headValue === 'number' && Number.isInteger(headValue)) {
              noteIntegralFloat(itemObj, head, true);
              anyIntegralFloat = true;
            }
            declared = active && (dMarkers !== NO_MARKERS || kConstraints !== undefined || kTypeHint !== undefined);
          }
        } else {
          itemObj._value = castRange(text, itemStart, itemEnd);
        }
        arr.push(itemObj);
        // The depth cap holds for an item's frame too, as in synx-core; an
        // item that gets no frame gets no declarations either.
        if (sp + 1 < MAX_PARSE_NESTING_DEPTH) {
          sp++;
          stackIndent[sp] = indent;
          stackObj[sp] = itemObj;
          stackItem[sp] = true;
          stackOrder[sp] = null;
          stackMeta[sp] = null;
          if (head !== '' && isIndexKey(head)) {
            const order = [head];
            stackOrder[sp] = order;
            KEY_ORDER.set(itemObj, order);
          }
          // spec: the key on the `- ` line is a key line like the ones under
          // it. Its markers and constraints were read and thrown away:
          // `- token:secret abc` stayed in the clear and `- host:env H` kept
          // the variable's name.
          if (declared) {
            saveMeta(stackMeta, sp, itemObj, head, dMarkers, dMarkerArgs, kConstraints !== undefined ? parseConstraints(kConstraints) : undefined, kTypeHint);
          }
        }
        continue;
      }

      arr.push(castRange(text, itemStart, itemEnd));
      continue;
    }

    // Close every list a non-item line is at or below the indent of, and with
    // them the item frames at or above it — whether or not the line turns out
    // to be a key, as in synx-core.
    if (lists > 0 && indent <= listIndent[lists - 1]) {
      do lists--; while (lists > 0 && indent <= listIndent[lists - 1]);
      while (sp > 0 && stackItem[sp] && stackIndent[sp] >= indent) sp--;
    }

    // ── Validate first char can start a key ──
    // SYNX §7: a key cannot open with `[`, `:`, `-`, `#`, `/` or `(`, and such
    // a line is discarded rather than turned into a key — a bare `-` must not
    // become the key `-`, a name no writer can produce.
    if (fc === 91 || fc === 58 || fc === 45 || fc === 47 || fc === 40) continue;

    // ── Key ──
    // One pass finds where the key ends and hashes it for the key table. A
    // space or tab ends a plain `key value` line; `(`, `[` or `:` sends the
    // line to the full scanner.
    let keyEnd = a;
    let hash = 0;
    let special = false;
    while (keyEnd < te) {
      const ch = text.charCodeAt(keyEnd);
      if (ch === 32 || ch === 9) break;
      if (ch === 40 || ch === 91 || ch === 58) { special = true; break; }
      hash = (Math.imul(hash, 31) + ch) | 0;
      keyEnd++;
    }

    let valueStart: number;
    let valueStop: number;
    let typeHint: string | undefined;
    let markers = NO_MARKERS;
    let markerArgs = NO_MARKERS;
    let constraints: SynxConstraints | undefined;
    if (!special) {
      // ── Fast path: simple key-value or section header ──
      valueStart = keyEnd;
      while (valueStart < te) {
        const ch = text.charCodeAt(valueStart);
        if (ch !== 32 && ch !== 9) break;
        valueStart++;
      }
      valueStop = valueStart < te ? valueEnd(text, valueStart, te) : valueStart;
    } else {
      // ── Full scanner for lines with ( [ : ──
      scanDeclaration(text, a, te);
      valueStart = kValueStart;
      valueStop = dValueStop;
      typeHint = kTypeHint;
      markers = dMarkers;
      markerArgs = dMarkerArgs;
      if (active && kConstraints !== undefined) constraints = parseConstraints(kConstraints);
    }

    // ── Reject prototype-polluting keys ──
    //
    // Checked *before* the stack is repaired, matching the reference
    // implementation. Popping first would let `__proto__` at the depth of a
    // real key close that key's subtree, so the lines underneath re-open it
    // and overwrite the sibling the document actually declared — a rejected
    // key must not be able to reshape the record around it.
    const keyLen = keyEnd - a;
    const key = internKey(text, a, keyEnd, hash);
    if ((keyLen === 9 || keyLen === 11) && isReservedKey(key)) {
      droppedBelow = indent;
      continue;
    }

    // ── Pop stack to correct parent ──
    while (sp > 0 && stackIndent[sp] >= indent) sp--;
    const parent = stackObj[sp];
    if (rootLineSink !== null && sp === 0 && !rootLineSink.has(key)) rootLineSink.set(key, lineNo - 1);

    // spec: empty `()` and `[]` are declarations too, as in synx-core — a key
    // declared again with them drops the markers of its first declaration.
    const declared = active && (markers !== NO_MARKERS || constraints !== undefined || typeHint !== undefined);
    // The frame of `parent`: a group opened by this line takes the next one.
    const declaredAt = sp;
    // Only an object with an index key (`10`, `2`) needs its order kept.
    if (stackOrder[sp] !== null || (fc >= 48 && fc <= 57)) noteKeyOrder(stackOrder, sp, parent, key);

    // A line under an open list may replace the very key that holds it.
    if (lists > 0) unlinkLists(listParent, listKey, listArr, lists, parent, key, null);

    // ── Determine what this line creates ──
    if (valueStart === valueStop) {
      if ((markers !== NO_MARKERS &&
           (markers.includes('random') || markers.includes('unique') ||
            markers.includes('geo') || markers.includes('join'))) ||
          // No value → nested object (group) OR plain list: it is a list when
          // the next line of content is an item.
          (peekNextContent(text, pos, lineNo) !== -1 && peekIsItem)) {
        // A list is not a frame: a deeper line that is not an item belongs to
        // the list's parent, as in synx-core. Leaving the array on the stack
        // wrote such keys onto the array as named properties, which no
        // serializer emits — the data vanished.
        const list: SynxArray = [];
        setOwn(parent, key, list);
        // An open list of this same key takes its items into the new one.
        if (lists > 0) unlinkLists(listParent, listKey, listArr, lists, parent, key, list);
        listIndent[lists] = indent;
        listParent[lists] = parent;
        listKey[lists] = key;
        listArr[lists] = list;
        lists++;
      } else {
        const group: SynxObject = {};
        setOwn(parent, key, group);
        // Cap nesting depth: still insert the object but don't push deeper.
        if (sp + 1 < MAX_PARSE_NESTING_DEPTH) {
          sp++;
          stackIndent[sp] = indent;
          stackObj[sp] = group;
          stackItem[sp] = false;
          stackOrder[sp] = null;
          stackMeta[sp] = null;
        }
      }
    } else {
      const first = text.charCodeAt(valueStart);
      const valueLen = valueStop - valueStart;
      if (first === 124 && (valueLen === 1 || (valueLen === 2 && text.charCodeAt(valueStart + 1) === 43))) {
        // Multiline block. `|` (3.6) trims each continuation line; `|+` (3.7)
        // preserves indent relative to the first non-empty continuation line.
        setOwn(parent, key, '');
        block = {
          indent, obj: parent, key,
          preserveIndent: valueLen === 2,
          baseIndent: -1,
          parts: [],
          byteLen: 0,
          byteBound: 0,
          counted: false,
        };
      } else {
        // Simple key-value
        castFloat = false;
        const value = typeHint
          ? castTyped(text.substring(valueStart, valueStop), typeHint, nondeterministic)
          : castRange(text, valueStart, valueStop);
        setOwn(parent, key, value);
        // A float with markers is still a float: `port[type:int]:default:1 8080.0`
        // is refused in synx-core, and the marker that replaces the value
        // notes what it puts there.
        lineFloat = castFloat && (active || floatsWanted) && typeof value === 'number' && Number.isInteger(value);
      }
    }
    if (lineFloat) {
      noteIntegralFloat(parent, key, true);
      lineFloat = false;
      anyIntegralFloat = true;
    } else if (anyIntegralFloat) {
      noteIntegralFloat(parent, key, false);
    }
    if (declared) saveMeta(stackMeta, declaredAt, parent, key, markers, markerArgs, constraints, typeHint);
  }

  // Flush a block still open at end of input (no dedent line closed it).
  if (block !== null) setOwn(block.obj, block.key, block.parts.join('\n'));

  const mode: SynxMode = active ? 'active' : 'static';
  const result: SynxParseResult = { root, mode, locked };
  if (llm) result.llm = true;
  if (tool) result.tool = true;
  if (schema) result.schema = true;
  result.includes = includes !== undefined && includes.length > 0 ? includes : undefined;
  // The line cap cut the text when a line break is still ahead of `pos`.
  if (inputDropped || (lineNo >= MAX_LINE_STARTS && pos <= textLen)) result.truncated = true;
  return result;
}

/** Alias of an `!include` without one: the file name less its folder and a `.synx` / `.SYNX` ending. */
function includeAlias(path: string): string {
  const slash = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  const name = slash === -1 ? path : path.substring(slash + 1);
  return name.endsWith('.synx') || name.endsWith('.SYNX') ? name.substring(0, name.length - 5) : name;
}
