/**
 * SYNX Parser — @aperturesyndicate/synx-format
 *
 * Converts raw .synx text into a structured object tree
 * with hidden metadata (__synx) for the Engine to resolve.
 *
 * Performance-optimized: charCode-based parsing, fast path
 * for simple key-value lines, no regex on hot paths.
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
 */
export function parseI64(text: string): number | bigint | null {
  let big: bigint;
  try {
    big = BigInt(text);
  } catch {
    return null;
  }
  if (big < I64_MIN || big > I64_MAX) return null;
  const asNumber = Number(big);
  return Number.isSafeInteger(asNumber) ? asNumber : big;
}

// ─── Resource limits (per SYNX 3.6 §3) ────────────────────

/** Maximum UTF-8 bytes accepted per parse() — text is truncated to a valid UTF-16 prefix not exceeding this length. */
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

// ─── Helpers ──────────────────────────────────────────────

/**
 * Unicode `White_Space` — the set SYNX §4's `ltrim` removes when measuring
 * indentation. Restricting this to space and tab makes a line opening with
 * U+00A0 look like a key at indent 0, which loses the block field it belonged
 * to and invents a diagnostic in its place.
 */
export function isUnicodeSpaceCode(c: number): boolean {
  return (c >= 0x09 && c <= 0x0d) || c === 0x20 || c === 0x85 || c === 0xa0 ||
    c === 0x1680 || (c >= 0x2000 && c <= 0x200a) ||
    c === 0x2028 || c === 0x2029 || c === 0x202f || c === 0x205f || c === 0x3000;
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

/** Longest prefix of `s` fitting `maxBytes` of UTF-8, never splitting a pair. */
export function truncateUtf8Prefix(s: string, maxBytes: number): string {
  if (maxBytes <= 0) return '';
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

/**
 * Cast a raw string value to a JS primitive (SYNX §8.3 automatic casting).
 *
 * Exported (additively) so that the SYNXL reader can reuse the exact same
 * numeric/boolean/quote semantics instead of growing a second caster —
 * SYNXL §8.1 defines inline casting by reference to this function.
 */
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
    case 'float': { const n = parseFloat(raw); return isNaN(n) ? 0 : n; }
    case 'bool': return raw.trim() === 'true';
    case 'string': return raw;
    // Match Rust `rng::random_i64()` as closely as JS can — full safe-integer
    // range including negatives. JS Number caps at 2^53-1 precisely; values
    // beyond that lose precision in any roundtrip with .synxb.
    case 'random':
    case 'random:int':
      if (!nondeterministic) return castType(raw, false);
      return Math.floor(Math.random() * Number.MAX_SAFE_INTEGER * 2) - Number.MAX_SAFE_INTEGER;
    case 'random:float':
      if (!nondeterministic) return castType(raw, false);
      return Math.random();
    case 'random:bool':
      if (!nondeterministic) return castType(raw, false);
      return Math.random() < 0.5;
    // Unknown hint — the bare value is cast as usual (Rust falls back to `cast`).
    default: return castType(raw, nondeterministic);
  }
}

export function castType(val: string, nondeterministic = true): SynxValue {
  const len = val.length;

  // Quoted strings preserve literal value (bypass auto-casting).
  // Per SYNX 3.6 §8.3.1: "<inner>" or '<inner>' with length >= 2 → string of inner text.
  if (len >= 2) {
    const c0q = val.charCodeAt(0);
    const cN = val.charCodeAt(len - 1);
    if ((c0q === 34 && cN === 34) || (c0q === 39 && cN === 39)) {
      return val.substring(1, len - 1);
    }
  }

  if (val === 'true') return true;
  if (val === 'false') return false;
  if (val === 'null') return null;

  if (len === 0) return val;

  const c0 = val.charCodeAt(0);

  // Auto number detection via charCode (no regex)
  let firstDigit = 0;
  let fc = c0;
  if (fc === 45) { // '-'
    if (len === 1) return val;
    firstDigit = 1;
    fc = val.charCodeAt(1);
  }
  if (fc >= 48 && fc <= 57) { // '0'-'9'
    let allNumeric = true;
    let dotPos = -1;
    for (let i = firstDigit + 1; i < len; i++) {
      const ch = val.charCodeAt(i);
      if (ch === 46) { // '.'
        if (dotPos !== -1) { allNumeric = false; break; }
        dotPos = i;
      } else if (ch < 48 || ch > 57) {
        allNumeric = false;
        break;
      }
    }
    if (allNumeric) {
      // SYNX §8.3: integer, then decimal float, then string. The reference
      // implementation parses into `i64`, so an integer that overflows it is a
      // string here too, and one that merely leaves JavaScript's safe range is
      // kept exact as a `bigint` — `parseInt` would round a 19-digit id to a
      // different id with nothing to signal it.
      if (dotPos === -1) return parseI64(val) ?? val;
      if (dotPos > firstDigit && dotPos < len - 1) return parseFloat(val);
    }
  }

  return val;
}

/** Strip inline comment from a value string */
function stripInlineComment(val: string): string {
  // Fast path: no space means no inline comment possible
  if (val.indexOf(' ') === -1) return val;

  let result = val;
  const slashIdx = result.indexOf(' //');
  if (slashIdx !== -1) result = result.substring(0, slashIdx);
  const hashIdx = result.indexOf(' #');
  if (hashIdx !== -1) result = result.substring(0, hashIdx);
  return result.trimEnd();
}

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
    const part = raw.substring(start, end).trim();
    start = end + 1;
    if (!part) continue;

    if (part === 'required') {
      constraints.required = true;
    } else if (part === 'readonly') {
      constraints.readonly = true;
    } else {
      const colonIdx = part.indexOf(':');
      if (colonIdx !== -1) {
        const key = part.substring(0, colonIdx).trim();
        const value = part.substring(colonIdx + 1).trim();
        switch (key) {
          case 'min': constraints.min = Number(value); break;
          case 'max': constraints.max = Number(value); break;
          case 'type': constraints.type = value; break;
          case 'pattern': constraints.pattern = value; break;
          case 'enum': constraints.enum = value.split('|').slice(0, MAX_CONSTRAINT_ENUM_PARTS); break;
        }
      }
    }
  }
  return constraints;
}

/** Attach hidden __synx metadata to an object */
function saveMeta(
  obj: SynxObject,
  key: string,
  markers: string[],
  args: string[],
  constraints: SynxConstraints | undefined,
  mode: SynxMode,
  typeHint?: string,
): void {
  if (mode !== 'active') return;
  if (markers.length === 0 && !constraints && !typeHint) return;

  let metaMap: SynxMetaMap;
  if ((obj as any).__synx) {
    metaMap = (obj as any).__synx;
  } else {
    metaMap = {};
    Object.defineProperty(obj, '__synx', {
      value: metaMap,
      enumerable: false,
      writable: true,
      configurable: true,
    });
  }

  const meta: SynxMeta = { markers };
  if (args.length > 0) meta.args = args;
  if (constraints) meta.constraints = constraints;
  if (typeHint) meta.typeHint = typeHint;
  metaMap[key] = meta;
}

/**
 * Parse a key line manually, supporting balanced brackets in [constraints].
 * Returns null if the line is not a valid key line.
 */
function parseKeyLine(trimmed: string): {
  key: string;
  typeHint?: string;
  constraintStr?: string;
  markerChain?: string;
  rawValue: string;
} | null {
  const len = trimmed.length;
  if (len === 0) return null;

  const first = trimmed.charCodeAt(0);
  // First char cannot be [, :, -, #, /, ( per §7
  if (first === 91 || first === 58 || first === 45 || first === 35 || first === 47 || first === 40) {
    return null;
  }

  // Extract key — stop at space, tab, [, :, (
  let pos = 0;
  while (pos < len) {
    const ch = trimmed.charCodeAt(pos);
    if (ch === 32 || ch === 9 || ch === 91 || ch === 58 || ch === 40) break;
    pos++;
  }
  if (pos === 0) return null;
  const key = trimmed.substring(0, pos);

  let typeHint: string | undefined;
  let constraintStr: string | undefined;
  let markerChain: string | undefined;

  // Optional (type)
  if (pos < len && trimmed.charCodeAt(pos) === 40) { // '('
    const closeIdx = trimmed.indexOf(')', pos + 1);
    if (closeIdx !== -1) {
      typeHint = trimmed.substring(pos + 1, closeIdx);
      pos = closeIdx + 1;
    } else {
      pos++;
    }
  }

  // Optional [constraints] — balanced bracket scan to support patterns like ^[A-Z]{2}$
  if (pos < len && trimmed.charCodeAt(pos) === 91) { // '['
    pos++;
    const cStart = pos;
    let depth = 1;
    while (pos < len && depth > 0) {
      const ch = trimmed.charCodeAt(pos);
      if (ch === 91) depth++;
      else if (ch === 93) depth--;
      if (depth > 0) pos++;
    }
    if (depth === 0) {
      constraintStr = trimmed.substring(cStart, pos);
      pos++; // skip closing ']'
    } else {
      // Unbalanced — fallback to first ']' if any
      const fb = trimmed.indexOf(']', cStart);
      if (fb !== -1) {
        constraintStr = trimmed.substring(cStart, fb);
        pos = fb + 1;
      } else {
        constraintStr = trimmed.substring(cStart);
        pos = len;
      }
    }
  }

  // Optional :markers
  if (pos < len && trimmed.charCodeAt(pos) === 58) { // ':'
    const mStart = pos + 1;
    let mEnd = mStart;
    while (mEnd < len) {
      const ch = trimmed.charCodeAt(mEnd);
      if (ch === 32 || ch === 9) break;
      mEnd++;
    }
    markerChain = trimmed.substring(mStart, mEnd);
    pos = mEnd;
  }

  // Skip whitespace, then take rest as value
  while (pos < len) {
    const ch = trimmed.charCodeAt(pos);
    if (ch !== 32 && ch !== 9) break;
    pos++;
  }
  const rawValue = pos < len ? trimmed.substring(pos) : '';

  return { key, typeHint, constraintStr, markerChain, rawValue };
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
}

export function parseData(text: string, options?: ParseDataOptions): SynxParseResult {
  const directivesEnabled = options?.directives !== false;
  const nondeterministic = options?.nondeterministicHints !== false;

  // A single leading U+FEFF is dropped (SYNX §2.1), as in synx-core. `trim()`
  // already hid it in front of `!active`, but a static document kept it glued
  // to its first key.
  if (text.charCodeAt(0) === 0xfeff) {
    text = text.slice(1);
  }

  // Truncate input to MAX_SYNX_INPUT_BYTES (UTF-16 code-unit boundary).
  if (text.length > MAX_SYNX_INPUT_BYTES) {
    text = text.substring(0, MAX_SYNX_INPUT_BYTES);
  }

  let lines = text.split('\n');
  if (lines.length > MAX_LINE_STARTS) {
    lines = lines.slice(0, MAX_LINE_STARTS);
  }

  const root: SynxObject = {};
  const stack: Array<{ indent: number; obj: SynxObject | SynxArray }> = [
    { indent: -1, obj: root },
  ];

  let mode: SynxMode = 'static';
  let locked = false;
  let llm = false;
  let tool = false;
  let schema = false;
  const includes: SynxInclude[] = [];
  // `preserveIndent` toggles SYNX 3.7 `|+` semantics — keep relative indent of
  // each continuation line instead of full per-line trim. `baseIndent` is the
  // indent of the first non-empty continuation line (used as the strip prefix).
  let currentBlock: {
    indent: number; obj: SynxObject; key: string;
    preserveIndent: boolean; baseIndent: number;
    // Accumulated continuation lines and their running UTF-8 byte length,
    // materialised (joined) only when the block closes — keeps it O(n).
    parts: string[]; byteLen: number;
  } | null = null;
  let currentList: { indent: number; arr: SynxArray } | null = null;
  let inBlockComment = false;

  for (let i = 0; i < lines.length; i++) {
    const rawLine = lines[i];
    const rawLen = rawLine.length;

    // ── Manual indent computation (no regex) ──
    //
    // Unicode-aware, matching SYNX §4's `length(raw) - length(ltrim(raw))`: a
    // line opening with U+00A0 or U+3000 is indented, so it stays block content
    // instead of turning into a spurious key. The scan and every slice below
    // work in the same UTF-16 frame, so no offset lands mid-character.
    let indent = 0;
    while (indent < rawLen) {
      if (!isUnicodeSpaceCode(rawLine.charCodeAt(indent))) break;
      indent++;
    }

    // ── Empty line ──
    if (indent === rawLen) continue;

    const fc = rawLine.charCodeAt(indent); // first non-whitespace char

    // ── Compute trimmed string (manual trim, no regex) ──
    let trimEndPos = rawLen;
    while (trimEndPos > indent) {
      const ch = rawLine.charCodeAt(trimEndPos - 1);
      if (ch !== 32 && ch !== 9 && ch !== 13 && ch !== 10) break;
      trimEndPos--;
    }
    const trimmed = rawLine.substring(indent, trimEndPos);
    const trimmedLen = trimmed.length;

    // ── Continue multiline block ──
    //
    // Ahead of every line-class rule below, and deliberately so: a `|` / `|+`
    // body is raw text (SYNX §8.4.1), so `#`, `//`, `###` and `!` lines inside
    // one are content. Classifying first deletes comments out of code samples,
    // lets a stray `###` swallow the rest of the document, and — for `!` —
    // turns a data row into a directive, which SYNXL §9.4 forbids outright.
    if (currentBlock && indent > currentBlock.indent) {
      // Accumulate into an array with a running byte count. Reading back the
      // whole `obj[key]` string and calling utf8Length on it every line made
      // this O(n²) — a block of a few thousand lines took tens of seconds. The
      // running counter and a single join at close keep it linear (Rust tracks
      // the byte length the same way).
      if (currentBlock.byteLen < MAX_MULTILINE_BLOCK_BYTES) {
        let line: string;
        if (currentBlock.preserveIndent) {
          // SYNX 3.7 `|+`: lock the base indent to the FIRST non-empty
          // continuation line, then strip exactly that prefix from every line.
          // Anything indented further than the base is preserved verbatim,
          // making the block safe for embedding indent-sensitive content
          // (code, SYNX examples, ASCII art, etc.).
          if (currentBlock.baseIndent < 0) currentBlock.baseIndent = indent;
          const strip = Math.min(indent, currentBlock.baseIndent);
          line = rawLine.substring(strip, trimEndPos);
        } else {
          line = trimmed;
        }
        const sepBytes = currentBlock.parts.length ? 1 : 0; // the '\n' join adds
        const room = MAX_MULTILINE_BLOCK_BYTES - currentBlock.byteLen - sepBytes;
        if (room > 0) {
          // Byte budget, UTF-8 boundary: `substring(0, room)` counts UTF-16
          // units, so it both overshoots the cap and can split a surrogate
          // pair, leaving a lone surrogate no strict JSON consumer accepts.
          const taken = truncateUtf8Prefix(line, room);
          currentBlock.parts.push(taken);
          currentBlock.byteLen += sepBytes + utf8Length(taken);
        }
      }
      continue;
    } else if (currentBlock) {
      currentBlock.obj[currentBlock.key] = currentBlock.parts.join('\n');
      currentBlock = null;
    }

    // ── Block comment toggle: ### ──
    if (fc === 35) {
      const rest = rawLine.substring(indent).trimEnd();
      if (rest === '###') {
        inBlockComment = !inBlockComment;
        continue;
      }
    }
    if (inBlockComment) continue;

    // ── Directives: !active / !lock / !llm / !tool / !schema / !include ──
    if (fc === 33) {
      if (directivesEnabled) {
        if (trimmed === '!active') { mode = 'active'; continue; }
        if (trimmed === '!lock') { locked = true; continue; }
        if (trimmed === '!llm') { llm = true; continue; }
        if (trimmed === '!tool') { tool = true; continue; }
        if (trimmed === '!schema') { schema = true; continue; }
        if (trimmed.startsWith('!include ')) {
          if (includes.length < MAX_INCLUDE_DIRECTIVES) {
            const parts = trimmed.substring(9).trim().split(/\s+/);
            const inclPath = parts[0];
            const alias = parts[1] || inclPath.replace(/^.*[\/\\]/, '').replace(/\.synx$/i, '');
            includes.push({ path: inclPath, alias });
          }
          continue;
        }
        // `!use @scope/pkg [as alias]` is a directive, not data. The Rust
        // reference (parser.rs) consumes it; without this branch it fell through
        // to key-line parsing and surfaced as a literal `"!use"` key — the
        // divergence conformance case 012-use-directive catches.
        if (trimmed.startsWith('!use ')) { continue; }
      } else {
        // Directives disabled (SYNXL §9.4): a `!` line reaching here is not
        // multiline body — the branch above already consumed and preserved
        // those — so it is discarded exactly like a comment.
        continue;
      }
    }


    // ── Comments: # or // ──
    // After the directive block, so the `#!mode:` pragma still resolves.
    if (fc === 35) { // #
      // Legacy: #!mode:active / #!mode:static
      if (directivesEnabled && rawLen - indent > 7 && rawLine.charCodeAt(indent + 1) === 33) { // !
        if (rawLine.substring(indent, indent + 7) === '#!mode:') {
          const declared = rawLine.substring(indent + 7, rawLen).trim();
          mode = declared === 'active' ? 'active' : 'static';
        }
      }
      continue;
    }
    if (fc === 47 && indent + 1 < rawLen && rawLine.charCodeAt(indent + 1) === 47) continue; // //

    // ── List items: '- ' ──
    if (fc === 45 && trimmedLen > 1 && trimmed.charCodeAt(1) === 32) {
      if (currentList && indent > currentList.indent) {
        if (currentList.arr.length >= MAX_LIST_ITEMS) { continue; }
        const val = stripInlineComment(trimmed.substring(2).trim());

        // Check if this list item has sub-keys (peek next line)
        let nextNonEmpty = i + 1;
        while (nextNonEmpty < lines.length) {
          const nl = lines[nextNonEmpty];
          let ni = 0;
          while (ni < nl.length && (nl.charCodeAt(ni) === 32 || nl.charCodeAt(ni) === 9 || nl.charCodeAt(ni) === 13)) ni++;
          if (ni < nl.length) break;
          nextNonEmpty++;
        }

        if (nextNonEmpty < lines.length) {
          const nextLine = lines[nextNonEmpty];
          let nextIndent = 0;
          while (nextIndent < nextLine.length && (nextLine.charCodeAt(nextIndent) === 32 || nextLine.charCodeAt(nextIndent) === 9 || nextLine.charCodeAt(nextIndent) === 13)) nextIndent++;
          const nfc = nextLine.charCodeAt(nextIndent);
          if (nextIndent > indent && nextIndent < nextLine.length &&
              nfc !== 45 && nfc !== 35 &&
              !(nfc === 47 && nextIndent + 1 < nextLine.length && nextLine.charCodeAt(nextIndent + 1) === 47)) {
            const itemObj: SynxObject = {};
            const parsed = parseKeyLine(val);
            if (parsed) {
              const iValue = parsed.rawValue ? stripInlineComment(parsed.rawValue) : '';
              itemObj[parsed.key] = !iValue ? {}
                : parsed.typeHint ? castTyped(iValue, parsed.typeHint, nondeterministic)
                : castType(iValue, nondeterministic);
            } else {
              itemObj['_value'] = castType(val, nondeterministic);
            }
            currentList.arr.push(itemObj);
            stack.push({ indent, obj: itemObj });
            continue;
          }
        }

        currentList.arr.push(castType(val, nondeterministic));
        continue;
      }
      // Not in list context — skip (no key-line starting with '- ' is valid)
      continue;
    }

    // Close list if needed (non-list-item line at <= list indent)
    if (currentList && indent <= currentList.indent) {
      currentList = null;
    }

    // ── Validate first char can start a key ──
    // First char cannot be [ ( : / (already filtered #, //, -)
    if (fc === 91 || fc === 40 || fc === 58 || fc === 47) continue; // [ ( : /

    // ── Parse key line ──
    // SYNX §7: a key cannot open with `[`, `:`, `-`, `#`, `/` or `(`, and such
    // a line is discarded rather than turned into a key. `parseKeyLine` applies
    // this, but the fast path below bypasses it, which let a bare `-` become
    // the key `-` — a name no writer can produce and the reference
    // implementation never yields.
    if (fc === 91 || fc === 58 || fc === 45 || fc === 35 || fc === 47 || fc === 40) {
      continue;
    }

    // Manual scanner with balanced [] support for patterns like ^[A-Z]{2}$.
    let key: string;
    let typeHint: string | undefined;
    let constraintStr: string | undefined;
    let markerChain: string | undefined;
    let rawValue: string;

    let hasSpecial = false;
    let spaceIdx = -1;
    for (let j = 0; j < trimmedLen; j++) {
      const ch = trimmed.charCodeAt(j);
      if (ch === 32 || ch === 9) { // space or tab
        spaceIdx = j;
        break;
      }
      if (ch === 40 || ch === 91 || ch === 58) { // ( [ :
        hasSpecial = true;
        break;
      }
    }

    if (!hasSpecial) {
      // ── Fast path: simple key-value or section header ──
      if (spaceIdx === -1) {
        key = trimmed;
        rawValue = '';
      } else {
        key = trimmed.substring(0, spaceIdx);
        // Skip whitespace between key and value
        let valStart = spaceIdx;
        while (valStart < trimmedLen && (trimmed.charCodeAt(valStart) === 32 || trimmed.charCodeAt(valStart) === 9)) valStart++;
        rawValue = valStart < trimmedLen ? stripInlineComment(trimmed.substring(valStart)) : '';
      }
      typeHint = undefined;
      constraintStr = undefined;
      markerChain = undefined;
    } else {
      // ── Slow path: manual scanner for lines with ( [ : ──
      const parsed = parseKeyLine(trimmed);
      if (!parsed) continue;
      key = parsed.key;
      typeHint = parsed.typeHint;
      constraintStr = parsed.constraintStr;
      markerChain = parsed.markerChain;
      rawValue = parsed.rawValue ? stripInlineComment(parsed.rawValue) : '';
    }

    // Parse markers chain (with segment cap)
    let markers: string[] = [];
    let markerArgs: string[] = [];
    if (markerChain) {
      markers = markerChain.split(':').slice(0, MAX_MARKER_CHAIN_SEGMENTS);
    }

    // For :random — the rawValue may contain weight percentages
    if (markers.length > 0 && markers.includes('random') && rawValue) {
      const nums = rawValue.split(/\s+/).filter(s => /^\d+(\.\d+)?$/.test(s));
      if (nums.length > 0) {
        markerArgs = nums;
        rawValue = '';
      }
    }

    // For :inherit — a non-empty value is the parent-key name, not a scalar value.
    // Promote it into markerArgs and clear rawValue so the line opens a group.
    if (markers.length > 0 && markers.includes('inherit') && rawValue) {
      markerArgs = [rawValue.trim()];
      rawValue = '';
    }

    const constraints = constraintStr ? parseConstraints(constraintStr) : undefined;

    // ── Reject prototype-polluting keys ──
    //
    // Checked *before* the stack is repaired, matching the reference
    // implementation. Popping first would let `__proto__` at the depth of a
    // real key close that key's subtree, so the lines underneath re-open it
    // and overwrite the sibling the document actually declared — a rejected
    // key must not be able to reshape the record around it.
    if (key === '__proto__' || key === 'constructor' || key === 'prototype') continue;

    // ── Pop stack to correct parent ──
    while (stack.length > 1 && stack[stack.length - 1].indent >= indent) {
      stack.pop();
    }
    const parent = stack[stack.length - 1].obj as SynxObject;

    // ── Determine what this line creates ──
    if (rawValue === '|' || rawValue === '|+') {
      // Multiline block. `|` (3.6) trims each continuation line; `|+` (3.7)
      // preserves indent relative to the first non-empty continuation line.
      parent[key] = '';
      currentBlock = {
        indent, obj: parent, key,
        preserveIndent: rawValue === '|+',
        baseIndent: -1,
        parts: [],
        byteLen: 0,
      };
      saveMeta(parent, key, markers, markerArgs, constraints, mode, typeHint);
    } else if (!rawValue && markers.length > 0 &&
               (markers.includes('random') || markers.includes('unique') ||
                markers.includes('geo') || markers.includes('join'))) {
      // List with markers (items follow with -)
      parent[key] = [];
      currentList = { indent, arr: parent[key] as SynxArray };
      saveMeta(parent, key, markers, markerArgs, constraints, mode, typeHint);
    } else if (!rawValue) {
      // No value → nested object (group) OR plain list
      parent[key] = {};
      // Cap nesting depth: still insert the object but don't push deeper.
      if (stack.length < MAX_PARSE_NESTING_DEPTH) {
        stack.push({ indent, obj: parent[key] as SynxObject });
      }
      // Peek ahead: if next meaningful line starts with -, it's a list
      let peekIdx = i + 1;
      while (peekIdx < lines.length) {
        const pl = lines[peekIdx];
        let pi = 0;
        while (pi < pl.length && (pl.charCodeAt(pi) === 32 || pl.charCodeAt(pi) === 9 || pl.charCodeAt(pi) === 13)) pi++;
        if (pi < pl.length) break;
        peekIdx++;
      }
      if (peekIdx < lines.length) {
        const pl = lines[peekIdx];
        let pi = 0;
        while (pi < pl.length && (pl.charCodeAt(pi) === 32 || pl.charCodeAt(pi) === 9 || pl.charCodeAt(pi) === 13)) pi++;
        if (pi + 1 < pl.length && pl.charCodeAt(pi) === 45 && pl.charCodeAt(pi + 1) === 32) {
          parent[key] = [];
          stack[stack.length - 1] = { indent, obj: parent[key] as unknown as SynxObject };
          currentList = { indent, arr: parent[key] as SynxArray };
        }
      }
      saveMeta(parent, key, markers, markerArgs, constraints, mode, typeHint);
    } else {
      // Simple key-value
      parent[key] = typeHint
        ? castTyped(rawValue, typeHint, nondeterministic)
        : castType(rawValue, nondeterministic);
      saveMeta(parent, key, markers, markerArgs, constraints, mode, typeHint);
    }
  }

  // Flush a block still open at end of input (no dedent line closed it).
  if (currentBlock) {
    currentBlock.obj[currentBlock.key] = currentBlock.parts.join('\n');
  }

  return {
    root,
    mode,
    locked,
    ...(llm ? { llm: true } : {}),
    ...(tool ? { tool: true } : {}),
    ...(schema ? { schema: true } : {}),
    includes: includes.length > 0 ? includes : undefined,
  };
}
