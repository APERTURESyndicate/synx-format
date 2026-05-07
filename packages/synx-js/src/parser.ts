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

/** Cast a raw string value to a JS primitive */
function castType(val: string): SynxValue {
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

  // Explicit cast: (int)007, (string)90210, (float)3.0, (bool)true, (random), (random:bool)
  // Only treat the leading `(...)` as a type hint when it looks like an
  // identifier-shaped token (`[A-Za-z_][A-Za-z0-9_:]*`). Anything else (e.g.
  // a value that happens to start with a parenthesised arithmetic expression)
  // is left untouched as a plain string.
  if (c0 === 40 && len > 2) { // '('
    const closeIdx = val.indexOf(')');
    if (closeIdx > 1) {
      const hint = val.substring(1, closeIdx);
      if (/^[A-Za-z_][A-Za-z0-9_:]*$/.test(hint)) {
        const raw = val.substring(closeIdx + 1);
        switch (hint) {
          case 'int': { const n = parseInt(raw, 10); return isNaN(n) ? 0 : n; }
          case 'float': { const n = parseFloat(raw); return isNaN(n) ? 0 : n; }
          case 'bool': return raw.trim() === 'true';
          case 'string': return raw;
          case 'random': return Math.floor(Math.random() * 2147483647);
          case 'random:int': return Math.floor(Math.random() * 2147483647);
          case 'random:float': return Math.random();
          case 'random:bool': return Math.random() < 0.5;
          // Unknown hint — strip the (hint) wrapper and re-cast the bare value
          // (matches the Rust `cast_typed` fallback to `cast`).
          default: return castType(raw);
        }
      }
    }
  }

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
      if (dotPos === -1) return parseInt(val, 10);
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

/** Parse constraint string like "min:3, max:30, required, type:int" */
function parseConstraints(raw: string): SynxConstraints {
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

export function parseData(text: string): SynxParseResult {
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
  let currentBlock: { indent: number; obj: SynxObject; key: string } | null = null;
  let currentList: { indent: number; arr: SynxArray } | null = null;
  let inBlockComment = false;

  for (let i = 0; i < lines.length; i++) {
    const rawLine = lines[i];
    const rawLen = rawLine.length;

    // ── Manual indent computation (no regex) ──
    let indent = 0;
    while (indent < rawLen) {
      const ch = rawLine.charCodeAt(indent);
      if (ch !== 32 && ch !== 9 && ch !== 13) break; // space, tab, \r
      indent++;
    }

    // ── Empty line ──
    if (indent === rawLen) continue;

    const fc = rawLine.charCodeAt(indent); // first non-whitespace char

    // ── Block comment toggle: ### ──
    if (fc === 35) {
      const rest = rawLine.substring(indent).trimEnd();
      if (rest === '###') {
        inBlockComment = !inBlockComment;
        continue;
      }
    }
    if (inBlockComment) continue;

    // ── Comments: # or // ──
    if (fc === 35) { // #
      // Legacy: #!mode:active / #!mode:static
      if (rawLen - indent > 7 && rawLine.charCodeAt(indent + 1) === 33) { // !
        if (rawLine.substring(indent, indent + 7) === '#!mode:') {
          const declared = rawLine.substring(indent + 7, rawLen).trim();
          mode = declared === 'active' ? 'active' : 'static';
        }
      }
      continue;
    }
    if (fc === 47 && indent + 1 < rawLen && rawLine.charCodeAt(indent + 1) === 47) continue; // //

    // ── Compute trimmed string (manual trim, no regex) ──
    let trimEndPos = rawLen;
    while (trimEndPos > indent) {
      const ch = rawLine.charCodeAt(trimEndPos - 1);
      if (ch !== 32 && ch !== 9 && ch !== 13 && ch !== 10) break;
      trimEndPos--;
    }
    const trimmed = rawLine.substring(indent, trimEndPos);
    const trimmedLen = trimmed.length;

    // ── Directives: !active / !lock / !llm / !tool / !schema / !include ──
    if (fc === 33) {
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
    }

    // ── Continue multiline block ──
    if (currentBlock && indent > currentBlock.indent) {
      const existing = currentBlock.obj[currentBlock.key] as string;
      if (existing.length < MAX_MULTILINE_BLOCK_BYTES) {
        const line = trimmed;
        const room = MAX_MULTILINE_BLOCK_BYTES - existing.length;
        const sep = existing ? '\n' : '';
        const append = sep + line;
        currentBlock.obj[currentBlock.key] = existing + append.substring(0, room);
      }
      continue;
    } else {
      currentBlock = null;
    }

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
              let iValue = parsed.rawValue;
              if (parsed.typeHint) iValue = `(${parsed.typeHint})${iValue}`;
              itemObj[parsed.key] = iValue ? castType(stripInlineComment(iValue)) : {};
            } else {
              itemObj['_value'] = castType(val);
            }
            currentList.arr.push(itemObj);
            stack.push({ indent, obj: itemObj });
            continue;
          }
        }

        currentList.arr.push(castType(val));
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

    // Apply explicit type cast
    if (typeHint) rawValue = `(${typeHint})${rawValue}`;

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

    // ── Pop stack to correct parent ──
    while (stack.length > 1 && stack[stack.length - 1].indent >= indent) {
      stack.pop();
    }
    const parent = stack[stack.length - 1].obj as SynxObject;

    // ── Reject prototype-polluting keys ──
    if (key === '__proto__' || key === 'constructor' || key === 'prototype') continue;

    // ── Determine what this line creates ──
    if (rawValue === '|') {
      // Multiline block
      parent[key] = '';
      currentBlock = { indent, obj: parent, key };
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
      parent[key] = castType(rawValue);
      saveMeta(parent, key, markers, markerArgs, constraints, mode, typeHint);
    }
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
