/**
 * SYNXL — "SYNX Lines" — @aperturesyndicate/synx-format
 *
 * Record-stream format: the SYNX-native counterpart of JSONL and CSV.
 * Implements SYNXL format version 1 (`docs/spec/SYNXL-1-NORMATIVE.md`),
 * embedding SYNX 3.7 for block fields.
 *
 * A SYNXL document declares a field list once, then carries records:
 *
 * ```synxl
 * !synxl 1
 * !fields id[type:int, required] ; score[type:float] ; messages[block]
 *
 * 1 ; 0.91
 *   messages
 *     - role user
 *       content Hello
 * ```
 *
 * Design notes:
 * - Block fields are parsed by delegation to the existing SYNX parser
 *   (§9.3) — no dedent pre-pass, no second tree builder.
 * - Directives are disabled *inside* the embedded parse (§9.4), so a dataset
 *   row can never become an `!include` file-read primitive.
 * - Hard errors (§11.1) throw `SynxError`; everything else is a diagnostic
 *   (§11.2) carried on the record and on the document.
 *
 * @packageDocumentation
 */

import {
  parseData, castType, parseConstraints, parseI64, rootKeyLines, inputText, keysOf, metaMapOf,
  compareUnicodeScalar, scalarLength, expandExponent, SynxError,
  SYNXL_VERSION, MAX_SYNXL_RECORD_BYTES, MAX_SYNXL_FIELDS, MAX_SYNXL_FIELD_NAME_BYTES,
  MAX_SYNXL_FIELD_LISTS, MAX_SYNXL_RECORDS,
} from './parser';
import { toCanonicalJSONString } from './json';
import { matchesLinear } from './pattern';
import type {
  SynxObject,
  SynxValue,
  SynxConstraints,
  SynxMeta,
  SynxlDiagnostic,
  SynxlDiagnosticKind,
  SynxlDocument,
  SynxlField,
  SynxlFieldList,
  SynxlOptions,
  SynxlRecord,
  SynxlWriteOptions,
} from './types';

// Format identity and resource limits (SYNXL §1.3, §13), the pattern matcher
// and two text helpers are declared in modules the entry point can read
// without loading this one; they stay part of what this module exports.
export {
  SYNXL_VERSION, MAX_SYNXL_RECORD_BYTES, MAX_SYNXL_FIELDS, MAX_SYNXL_FIELD_NAME_BYTES,
  MAX_SYNXL_FIELD_LISTS, MAX_SYNXL_RECORDS, matchesLinear, scalarLength, expandExponent,
};

/** Type hints whose result varies between reads — forbidden by SYNXL §8.3. */
const NON_DETERMINISTIC_HINTS = new Set([
  'random',
  'random:int',
  'random:float',
  'random:bool',
]);


// ─── Hard errors (§11.1) ──────────────────────────────────

/**
 * Condition tokens for the §11.1 hard errors. These are the tokens used by the
 * conformance suite's `*.expected.error` fixtures (§18).
 */
export type SynxlErrorCondition =
  | 'MissingOrMalformedPrologue'
  | 'UnsupportedFormatVersion'
  | 'RepeatedPrologueVersionMismatch'
  | 'UnknownDirective'
  | 'RecordWithoutFieldList'
  | 'MalformedFieldList'
  | 'DuplicateFieldName'
  | 'MarkerChainInFieldDecl'
  | 'NonDeterministicTypeHint'
  | 'BlockCombinedWithType'
  | 'ZeroArityFieldList'
  | 'LimitExceeded';

/**
 * A SYNXL hard error (§11.1). Subclasses `SynxError` — callers catching the
 * package's error type keep working — and adds the machine-readable condition
 * token the conformance suite compares against.
 */
export class SynxlError extends SynxError {
  readonly condition: SynxlErrorCondition;
  /** 1-based source line the error was raised on. */
  readonly line: number;

  constructor(condition: SynxlErrorCondition, message: string, line: number) {
    super(`SYNXL_ERR: ${message} (line ${line})`);
    this.name = 'SynxlError';
    this.condition = condition;
    this.line = line;
  }
}

// ─── Small utilities ──────────────────────────────────────

/** Raise a hard error (SYNXL §11.1): aborts the parse, no partial result. */
function hardError(condition: SynxlErrorCondition, message: string, line: number): never {
  throw new SynxlError(condition, message, line);
}

/** UTF-8 byte length of a UTF-16 JS string (§13 limits are byte limits). */
export function utf8Length(s: string): number {
  let bytes = 0;
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c < 0x80) {
      bytes += 1;
    } else if (c < 0x800) {
      bytes += 2;
    } else if (c >= 0xd800 && c <= 0xdbff && i + 1 < s.length) {
      const lo = s.charCodeAt(i + 1);
      if (lo >= 0xdc00 && lo <= 0xdfff) { bytes += 4; i++; } else { bytes += 3; }
    } else {
      bytes += 3;
    }
  }
  return bytes;
}

/**
 * Longest prefix of `s` whose UTF-8 encoding fits in `maxBytes`, cut at a
 * valid UTF-8 boundary (never inside a code point / surrogate pair).
 */
function truncateUtf8(s: string, maxBytes: number): string {
  if (maxBytes <= 0) return '';
  let bytes = 0;
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    let size = 3;
    let step = 1;
    if (c < 0x80) {
      size = 1;
    } else if (c < 0x800) {
      size = 2;
    } else if (c >= 0xd800 && c <= 0xdbff && i + 1 < s.length) {
      const lo = s.charCodeAt(i + 1);
      if (lo >= 0xdc00 && lo <= 0xdfff) { size = 4; step = 2; }
    }
    if (bytes + size > maxBytes) return s.substring(0, i);
    bytes += size;
    i += step - 1;
  }
  return s;
}

/** Assign without triggering the `__proto__` setter (prototype pollution). */
function setField(obj: SynxObject, key: string, value: SynxValue): void {
  // perf: three string compares beat a Set lookup on every cell.
  if (key === '__proto__' || key === 'constructor' || key === 'prototype') {
    Object.defineProperty(obj, key, {
      value, enumerable: true, writable: true, configurable: true,
    });
    return;
  }
  obj[key] = value;
}

/** Unicode `White_Space` property — the set `ltrim` removes (§3.3). */
function isUnicodeSpace(c: number): boolean {
  return (c >= 0x09 && c <= 0x0d) || c === 0x20 || c === 0x85 || c === 0xa0 ||
    c === 0x1680 || (c >= 0x2000 && c <= 0x200a) ||
    c === 0x2028 || c === 0x2029 || c === 0x202f || c === 0x205f || c === 0x3000;
}

/** Trim Unicode whitespace — the set that decides indentation (§3.3). */
function trimUnicode(s: string): string {
  let a = 0;
  let b = s.length;
  while (a < b && isUnicodeSpace(s.charCodeAt(a))) a++;
  while (b > a && isUnicodeSpace(s.charCodeAt(b - 1))) b--;
  return s.substring(a, b);
}

/**
 * Indent per SYNXL §3.3: `length(raw) - length(ltrim(raw))` in UTF-8 code
 * units, not visual columns. `ltrim` is Unicode-aware, so a line opening with
 * U+00A0 is indented (block content) — unlike §7.1.4 part trimming, which is
 * deliberately restricted to ASCII space and tab.
 *
 * Returns the byte count together with the UTF-16 offset just past the run, so
 * callers can both compare depths and slice the line.
 */
function scanIndent(line: string): { indent: number; end: number } {
  let i = 0;
  let bytes = 0;
  while (i < line.length) {
    const c = line.charCodeAt(i);
    if (!isUnicodeSpace(c)) break;
    bytes += c < 0x80 ? 1 : c < 0x800 ? 2 : 3; // every White_Space char is BMP
    i++;
  }
  return { indent: bytes, end: i };
}

/**
 * Offset of the first character SYNX itself would treat as content.
 *
 * SYNX measures indentation over Unicode whitespace (§4), so this has to match
 * — a line opening with U+00A0 yields the key `second`, not ` second`, and
 * looking for it under an ASCII-only offset finds nothing and mis-attributes
 * the diagnostic to the record line (§11.2).
 */
/** UTF-16 offset just past the leading Unicode-whitespace run of `line`. */
function scanIndentEnd(line: string): number {
  let i = 0;
  while (i < line.length && isUnicodeSpace(line.charCodeAt(i))) i++;
  return i;
}

// ─── §7.1 Splitting (quote recognition in the same pass) ──

/**
 * Trim ASCII spaces and horizontal tabs only (§7.1.4) — deliberately not
 * `String.prototype.trim`, which also removes NBSP and other Unicode
 * whitespace that SYNXL treats as ordinary content.
 */
function trimSpacesTabs(s: string): string {
  let a = 0;
  let b = s.length;
  while (a < b) {
    const ch = s.charCodeAt(a);
    if (ch !== 32 && ch !== 9) break;
    a++;
  }
  while (b > a) {
    const ch = s.charCodeAt(b - 1);
    if (ch !== 32 && ch !== 9) break;
    b--;
  }
  return a === 0 && b === s.length ? s : s.substring(a, b);
}

/** One inline part of a record line. */
export interface SynxlPart {
  /** Part text: inner text for a quoted part, trimmed text otherwise. */
  value: string;
  /** True when the part was recognized as quoted per §7.1 step 2. */
  quoted: boolean;
}

/**
 * Split a record line into parts on `;` (SYNXL §7.1).
 *
 * Splitting and quote recognition are a single pass: a `;` inside a recognized
 * quoted part is not a delimiter. A quote that has no match, or that is
 * followed by anything other than whitespace up to the next `;`, falls back to
 * an ordinary unquoted part — SYNXL has no escapes (§7.4), so this fallback is
 * what keeps such lines deterministic instead of swallowing the rest of them.
 */
export function splitRecordLine(line: string): SynxlPart[] {
  const parts: SynxlPart[] = [];
  let pos = 0;
  do {
    pos = scanPart(line, pos);
    parts.push({ value: line.substring(partStart, partEnd), quoted: partQuoted });
  } while (pos !== -1);
  return parts;
}

// The part found by the last `scanPart`: [partStart, partEnd) of the line.
let partStart = 0;
let partEnd = 0;
let partQuoted = false;

/**
 * Scan the part that begins at offset `i` (§7.1): skip spaces and tabs, try a
 * quoted run, fall back to an unquoted part trimmed of spaces and tabs.
 * Returns the offset of the next part, or -1 when this was the last.
 *
 * perf: the record builder walks parts with this cursor instead of
 * materialising an array of part objects per record.
 */
function scanPart(line: string, i: number): number {
  const n = line.length;

  // 1. Skip spaces and horizontal tabs.
  while (i < n) {
    const ch = line.charCodeAt(i);
    if (ch !== 32 && ch !== 9) break;
    i++;
  }

  // 2. Quoted part?
  const q = line.charCodeAt(i);
  if (i < n && (q === 34 || q === 39)) {
    const close = line.indexOf(q === 34 ? '"' : "'", i + 1);
    if (close !== -1) {
      let j = close + 1;
      while (j < n) {
        const ch = line.charCodeAt(j);
        if (ch !== 32 && ch !== 9) break;
        j++;
      }
      if (j >= n || line.charCodeAt(j) === 59) { // end of line or ';'
        partStart = i + 1;
        partEnd = close;
        partQuoted = true;
        return j >= n ? -1 : j + 1;
      }
    }
  }

  // 3. Unquoted part — extends to the next `;` or to end of line, trimmed of
  //    spaces and tabs.
  const semi = line.indexOf(';', i);
  const end = semi === -1 ? n : semi;
  let b = end;
  while (b > i) {
    const ch = line.charCodeAt(b - 1);
    if (ch !== 32 && ch !== 9) break;
    b--;
  }
  partStart = i;
  partEnd = b;
  partQuoted = false;
  return semi === -1 ? -1 : semi + 1;
}

// ─── §8 Casting ───────────────────────────────────────────

/** Result of a typed cast attempt (§8.2). */
interface CastResult {
  ok: boolean;
  value: SynxValue;
}

const INT_RE = /^[+-]?\d+$/;
const FLOAT_RE = /^[+-]?(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?$/;


/**
 * SYNX §8.3 typed casting, with the SYNXL §8.2 failure contract: a value that
 * does not cast yields `null` plus a `CastFailed` diagnostic instead of the
 * SYNX fallback of `0` / `false`, so a single unparsable cell cannot silently
 * masquerade as data.
 */
function castTyped(raw: string, type: string): CastResult {
  const value = castTypedValue(raw, type);
  return { ok: lastCastOk, value };
}

/** Whether the last `castTypedValue` succeeded. */
let lastCastOk = true;

/**
 * `castTyped` without a result object per cell (perf): the value is returned
 * and success is left in `lastCastOk`.
 */
function castTypedValue(raw: string, type: string): SynxValue {
  const r = castTypedInner(raw, type);
  lastCastOk = r !== CAST_FAILED;
  return r === CAST_FAILED ? null : r as SynxValue;
}

const CAST_FAILED = Symbol('cast failed');

function castTypedInner(raw: string, type: string): SynxValue | typeof CAST_FAILED {
  switch (type) {
    case 'int': {
      // spec: §7.4 trims a part of spaces and tabs only; a U+00A0 left in it
      // is data, so `5` followed by U+00A0 does not parse — as in synx-core. `trim()`
      // removed it and let the cell pass.
      const t = raw;
      if (!INT_RE.test(t)) return CAST_FAILED;
      // §8.2 — typed casting *fails* when the part does not parse, and an
      // integer outside i64 does not. `parseInt` never fails, so without this
      // an overflowing id used to arrive as a rounded number with no
      // `CastFailed` to warn the consumer.
      // perf: up to 15 digits is always a safe integer — no BigInt round trip.
      // `-0` reads as 0, exactly as BigInt makes it.
      const digits = t.charCodeAt(0) === 43 || t.charCodeAt(0) === 45 ? t.length - 1 : t.length;
      if (digits <= 15) {
        const v = +t;
        return v === 0 ? 0 : v;
      }
      const n = parseI64(t);
      if (n === null) return CAST_FAILED;
      return n;
    }
    case 'float': {
      const t = raw;
      if (!FLOAT_RE.test(t)) return CAST_FAILED;
      const n = Number(t);
      if (!Number.isFinite(n)) return CAST_FAILED;
      return n;
    }
    case 'bool': {
      const t = raw;
      if (t === 'true') return true;
      if (t === 'false') return false;
      return CAST_FAILED;
    }
    case 'string':
      return raw;
    default:
      // Unknown hint → automatic cast, matching SYNX `cast_typed`'s fallback.
      return castAuto(raw);
  }
}

/**
 * Automatic casting of an inline part (SYNXL §8.1): `true` / `false` / `null`,
 * then integer, then decimal float, then string — using the same numeric
 * recognition as the SYNX parser, but deliberately NOT the SYNX `cast()`
 * function, because SYNXL removes two of its steps:
 *
 * - **no quote stripping** (§8.1.2). Quoting was resolved by the splitter; a
 *   part that arrives here unquoted holds quote characters as ordinary text.
 *   Stripping here would make the §7.1 step 3 fallback unwritable (§14.1).
 * - **no inline comment stripping** (§7.5) — `#` and `//` are data.
 * - **no `(hint)` interpretation** (§8.3): casting is driven exclusively by the
 *   field list, so a cell reading `(random)` is that literal string.
 */
function castAuto(raw: string): SynxValue {
  if (raw === 'true') return true;
  if (raw === 'false') return false;
  if (raw === 'null') return null;

  const len = raw.length;
  if (len === 0) return raw;

  let firstDigit = 0;
  let fc = raw.charCodeAt(0);
  if (fc === 45) { // '-'
    if (len === 1) return raw;
    firstDigit = 1;
    fc = raw.charCodeAt(1);
  }
  if (fc >= 48 && fc <= 57) { // '0'-'9'
    let allNumeric = true;
    let dotPos = -1;
    for (let i = firstDigit + 1; i < len; i++) {
      const ch = raw.charCodeAt(i);
      if (ch === 46) { // '.'
        if (dotPos !== -1) { allNumeric = false; break; }
        dotPos = i;
      } else if (ch < 48 || ch > 57) {
        allNumeric = false;
        break;
      }
    }
    if (allNumeric) {
      // §8.1 — literals, then integer, then decimal float, then string. An
      // integer that overflows i64 falls through to string, exactly as in the
      // reference implementation.
      if (dotPos === -1) {
        // perf: see castTyped — no BigInt for a safe-size integer.
        if (len - firstDigit <= 15) {
          const v = +raw;
          return v === 0 ? 0 : v;
        }
        return parseI64(raw) ?? raw;
      }
      if (dotPos > firstDigit && dotPos < len - 1) return parseFloat(raw);
    }
  }
  return raw;
}

// ─── §5 Field list ────────────────────────────────────────

/** Parse one `name(type)[constraints]` declaration (SYNXL §5). */
function parseFieldDecl(decl: string, line: number): SynxlField {
  const src = trimUnicode(decl);
  if (!src) hardError('MalformedFieldList', 'malformed field list: empty field declaration', line);

  const len = src.length;
  let pos = 0;
  while (pos < len) {
    const ch = src.charCodeAt(pos);
    // name excludes ; [ ( : and whitespace (§5.1)
    if (ch === 32 || ch === 9 || ch === 91 || ch === 40 || ch === 58) break;
    pos++;
  }
  const name = src.substring(0, pos);
  // spec: a declaration with no name is malformed whatever follows — `:` too,
  // as in synx-core (B-064); a marker chain is one after a name.
  if (!name) hardError('MalformedFieldList', `malformed field declaration "${src}"`, line);
  if (utf8Length(name) > MAX_SYNXL_FIELD_NAME_BYTES) {
    hardError('LimitExceeded', `field name exceeds MAX_SYNXL_FIELD_NAME_BYTES (${MAX_SYNXL_FIELD_NAME_BYTES})`, line);
  }

  let typeHint: string | undefined;
  let constraintStr: string | undefined;

  // Optional (type)
  if (pos < len && src.charCodeAt(pos) === 40) {
    const close = src.indexOf(')', pos + 1);
    if (close === -1) hardError('MalformedFieldList', `malformed field declaration "${src}": unterminated (type)`, line);
    typeHint = src.substring(pos + 1, close).trim();
    pos = close + 1;
  }

  // Optional [constraints] — balanced scan, so patterns like ^[A-Z]{2}$ survive.
  if (pos < len && src.charCodeAt(pos) === 91) {
    pos++;
    const start = pos;
    let depth = 1;
    while (pos < len && depth > 0) {
      const ch = src.charCodeAt(pos);
      if (ch === 91) depth++;
      else if (ch === 93) depth--;
      if (depth > 0) pos++;
    }
    if (depth !== 0) hardError('MalformedFieldList', `malformed field declaration "${src}": unbalanced [constraints]`, line);
    constraintStr = src.substring(start, pos);
    pos++; // skip ']'
  }

  // Nothing but whitespace may follow. Marker chains drive the !active engine
  // and have no meaning in a record.
  // spec: the rest is trimmed first, as in synx-core — `a :` is a marker
  // chain like `a:`, where this reader saw a malformed declaration (B-064.2).
  const tail = trimUnicode(src.substring(pos));
  if (tail !== '') {
    if (tail.charCodeAt(0) === 58) {
      hardError('MarkerChainInFieldDecl', `a marker is not allowed in a field declaration "${src}"`, line);
    }
    hardError('MalformedFieldList', `malformed field declaration "${src}"`, line);
  }

  // Reuse the existing SYNX constraint parser (§5.2).
  const constraints: SynxConstraints | undefined =
    constraintStr !== undefined ? parseConstraints(constraintStr) : undefined;

  // `block` is a SYNXL-only bare flag; the SYNX constraint parser ignores it.
  let block = false;
  if (constraintStr !== undefined) {
    for (const part of constraintStr.split(',')) {
      if (part.trim() === 'block') { block = true; break; }
    }
  }

  // spec: an empty `()` is a hint too, as in synx-core (B-063): the value is
  // cast automatically and `type:` of the constraints is only checked, not
  // cast by; with `[block]` it is a type combined with a block.
  const type = typeHint !== undefined ? typeHint : constraints?.type;

  for (const hint of [typeHint, constraints?.type]) {
    if (hint !== undefined && NON_DETERMINISTIC_HINTS.has(hint)) {
      hardError('NonDeterministicTypeHint', `non-deterministic type hint "${hint}" is forbidden in SYNXL`, line);
    }
  }
  if (block && (typeHint !== undefined || constraints?.type !== undefined)) {
    hardError('BlockCombinedWithType', `field "${name}" combines [block] with a type — the shape of a block value comes from the embedded SYNX document`, line);
  }

  const field: SynxlField = { name, block, decl: src };
  if (type !== undefined) field.type = type;
  if (constraints && Object.keys(constraints).length > 0) field.constraints = constraints;
  return field;
}

/** Parse a whole `!fields` line (SYNXL §5). */
function parseFieldList(trimmed: string, line: number): SynxlFieldList {
  const rest = trimmed.substring('!fields'.length);
  if (rest.length === 0 || (rest.charCodeAt(0) !== 32 && rest.charCodeAt(0) !== 9)) {
    hardError('MalformedFieldList', 'malformed field list: expected whitespace after !fields', line);
  }
  const body = trimUnicode(rest);
  if (!body) hardError('MalformedFieldList', 'malformed field list: no field declarations', line);

  const decls = body.split(';');
  if (decls.length > MAX_SYNXL_FIELDS) {
    hardError('LimitExceeded', `field list exceeds MAX_SYNXL_FIELDS (${MAX_SYNXL_FIELDS})`, line);
  }

  const fields: SynxlField[] = [];
  const seen = new Set<string>();
  for (const decl of decls) {
    const field = parseFieldDecl(decl, line);
    if (seen.has(field.name)) {
      hardError('DuplicateFieldName', `duplicate field name "${field.name}"`, line);
    }
    seen.add(field.name);
    fields.push(field);
  }

  let arity = 0;
  for (const f of fields) if (!f.block) arity++;
  if (arity === 0) {
    // §5.3.4 — a zero-arity field list has no representation: the record line
    // would be empty, and §3.5 makes empty lines invisible to the structure.
    hardError('ZeroArityFieldList', 'field list has arity 0 — every field is [block]; at least one inline field is required', line);
  }
  return { fields, arity, line };
}

// ─── §8.4 Validation (opt-in) ─────────────────────────────

/**
 * Check one value against declared constraints, returning a message or null.
 *
 * Mirrors the SYNX engine's `validateConstraints` semantics (numbers compare by
 * value, strings by length, `pattern:` capped to guard against ReDoS) but
 * reports instead of overwriting the value with a `CONSTRAINT_ERR` string —
 * SYNXL §11.2 requires diagnostics, and corrupting a cell to report on it would
 * defeat the purpose in a dataset.
 */
function checkConstraints(name: string, value: SynxValue, c: SynxConstraints, isFloat = false): string | null {
  if (c.required && (value === null || value === undefined || value === '')) {
    return `'${name}' is required`;
  }
  if (value === null || value === undefined) return null;

  if (c.type) {
    const ok = (() => {
      switch (c.type) {
        // spec: an integer beyond 2^53 is a bigint and an int (B-061.3); a
        // float with a whole value is not one (B-063.4) — as in synx-core.
        case 'int': return (typeof value === 'number' && Number.isInteger(value) && !isFloat) || typeof value === 'bigint';
        case 'float': return typeof value === 'number' || typeof value === 'bigint';
        case 'bool': return typeof value === 'boolean';
        case 'string': return typeof value === 'string';
        default: return true;
      }
    })();
    if (!ok) return `'${name}' expected type '${c.type}'`;
  }

  if (c.enum && !c.enum.includes(String(value))) {
    return `'${name}' must be one of [${c.enum.join('|')}]`;
  }

  // spec: a bigint is compared as synx-core compares an i64, by its f64 value
  // (B-061.1, .2) — it went past `[min]` and `[max]` untested.
  const n = typeof value === 'number'
    ? value
    : typeof value === 'bigint'
      ? Number(value)
    : typeof value === 'string' && (c.min !== undefined || c.max !== undefined)
      // Unicode scalar values, as the engine and synx-core count — `length`
      // counted UTF-16 units and synx-core counted UTF-8 bytes, so `[max:2]`
      // on an emoji disagreed between the two implementations.
      ? scalarLength(value)
      : null;
  if (n !== null) {
    const shown = typeof value === 'bigint' ? value.toString() : n;
    if (c.min !== undefined && n < c.min) return `'${name}' value ${shown} is below min ${c.min}`;
    if (c.max !== undefined && n > c.max) return `'${name}' value ${shown} exceeds max ${c.max}`;
  }

  // Same 256-character cap as the engine and synx-core (this reader used 128,
  // so a 129–256 character pattern was checked in Rust and skipped here).
  if (c.pattern && typeof value === 'string' && c.pattern.length <= 256) {
    // Both the pattern and the value come out of the document, which §16 says
    // to treat as hostile. JavaScript's regex engine backtracks, so `^(a+)+$`
    // against 45 characters never returns — a 88-byte file that hangs the
    // whole event loop. The length cap does not help: the blow-up is in the
    // pattern's shape, not its size. `matchesLinear` runs the pattern as an
    // NFA in O(pattern × value), which is what the reference implementation
    // gets from Rust's non-backtracking `regex` crate.
    const verdict = matchesLinear(c.pattern, value);
    if (verdict === false) {
      return `'${name}' does not match pattern /${c.pattern}/`;
    }
  }
  return null;
}

// ─── Reader ───────────────────────────────────────────────

interface PendingRecord {
  line: number;
  recordLine: string;
  /** Block lines; `null` until the record has one (most records do not). */
  blockLines: string[] | null;
  /** Source line number of each entry in `blockLines` (§11.2 line attribution). */
  blockLineNos: number[] | null;
  fields: SynxlFieldList;
  /** Byte budget left for the block; -1 until the first block line needs it. */
  bytesLeft: number;
  truncated: boolean;
  /** Lazily built by `blockKeyIndex`; see the note there on why it is cached. */
  keyLines?: Map<string, number>;
}

/**
 * Index a block's top-level keys to their source lines (§11.2).
 *
 * Built once per record and cached, because it is consulted once per block
 * diagnostic: re-scanning the block for each one is quadratic in the number of
 * keys, and §13 lets a single record hold 16 MiB of them — enough to turn one
 * hostile row into hours of CPU while every limit reads as satisfied.
 *
 * The SYNX parser itself records which line put each key at the root, so
 * stack repair, list items and multiline bodies are decided exactly as in the
 * parse.
 *
 * spec: until 3.8.0 a "running minimum indent" stood in for the parser, in
 * UTF-16 units: a deeper line after a key with a value — which the parser
 * lifts to the root — and keys after a U+3000 indent fell back to the record
 * line, and differently from synx-core.
 */
function blockKeyIndex(p: PendingRecord): Map<string, number> {
  if (p.keyLines) return p.keyLines;
  const map = new Map<string, number>();
  const nos = p.blockLineNos ?? [];
  const lines = rootKeyLines((p.blockLines ?? []).join('\n'), { directives: false, nondeterministicHints: false });
  for (const [key, i] of lines) map.set(key, nos[i]);
  p.keyLines = map;
  return map;
}

/** Source line of a block's top-level `key`, or the record line if unknown. */
function blockKeyLine(p: PendingRecord, key: string): number {
  return blockKeyIndex(p).get(key) ?? p.line;
}

/**
 * Incremental SYNXL reader. Feed it lines; it returns a record whenever one
 * completes. Record boundaries are decidable from a single byte (§3.4), which
 * is what makes this possible without buffering the document.
 */
class SynxlReader {
  version = -1;
  readonly fieldLists: SynxlFieldList[] = [];

  private readonly opts: SynxlOptions;
  private readonly maxRecordBytes: number;
  private prologueSeen = false;
  private fields: SynxlFieldList | null = null;
  private inBlockComment = false;
  private pending: PendingRecord | null = null;
  private recordIndex = 0;
  private unterminated = false;
  /** OrphanBlockLine diagnostics waiting for the record they attach to (§11.2). */
  private orphans: SynxlDiagnostic[] = [];

  constructor(options: SynxlOptions = {}) {
    this.opts = options;
    this.maxRecordBytes = options.maxRecordBytes ?? MAX_SYNXL_RECORD_BYTES;
  }

  /** Flag that the input ended without a trailing newline (§15.2). */
  markUnterminated(): void {
    this.unterminated = true;
  }

  /** Feed one source line (1-based `lineNo`). Returns a completed record. */
  push(raw: string, lineNo: number): SynxlRecord | null {
    let line = raw;
    if (lineNo === 1 && line.charCodeAt(0) === 0xfeff) line = line.substring(1); // BOM (§3.1)
    if (line.charCodeAt(line.length - 1) === 13) line = line.substring(0, line.length - 1); // CR (§3.2)

    const { indent, end } = scanIndent(line);
    const empty = end === line.length;

    // §3.5 — an empty line is ignored structurally and never ends a block.
    if (empty) {
      if (this.pending) this.appendBlockLine(line, lineNo);
      return null;
    }

    // §3.4 — indent > 0 is always block content, but only once the document
    // has started: §4.1 matches the prologue against a line's *trimmed* form,
    // so a leading space before `!synxl 1` does not disqualify it. Before the
    // prologue, an indented line falls through to the structural handling
    // below, which accepts it as the prologue or raises the §4.1 hard error.
    if (indent > 0 && this.prologueSeen) {
      // §4.3 — a `###` block comment ends at the closing marker whatever its
      // indent, as in synx-core (B-062); the comment swallowed the rest of
      // the document here.
      if (this.inBlockComment) {
        if (trimUnicode(line) === '###') this.inBlockComment = false;
        return null;
      }
      if (this.pending) {
        this.appendBlockLine(line, lineNo);
      } else {
        // §11.2 — block content with no record open: discard and report against
        // the record that follows.
        this.orphans.push({
          kind: 'OrphanBlockLine',
          record: this.recordIndex,
          line: lineNo,
          message: 'indented line with no record open — discarded',
        });
      }
      return null;
    }

    // Indent 0 is structural: whatever it is, it closes the open record.
    const done = this.finish();
    // `trimUnicode`, not `String.prototype.trim`: ECMAScript's WhiteSpace set
    // differs from Unicode's on two characters — it strips U+FEFF, which
    // §3.1 only allows to be ignored at the start of input, and keeps U+0085,
    // which §3.3's `ltrim` removes. Each difference decides whether a document
    // is accepted at all, so the classifier has to use the format's set.
    // perf: classify by the first significant character; only a line that can
    // be structural (`#`, `!`, `/`) pays for a trimmed copy.
    const first = line.charCodeAt(scanIndentEnd(line));
    const trimmed = first === 35 || first === 33 || first === 47 ? trimUnicode(line) : '';

    if (trimmed === '###') { // §4.3 block-comment toggle
      this.inBlockComment = !this.inBlockComment;
      return done;
    }
    if (this.inBlockComment) return done;

    // §4.3 — `!synxl` / `!fields` are matched before the comment rules, and
    // both start with `!`, so plain prefix checks are unambiguous here.
    if (first === 33) {
      if (trimmed === '!synxl' || trimmed.startsWith('!synxl ') || trimmed.startsWith('!synxl\t')) {
        this.handlePrologue(trimmed, lineNo);
        return done;
      }
      if (!this.prologueSeen) {
        hardError('MissingOrMalformedPrologue', 'missing prologue: the first significant line must be "!synxl 1"', lineNo);
      }
      if (trimmed === '!fields' || trimmed.startsWith('!fields ') || trimmed.startsWith('!fields\t')) {
        if (this.fieldLists.length >= MAX_SYNXL_FIELD_LISTS) {
          hardError('LimitExceeded', `document exceeds MAX_SYNXL_FIELD_LISTS (${MAX_SYNXL_FIELD_LISTS})`, lineNo);
        }
        const fl = parseFieldList(trimmed, lineNo);
        this.fields = fl;
        this.fieldLists.push(fl);
        return done;
      }
      // §6: `!` is reserved at indent 0. A record meant to start with it must
      // be quoted, so an unknown directive is a hard error rather than data.
      hardError('UnknownDirective', `unknown directive "${trimmed}" — a record starting with "!" must be quoted`, lineNo);
    }

    if (first === 35) return done;                       // # comment
    if (first === 47 && trimmed.charCodeAt(1) === 47) return done; // // comment

    if (!this.prologueSeen) {
      hardError('MissingOrMalformedPrologue', 'missing prologue: the first significant line must be "!synxl 1"', lineNo);
    }
    if (!this.fields) {
      hardError('RecordWithoutFieldList', 'record line with no field list in effect', lineNo);
    }

    // §6 — record line. The SYNX §7 first-character filter is NOT applied:
    // `-5`, `[unparsed]`, `/var/log/app` and `@handle` are ordinary values.
    const budget = this.maxRecordBytes;
    let recordLine = line;
    let truncated = false;
    // perf: a UTF-16 unit is at most 3 UTF-8 bytes, so most lines are known to
    // fit without measuring; the exact count is taken only near the budget or
    // when a block line needs what is left of it.
    if (line.length * 3 + 1 > budget && utf8Length(line) + 1 > budget) {
      recordLine = truncateUtf8(line, budget);
      truncated = true;
    }
    this.pending = {
      line: lineNo,
      recordLine,
      blockLines: null,
      blockLineNos: null,
      fields: this.fields,
      bytesLeft: truncated ? 0 : -1,
      truncated,
    };
    return done;
  }

  /** Finish the stream: returns the final pending record, if any. */
  end(): SynxlRecord | null {
    return this.finish();
  }

  private appendBlockLine(line: string, lineNo: number): void {
    const p = this.pending;
    if (!p) return;
    if (p.bytesLeft < 0) p.bytesLeft = this.maxRecordBytes - (utf8Length(p.recordLine) + 1);
    if (p.bytesLeft <= 0) { p.truncated = true; return; }
    if (!p.blockLines) { p.blockLines = []; p.blockLineNos = []; }
    const cost = utf8Length(line) + 1;
    if (cost > p.bytesLeft) {
      p.blockLines.push(truncateUtf8(line, p.bytesLeft));
      p.blockLineNos!.push(lineNo);
      p.bytesLeft = 0;
      p.truncated = true;
      return;
    }
    p.bytesLeft -= cost;
    p.blockLines.push(line);
    p.blockLineNos!.push(lineNo);
  }

  private handlePrologue(trimmed: string, lineNo: number): void {
    const rest = trimUnicode(trimmed.substring('!synxl'.length));
    if (!/^\d+$/.test(rest)) {
      hardError('MissingOrMalformedPrologue', `malformed prologue "${trimmed}": expected "!synxl <version>"`, lineNo);
    }
    const version = parseInt(rest, 10);
    // The repeated-prologue rule is the more specific one and is checked first,
    // so `!synxl 2` after `!synxl 1` reports the mismatch rather than the
    // generic unsupported-version condition.
    if (this.prologueSeen && version !== this.version) {
      hardError('RepeatedPrologueVersionMismatch', `repeated prologue declares version ${version}, document declared ${this.version}`, lineNo);
    }
    if (version !== SYNXL_VERSION) {
      hardError('UnsupportedFormatVersion', `unsupported SYNXL format version ${version} (this implementation supports ${SYNXL_VERSION})`, lineNo);
    }
    // A repeated identical prologue is accepted and ignored so that
    // concatenated shards (§4.1, §15.3) parse.
    this.prologueSeen = true;
    this.version = version;
  }

  /**
   * OrphanBlockLine diagnostics that never found a following record. Only
   * reachable at end of input; surfaced on the document result.
   */
  takeOrphans(): SynxlDiagnostic[] {
    const out = this.orphans;
    this.orphans = [];
    return out;
  }

  /** Materialise the pending record (record line + block). */
  private finish(): SynxlRecord | null {
    const p = this.pending;
    this.pending = null;
    if (!p) return null;

    const index = this.recordIndex++;

    // §11.2 — order within a record is normative: OrphanBlockLine (which
    // precedes the record in the source), RecordTruncated, CastFailed in field
    // order, MissingFields/ExtraFields, block diagnostics in sorted key order,
    // ConstraintViolation in field order.
    // perf: one array, filled in exactly the normative order — each kind is
    // produced after the one before it, so per-kind arrays merged at the end
    // (six arrays, a closure and a spread per record) gave the same sequence.
    let diagnostics: SynxlDiagnostic[];
    if (this.orphans.length > 0) {
      diagnostics = this.orphans;
      this.orphans = [];
      for (const d of diagnostics) d.record = index;
    } else {
      diagnostics = [];
    }
    const make = (
      kind: SynxlDiagnosticKind,
      line: number,
      message: string,
      field?: string,
    ): void => {
      const d: SynxlDiagnostic = { kind, record: index, line, message };
      if (field !== undefined) d.field = field;
      diagnostics.push(d);
    };

    if (p.truncated) {
      make('RecordTruncated', p.line,
        `record exceeds the ${this.maxRecordBytes}-byte budget and was truncated at a UTF-8 boundary`);
    } else if (this.unterminated) {
      make('RecordTruncated', p.line,
        'input ended without a newline — the final record may be incomplete');
    }

    const values: SynxObject = {};
    const { fields, arity } = p.fields;
    let wholeFloats: Set<string> | null = null;

    // §7.2 — the all-null record form. A record line that is exactly `;` sets
    // every inline field to null at any arity and reports nothing. It is
    // recognized before the §7.1 split, which would otherwise see two empty
    // parts and score the record against the arity.
    if (trimSpacesTabs(p.recordLine) === ';') {
      for (const f of fields) setField(values, f.name, null);
    } else {
      // ── Inline fields (§7, §8) ──
      // perf: parts are scanned in step with the fields — no part array.
      const line = p.recordLine;
      let pos = 0;
      let partCount = 0;
      for (const f of fields) {
        if (f.block) { setField(values, f.name, null); continue; }
        if (pos === -1) { setField(values, f.name, null); continue; }
        pos = scanPart(line, pos);
        partCount++;
        const value = line.substring(partStart, partEnd);
        if (partQuoted) {
          // §7.4 — literal inner text: no escapes, no casting, no comment strip.
          setField(values, f.name, value);
          continue;
        }
        if (value === '') { setField(values, f.name, null); continue; } // §7.2
        let cast: SynxValue;
        if (f.type !== undefined) {
          cast = castTypedValue(value, f.type);
          if (!lastCastOk) {
            make('CastFailed', p.line, `cannot cast "${value}" to ${f.type}`, f.name);
          }
        } else {
          cast = castAuto(value);
        }
        setField(values, f.name, cast);
        // A whole number read as a float (`3.0`, or `3` under `(float)`) is not
        // an int to `[type:int]`, as in synx-core; a JS number cannot say so.
        if (this.opts.validate && typeof cast === 'number' && Number.isInteger(cast)
            && (f.type === 'float' || (f.type !== 'int' && value.indexOf('.') !== -1))) {
          (wholeFloats ??= new Set<string>()).add(f.name);
        }
      }
      // Surplus parts are counted, not kept (§7.3).
      while (pos !== -1) {
        pos = scanPart(line, pos);
        partCount++;
      }

      if (partCount < arity) {
        make('MissingFields', p.line,
          `record has ${partCount} inline field(s), field list declares ${arity}; missing trailing fields are null`);
      } else if (partCount > arity) {
        make('ExtraFields', p.line,
          `record has ${partCount} inline field(s), field list declares ${arity}; surplus discarded`);
      }
    }

    // ── Block fields (§9) ──
    const blockText = p.blockLines ? p.blockLines.join('\n') : '';
    if (blockText !== '' && blockText.trim() !== '') {
      // §9.3 — delegate to the SYNX parser with the block's ORIGINAL
      // indentation; SYNX's stack repair attaches the shallowest keys to the
      // sub-document root, so no dedent pre-pass is needed.
      // §9.4 — directives are disabled inside this parse (not pre-filtered,
      // which would corrupt `|+` bodies).
      const { root } = parseData(blockText, {
        directives: false,
        nondeterministicHints: false,
      });
      const byName = new Map<string, SynxlField>();
      for (const f of fields) byName.set(f.name, f);

      // §9.3 — visit in Unicode scalar order so diagnostics are reproducible.
      const keys = Object.keys(root).sort(compareUnicodeScalar);
      for (const key of keys) {
        const f = byName.get(key);
        if (f && f.block) {
          setField(values, key, root[key]);
        } else if (f) {
          make('BlockFieldNotDeclared', blockKeyLine(p, key),
            `block key "${key}" is not declared [block]; the inline value is authoritative`, key);
        } else {
          make('UnknownBlockKey', blockKeyLine(p, key),
            `block key "${key}" matches no field in the field list`, key);
        }
      }
    }

    // ── Opt-in validation (§8.4) ──
    if (this.opts.validate) {
      for (const f of fields) {
        if (!f.constraints) continue;
        const msg = checkConstraints(f.name, values[f.name], f.constraints, wholeFloats !== null && wholeFloats.has(f.name));
        // §11.2 — an inline field reports the record line, a block field the
        // line its key sits on.
        if (msg) make('ConstraintViolation', f.block ? blockKeyLine(p, f.name) : p.line, msg, f.name);
      }
    }

    return { index, line: p.line, values, fields: p.fields, diagnostics };
  }
}

// ─── Public parse API ─────────────────────────────────────

/**
 * Parse a whole SYNXL document into records, diagnostics, and the field lists
 * that were in effect (SYNXL §4–§11).
 *
 * @throws SynxError on any hard error of §11.1 (no partial result is returned).
 */
export function parseSynxl(text: string, options: SynxlOptions = {}): SynxlDocument {
  text = inputText(text, 'parseSynxl');
  const reader = new SynxlReader(options);
  const records: SynxlRecord[] = [];
  const lines = text.split('\n');

  const collect = (rec: SynxlRecord | null, line: number): void => {
    if (!rec) return;
    if (records.length >= MAX_SYNXL_RECORDS) {
      hardError('LimitExceeded', `document exceeds MAX_SYNXL_RECORDS (${MAX_SYNXL_RECORDS})`, line);
    }
    records.push(rec);
  };

  for (let i = 0; i < lines.length; i++) {
    collect(reader.push(lines[i], i + 1), i + 1);
  }
  if (options.reportTruncatedTail && text.length > 0 && !text.endsWith('\n')) {
    reader.markUnterminated();
  }
  collect(reader.end(), lines.length);

  if (reader.version < 0) {
    // spec: reported at the last line read, as synx-core does (B-060).
    const lastLine = text.endsWith('\n') ? lines.length - 1 : lines.length;
    hardError('MissingOrMalformedPrologue', 'missing prologue: the first significant line must be "!synxl 1"', Math.max(1, lastLine));
  }

  const diagnostics: SynxlDiagnostic[] = [];
  for (const r of records) diagnostics.push(...r.diagnostics);
  // Orphan block lines after the last record have no record to attach to; they
  // still belong on the result (§11.2), carrying the index a record would have.
  diagnostics.push(...reader.takeOrphans());

  return { version: reader.version, records, fieldLists: reader.fieldLists, diagnostics };
}

/**
 * Streaming reader (SYNXL §15.1): yields records incrementally without
 * materialising the document. Accepts the document text or any iterable of
 * lines (e.g. `readline` output).
 */
export function* streamSynxl(
  source: string | Iterable<string>,
  options: SynxlOptions = {},
): Generator<SynxlRecord, void, undefined> {
  const reader = new SynxlReader(options);
  let lineNo = 0;

  if (typeof source === 'string') {
    // Walked by index rather than `split('\n')`: splitting allocates an array
    // of every line in the document up front, which is exactly the
    // materialisation §15.1 says a streaming reader must avoid — on a 9 MB
    // document that array alone costs more than the text.
    let pos = 0;
    while (pos <= source.length) {
      let nl = source.indexOf('\n', pos);
      if (nl === -1) nl = source.length;
      const rec = reader.push(source.substring(pos, nl), ++lineNo);
      if (rec) yield rec;
      if (nl === source.length) break;
      pos = nl + 1;
    }
    if (options.reportTruncatedTail && source.length > 0 && !source.endsWith('\n')) {
      reader.markUnterminated();
    }
  } else {
    if (source === null || source === undefined || typeof (source as Iterable<string>)[Symbol.iterator] !== 'function') {
      throw new SynxError('INPUT_ERR: streamSynxl expects a string or an iterable of lines');
    }
    for (const line of source) {
      const rec = reader.push(inputText(line, 'streamSynxl'), ++lineNo);
      if (rec) yield rec;
    }
  }

  const last = reader.end();
  if (last) yield last;
}

/**
 * Async streaming reader (SYNXL §15.1) over chunks — e.g. a Node read stream or
 * a `fetch` body. Chunks are split into lines internally, so a record may span
 * any number of chunks.
 */
export async function* streamSynxlAsync(
  source: AsyncIterable<string | Uint8Array>,
  options: SynxlOptions = {},
): AsyncGenerator<SynxlRecord, void, undefined> {
  const reader = new SynxlReader(options);
  // Structural type: the global TextDecoder is a value in every supported
  // runtime, but its *type* is not in `lib: ES2020`.
  let decoder: { decode(input?: Uint8Array, options?: { stream?: boolean }): string } | null = null;
  let buf = '';
  let lineNo = 0;
  // The line buffer is bounded by the per-record budget (§13, §16.3): without
  // a cap, input with no `LF` in it grows the buffer without limit, so a
  // single uploaded file turns "streaming" into holding the whole thing — and
  // the repeated `substring` on a multi-megabyte buffer makes it quadratic on
  // top of that. Over the cap the line is truncated at a UTF-8 boundary, which
  // is the same outcome an oversized record has (§13).
  const lineCap = options.maxRecordBytes ?? MAX_SYNXL_RECORD_BYTES;
  let overflowed = false;

  for await (const chunk of source) {
    let text: string;
    if (typeof chunk === 'string') text = chunk;
    else {
      // safety: SYNXL is UTF-8 (§3.1). A lenient decoder turned every other
      // byte into U+FFFD and went on: a dataset in the wrong encoding was
      // read as records nobody wrote.
      if (decoder === null) decoder = new (TextDecoder as any)('utf-8', { fatal: true }) as { decode(input?: Uint8Array, options?: { stream?: boolean }): string };
      try {
        text = decoder.decode(chunk, { stream: true });
      } catch {
        throw new SynxError(`INPUT_ERR: line ${lineNo + 1} is not valid UTF-8 (§3.1)`);
      }
    }
    buf += text;

    let nl = buf.indexOf('\n');
    while (nl !== -1) {
      const line = buf.substring(0, nl);
      buf = buf.substring(nl + 1);
      const rec = reader.push(overflowed ? truncateUtf8(line, lineCap) : line, ++lineNo);
      overflowed = false;
      if (rec) yield rec;
      nl = buf.indexOf('\n');
    }
    if (utf8Length(buf) > lineCap) {
      buf = truncateUtf8(buf, lineCap);
      overflowed = true;
    }
  }
  if (decoder) {
    try {
      buf += decoder.decode();
    } catch {
      // The stream ended inside a character.
      throw new SynxError(`INPUT_ERR: line ${lineNo + 1} is not valid UTF-8 (§3.1)`);
    }
  }

  if (buf.length > 0) {
    const rec = reader.push(buf, ++lineNo);
    if (rec) yield rec;
    // Flag *after* the push: the flag must only reach the record that the
    // unterminated line belongs to, which `end()` materialises.
    if (options.reportTruncatedTail) reader.markUnterminated();
  }
  const last = reader.end();
  if (last) yield last;
}

// ─── §12 Canonical JSON projection ────────────────────────

/**
 * The package's canonical JSON writer (SYNX §10: keys sorted lexicographically,
 * no insignificant whitespace) — the same one `Synx.toJSON` uses.
 */
function canonicalJson(values: SynxObject): string {
  // §9.5 — SYNXL captures no metadata, so a `__synx` key in a record is caller
  // data and must survive into the projection (§12.1).
  return toCanonicalJSONString(values, false, false);
}

function asDocument(source: string | SynxlDocument, options?: SynxlOptions): SynxlDocument {
  if (source !== null && typeof source === 'object' && Array.isArray((source as SynxlDocument).records)) return source;
  return parseSynxl(source as string, options);
}

/** Canonical JSON **array** projection of a document (SYNXL §12.1). */
export function synxlToJSON(source: string | SynxlDocument, options: SynxlOptions = {}): string {
  const doc = asDocument(source, options);
  const out: string[] = [];
  for (const r of doc.records) out.push(canonicalJson(r.values));
  return `[${out.join(',')}]`;
}

/**
 * Canonical **NDJSON** projection (SYNXL §12.2): one canonical JSON object per
 * line, `LF`-separated.
 *
 * Each record is terminated, not separated — the NDJSON convention, what the
 * reference implementation emits, and what makes appending a record a pure
 * concatenation. §1.4.3 measures conformance on these bytes, so the trailing
 * `LF` is part of the projection rather than a formatting nicety. An empty
 * document projects to the empty string.
 */
export function synxlToNDJSON(source: string | SynxlDocument, options: SynxlOptions = {}): string {
  const doc = asDocument(source, options);
  let out = '';
  for (const r of doc.records) out += canonicalJson(r.values) + '\n';
  return out;
}

// ─── §14 Canonical serialization (writer) ─────────────────

/**
 * Render a number in a form SYNX can read back (§14.3): SYNX §8.3 recognizes
 * only `-?digits[.digits]`, so exponent forms must be expanded. Returns null
 * for a non-finite value, which has no SYNX representation.
 *
 * Note: JS has a single number type, so an integral value inside the safe
 * range is always emitted in integer form; the spec's "integral floats carry
 * `.0`" rule is unobservable there and does not affect the projection.
 *
 * Outside that range it *is* observable: a bare `1000000000000000000000`
 * overflows i64 on the way back in and returns as a string (§8.1), so an
 * integral value too large to be an integer is written with `.0` to make the
 * reader take it as the float it was — which is what §14.1 round-trip needs.
 */
function numberText(n: number): string | null {
  if (!Number.isFinite(n)) return null;
  // `String(-0)` is "0": the sign — which the JSON projection keeps as `-0.0`
  // — was lost on the way through the writer.
  if (Object.is(n, -0)) return '-0.0';
  if (Number.isInteger(n) && !Number.isSafeInteger(n)) {
    return `${expandExponent(String(n))}.0`;
  }
  return expandExponent(String(n));
}

/**
 * Render a scalar for a SYNX **block** value, or null when it needs a `|+`
 * multiline block (multi-line text, or text that quoting cannot express).
 */
function synxScalarText(value: SynxValue): string | null {
  if (value === null || value === undefined) return 'null';
  if (typeof value === 'bigint') return value.toString();
  if (typeof value === 'number') return numberText(value) ?? 'null';
  if (typeof value === 'boolean') return String(value);
  // safety: a secret is written as its mask, never as its fields or plaintext.
  if (typeof value !== 'string') return isSecretValue(value) ? '[SECRET]' : null;
  if (value === '') return '""';
  if (value.indexOf('\n') !== -1) return null;
  // ` //` and ` #` are stripped by SYNX §8.3 *before* quotes are recognised, so
  // quoting does not protect them — `"a // b"` reads back as `"a`. The only
  // faithful rendering is a `|+` body, where neither sequence means anything.
  if (value.indexOf(' //') !== -1 || value.indexOf(' #') !== -1) return null;

  const needsQuote =
    // Edge whitespace, checked on the two edge characters — `trimUnicode`
    // built a copy of every value to compare it.
    isUnicodeSpace(value.charCodeAt(0)) || isUnicodeSpace(value.charCodeAt(value.length - 1)) ||
    value === '|' || value === '|+' ||
    // Wrapped in quotes it would be read as the text between them. A quote at
    // one end only is text as it stands, and so is a CR inside the line — as
    // synx-core writes them.
    castType(value) !== value;

  if (!needsQuote) return value;
  if (value.indexOf('"') === -1) return `"${value}"`;
  if (value.indexOf("'") === -1) return `'${value}'`;
  return null; // both quote characters present → carry it as a `|+` body
}

/** Effective type of a field for writing: `(type)` wins over `type:` (§8.2). */
function fieldType(f: SynxlField): string | undefined {
  return f.type ?? f.constraints?.type;
}

/**
 * Strip trailing spaces and tabs.
 *
 * Hand-rolled rather than `/[ \t]+$/`: an unanchored trailing-run pattern
 * restarts at every position inside a long run of spaces, so a single cell
 * holding 100k spaces turns one write into seconds of CPU.
 */
function trimTrailingSpacesTabs(s: string): string {
  let end = s.length;
  while (end > 0) {
    const ch = s.charCodeAt(end - 1);
    if (ch !== 32 && ch !== 9) break;
    end--;
  }
  return end === s.length ? s : s.substring(0, end);
}

/**
 * Render a value for an **inline** record part, or null when the field must be
 * promoted to a block field (§14.3).
 */
function inlineText(value: SynxValue, type?: string): string | null {
  if (value === null || value === undefined) return '';
  if (typeof value === 'boolean') return String(value);
  if (typeof value === 'bigint') return value.toString();
  // §14.3 — a non-finite float is written as an empty part, reading back as
  // `null`, which is also what the JSON projection carries.
  if (typeof value === 'number') return numberText(value) ?? '';
  if (typeof value !== 'string') return null; // objects / arrays are block-only
  if (value === '') return '""';
  // A `CR` is ordinary content in an inline part (only `CRLF` is a line ending,
  // §3.2) and quoting carries it fine. Promoting it to a `|+` body would lose
  // it, because a multiline body cannot hold one.
  if (value.indexOf('\n') !== -1) return null;

  const needsQuote =
    // Unicode whitespace, not just ASCII: a leading U+00A0 gives the written
    // line a non-zero indent (§3.3) and the record is read as block content.
    value !== trimUnicode(value) ||
    value.indexOf(';') !== -1 ||
    value.indexOf('\r') !== -1 ||
    value.startsWith('#') || value.startsWith('//') || value.startsWith('!') ||
    // §14.3 — a leading quote character would be re-read as an opening quote.
    value.charCodeAt(0) === 34 || value.charCodeAt(0) === 39 ||
    // §8.2 — a typed column casts what it reads, so an unquoted `N/A` in an
    // `int` column returns as null plus a `CastFailed`. §7.4 exempts quoted
    // parts from casting, which is what keeps the string a string.
    (type ? castTyped(value, type).value !== value : castAuto(value) !== value);

  if (!needsQuote) return value;
  if (value.indexOf('"') === -1) return `"${value}"`;
  if (value.indexOf("'") === -1) return `'${value}'`;
  // Both quote characters present. §7.1 step 3 falls back to *unquoted* on
  // trailing garbage after a closing quote, so a value that only needs quoting
  // because it starts with one round-trips bare. Promote only when something
  // else — a `;`, edge whitespace, a reserved prefix — forces the issue.
  const onlyLeadingQuote =
    (value.charCodeAt(0) === 34 || value.charCodeAt(0) === 39) &&
    value === trimUnicode(value) &&
    value.indexOf(';') === -1 &&
    value.indexOf('\r') === -1 &&
    !value.startsWith('#') && !value.startsWith('//') && !value.startsWith('!');
  return onlyLeadingQuote ? value : null;
}

/** A `:secret` value (branded by the engine; see SynxSecret). */
function isSecretValue(v: unknown): boolean {
  return v !== null && typeof v === 'object' && (v as Record<symbol, unknown>)[Symbol.for('synx.secret')] === true;
}

/** Deepest nesting the block writer follows; what lies deeper is not written, as in synx-core. */
const MAX_WRITE_DEPTH = 128;

/** A value that is a group: an object that is neither a list nor a secret. */
function isGroup(v: unknown): v is SynxObject {
  return v !== null && typeof v === 'object' && !Array.isArray(v) && !isSecretValue(v);
}

/** May this value share the `- ` line of a list item? */
function dashLineSafe(v: SynxValue): boolean {
  if (typeof v === 'string') return v !== '' && v.indexOf('\n') === -1 && synxScalarText(v) !== null;
  if (v === null || typeof v !== 'object' || isSecretValue(v)) return true;
  if (Array.isArray(v)) return false;
  return Object.keys(v).length === 0;
}

// ─── Keys as the writers put them on a line ───────────────

/** A control character, as Unicode's `Cc`. */
function isControl(c: number): boolean {
  return c < 0x20 || (c >= 0x7f && c <= 0x9f);
}

/** Whether `key`, written at the start of a line, is read back as that key and nothing else. */
function keyReadsBack(key: string): boolean {
  if (key === '') return false;
  const first = key.charCodeAt(0);
  // `-`, `#`, `/`, `[`, `:`, `(` cannot open a key.
  if (first === 45 || first === 35 || first === 47 || first === 91 || first === 58 || first === 40) return false;
  if (first === 33) {
    // Lines the parser takes for directives.
    if (key === '!active' || key === '!lock' || key === '!tool' || key === '!schema' || key === '!llm'
        || key === '!include' || key === '!use') {
      return false;
    }
  }
  // Whitespace at an edge is trimmed away with the line; inside, only a space
  // or a tab ends a key (U+00A0 is a letter like any other).
  if (isUnicodeSpace(first) || isUnicodeSpace(key.charCodeAt(key.length - 1))) return false;
  for (let i = 0; i < key.length; i++) {
    const c = key.charCodeAt(i);
    if (c === 32 || c === 91 || c === 58 || c === 40 || isControl(c)) return false;
  }
  return true;
}

/**
 * `key` as the writers put it on a line.
 *
 * safety: SYNX has no quoting for keys, and a key was written as it came.
 * `port:env` gave the next reader a marker, `!active` on a line of its own
 * switched the document to active mode, a line break began new lines — a map
 * with caller-chosen keys, saved and loaded, could read the server's
 * environment. A character that would end the key or start something else is
 * written as `_`; a key that reads back as itself is untouched.
 */
function writableKey(key: string): string {
  if (keyReadsBack(key)) return key;
  const chars = Array.from(key);
  const last = chars.length - 1;
  let out = '';
  for (let i = 0; i < chars.length; i++) {
    const ch = chars[i];
    const c = ch.charCodeAt(0);
    const breaks = isControl(c) || c === 32 || c === 91 || c === 58 || c === 40
      || ((i === 0 || i === last) && ch.length === 1 && isUnicodeSpace(c))
      || (i === 0 && (c === 45 || c === 35 || c === 47 || c === 33));
    out += breaks ? '_' : ch;
  }
  return out === '' ? '_' : out;
}

/**
 * The names the keys of one object are written under, or `null` when every
 * key is written as it is — the usual object.
 *
 * safety: a key that does not read back as itself is renamed
 * (`writableKey`), and two keys must not end up under one name — `a b`, `a:b`
 * and `a_b` were all `a_b` to the reader, which kept the last of the three. A
 * name already taken in the object gets a number: `a_b`, `a_b_2`, `a_b_3`, in
 * the order of the object's keys.
 */
function keyNames(obj: SynxObject, keys: string[]): Map<string, string> | null {
  let renamed: Map<string, string> | null = null;
  let taken: Set<string> | null = null;
  let next: Map<string, number> | null = null;
  for (let i = 0; i < keys.length; i++) {
    const key = keys[i];
    if (keyReadsBack(key)) continue;
    if (renamed === null) {
      renamed = new Map<string, string>();
      taken = new Set<string>();
      next = new Map<string, number>();
    }
    const base = writableKey(key);
    let name = base;
    while (Object.prototype.hasOwnProperty.call(obj, name) || taken!.has(name)) {
      const n = next!.get(base) ?? 2;
      name = `${base}_${n}`;
      next!.set(base, n + 1);
    }
    taken!.add(name);
    renamed.set(key, name);
  }
  return renamed;
}

/** The keys of an object as a writer walks them: as the object has them, or sorted by code point. */
function writeOrder(obj: SynxObject, sorted: boolean): string[] {
  return sorted ? Object.keys(obj).sort(compareUnicodeScalar) : keysOf(obj);
}

// ─── Block writer ─────────────────────────────────────────

/** `key |+` plus an indented body (§14.3). */
function emitMultiline(name: string, body: string, indent: number, out: string[], step: number): void {
  const pad = ' '.repeat(indent);
  out.push(`${pad}${name} |+`);
  const inner = pad + ' '.repeat(step);
  // perf: one pass over the text; `split` built an array of every line first.
  let from = 0;
  for (;;) {
    const nl = body.indexOf('\n', from);
    if (nl === -1) {
      out.push(inner + body.substring(from));
      return;
    }
    out.push(inner + body.substring(from, nl));
    from = nl + 1;
  }
}

/** Write one `- item` list entry. */
function emitListItem(item: SynxValue, indent: number, out: string[], step: number, depth: number, sorted: boolean): void {
  if (depth > MAX_WRITE_DEPTH) return;
  const pad = ' '.repeat(indent);
  if (isGroup(item)) {
    const keys = writeOrder(item, sorted);
    // An empty object in a list has no key to hang on the `- ` line.
    if (keys.length === 0) {
      out.push(`${pad}-`);
      return;
    }
    const names = keyNames(item, keys);
    // The `- ` line has to carry a key whose value fits on it; SYNX attaches
    // everything deeper to the *item*, not to that key.
    let head = -1;
    for (let i = 0; i < keys.length && head < 0; i++) if (dashLineSafe(item[keys[i]])) head = i;
    // §14.1 — an item is an object only when a deeper line follows its `- `
    // line: a one-key item came back as a string, and an item with no value
    // that fits the line lost its head. Writing the head again in full below
    // covers both (a repeated key is legal; the last one wins).
    const repeatHead = head < 0 || keys.length === 1;
    if (head < 0) head = 0;
    const headKey = keys[head];
    const text = synxScalarText(item[headKey]);
    const name = names?.get(headKey) ?? headKey;
    out.push(text === null || text === '' ? `${pad}- ${name}` : `${pad}- ${name} ${text}`);
    for (let i = 0; i < keys.length; i++) {
      if (i === head && !repeatHead) continue;
      emitNamedEntry(names?.get(keys[i]) ?? keys[i], item[keys[i]], indent + step, out, step, depth + 1, sorted);
    }
    return;
  }
  if (Array.isArray(item)) {
    // A list inside a list — SYNX has no syntax for it; a single-key wrapper
    // keeps the data from being dropped without a word.
    out.push(`${pad}- _value`);
    emitNamedEntry('_value', item, indent + step, out, step, depth + 1, sorted);
    return;
  }
  const text = synxScalarText(item);
  if (text !== null) {
    out.push(`${pad}- ${text}`);
    return;
  }
  // safety: a list item is one line and SYNX has no block form for it, so a
  // line break is written as a space. Written as it was, the rest of the text
  // became lines of its own: keys in a `.synx`, forged records in a SYNXL block.
  out.push(`${pad}- ${typeof item === 'string' ? item.replace(LINE_BREAKS, ' ') : ''}`);
}

/** `\n` and `\r`, one at a time: nothing here can backtrack. */
const LINE_BREAKS = /[\n\r]/g;

/** Write `name value` as SYNX at `indent` columns; `name` is the key as it goes on the line. */
function emitNamedEntry(
  name: string, value: SynxValue, indent: number, out: string[], step: number, depth: number, sorted: boolean,
): void {
  if (depth > MAX_WRITE_DEPTH) return;
  const pad = ' '.repeat(indent);
  if (Array.isArray(value)) {
    // A bare key reads back as an empty object; the `:join` list marker is
    // the SYNX surface that yields an empty array, as synx-core writes it.
    out.push(value.length === 0 ? `${pad}${name}:join` : `${pad}${name}`);
    for (let i = 0; i < value.length; i++) emitListItem(value[i], indent + step, out, step, depth + 1, sorted);
    return;
  }
  if (isGroup(value)) {
    out.push(`${pad}${name}`);
    emitObject(value, indent + step, out, step, depth + 1, sorted);
    return;
  }
  const text = synxScalarText(value);
  if (text !== null) {
    out.push(`${pad}${name} ${text}`);
    return;
  }
  // Multi-line (or unquotable) text → §14.3 mandates `|+`, which preserves
  // indentation relative to the first continuation line.
  emitMultiline(name, typeof value === 'string' ? value : '', indent, out, step);
}

/** Write the entries of one object at `indent` columns. */
function emitObject(obj: SynxObject, indent: number, out: string[], step: number, depth: number, sorted: boolean): void {
  const keys = writeOrder(obj, sorted);
  const names = keyNames(obj, keys);
  for (let i = 0; i < keys.length; i++) {
    emitNamedEntry(names?.get(keys[i]) ?? keys[i], obj[keys[i]], indent, out, step, depth, sorted);
  }
}

/** A field of a SYNXL record written as a block entry (§14.3), under its field name. */
function emitBlockEntry(key: string, value: SynxValue, indent: number, out: string[], step: number): void {
  emitNamedEntry(key, value, indent, out, step, 0, false);
}

/**
 * Write an object as a SYNX document body that parses back to the same data
 * — the block writer of §14.1. Used by `Synx.stringify` and `Synx.decompile`.
 *
 * Keys go in the order the object has them (for a parsed document, the order
 * of the document); with `sorted`, by code point at every level, as synx-core
 * writes a document it has no order for.
 */
export function writeSynxBody(obj: SynxObject, step = 2, sorted = false): string {
  const out: string[] = [];
  emitObject(obj, 0, out, step, 0, sorted);
  return out.length ? out.join('\n') + '\n' : '';
}

// ─── Declared writer ──────────────────────────────────────

/** A constraint bound as synx-core prints it: no exponent, no trailing `.0`. */
function boundText(n: number): string {
  if (Number.isNaN(n)) return 'NaN';
  if (!Number.isFinite(n)) return n > 0 ? 'inf' : '-inf';
  return Object.is(n, -0) ? '-0' : expandExponent(String(n));
}

/** `name(type)[constraints]:markers` as the parser reads it back. */
function declaredHeader(name: string, meta: SynxMeta | undefined): string {
  if (meta === undefined) return name;
  let header = name;
  if (meta.typeHint !== undefined) header += `(${meta.typeHint})`;
  const c = meta.constraints;
  if (c) {
    const parts: string[] = [];
    if (c.type !== undefined) parts.push(`type:${c.type}`);
    if (c.required) parts.push('required');
    if (c.readonly) parts.push('readonly');
    if (c.min !== undefined) parts.push(`min:${boundText(c.min)}`);
    if (c.max !== undefined) parts.push(`max:${boundText(c.max)}`);
    if (c.pattern !== undefined) parts.push(`pattern:${c.pattern}`);
    if (c.enum !== undefined) parts.push(`enum:${c.enum.join('|')}`);
    header += `[${parts.join(', ')}]`;
  }
  for (let i = 0; i < meta.markers.length; i++) header += `:${meta.markers[i]}`;
  return header;
}

function emitDeclaredObject(obj: SynxObject, indent: number, out: string[], depth: number): void {
  if (depth > MAX_WRITE_DEPTH) return;
  const metaMap = metaMapOf(obj);
  const keys = keysOf(obj);
  const names = keyNames(obj, keys);
  for (let i = 0; i < keys.length; i++) {
    const key = keys[i];
    emitDeclaredEntry(names?.get(key) ?? key, metaMap?.get(key), obj[key], indent, out, depth);
  }
}

function emitDeclaredEntry(
  name: string, meta: SynxMeta | undefined, value: SynxValue, indent: number, out: string[], depth: number,
): void {
  if (depth > MAX_WRITE_DEPTH) return;
  const header = declaredHeader(name, meta);
  // `tier:random 90 5 5` and `child:inherit base` keep their arguments on
  // the key line; the parser moved them out of the value.
  const args = meta !== undefined && meta.args ? meta.args : NO_ARGS;
  const emptyList = Array.isArray(value) && value.length === 0;
  if (meta === undefined && !emptyList) {
    emitDeclaredBody(header, meta, args, value, indent, out, depth);
    return;
  }
  // spec: a key keeps the declarations of its first line and the value of
  // its last (`x(int)` / `x h` is `"h"` declared `int`), as in synx-core
  // (B-082). Written as one line, `x(int) h`, the declaration acted on the
  // value again and gave `0`; `b:join` with a group gave a list, `a:inherit`
  // with a text took the text as parents. When the one line does not read
  // back as this value, the declaration goes on a line of its own and the
  // value follows on a plain line, which keeps the declarations.
  let readsBack: boolean;
  if (Array.isArray(value) || isGroup(value)) {
    let line = header;
    for (const arg of args) line += ` ${arg}`;
    readsBack = declarationReadsBack(line, name, value);
  } else {
    const chunk: string[] = [];
    emitDeclaredBody(header, meta, args, value, 0, chunk, depth);
    readsBack = declarationReadsBack(chunk.join('\n'), name, value);
  }
  if (readsBack) {
    emitDeclaredBody(header, meta, args, value, indent, out, depth);
    return;
  }
  const pad = ' '.repeat(indent);
  if (meta !== undefined) {
    let line = pad + header;
    for (const arg of args) line += ` ${arg}`;
    out.push(line);
  }
  if (emptyList) {
    // A bare key is an empty list when the next line is a `- ` line that no
    // open list takes (§8.5.2) — at the root no list is open. Deeper down an
    // outer list could take it, so there the key stays a group.
    out.push(pad + name);
    if (indent === 0) out.push('- _');
    return;
  }
  if (Array.isArray(value) || isGroup(value)) emitDeclaredBody(name, undefined, NO_ARGS, value, indent, out, depth);
  else emitNamedEntry(name, value, indent, out, 2, depth, false);
}

const NO_ARGS: readonly string[] = [];

/**
 * Whether `chunk`, the lines of one root entry, gives `name` this value when
 * read in an `!active` document — for an object or a list, whose lines
 * follow, a container of the same kind with nothing in it yet.
 * `declaration_reads_back` in synx-core.
 */
function declarationReadsBack(chunk: string, name: string, value: SynxValue): boolean {
  const root = parseData(`!active\n${chunk}`).root;
  if (!Object.prototype.hasOwnProperty.call(root, name)) return false;
  const got = root[name];
  const gotGroup = got !== null && typeof got === 'object' && !Array.isArray(got);
  const gotEmptyGroup = gotGroup && Object.keys(got as SynxObject).length === 0;
  if (Array.isArray(value)) {
    if (Array.isArray(got)) return got.length === 0;
    return value.length > 0 && gotEmptyGroup;
  }
  if (isGroup(value)) return gotEmptyGroup;
  return got === value;
}

/** The lines of one declared entry, its key line starting with `header`. */
function emitDeclaredBody(
  header: string, meta: SynxMeta | undefined, args: readonly string[], value: SynxValue,
  indent: number, out: string[], depth: number,
): void {
  const pad = ' '.repeat(indent);
  if (Array.isArray(value) || isGroup(value)) {
    let line = pad + header;
    for (const arg of args) line += ` ${arg}`;
    out.push(line);
    if (Array.isArray(value)) {
      for (let i = 0; i < value.length; i++) {
        const item = value[i];
        if (isGroup(item)) emitDeclaredItem(item, indent + 2, out, depth + 1);
        else emitListItem(item, indent + 2, out, 2, depth + 1, false);
      }
    } else {
      emitDeclaredObject(value, indent + 2, out, depth + 1);
    }
    return;
  }
  let text: string | null;
  if (meta !== undefined && meta.typeHint === 'string' && typeof value === 'string') {
    // spec: `(string)` takes the text as written, quotes included, so it is
    // written bare or — when a bare line cannot carry it — as a block, never
    // quoted.
    const bare = value !== '' && value.indexOf('\n') === -1
      && !isUnicodeSpace(value.charCodeAt(0)) && !isUnicodeSpace(value.charCodeAt(value.length - 1))
      && value.indexOf(' #') === -1 && value.indexOf(' //') === -1 && value !== '|' && value !== '|+';
    text = bare ? value : null;
  } else {
    text = synxScalarText(value);
  }
  if (text !== null) out.push(`${pad}${header} ${text}`);
  else emitMultiline(header, typeof value === 'string' ? value : '', indent, out, 2);
}

/**
 * One list item that is an object: the first key rides the `- ` line, as it
 * did in the source, and the rest follow in order.
 */
function emitDeclaredItem(fields: SynxObject, indent: number, out: string[], depth: number): void {
  if (depth > MAX_WRITE_DEPTH) return;
  const pad = ' '.repeat(indent);
  const keys = keysOf(fields);
  if (keys.length === 0) {
    out.push(`${pad}-`);
    return;
  }
  const metaMap = metaMapOf(fields);
  const names = keyNames(fields, keys);
  const headKey = keys[0];
  const headValue = fields[headKey];
  const headMeta = metaMap?.get(headKey);
  const headName = names?.get(headKey) ?? headKey;
  let line = `${pad}- `;
  let skipHead = false;
  let headLine: string | null = null;
  if (dashLineSafe(headValue) && !(headMeta !== undefined && headMeta.typeHint === 'string')) {
    headLine = declaredHeader(headName, headMeta);
    const text = synxScalarText(headValue);
    if (text !== null && text !== '') headLine += ` ${text}`;
    // The declaration must not act on the value a second time (B-082).
    if (headMeta !== undefined && !declarationReadsBack(headLine, headName, headValue)) headLine = null;
  }
  if (headLine !== null) {
    line += headLine;
    // An item is an object only when a deeper line follows its `- ` line.
    skipHead = keys.length > 1;
  } else {
    // The head does not fit the dash line: a bare key opens the item and the
    // entry is written in full below (the later one wins).
    line += headName;
  }
  out.push(line);
  for (let i = 0; i < keys.length; i++) {
    if (i === 0 && skipHead) continue;
    const key = keys[i];
    emitDeclaredEntry(names?.get(key) ?? key, metaMap?.get(key), fields[key], indent + 2, out, depth + 1);
  }
}

/**
 * Write an `!active` document with the declarations it carries: every key as
 * `key(type)[constraints]:markers value`, in the order the tree has — the
 * order its markers resolve in. Used by `Synx.decompile`.
 */
export function writeSynxDeclared(root: SynxObject): string {
  const out: string[] = [];
  emitDeclaredObject(root, 0, out, 0);
  return out.length ? out.join('\n') + '\n' : '';
}

/** A group of records sharing one field list, as emitted by the writer. */
interface WriteGroup {
  fields: SynxlField[];
  records: SynxObject[];
}

function synthesizeFields(records: readonly SynxObject[]): SynxlField[] {
  const names: string[] = [];
  const seen = new Set<string>();
  for (const rec of records) {
    for (const k of Object.keys(rec)) {
      if (seen.has(k)) continue;
      seen.add(k);
      names.push(k);
    }
  }
  return names.map<SynxlField>(name => ({ name, block: false, decl: name }));
}

function emitGroup(group: WriteGroup, out: string[], step: number): void {
  // §14.3 — a field whose value cannot live inline in *any* record must be a
  // block field for the whole group; inline arity is a per-group constant.
  const blockNames = new Set<string>();
  for (const f of group.fields) if (f.block) blockNames.add(f.name);
  // perf: each cell's inline text is computed once here and reused below —
  // it was computed twice, once to decide promotion and once to write.
  const types = group.fields.map(fieldType);
  const texts: (string | null)[][] = [];
  for (const rec of group.records) {
    const row: (string | null)[] = [];
    for (let i = 0; i < group.fields.length; i++) {
      const f = group.fields[i];
      if (blockNames.has(f.name)) { row.push(null); continue; }
      const t = inlineText(rec[f.name] ?? null, types[i]);
      if (t === null) blockNames.add(f.name);
      row.push(t);
    }
    texts.push(row);
  }

  if (blockNames.size === group.fields.length) {
    // §5.3.4 forbids a zero-arity field list, and §7.4 offers no escapes, so a
    // single-field group whose only value needs promotion has no valid
    // rendering. Fail loudly rather than emit a document that cannot be read.
    hardError(
      'ZeroArityFieldList',
      'cannot write these records: every field must be promoted to [block], which would leave inline arity 0 — add an inline field (an id) to the record shape',
      1,
    );
  }

  const decls = group.fields.map(f => {
    if (!blockNames.has(f.name)) return f.decl;
    if (f.block) return f.decl;
    return `${f.name}[block]`; // promoted: `block` cannot be combined with a type
  });
  out.push(`!fields ${decls.join('; ')}`);

  const inlineIdx: number[] = [];
  for (let i = 0; i < group.fields.length; i++) if (!blockNames.has(group.fields[i].name)) inlineIdx.push(i);
  for (let r = 0; r < group.records.length; r++) {
    const rec = group.records[r];
    const row = texts[r];
    const parts: string[] = [];
    for (const i of inlineIdx) {
      // A field promoted after this row was scanned has no cached text; it is
      // block now, so it is not in `inlineIdx` either.
      parts.push(row[i] ?? '');
    }
    // §7.2 — a record whose every inline field is null renders as the all-null
    // form, exactly `;`, which the reader recognizes at any arity without
    // diagnostics. Emitting empty parts instead would give a blank line at
    // arity 1, and §3.5 makes blank lines invisible.
    const line = parts.every(t => t === '')
      ? ';'
      : trimTrailingSpacesTabs(parts.join('; '));
    out.push(line);

    for (const f of group.fields) {
      if (!blockNames.has(f.name)) continue;
      const v = rec[f.name];
      if (v === null || v === undefined) continue; // §9.2 — absent block is null
      emitBlockEntry(f.name, v, step, out, step);
    }
  }
}

/**
 * Serialize records back to SYNXL text (SYNXL §14).
 *
 * Accepts either a parsed {@link SynxlDocument} — in which case the original
 * field lists (including mid-file schema changes) are preserved — or a plain
 * array of objects, whose field list is derived from the keys.
 *
 * Quoting and block promotion are automatic (§14.3), so
 * `parseSynxl(writeSynxl(parseSynxl(d)))` projects to the same JSON as
 * `parseSynxl(d)` (§14.1).
 */
export function writeSynxl(
  input: SynxlDocument | readonly SynxObject[],
  options: SynxlWriteOptions = {},
): string {
  const step = options.indent ?? 2;
  const out: string[] = [`!synxl ${SYNXL_VERSION}`];

  if (Array.isArray(input)) {
    const records = input as readonly SynxObject[];
    emitGroup({ fields: synthesizeFields(records), records: [...records] }, out, step);
    return out.join('\n') + '\n';
  }

  const doc = input as SynxlDocument;
  if (doc === null || typeof doc !== 'object' || !Array.isArray(doc.records) || !Array.isArray(doc.fieldLists)) {
    throw new SynxError('INPUT_ERR: writeSynxl expects a parsed document or an array of records');
  }
  const byList = new Map<SynxlFieldList, SynxObject[]>();
  for (const fl of doc.fieldLists) byList.set(fl, []);
  for (const r of doc.records) {
    const bucket = byList.get(r.fields);
    if (bucket) bucket.push(r.values);
    else byList.set(r.fields, [r.values]);
  }
  for (const [fl, records] of byList) {
    emitGroup({ fields: fl.fields, records }, out, step);
  }
  return out.join('\n') + '\n';
}
