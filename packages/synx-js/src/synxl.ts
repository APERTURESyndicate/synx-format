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

import { parseData, castType, parseConstraints } from './parser';
import { toCanonicalJSONString, compareUnicodeScalar } from './json';
import type {
  SynxObject,
  SynxValue,
  SynxConstraints,
  SynxlDiagnostic,
  SynxlDiagnosticKind,
  SynxlDocument,
  SynxlField,
  SynxlFieldList,
  SynxlOptions,
  SynxlRecord,
  SynxlWriteOptions,
} from './types';
import { SynxError } from './types';

// ─── Format identity & resource limits (SYNXL §1.3, §13) ──

/** The SYNXL format version implemented here. */
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

/** Type hints whose result varies between reads — forbidden by SYNXL §8.3. */
const NON_DETERMINISTIC_HINTS = new Set([
  'random',
  'random:int',
  'random:float',
  'random:bool',
]);

const UNSAFE_KEYS = new Set(['__proto__', 'constructor', 'prototype']);

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
  if (UNSAFE_KEYS.has(key)) {
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
 * Offset of the first character SYNX itself would treat as content — SYNX
 * counts only spaces and tabs as indentation, so this is the frame of
 * reference for locating a key inside a delegated block (§9.3).
 */
function synxIndentOf(line: string): number {
  let i = 0;
  while (i < line.length) {
    const ch = line.charCodeAt(i);
    if (ch !== 32 && ch !== 9) break;
    i++;
  }
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
interface SynxlPart {
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
  const n = line.length;
  let i = 0;

  for (;;) {
    // 1. Skip spaces and horizontal tabs.
    while (i < n) {
      const ch = line.charCodeAt(i);
      if (ch !== 32 && ch !== 9) break;
      i++;
    }

    // 2. Quoted part?
    const q = line.charCodeAt(i);
    if (i < n && (q === 34 || q === 39)) {
      const close = line.indexOf(line[i], i + 1);
      if (close !== -1) {
        let j = close + 1;
        while (j < n) {
          const ch = line.charCodeAt(j);
          if (ch !== 32 && ch !== 9) break;
          j++;
        }
        if (j >= n || line.charCodeAt(j) === 59) { // end of line or ';'
          parts.push({ value: line.substring(i + 1, close), quoted: true });
          if (j >= n) return parts;
          i = j + 1;
          continue;
        }
      }
    }

    // 3. Unquoted part — extends to the next `;` or to end of line.
    const semi = line.indexOf(';', i);
    const end = semi === -1 ? n : semi;
    parts.push({ value: trimSpacesTabs(line.substring(i, end)), quoted: false });
    if (semi === -1) return parts;
    i = semi + 1;
  }
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
  switch (type) {
    case 'int': {
      const t = raw.trim();
      if (!INT_RE.test(t)) return { ok: false, value: null };
      // Same i64→number treatment as the SYNX parser: values beyond 2^53-1
      // lose precision in JS. Kept identical on purpose.
      return { ok: true, value: parseInt(t, 10) };
    }
    case 'float': {
      const t = raw.trim();
      if (!FLOAT_RE.test(t)) return { ok: false, value: null };
      const n = Number(t);
      if (!Number.isFinite(n)) return { ok: false, value: null };
      return { ok: true, value: n };
    }
    case 'bool': {
      const t = raw.trim();
      if (t === 'true') return { ok: true, value: true };
      if (t === 'false') return { ok: true, value: false };
      return { ok: false, value: null };
    }
    case 'string':
      return { ok: true, value: raw };
    default:
      // Unknown hint → automatic cast, matching SYNX `cast_typed`'s fallback.
      return { ok: true, value: castAuto(raw) };
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
      if (dotPos === -1) return parseInt(raw, 10);
      if (dotPos > firstDigit && dotPos < len - 1) return parseFloat(raw);
    }
  }
  return raw;
}

// ─── §5 Field list ────────────────────────────────────────

/** Parse one `name(type)[constraints]` declaration (SYNXL §5). */
function parseFieldDecl(decl: string, line: number): SynxlField {
  const src = decl.trim();
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
  if (!name) {
    if (src.charCodeAt(0) === 58) {
      hardError('MarkerChainInFieldDecl', `a marker is not allowed in a field declaration "${src}"`, line);
    }
    hardError('MalformedFieldList', `malformed field declaration "${src}"`, line);
  }
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

  // Marker chains drive the !active engine and have no meaning in a record.
  if (pos < len && src.charCodeAt(pos) === 58) {
    hardError('MarkerChainInFieldDecl', `a marker is not allowed in a field declaration "${src}"`, line);
  }

  while (pos < len) {
    const ch = src.charCodeAt(pos);
    if (ch !== 32 && ch !== 9) break;
    pos++;
  }
  if (pos < len) hardError('MalformedFieldList', `malformed field declaration "${src}"`, line);

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

  const type = typeHint || constraints?.type;

  if (type && NON_DETERMINISTIC_HINTS.has(type)) {
    hardError('NonDeterministicTypeHint', `non-deterministic type hint "${type}" is forbidden in SYNXL`, line);
  }
  if (block && type) {
    hardError('BlockCombinedWithType', `field "${name}" combines [block] with a type — the shape of a block value comes from the embedded SYNX document`, line);
  }

  const field: SynxlField = { name, block, decl: src };
  if (type) field.type = type;
  if (constraints && Object.keys(constraints).length > 0) field.constraints = constraints;
  return field;
}

/** Parse a whole `!fields` line (SYNXL §5). */
function parseFieldList(trimmed: string, line: number): SynxlFieldList {
  const rest = trimmed.substring('!fields'.length);
  if (rest.length === 0 || (rest.charCodeAt(0) !== 32 && rest.charCodeAt(0) !== 9)) {
    hardError('MalformedFieldList', 'malformed field list: expected whitespace after !fields', line);
  }
  const body = rest.trim();
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
function checkConstraints(name: string, value: SynxValue, c: SynxConstraints): string | null {
  if (c.required && (value === null || value === undefined || value === '')) {
    return `'${name}' is required`;
  }
  if (value === null || value === undefined) return null;

  if (c.type) {
    const ok = (() => {
      switch (c.type) {
        case 'int': return typeof value === 'number' && Number.isInteger(value);
        case 'float': return typeof value === 'number';
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

  const n = typeof value === 'number'
    ? value
    : typeof value === 'string' && (c.min !== undefined || c.max !== undefined)
      ? value.length
      : null;
  if (n !== null) {
    if (c.min !== undefined && n < c.min) return `'${name}' value ${n} is below min ${c.min}`;
    if (c.max !== undefined && n > c.max) return `'${name}' value ${n} exceeds max ${c.max}`;
  }

  if (c.pattern && typeof value === 'string' && c.pattern.length <= 128) {
    try {
      if (!new RegExp(c.pattern).test(value)) {
        return `'${name}' does not match pattern /${c.pattern}/`;
      }
    } catch { /* invalid regex — skip, as the SYNX engine does */ }
  }
  return null;
}

// ─── Reader ───────────────────────────────────────────────

interface PendingRecord {
  line: number;
  recordLine: string;
  blockLines: string[];
  /** Source line number of each entry in `blockLines` (§11.2 line attribution). */
  blockLineNos: number[];
  fields: SynxlFieldList;
  bytesLeft: number;
  truncated: boolean;
}

/** Key token of a SYNX line: text up to whitespace, `(`, `[` or `:`. */
function keyTokenOf(line: string, from: number): string {
  let i = from;
  while (i < line.length) {
    const ch = line.charCodeAt(i);
    if (ch === 32 || ch === 9 || ch === 40 || ch === 91 || ch === 58) break;
    i++;
  }
  return line.substring(from, i);
}

/**
 * Source line of a block's top-level `key` (§11.2): the block's root keys sit
 * at its shallowest indent, so prefer a match there and fall back to any
 * matching line, then to the record line.
 */
function blockKeyLine(p: PendingRecord, key: string): number {
  let min = Infinity;
  for (const l of p.blockLines) {
    const ind = synxIndentOf(l);
    if (ind === l.length) continue;
    if (ind < min) min = ind;
  }
  let fallback = -1;
  for (let i = 0; i < p.blockLines.length; i++) {
    const l = p.blockLines[i];
    const ind = synxIndentOf(l);
    if (ind === l.length) continue;
    if (keyTokenOf(l, ind) !== key) continue;
    if (ind === min) return p.blockLineNos[i];
    if (fallback < 0) fallback = p.blockLineNos[i];
  }
  return fallback >= 0 ? fallback : p.line;
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

    // §3.4 — indent > 0 is always block content.
    if (indent > 0) {
      if (this.inBlockComment) return null;
      if (this.pending) {
        this.appendBlockLine(line, lineNo);
      } else if (!this.prologueSeen) {
        // §4.1 — an indented line is still a non-empty, non-comment line, so a
        // document that opens with one has no prologue.
        hardError('MissingOrMalformedPrologue', 'missing prologue: the first significant line must be "!synxl 1"', lineNo);
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
    const trimmed = line.trim();

    if (trimmed === '###') { // §4.3 block-comment toggle
      this.inBlockComment = !this.inBlockComment;
      return done;
    }
    if (this.inBlockComment) return done;

    // §4.3 — `!synxl` / `!fields` are matched before the comment rules, and
    // both start with `!`, so plain prefix checks are unambiguous here.
    if (trimmed.charCodeAt(0) === 33) {
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

    if (trimmed.charCodeAt(0) === 35) return done;                       // # comment
    if (trimmed.charCodeAt(0) === 47 && trimmed.charCodeAt(1) === 47) return done; // // comment

    if (!this.prologueSeen) {
      hardError('MissingOrMalformedPrologue', 'missing prologue: the first significant line must be "!synxl 1"', lineNo);
    }
    if (!this.fields) {
      hardError('RecordWithoutFieldList', 'record line with no field list in effect', lineNo);
    }

    // §6 — record line. The SYNX §7 first-character filter is NOT applied:
    // `-5`, `[unparsed]`, `/var/log/app` and `@handle` are ordinary values.
    const budget = this.maxRecordBytes;
    const cost = utf8Length(line) + 1;
    let recordLine = line;
    let truncated = false;
    if (cost > budget) {
      recordLine = truncateUtf8(line, budget);
      truncated = true;
    }
    this.pending = {
      line: lineNo,
      recordLine,
      blockLines: [],
      blockLineNos: [],
      fields: this.fields,
      bytesLeft: truncated ? 0 : budget - cost,
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
    if (p.bytesLeft <= 0) { p.truncated = true; return; }
    const cost = utf8Length(line) + 1;
    if (cost > p.bytesLeft) {
      p.blockLines.push(truncateUtf8(line, p.bytesLeft));
      p.blockLineNos.push(lineNo);
      p.bytesLeft = 0;
      p.truncated = true;
      return;
    }
    p.bytesLeft -= cost;
    p.blockLines.push(line);
    p.blockLineNos.push(lineNo);
  }

  private handlePrologue(trimmed: string, lineNo: number): void {
    const rest = trimmed.substring('!synxl'.length).trim();
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
    const orphanDiags = this.orphans;
    this.orphans = [];
    for (const d of orphanDiags) d.record = index;
    const truncDiags: SynxlDiagnostic[] = [];
    const castDiags: SynxlDiagnostic[] = [];
    const arityDiags: SynxlDiagnostic[] = [];
    const blockDiags: SynxlDiagnostic[] = [];
    const constraintDiags: SynxlDiagnostic[] = [];

    const make = (
      into: SynxlDiagnostic[],
      kind: SynxlDiagnosticKind,
      line: number,
      message: string,
      field?: string,
    ): void => {
      const d: SynxlDiagnostic = { kind, record: index, line, message };
      if (field !== undefined) d.field = field;
      into.push(d);
    };

    if (p.truncated) {
      make(truncDiags, 'RecordTruncated', p.line,
        `record exceeds the ${this.maxRecordBytes}-byte budget and was truncated at a UTF-8 boundary`);
    } else if (this.unterminated) {
      make(truncDiags, 'RecordTruncated', p.line,
        'input ended without a newline — the final record may be incomplete');
    }

    const values: SynxObject = {};
    const { fields, arity } = p.fields;

    // §7.2 — the all-null record form. A record line that is exactly `;` sets
    // every inline field to null at any arity and reports nothing. It is
    // recognized before the §7.1 split, which would otherwise see two empty
    // parts and score the record against the arity.
    if (trimSpacesTabs(p.recordLine) === ';') {
      for (const f of fields) setField(values, f.name, null);
    } else {
      const parts = splitRecordLine(p.recordLine);

      // ── Inline fields (§7, §8) ──
      let inlineIdx = 0;
      for (const f of fields) {
        if (f.block) { setField(values, f.name, null); continue; }
        const part: SynxlPart | undefined = parts[inlineIdx++];
        if (part === undefined) { setField(values, f.name, null); continue; }
        if (part.quoted) {
          // §7.4 — literal inner text: no escapes, no casting, no comment strip.
          setField(values, f.name, part.value);
          continue;
        }
        if (part.value === '') { setField(values, f.name, null); continue; } // §7.2
        if (f.type) {
          const cast = castTyped(part.value, f.type);
          if (!cast.ok) {
            make(castDiags, 'CastFailed', p.line, `cannot cast "${part.value}" to ${f.type}`, f.name);
          }
          setField(values, f.name, cast.value);
        } else {
          setField(values, f.name, castAuto(part.value));
        }
      }

      if (parts.length < arity) {
        make(arityDiags, 'MissingFields', p.line,
          `record has ${parts.length} inline field(s), field list declares ${arity}; missing trailing fields are null`);
      } else if (parts.length > arity) {
        make(arityDiags, 'ExtraFields', p.line,
          `record has ${parts.length} inline field(s), field list declares ${arity}; surplus discarded`);
      }
    }

    // ── Block fields (§9) ──
    const blockText = p.blockLines.join('\n');
    if (blockText.trim() !== '') {
      // §9.3 — delegate to the SYNX parser with the block's ORIGINAL
      // indentation; SYNX's stack repair attaches the shallowest keys to the
      // sub-document root, so no dedent pre-pass is needed.
      // §9.4 — directives are disabled inside this parse (not pre-filtered,
      // which would corrupt `|+` bodies).
      const { root } = parseData(blockText, { directives: false });
      const byName = new Map<string, SynxlField>();
      for (const f of fields) byName.set(f.name, f);

      // §9.3 — visit in Unicode scalar order so diagnostics are reproducible.
      const keys = Object.keys(root).filter(k => !k.startsWith('__synx')).sort(compareUnicodeScalar);
      for (const key of keys) {
        const f = byName.get(key);
        if (f && f.block) {
          setField(values, key, root[key]);
        } else if (f) {
          make(blockDiags, 'BlockFieldNotDeclared', blockKeyLine(p, key),
            `block key "${key}" is not declared [block]; the inline value is authoritative`, key);
        } else {
          make(blockDiags, 'UnknownBlockKey', blockKeyLine(p, key),
            `block key "${key}" matches no field in the field list`, key);
        }
      }
    }

    // ── Opt-in validation (§8.4) ──
    if (this.opts.validate) {
      for (const f of fields) {
        if (!f.constraints) continue;
        const msg = checkConstraints(f.name, values[f.name], f.constraints);
        // §11.2 — an inline field reports the record line, a block field the
        // line its key sits on.
        if (msg) make(constraintDiags, 'ConstraintViolation', f.block ? blockKeyLine(p, f.name) : p.line, msg, f.name);
      }
    }

    const diagnostics = [
      ...orphanDiags,
      ...truncDiags,
      ...castDiags,
      ...arityDiags,
      ...blockDiags,
      ...constraintDiags,
    ];
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
    hardError('MissingOrMalformedPrologue', 'missing prologue: the first significant line must be "!synxl 1"', 1);
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
    const lines = source.split('\n');
    for (const line of lines) {
      const rec = reader.push(line, ++lineNo);
      if (rec) yield rec;
    }
    if (options.reportTruncatedTail && source.length > 0 && !source.endsWith('\n')) {
      reader.markUnterminated();
    }
  } else {
    for (const line of source) {
      const rec = reader.push(line, ++lineNo);
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

  for await (const chunk of source) {
    if (typeof chunk === 'string') {
      buf += chunk;
    } else {
      decoder = decoder ?? new TextDecoder('utf-8');
      buf += decoder.decode(chunk, { stream: true });
    }
    let nl = buf.indexOf('\n');
    while (nl !== -1) {
      const line = buf.substring(0, nl);
      buf = buf.substring(nl + 1);
      const rec = reader.push(line, ++lineNo);
      if (rec) yield rec;
      nl = buf.indexOf('\n');
    }
  }
  if (decoder) buf += decoder.decode();

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
  return toCanonicalJSONString(values, false);
}

function asDocument(source: string | SynxlDocument, options?: SynxlOptions): SynxlDocument {
  return typeof source === 'string' ? parseSynxl(source, options) : source;
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
 * line, `LF`-separated, with no enclosing array and no trailing newline.
 */
export function synxlToNDJSON(source: string | SynxlDocument, options: SynxlOptions = {}): string {
  const doc = asDocument(source, options);
  const out: string[] = [];
  for (const r of doc.records) out.push(canonicalJson(r.values));
  return out.join('\n');
}

// ─── §14 Canonical serialization (writer) ─────────────────

/** True for values that can sit on the first line of a `- ` list item. */
function isInlineScalar(v: SynxValue): boolean {
  if (v === null) return true;
  if (typeof v === 'object') return false;
  return synxScalarText(v) !== null;
}

/**
 * Render a number in a form SYNX can read back (§14.3): SYNX §8.3 recognizes
 * only `-?digits[.digits]`, so exponent forms must be expanded. Returns null
 * for a non-finite value, which has no SYNX representation.
 *
 * Note: JS has a single number type, so an integral value is always emitted in
 * integer form; the spec's "integral floats carry `.0`" rule is unobservable
 * here and does not affect the projection (both read back as the same number).
 */
function numberText(n: number): string | null {
  if (!Number.isFinite(n)) return null;
  const s = String(n); // shortest form that round-trips through Number()
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

/**
 * Render a scalar for a SYNX **block** value, or null when it needs a `|+`
 * multiline block (multi-line text, or text that quoting cannot express).
 */
function synxScalarText(value: SynxValue): string | null {
  if (value === null) return 'null';
  if (typeof value === 'number') return numberText(value) ?? 'null';
  if (typeof value === 'boolean') return String(value);
  if (typeof value !== 'string') return null;
  if (value === '') return '""';
  if (value.indexOf('\n') !== -1 || value.indexOf('\r') !== -1) return null;

  const needsQuote =
    value !== value.trim() ||
    value.indexOf(' //') !== -1 ||
    value.indexOf(' #') !== -1 ||
    value === '|' || value === '|+' ||
    value.charCodeAt(0) === 34 || value.charCodeAt(0) === 39 ||
    castType(value) !== value;

  if (!needsQuote) return value;
  if (value.indexOf('"') === -1) return `"${value}"`;
  if (value.indexOf("'") === -1) return `'${value}'`;
  return null; // both quote characters present → carry it as a `|+` body
}

/**
 * Render a value for an **inline** record part, or null when the field must be
 * promoted to a block field (§14.3).
 */
function inlineText(value: SynxValue): string | null {
  if (value === null || value === undefined) return '';
  if (typeof value === 'boolean') return String(value);
  // §14.3 — a non-finite float is written as an empty part, reading back as
  // `null`, which is also what the JSON projection carries.
  if (typeof value === 'number') return numberText(value) ?? '';
  if (typeof value !== 'string') return null; // objects / arrays are block-only
  if (value === '') return '""';
  if (value.indexOf('\n') !== -1 || value.indexOf('\r') !== -1) return null;

  const needsQuote =
    value !== value.trim() ||
    value.indexOf(';') !== -1 ||
    value.startsWith('#') || value.startsWith('//') || value.startsWith('!') ||
    // §14.3 — a leading quote character would be re-read as an opening quote.
    value.charCodeAt(0) === 34 || value.charCodeAt(0) === 39 ||
    castAuto(value) !== value;

  if (!needsQuote) return value;
  if (value.indexOf('"') === -1) return `"${value}"`;
  if (value.indexOf("'") === -1) return `'${value}'`;
  return null; // contains `;` (or other trigger) plus both quote chars → block
}

function emitListItem(item: SynxValue, indent: number, out: string[], step: number): void {
  const pad = ' '.repeat(indent);
  if (item !== null && typeof item === 'object' && !Array.isArray(item)) {
    const entries = Object.entries(item as SynxObject).filter(([k]) => !k.startsWith('__synx'));
    if (entries.length === 0) { out.push(`${pad}- {}`); return; }
    // The SYNX list-item form requires a scalar first key on the `- ` line;
    // reorder so that one is available. Key order is not semantically
    // significant (the canonical projection sorts keys anyway).
    let first = entries.findIndex(([, v]) => isInlineScalar(v));
    if (first < 0) first = 0;
    const [fk, fv] = entries[first];
    const text = isInlineScalar(fv) ? synxScalarText(fv) : null;
    out.push(text === null ? `${pad}- ${fk}` : `${pad}- ${fk} ${text}`);
    for (let i = 0; i < entries.length; i++) {
      if (i === first) continue;
      emitBlockEntry(entries[i][0], entries[i][1], indent + step, out, step);
    }
    return;
  }
  if (item !== null && typeof item === 'object') {
    // Nested array inside a list — SYNX has no syntax for it; emit as a
    // single-key wrapper so the data is at least not silently dropped.
    out.push(`${pad}- _value`);
    emitBlockEntry('_value', item, indent + step, out, step);
    return;
  }
  const text = synxScalarText(item);
  out.push(`${pad}- ${text === null ? String(item) : text}`);
}

function emitBlockEntry(key: string, value: SynxValue, indent: number, out: string[], step: number): void {
  const pad = ' '.repeat(indent);
  if (Array.isArray(value)) {
    out.push(`${pad}${key}`);
    for (const item of value) emitListItem(item, indent + step, out, step);
    return;
  }
  if (value !== null && typeof value === 'object') {
    out.push(`${pad}${key}`);
    for (const [k, v] of Object.entries(value as SynxObject)) {
      if (k.startsWith('__synx')) continue;
      emitBlockEntry(k, v, indent + step, out, step);
    }
    return;
  }
  const text = synxScalarText(value);
  if (text !== null) { out.push(`${pad}${key} ${text}`); return; }

  // Multi-line (or unquotable) text → §14.3 mandates `|+`, which preserves
  // indentation relative to the first continuation line.
  out.push(`${pad}${key} |+`);
  const body = String(value).split('\n');
  for (const bodyLine of body) out.push(`${pad}${' '.repeat(step)}${bodyLine}`);
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
      if (k.startsWith('__synx') || seen.has(k)) continue;
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
  for (const rec of group.records) {
    for (const f of group.fields) {
      if (blockNames.has(f.name)) continue;
      if (inlineText(rec[f.name] ?? null) === null) blockNames.add(f.name);
    }
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

  for (const rec of group.records) {
    const parts: string[] = [];
    for (const f of group.fields) {
      if (blockNames.has(f.name)) continue;
      parts.push(inlineText(rec[f.name] ?? null) ?? '');
    }
    // §7.2 — a record whose every inline field is null renders as the all-null
    // form, exactly `;`, which the reader recognizes at any arity without
    // diagnostics. Emitting empty parts instead would give a blank line at
    // arity 1, and §3.5 makes blank lines invisible.
    const line = parts.every(t => t === '')
      ? ';'
      : parts.join('; ').replace(/[ \t]+$/, '');
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
