/**
 * Canonical formatter — @aperturesyndicate/synx-format
 *
 * The output is the same document with two-space indentation, comments
 * removed and — in a static document — keys sorted at every level outside
 * list items. It reads back as the same data, always: the result is parsed
 * and compared with the input before it is returned, and a document the
 * formatter cannot reproduce is returned exactly as it came in.
 *
 * A port of `format.rs` in synx-core, line for line: the two write the same
 * bytes for the same document.
 */

import {
  parseData, clampInput, keyLineShape, isUnicodeSpaceCode, wsUtf8Width, metaMapOf, keysOf,
  SHAPE_NOT_A_KEY, SHAPE_EMPTY, SHAPE_BLOCK, SHAPE_BLOCK_KEEP, SHAPE_LIST,
} from './parser';
import { compareUnicodeScalar } from './json';
import type { SynxParseResult, SynxValue, SynxObject, SynxMeta, SynxMetaMap, SynxConstraints } from './types';

/** Deepest stack of open keys, as in the parser. */
const MAX_PARSE_NESTING_DEPTH = 128;

/** What a line is to the tree. */
const LINE_KEY = 0;
const LINE_ITEM = 1;
const LINE_DIRECTIVE = 2;

/** One line that takes part in the document's structure. */
interface Line {
  /** Leading whitespace in bytes, as the parser counts it. */
  indent: number;
  /** The line, trimmed. */
  text: string;
  /** Body of the `|` / `|+` block this line opens: raw lines, indent intact. */
  body: string[] | null;
  kind: number;
  /** `keyLineShape` of a key line. */
  shape: number;
}

/** A key line or a list item with what hangs under it. */
interface Node {
  header: string;
  /** Child keys of a group, or the fields of a list item. */
  children: number[];
  /** Items of a list. */
  items: number[];
  body: string[] | null;
  /** `|+`: the body keeps its indentation. */
  keepIndent: boolean;
}

function isDirective(t: string): boolean {
  const first = t.charCodeAt(0);
  if (first === 33) {
    return t === '!active' || t === '!lock' || t === '!tool' || t === '!schema' || t === '!llm'
      || t.startsWith('!include ') || t.startsWith('!use ');
  }
  return first === 35 && t.startsWith('#!mode:');
}

/**
 * The document's lines in the classes the parser gives them, in its order of
 * precedence: a block body first, then `###`, directives, comments.
 */
function scan(text: string): Line[] {
  const lines: Line[] = [];
  // Indent of the open block's key line; its body goes to the last line.
  let block = -1;
  let inBlockComment = false;
  const len = text.length;
  let pos = 0;
  while (pos < len) {
    const start = pos;
    let nl = text.indexOf('\n', pos);
    if (nl === -1) nl = len;
    pos = nl + 1;
    // A line ends before `\r\n`; a `\r` anywhere else is part of it.
    const rawEnd = nl < len && nl > start && text.charCodeAt(nl - 1) === 13 ? nl - 1 : nl;

    let a = start;
    let indent = 0;
    while (a < rawEnd) {
      const c = text.charCodeAt(a);
      if (c === 32 || c === 9) indent++;
      else if ((c < 32 || c > 127) && isUnicodeSpaceCode(c)) indent += wsUtf8Width(c);
      else break;
      a++;
    }
    if (a >= rawEnd) continue;
    let te = rawEnd;
    while (te > a && isUnicodeSpaceCode(text.charCodeAt(te - 1))) te--;

    if (block >= 0) {
      if (indent > block) {
        const opener = lines[lines.length - 1];
        (opener.body ??= []).push(text.substring(start, rawEnd));
        continue;
      }
      block = -1;
    }
    const first = text.charCodeAt(a);
    if (first === 35 && te - a === 3 && text.charCodeAt(a + 1) === 35 && text.charCodeAt(a + 2) === 35) {
      inBlockComment = !inBlockComment;
      continue;
    }
    if (inBlockComment) continue;
    const trimmed = text.substring(a, te);
    if ((first === 33 || first === 35) && isDirective(trimmed)) {
      lines.push({ indent, text: trimmed, body: null, kind: LINE_DIRECTIVE, shape: SHAPE_NOT_A_KEY });
      continue;
    }
    if (first === 35 || (first === 47 && text.charCodeAt(a + 1) === 47 && a + 1 < te)) continue;
    if (first === 45 && a + 1 < te && text.charCodeAt(a + 1) === 32) {
      lines.push({ indent, text: trimmed, body: null, kind: LINE_ITEM, shape: SHAPE_NOT_A_KEY });
      continue;
    }
    const shape = keyLineShape(text, a, te);
    if (shape !== SHAPE_NOT_A_KEY && (shape & (SHAPE_BLOCK | SHAPE_BLOCK_KEEP)) !== 0) block = indent;
    lines.push({ indent, text: trimmed, body: null, kind: LINE_KEY, shape });
  }
  return lines;
}

/**
 * Build the tree the parser would build from `lines`: the same stack repair,
 * the same list rules. Node 0 is the root.
 */
function build(lines: Line[]): Node[] {
  const nodes: Node[] = [{ header: '', children: [], items: [], body: null, keepIndent: false }];
  // The stack of open keys as columns: indent, node, whether it is a list item.
  const stackIndent: number[] = [0];
  const stackNode: number[] = [0];
  const stackItem: boolean[] = [false];
  let sp = 0;
  // Open lists, outermost first: indent of the key line, and its node.
  const listIndent: number[] = [];
  const listNode: number[] = [];
  let lists = 0;

  for (let n = 0; n < lines.length; n++) {
    const line = lines[n];
    if (line.kind === LINE_DIRECTIVE) continue;
    const indent = line.indent;
    const next = n + 1 < lines.length ? lines[n + 1] : null;
    if (line.kind === LINE_ITEM) {
      let accepting = lists;
      while (accepting > 0 && indent < listIndent[accepting - 1]) accepting--;
      if (accepting > 0 && indent > listIndent[accepting - 1]) lists = accepting;
      if (lists === 0 || indent <= listIndent[lists - 1]) continue;
      while (sp > 0 && stackIndent[sp] >= indent) sp--;
      const id = nodes.length;
      nodes.push({ header: line.text, children: [], items: [], body: null, keepIndent: false });
      nodes[listNode[lists - 1]].items.push(id);
      const nested = next !== null && next.indent > indent && next.kind !== LINE_ITEM;
      if (nested && sp + 1 < MAX_PARSE_NESTING_DEPTH) {
        sp++;
        stackIndent[sp] = indent;
        stackNode[sp] = id;
        stackItem[sp] = true;
      }
      continue;
    }

    if (lists > 0 && indent <= listIndent[lists - 1]) {
      do lists--; while (lists > 0 && indent <= listIndent[lists - 1]);
      while (sp > 0 && stackItem[sp] && stackIndent[sp] >= indent) sp--;
    }
    const shape = line.shape;
    if (shape === SHAPE_NOT_A_KEY) continue;
    while (sp > 0 && stackIndent[sp] >= indent) sp--;
    const id = nodes.length;
    const isBlock = (shape & (SHAPE_BLOCK | SHAPE_BLOCK_KEEP)) !== 0;
    nodes.push({ header: line.text, children: [], items: [], body: line.body, keepIndent: (shape & SHAPE_BLOCK_KEEP) !== 0 });
    nodes[stackNode[sp]].children.push(id);
    if (isBlock) continue;
    if ((shape & SHAPE_EMPTY) === 0) continue;
    if ((shape & SHAPE_LIST) !== 0 || (next !== null && next.kind === LINE_ITEM)) {
      listIndent[lists] = indent;
      listNode[lists] = id;
      lists++;
    } else if (sp + 1 < MAX_PARSE_NESTING_DEPTH) {
      sp++;
      stackIndent[sp] = indent;
      stackNode[sp] = id;
      stackItem[sp] = false;
    }
  }
  return nodes;
}

/** Bytes of leading whitespace of `s`. */
function leadingSpaceBytes(s: string): number {
  let bytes = 0;
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (!isUnicodeSpaceCode(c)) break;
    bytes += c < 0x80 ? 1 : wsUtf8Width(c);
  }
  return bytes;
}

/** Characters of at most `limit` bytes of leading whitespace, whole characters only. */
function spacePrefixLength(s: string, limit: number): number {
  let bytes = 0;
  let i = 0;
  while (i < s.length && bytes < limit) {
    const c = s.charCodeAt(i);
    if (!isUnicodeSpaceCode(c)) break;
    bytes += c < 0x80 ? 1 : wsUtf8Width(c);
    i++;
  }
  return i;
}

function trimEndSpace(s: string): string {
  let e = s.length;
  while (e > 0 && isUnicodeSpaceCode(s.charCodeAt(e - 1))) e--;
  return e === s.length ? s : s.substring(0, e);
}

function trimBothSpace(s: string): string {
  let a = 0;
  let e = s.length;
  while (a < e && isUnicodeSpaceCode(s.charCodeAt(a))) a++;
  while (e > a && isUnicodeSpaceCode(s.charCodeAt(e - 1))) e--;
  return a === 0 && e === s.length ? s : s.substring(a, e);
}

/** What a key sorts by: its name, lowercased. */
function sortKey(header: string): string {
  let e = 0;
  while (e < header.length) {
    const c = header.charCodeAt(e);
    if (c === 91 || c === 58 || c === 40 || isUnicodeSpaceCode(c)) break;
    e++;
  }
  return header.substring(0, e).toLowerCase();
}

const INDENTS: string[] = [''];

function spaces(n: number): string {
  while (INDENTS.length <= n) INDENTS.push(INDENTS[INDENTS.length - 1] + ' ');
  return INDENTS[n];
}

/** The text being written by `emit`. */
let out = '';

/**
 * Write the keys of one object. `sorted` holds outside list items of a static
 * document; `top` adds the blank line after a top-level block.
 */
function emit(nodes: Node[], ids: number[], indent: number, sorted: boolean, top: boolean): void {
  let order = ids;
  if (sorted && ids.length > 1) {
    // Stable: keys that compare equal (a repeated key) keep document order,
    // so the last one still wins. perf: each key lowercased once.
    const keyed = ids.map(id => ({ id, key: sortKey(nodes[id].header) }));
    keyed.sort((x, y) => compareUnicodeScalar(x.key, y.key));
    order = keyed.map(k => k.id);
  }
  const pad = spaces(indent);
  const inner = spaces(indent + 2);
  for (let i = 0; i < order.length; i++) {
    const node = nodes[order[i]];
    out += pad + node.header + '\n';
    if (node.children.length > 0) emit(nodes, node.children, indent + 2, sorted, false);
    for (let j = 0; j < node.items.length; j++) {
      const item = nodes[node.items[j]];
      out += inner + item.header + '\n';
      // spec: what sits deeper than the dash is the item. Its fields keep
      // their order — which of them is last decides how an empty key before
      // the next `- ` line reads.
      if (item.children.length > 0) emit(nodes, item.children, indent + 4, false, false);
    }
    const body = node.body;
    if (body !== null) {
      if (node.keepIndent) {
        // `|+` bodies are text: indentation relative to the first line stays.
        const base = leadingSpaceBytes(body[0]);
        for (let j = 0; j < body.length; j++) {
          const line = trimEndSpace(body[j]);
          const own = leadingSpaceBytes(line);
          out += inner + line.substring(spacePrefixLength(line, Math.min(base, own))) + '\n';
        }
      } else {
        for (let j = 0; j < body.length; j++) out += inner + trimBothSpace(body[j]) + '\n';
      }
    }
    if (top && (node.children.length > 0 || node.items.length > 0 || body !== null)) out += '\n';
  }
}

// ─── The same document ────────────────────────────────────

function sameList(a: string[] | undefined, b: string[] | undefined): boolean {
  const n = a ? a.length : 0;
  if (n !== (b ? b.length : 0)) return false;
  for (let i = 0; i < n; i++) if (a![i] !== b![i]) return false;
  return true;
}

function sameConstraints(a: SynxConstraints | undefined, b: SynxConstraints | undefined): boolean {
  if (a === undefined || b === undefined) return a === b;
  return a.min === b.min && a.max === b.max && a.type === b.type && a.pattern === b.pattern
    && !!a.required === !!b.required && !!a.readonly === !!b.readonly
    && (a.enum === undefined || b.enum === undefined ? a.enum === b.enum : sameList(a.enum, b.enum));
}

function sameMeta(a: SynxMeta, b: SynxMeta): boolean {
  return sameList(a.markers, b.markers) && sameList(a.args, b.args) && a.typeHint === b.typeHint
    && sameConstraints(a.constraints, b.constraints);
}

function sameMetaMap(a: SynxMetaMap | undefined, b: SynxMetaMap | undefined): boolean {
  const n = a ? a.size : 0;
  if (n !== (b ? b.size : 0)) return false;
  if (n === 0) return true;
  for (const [key, meta] of a!) {
    const other = b!.get(key);
    if (other === undefined || !sameMeta(meta, other)) return false;
  }
  return true;
}

function isObject(v: unknown): v is SynxObject {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

/**
 * Whether `b` is the data of `a`. With `ordered` — an active document — also
 * in the same order, the order markers resolve in, and with the same
 * declarations on every object.
 */
function sameValue(a: SynxValue, b: SynxValue, ordered: boolean): boolean {
  if (Array.isArray(a)) {
    if (!Array.isArray(b) || a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) if (!sameValue(a[i], b[i], ordered)) return false;
    return true;
  }
  if (isObject(a)) {
    if (!isObject(b)) return false;
    const keys = keysOf(a);
    const other = keysOf(b);
    if (keys.length !== other.length) return false;
    for (let i = 0; i < keys.length; i++) {
      const k = keys[i];
      if (ordered ? other[i] !== k : !Object.prototype.hasOwnProperty.call(b, k)) return false;
      if (!sameValue(a[k], b[k], ordered)) return false;
    }
    return !ordered || sameMetaMap(metaMapOf(a), metaMapOf(b));
  }
  // A NaN is not the same as itself here, as in synx-core.
  return a === b;
}

/** Whether `b` is the document `a`: flags, includes and data. */
function sameDocument(a: SynxParseResult, b: SynxParseResult): boolean {
  if (a.mode !== b.mode || !!a.locked !== !!b.locked || !!a.tool !== !!b.tool
      || !!a.schema !== !!b.schema || !!a.llm !== !!b.llm) {
    return false;
  }
  const ai = a.includes ?? [];
  const bi = b.includes ?? [];
  if (ai.length !== bi.length) return false;
  for (let i = 0; i < ai.length; i++) {
    if (ai[i].path !== bi[i].path || ai[i].alias !== bi[i].alias) return false;
  }
  return sameValue(a.root, b.root, a.mode === 'active');
}

// The comparison must not see a `(random)` hint change between two reads.
const READ = { directives: true, nondeterministicHints: false };

/** `Synx.format` — see the module comment. */
export function formatSynx(input: string): string {
  const text = clampInput(input);
  const before = parseData(text, READ);
  // safety: input past a parser limit is not in `before`; formatting what was
  // read would write the document back without it.
  if (before.truncated) return input;

  const lines = scan(text);
  const nodes = build(lines);

  out = '';
  let wroteDirective = false;
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].kind !== LINE_DIRECTIVE) continue;
    out += lines[i].text + '\n';
    wroteDirective = true;
  }
  if (wroteDirective) out += '\n';
  // spec: markers resolve in document order, so sorting an active document
  // changes what it means (`total:calc price + tax` ahead of `tax:calc …`).
  emit(nodes, nodes[0].children, 0, before.mode === 'static', true);
  // Trim trailing blank lines, ensure single newline at end
  const result = trimEndSpace(out) + '\n';
  out = '';

  // safety: `format` rewrites files in place. The formatter reads lines on its
  // own, and wherever it and the parser disagreed data was lost — every list
  // of objects became one object. Now what it wrote is read back, and a
  // document it cannot reproduce is returned untouched.
  const after = parseData(result, READ);
  if (after.truncated || !sameDocument(before, after)) return input;
  return result;
}
