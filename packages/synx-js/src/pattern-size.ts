/**
 * Whether the `regex` crate would refuse a `[pattern:…]` for its size.
 *
 * spec: synx-core compiles a pattern with `regex::Regex::new`, and the crate
 * refuses one whose automaton takes more than 10 MiB — `^\w{300}$` is not
 * compiled, and the constraint goes unchecked (E16). The automaton is built
 * twice, forward and reversed, and either one over the limit refuses the
 * pattern. This file builds neither: it counts the states the crate's
 * compiler (regex-automata 0.4, `nfa::thompson`) would add, in the order it
 * adds them, and the bytes it accounts for each.
 *
 * The count is exact for classes written out as characters and ranges. A
 * class named by a Unicode property has the members this runtime's tables
 * give it, which can differ from the crate's tables by a Unicode version —
 * the limit then falls a copy or two to either side.
 */
import { A_TEXT_END, A_TEXT_START, CharSet, Node } from './pattern-syntax';

/** `RegexBuilder::size_limit`, as `Regex::new` leaves it. */
export const SIZE_LIMIT = 10 * (1 << 20);

/** `size_of::<State>()` of the compiler's builder, and what a transition and an alternate add to it. */
const STATE = 32;
const TRANSITION = 8;
const ALTERNATE = 4;

/** Thrown as soon as the automaton passes the limit. */
class TooBig extends Error {}

// ─── The pattern as the crate simplifies it ───────────────

type Hir =
  | { k: 'empty' }
  | { k: 'fail' }
  | { k: 'lit'; bytes: number[] }
  // Neither empty nor one single character. `sequences` and `alone` are worked out when first asked for.
  | { k: 'class'; ranges: number[]; sequences?: number[][]; alone?: number[] }
  | { k: 'look'; kind: number }
  | { k: 'rep'; min: number; max: number; sub: Hir } // max -1: unbounded
  | { k: 'cap'; sub: Hir }
  | { k: 'cat'; subs: Hir[] }
  | { k: 'alt'; subs: Hir[] };

const EMPTY: Hir = { k: 'empty' };
const FAIL: Hir = { k: 'fail' };

function utf8(c: number, out: number[]): void {
  if (c < 0x80) out.push(c);
  else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 0x3f));
  else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 0x3f), 0x80 | (c & 0x3f));
  else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 0x3f), 0x80 | ((c >> 6) & 0x3f), 0x80 | (c & 0x3f));
}

function utf8Length(c: number): number {
  return c < 0x80 ? 1 : c < 0x800 ? 2 : c < 0x10000 ? 3 : 4;
}

/** `Hir::class`: no member is a pattern that never matches, one member is that character. */
function hirClass(ranges: number[]): Hir {
  if (ranges.length === 0) return FAIL;
  if (ranges.length === 2 && ranges[0] === ranges[1]) {
    const bytes: number[] = [];
    utf8(ranges[0], bytes);
    return { k: 'lit', bytes };
  }
  return { k: 'class', ranges };
}

/** `Properties::maximum_len() == Some(0)`: the expression matches nothing but the empty string. */
function matchesOnlyEmpty(h: Hir): boolean {
  switch (h.k) {
    case 'empty': case 'look': return true;
    case 'lit': case 'class': case 'fail': return false;
    case 'rep': return matchesOnlyEmpty(h.sub);
    case 'cap': return matchesOnlyEmpty(h.sub);
    case 'cat': case 'alt': return h.subs.every(matchesOnlyEmpty);
  }
}

/** `Properties::minimum_len()`; `null` where the crate has `None`. */
function minimumLength(h: Hir): number | null {
  switch (h.k) {
    case 'empty': case 'look': return 0;
    case 'fail': return null;
    case 'lit': return h.bytes.length;
    case 'class': return utf8Length(h.ranges[0]);
    case 'cap': return minimumLength(h.sub);
    case 'rep': {
      const sub = minimumLength(h.sub);
      return sub === null ? null : sub * h.min;
    }
    case 'cat': {
      let sum = 0;
      for (const s of h.subs) {
        const len = minimumLength(s);
        if (len === null) return null;
        sum += len;
      }
      return sum;
    }
    case 'alt': {
      let least: number | null = null;
      for (const s of h.subs) {
        const len = minimumLength(s);
        if (len === null) return null;
        if (least === null || len < least) least = len;
      }
      return least;
    }
  }
}

/** Every match starts at the start of the text (`look_set_prefix` has `Start`); `atEnd` asks the same of the end. */
function anchored(h: Hir, atEnd: boolean): boolean {
  switch (h.k) {
    case 'look': return h.kind === (atEnd ? A_TEXT_END : A_TEXT_START);
    case 'cap': return anchored(h.sub, atEnd);
    case 'rep': return h.min > 0 && anchored(h.sub, atEnd);
    case 'alt': return h.subs.every(s => anchored(s, atEnd));
    case 'cat': {
      const n = h.subs.length;
      for (let i = 0; i < n; i++) {
        const sub = h.subs[atEnd ? n - 1 - i : i];
        if (anchored(sub, atEnd)) return true;
        if (!matchesOnlyEmpty(sub)) return false;
      }
      return false;
    }
    default: return false;
  }
}

function sameHir(a: Hir, b: Hir): boolean {
  if (a.k !== b.k) return false;
  switch (a.k) {
    case 'empty': case 'fail': return true;
    case 'lit': return sameNumbers(a.bytes, (b as typeof a).bytes);
    case 'class': return sameNumbers(a.ranges, (b as typeof a).ranges);
    case 'look': return a.kind === (b as typeof a).kind;
    case 'rep': {
      const o = b as typeof a;
      return a.min === o.min && a.max === o.max && sameHir(a.sub, o.sub);
    }
    // Every group has a number of its own.
    case 'cap': return false;
    case 'cat': case 'alt': {
      const o = b as typeof a;
      return a.subs.length === o.subs.length && a.subs.every((s, i) => sameHir(s, o.subs[i]));
    }
  }
}

function sameNumbers(a: number[], b: number[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

/** `Hir::concat`: nested concatenations flattened, neighbouring literals joined, empty ones dropped. */
function hirConcat(subs: Hir[]): Hir {
  const out: Hir[] = [];
  let literal: number[] | null = null;
  const flush = (): void => {
    if (literal !== null) {
      out.push({ k: 'lit', bytes: literal });
      literal = null;
    }
  };
  const take = (h: Hir): void => {
    if (h.k === 'lit') {
      if (literal === null) literal = h.bytes.slice();
      else for (const b of h.bytes) literal.push(b);
    } else if (h.k !== 'empty') {
      flush();
      out.push(h);
    }
  };
  for (const sub of subs) {
    if (sub.k === 'cat') for (const inner of sub.subs) {
      // One level only, and an empty expression inside it stays: the crate does the same.
      if (inner.k === 'lit') take(inner); else { flush(); out.push(inner); }
    } else take(sub);
  }
  flush();
  if (out.length === 0) return EMPTY;
  if (out.length === 1) return out[0];
  return { k: 'cat', subs: out };
}

function unionRanges(parts: number[][]): number[] {
  const pairs: number[] = [];
  for (const part of parts) for (const v of part) pairs.push(v);
  const order: number[] = [];
  for (let i = 0; i < pairs.length; i += 2) order.push(i);
  order.sort((a, b) => pairs[a] - pairs[b] || pairs[a + 1] - pairs[b + 1]);
  const out: number[] = [];
  for (const i of order) {
    const last = out.length - 1;
    if (last >= 0 && pairs[i] <= out[last] + 1) {
      if (pairs[i + 1] > out[last]) out[last] = pairs[i + 1];
    } else {
      out.push(pairs[i], pairs[i + 1]);
    }
  }
  return out;
}

/** A literal that is one whole character, as that character; else -1. */
function singleCharacter(bytes: number[]): number {
  const b0 = bytes[0];
  const n = b0 < 0x80 ? 1 : b0 < 0xe0 ? 2 : b0 < 0xf0 ? 3 : 4;
  if (bytes.length !== n) return -1;
  if (n === 1) return b0;
  if (n === 2) return ((b0 & 0x1f) << 6) | (bytes[1] & 0x3f);
  if (n === 3) return ((b0 & 0x0f) << 12) | ((bytes[1] & 0x3f) << 6) | (bytes[2] & 0x3f);
  return ((b0 & 0x07) << 18) | ((bytes[1] & 0x3f) << 12) | ((bytes[2] & 0x3f) << 6) | (bytes[3] & 0x3f);
}

/**
 * `Hir::alternation`: nested alternations flattened; single characters and
 * classes become one class; a prefix every branch starts with is taken out.
 */
function hirAlternation(subs: Hir[]): Hir {
  const flat: Hir[] = [];
  for (const sub of subs) {
    if (sub.k === 'alt') for (const inner of sub.subs) flat.push(inner); else flat.push(sub);
  }
  if (flat.length === 0) return FAIL;
  if (flat.length === 1) return flat[0];
  if (flat.every(h => h.k === 'lit' && singleCharacter(h.bytes) !== -1)) {
    return hirClass(unionRanges(flat.map(h => {
      const c = singleCharacter((h as { bytes: number[] }).bytes);
      return [c, c];
    })));
  }
  if (flat.every(h => h.k === 'class')) {
    return hirClass(unionRanges(flat.map(h => (h as { ranges: number[] }).ranges)));
  }
  // The prefix all branches share, when each is a concatenation.
  if (flat.every(h => h.k === 'cat')) {
    const branches = flat.map(h => (h as { subs: Hir[] }).subs);
    let common = branches[0].length;
    for (let i = 1; i < branches.length && common > 0; i++) {
      let same = 0;
      while (same < common && same < branches[i].length && sameHir(branches[0][same], branches[i][same])) same++;
      common = same;
    }
    if (common > 0) {
      const head = branches[0].slice(0, common);
      head.push(hirAlternation(branches.map(b => hirConcat(b.slice(common)))));
      return hirConcat(head);
    }
  }
  return { k: 'alt', subs: flat };
}

/** `Hir::repetition`. */
function hirRepetition(min: number, max: number, sub: Hir): Hir {
  if (matchesOnlyEmpty(sub)) {
    min = Math.min(min, 1);
    max = max === -1 ? 1 : Math.min(max, 1);
  }
  if (min === 0 && max === 0) return EMPTY;
  if (min === 1 && max === 1) return sub;
  return { k: 'rep', min, max, sub };
}

function toHir(node: Node): Hir {
  switch (node.t) {
    case 'empty': return EMPTY;
    case 'lit': {
      const bytes: number[] = [];
      utf8(node.c, bytes);
      return { k: 'lit', bytes };
    }
    case 'set': return hirClass(node.set.ranges());
    case 'assert': return { k: 'look', kind: node.kind };
    case 'cap': return { k: 'cap', sub: toHir(node.node) };
    case 'rep': return hirRepetition(node.min, node.max, toHir(node.node));
    case 'cat': return hirConcat(node.items.map(toHir));
    case 'alt': return hirAlternation(node.items.map(toHir));
  }
}

// ─── UTF-8 ────────────────────────────────────────────────

/**
 * The byte sequences of the characters `lo`–`hi`, in order: each a run of
 * byte ranges `[s0, e0, s1, e1, …]` (`regex_syntax::utf8::Utf8Sequences`).
 */
function utf8Sequences(lo: number, hi: number, out: number[][]): void {
  const stack: number[] = [lo, hi];
  while (stack.length > 0) {
    let e = stack.pop()!;
    let s = stack.pop()!;
    for (;;) {
      if (s < 0xe000 && e > 0xd7ff) {
        stack.push(0xe000, e);
        e = 0xd7ff;
        continue;
      }
      if (s > e) break;
      let split = false;
      for (let i = 1; i < 4 && !split; i++) {
        const max = i === 1 ? 0x7f : i === 2 ? 0x7ff : 0xffff;
        if (s <= max && max < e) {
          stack.push(max + 1, e);
          e = max;
          split = true;
        }
      }
      if (split) continue;
      if (e <= 0x7f) {
        out.push([s, e]);
        break;
      }
      for (let i = 1; i < 4 && !split; i++) {
        const m = (1 << (6 * i)) - 1;
        if ((s & ~m) !== (e & ~m)) {
          if ((s & m) !== 0) {
            stack.push((s | m) + 1, e);
            e = s | m;
            split = true;
          } else if ((e & m) !== m) {
            stack.push(e & ~m, e);
            e = (e & ~m) - 1;
            split = true;
          }
        }
      }
      if (split) continue;
      const from: number[] = [];
      const to: number[] = [];
      utf8(s, from);
      utf8(e, to);
      const seq: number[] = [];
      for (let i = 0; i < from.length; i++) seq.push(from[i], to[i]);
      out.push(seq);
      break;
    }
  }
}

// ─── FNV-1a over 64 bits, as the compiler's two caches hash ───

let hashHi = 0;
let hashLo = 0;

function hashStart(): void {
  hashHi = 0xcbf29ce4;
  hashLo = 0x84222325;
}

/** `h = (h ^ x).wrapping_mul(0x100000001B3)`, for `x` below 2^32. */
function hashStep(x: number): void {
  const lo = (hashLo ^ x) >>> 0;
  const low = lo * 435;
  const carry = Math.floor(low / 4294967296);
  hashLo = low - carry * 4294967296;
  hashHi = (carry + hashHi * 435 + ((lo << 8) >>> 0)) % 4294967296;
}

function hashSlot(capacity: number): number {
  return ((hashHi % capacity) * (4294967296 % capacity) + (hashLo % capacity)) % capacity;
}

// ─── The compiler's builder, counting ─────────────────────

const K_OTHER = 0;
const K_UNION = 1;

/** What the compiler returns for an expression: where it starts and where it ends. */
interface Ref { start: number; end: number }

/** The suffix cache of the forward UTF-8 compiler: 10 000 slots, last writer wins. */
const FORWARD_SLOTS = 10_000;
/** The suffix cache of the reversed one. */
const REVERSE_SLOTS = 1_000;

class Compiler {
  /** States added so far. */
  private count = 0;
  /** Bytes of transitions and alternates added so far. */
  private heap = 0;
  /** Which states are unions: a later patch adds an alternate to one. */
  private kinds = new Uint8Array(1024);

  private readonly forwardKeys: (number[] | undefined)[] = new Array(FORWARD_SLOTS);
  private readonly forwardIds = new Int32Array(FORWARD_SLOTS);
  private readonly forwardStamp = new Int32Array(FORWARD_SLOTS);
  private readonly reverseFrom = new Int32Array(REVERSE_SLOTS);
  private readonly reverseRange = new Int32Array(REVERSE_SLOTS);
  private readonly reverseIds = new Int32Array(REVERSE_SLOTS);
  private readonly reverseStamp = new Int32Array(REVERSE_SLOTS);
  private stamp = 0;

  constructor(private readonly reverse: boolean, private readonly limit: number) {}

  /** The whole pattern, as `Compiler::compile` builds it. */
  build(hir: Hir): void {
    const prefix = anchored(hir, this.reverse) ? this.empty() : this.unanchoredPrefix();
    // Groups are recorded only by the forward automaton.
    let one: Ref;
    if (this.reverse) {
      one = this.c(hir);
    } else {
      const start = this.add();
      const inner = this.c(hir);
      const end = this.add();
      this.patch(start, inner.start);
      this.patch(inner.end, end);
      one = { start, end };
    }
    const match = this.add();
    this.patch(one.end, match);
    this.patch(prefix.end, one.start);
  }

  /** What the crate accounts for the states added so far. */
  bytes(): number {
    return this.count * STATE + this.heap;
  }

  private add(kind = K_OTHER, heap = 0): number {
    const id = this.count++;
    if (id >= this.kinds.length) {
      const grown = new Uint8Array(this.kinds.length * 2);
      grown.set(this.kinds);
      this.kinds = grown;
    }
    this.kinds[id] = kind;
    this.heap += heap;
    if (this.count * STATE + this.heap > this.limit) throw new TooBig();
    return id;
  }

  private patch(from: number, _to: number): void {
    if (this.kinds[from] !== K_UNION) return;
    this.heap += ALTERNATE;
    if (this.count * STATE + this.heap > this.limit) throw new TooBig();
  }

  private empty(): Ref {
    const id = this.add();
    return { start: id, end: id };
  }

  /** `(?s-u:.)*?` in front of a pattern that may start anywhere. */
  private unanchoredPrefix(): Ref {
    const union = this.add(K_UNION);
    const end = this.add();
    const sparse = this.add(K_OTHER, TRANSITION);
    this.patch(union, sparse);
    this.patch(end, union);
    return { start: union, end: union };
  }

  private c(h: Hir): Ref {
    switch (h.k) {
      case 'empty': case 'fail': case 'look': return this.empty();
      case 'lit': return this.literal(h.bytes);
      case 'class': return this.class(h);
      case 'rep': return this.repetition(h);
      case 'cap': {
        if (this.reverse) return this.c(h.sub);
        const start = this.add();
        const inner = this.c(h.sub);
        const end = this.add();
        this.patch(start, inner.start);
        this.patch(inner.end, end);
        return { start, end };
      }
      case 'cat': return this.concat(h.subs.length, i => this.c(h.subs[i]));
      case 'alt': return this.alternation(h.subs);
    }
  }

  /** `c_concat`: the pieces in the order the automaton reads them. */
  private concat(n: number, piece: (i: number) => Ref): Ref {
    if (n === 0) return this.empty();
    const first = piece(this.reverse ? n - 1 : 0);
    const start = first.start;
    let end = first.end;
    for (let k = 1; k < n; k++) {
      const next = piece(this.reverse ? n - 1 - k : k);
      this.patch(end, next.start);
      end = next.end;
    }
    return { start, end };
  }

  private literal(bytes: number[]): Ref {
    // One state a byte; the order they are added in makes no difference to the count.
    const start = this.add();
    let end = start;
    for (let i = 1; i < bytes.length; i++) end = this.add();
    return { start, end };
  }

  private alternation(subs: Hir[]): Ref {
    let literals = 0;
    for (const s of subs) if (s.k === 'lit') literals++;
    if (literals > 1 && literals === subs.length) return this.literalTrie(subs as { k: 'lit'; bytes: number[] }[]);
    const first = this.c(subs[0]);
    const second = this.c(subs[1]);
    const union = this.add(K_UNION);
    const end = this.add();
    this.patch(union, first.start);
    this.patch(first.end, end);
    this.patch(union, second.start);
    this.patch(second.end, end);
    for (let i = 2; i < subs.length; i++) {
      const next = this.c(subs[i]);
      this.patch(union, next.start);
      this.patch(next.end, end);
    }
    return { start: union, end };
  }

  /** An alternation of literals: a trie of their bytes (`LiteralTrie`). */
  private literalTrie(subs: { bytes: number[] }[]): Ref {
    interface TrieState { bytes: number[]; next: number[]; chunks: number[] }
    const states: TrieState[] = [{ bytes: [], next: [], chunks: [] }];
    for (const { bytes } of subs) {
      let at = 0;
      for (let k = 0; k < bytes.length; k++) {
        const b = bytes[this.reverse ? bytes.length - 1 - k : k];
        const s = states[at];
        const activeStart = s.chunks.length === 0 ? 0 : s.chunks[s.chunks.length - 1];
        let i = activeStart;
        while (i < s.bytes.length && s.bytes[i] < b) i++;
        if (i < s.bytes.length && s.bytes[i] === b) {
          at = s.next[i];
        } else {
          states.push({ bytes: [], next: [], chunks: [] });
          s.bytes.splice(i, 0, b);
          s.next.splice(i, 0, states.length - 1);
          at = states.length - 1;
        }
      }
      const s = states[at];
      // A match closes the transitions added so far into a chunk of their own.
      if (!(s.bytes.length === 0 && s.chunks.length > 0)) s.chunks.push(s.bytes.length);
    }
    const end = this.add();
    // Each trie state: its chunks in order, the last one (still open) included.
    const compileState = (index: number): number => {
      const s = states[index];
      let alternates = 0;
      let from = 0;
      const bounds = s.chunks.concat([s.bytes.length]);
      for (let chunk = 0; chunk < bounds.length; chunk++) {
        if (chunk > 0) alternates++; // the end of a literal that stops here
        const to = bounds[chunk];
        for (let i = from; i < to; i++) {
          if (states[s.next[i]].bytes.length > 0) compileState(s.next[i]);
        }
        const width = to - from;
        if (width === 1) this.add();
        else if (width > 1) this.add(K_OTHER, width * TRANSITION);
        if (width > 0) alternates++;
        from = to;
      }
      return this.add(K_UNION, alternates * ALTERNATE);
    };
    return { start: compileState(0), end };
  }

  private repetition(h: { min: number; max: number; sub: Hir }): Ref {
    const { min, max, sub } = h;
    if (min === 0 && max === 1) {
      const union = this.add(K_UNION);
      const inner = this.c(sub);
      const empty = this.add();
      this.patch(union, inner.start);
      this.patch(union, empty);
      this.patch(inner.end, empty);
      return { start: union, end: empty };
    }
    if (max === -1) return this.atLeast(sub, min);
    const prefix = this.concat(min, () => this.c(sub));
    if (min === max) return prefix;
    const empty = this.add();
    let previous = prefix.end;
    for (let i = min; i < max; i++) {
      const union = this.add(K_UNION);
      const inner = this.c(sub);
      this.patch(previous, union);
      this.patch(union, inner.start);
      this.patch(union, empty);
      previous = inner.end;
    }
    this.patch(previous, empty);
    return { start: prefix.start, end: empty };
  }

  private atLeast(sub: Hir, n: number): Ref {
    if (n === 0) {
      const least = minimumLength(sub);
      if (least !== null && least > 0) {
        const union = this.add(K_UNION);
        const inner = this.c(sub);
        this.patch(union, inner.start);
        this.patch(inner.end, union);
        return { start: union, end: union };
      }
      const inner = this.c(sub);
      const plus = this.add(K_UNION);
      this.patch(inner.end, plus);
      this.patch(plus, inner.start);
      const question = this.add(K_UNION);
      const empty = this.add();
      this.patch(question, inner.start);
      this.patch(question, empty);
      this.patch(plus, empty);
      return { start: question, end: empty };
    }
    if (n === 1) {
      const inner = this.c(sub);
      const union = this.add(K_UNION);
      this.patch(inner.end, union);
      this.patch(union, inner.start);
      return { start: inner.start, end: union };
    }
    const prefix = this.concat(n - 1, () => this.c(sub));
    const last = this.c(sub);
    const union = this.add(K_UNION);
    this.patch(prefix.end, last.start);
    this.patch(last.end, union);
    this.patch(union, last.start);
    return { start: prefix.start, end: union };
  }

  private class(h: { ranges: number[]; sequences?: number[][] }): Ref {
    const ranges = h.ranges;
    if (ranges[ranges.length - 1] <= 0x7f) {
      const end = this.add();
      return { start: this.add(K_OTHER, (ranges.length >> 1) * TRANSITION), end };
    }
    // perf: a class under `{1000}` is compiled a thousand times from the same sequences.
    let sequences = h.sequences;
    if (sequences === undefined) {
      sequences = h.sequences = [];
      for (let i = 0; i < ranges.length; i += 2) utf8Sequences(ranges[i], ranges[i + 1], sequences);
    }
    return this.reverse ? this.classReversed(sequences) : this.classForward(sequences);
  }

  /** The bytes of one expression compiled on its own. */
  alone(h: Hir): number {
    this.c(h);
    return this.bytes();
  }

  /**
   * The forward automaton of a class: Daciuk's construction over the sorted
   * byte sequences, with a bounded cache of the states already built
   * (`Utf8Compiler`).
   */
  private classForward(sequences: number[][]): Ref {
    const target = this.add();
    const stamp = ++this.stamp;
    // A state not built yet: its transitions (start, end, next), and the range of the one still open.
    const trans: number[][] = [[]];
    const lastStart: number[] = [-1];
    const lastEnd: number[] = [-1];
    const compile = (node: number[]): number => {
      hashStart();
      for (let i = 0; i < node.length; i++) hashStep(node[i]);
      const slot = hashSlot(FORWARD_SLOTS);
      if (this.forwardStamp[slot] === stamp && sameNumbers(this.forwardKeys[slot]!, node)) return this.forwardIds[slot];
      const id = this.add(K_OTHER, (node.length / 3) * TRANSITION);
      this.forwardStamp[slot] = stamp;
      this.forwardKeys[slot] = node;
      this.forwardIds[slot] = id;
      return id;
    };
    const freeze = (at: number, next: number): void => {
      if (lastStart[at] === -1) return;
      trans[at].push(lastStart[at], lastEnd[at], next);
      lastStart[at] = -1;
    };
    const compileFrom = (from: number): void => {
      let next = target;
      while (from + 1 < trans.length) {
        const at = trans.length - 1;
        freeze(at, next);
        const node = trans.pop()!;
        lastStart.pop();
        lastEnd.pop();
        next = compile(node);
      }
      freeze(trans.length - 1, next);
    };
    for (const seq of sequences) {
      const length = seq.length >> 1;
      let shared = 0;
      while (shared < length && shared < trans.length
          && lastStart[shared] === seq[2 * shared] && lastEnd[shared] === seq[2 * shared + 1]) shared++;
      compileFrom(shared);
      const top = trans.length - 1;
      lastStart[top] = seq[2 * shared];
      lastEnd[top] = seq[2 * shared + 1];
      for (let i = shared + 1; i < length; i++) {
        trans.push([]);
        lastStart.push(seq[2 * i]);
        lastEnd.push(seq[2 * i + 1]);
      }
    }
    compileFrom(0);
    return { start: compile(trans[0]), end: target };
  }

  /**
   * The reversed automaton of a class: one path a sequence, sharing what the
   * cache of 1 000 suffixes still remembers (`c_unicode_class_reverse_with_suffix`).
   */
  private classReversed(sequences: number[][]): Ref {
    const stamp = ++this.stamp;
    const union = this.add(K_UNION);
    const end = this.add();
    for (const seq of sequences) {
      let from = end;
      for (let i = 0; i < seq.length; i += 2) {
        hashStart();
        hashStep(from);
        hashStep(seq[i]);
        hashStep(seq[i + 1]);
        const slot = hashSlot(REVERSE_SLOTS);
        const range = (seq[i] << 8) | seq[i + 1];
        if (this.reverseStamp[slot] === stamp && this.reverseFrom[slot] === from && this.reverseRange[slot] === range) {
          from = this.reverseIds[slot];
          continue;
        }
        const id = this.add();
        this.reverseStamp[slot] = stamp;
        this.reverseFrom[slot] = from;
        this.reverseRange[slot] = range;
        this.reverseIds[slot] = id;
        from = id;
      }
      this.patch(union, from);
    }
    return { start: union, end };
  }
}

// ─── A first look, without the runtime's tables ───────────

/** What a set costs at most, in either automaton. */
function ceilingOf(set: CharSet): number {
  if (set.known === null) return set.ceiling;
  const ranges = set.known;
  if (ranges.length === 0 || ranges[ranges.length - 1] <= 0x7f) return 2 * STATE + (ranges.length >> 1) * TRANSITION;
  // A range of characters is at most 17 byte sequences of at most 4 states.
  return 2 * STATE + (ranges.length >> 1) * 3000;
}

/** More than the pattern can cost the crate, without asking the runtime for any table; capped just past `limit`. */
function ceiling(node: Node, limit: number): number {
  switch (node.t) {
    case 'empty': case 'assert': return STATE;
    // In a trie of literals a byte is a state, a union over it and their alternates.
    case 'lit': return 12 * STATE;
    case 'set': return ceilingOf(node.set);
    case 'cap': return 2 * STATE + ceiling(node.node, limit);
    case 'cat': case 'alt': {
      // A union, its end, and an alternate for each branch.
      let sum = 2 * STATE;
      for (const item of node.items) {
        sum += ceiling(item, limit) + 2 * ALTERNATE;
        if (sum > limit) return sum;
      }
      return sum;
    }
    case 'rep': {
      // Each copy brings a union and two alternates of its own; an open end, three unions and an empty state.
      const copies = Math.max(node.min, node.max, 1);
      const one = ceiling(node.node, limit) + STATE + 4 * ALTERNATE;
      return Math.min(copies * one + 4 * STATE + 4 * ALTERNATE, 2 * limit + 1);
    }
  }
}

// ─── A second look: every repeated piece counted once ─────

/**
 * About what an expression takes: each class and each alternation of
 * literals compiled once, on its own, and multiplied by its copies. A copy in
 * its place can differ by the few states a full cache slot costs it — a
 * percent or two.
 */
function estimate(h: Hir, reverse: boolean, limit: number): number {
  switch (h.k) {
    case 'empty': case 'look': case 'fail': return STATE;
    case 'lit': return STATE * h.bytes.length;
    case 'class': {
      const alone = h.alone ?? (h.alone = [-1, -1]);
      const side = reverse ? 1 : 0;
      if (alone[side] === -1) alone[side] = new Compiler(reverse, Number.MAX_SAFE_INTEGER).alone(h);
      return alone[side];
    }
    case 'cap': return (reverse ? 0 : 2 * STATE) + estimate(h.sub, reverse, limit);
    case 'cat': {
      let sum = 0;
      for (const sub of h.subs) {
        sum += estimate(sub, reverse, limit);
        if (sum > 4 * limit) return sum;
      }
      return sum;
    }
    case 'alt': {
      if (h.subs.every(sub => sub.k === 'lit')) return new Compiler(reverse, Number.MAX_SAFE_INTEGER).alone(h);
      let sum = 2 * STATE;
      for (const sub of h.subs) {
        sum += estimate(sub, reverse, limit) + ALTERNATE;
        if (sum > 4 * limit) return sum;
      }
      return sum;
    }
    case 'rep': {
      const one = estimate(h.sub, reverse, limit);
      let sum: number;
      if (h.max === -1) {
        // `x*` of an `x` that cannot be empty is `x` and one union; else two unions and an empty state more.
        const least = h.min === 0 ? minimumLength(h.sub) : 1;
        const loop = least !== null && least > 0 ? STATE + 2 * ALTERNATE : 3 * STATE + 4 * ALTERNATE;
        sum = Math.max(h.min, 1) * one + loop;
      } else if (h.min === 0 && h.max === 1) {
        sum = one + 2 * STATE + 2 * ALTERNATE;
      } else {
        // No copy at all is still one empty state.
        sum = (h.min === 0 ? STATE : h.min * one) + (h.max === h.min ? 0 : STATE + (h.max - h.min) * (one + STATE + 3 * ALTERNATE));
      }
      return Math.min(sum, 4 * limit + 1);
    }
  }
}

/** What the crate adds around any pattern: the prefix, the outermost group, the match state. */
const AROUND = 8 * STATE;

/**
 * Would `Regex::new` refuse the pattern for its size?
 *
 * perf: most patterns are far below the limit whatever their classes hold,
 * and are answered from the tree alone. Only one that might come near has
 * its classes turned into ranges and its states counted.
 */
export function exceedsSizeLimit(tree: Node, limit = SIZE_LIMIT): boolean {
  if (ceiling(tree, limit) + AROUND <= limit) return false;
  const hir = toHir(tree);
  // safety: a pattern several times over the limit is refused at once — the
  // states of `(?:.{1000}){36}` would be counted one by one up to the limit
  // otherwise, a third of a second for each such pattern of a document. Only
  // a pattern within a quarter of the limit is counted state by state.
  const about = Math.max(estimate(hir, false, limit), estimate(hir, true, limit));
  if (about > 1.25 * limit) return true;
  if (about + AROUND < 0.8 * limit) return false;
  try {
    new Compiler(false, limit).build(hir);
    new Compiler(true, limit).build(hir);
    return false;
  } catch (e) {
    if (e instanceof TooBig) return true;
    throw e;
  }
}

/** The first look of {@link exceedsSizeLimit}: never less than either automaton takes — for tests. */
export function sizeCeiling(tree: Node): number {
  return ceiling(tree, Number.MAX_SAFE_INTEGER) + AROUND;
}

/** The second look of {@link exceedsSizeLimit}: about what the larger automaton takes — for tests. */
export function sizeEstimate(tree: Node): number {
  const hir = toHir(tree);
  const unbounded = Number.MAX_SAFE_INTEGER / 8;
  return Math.max(estimate(hir, false, unbounded), estimate(hir, true, unbounded)) + AROUND;
}

/** The bytes the crate accounts for the forward and the reversed automaton — for tests and measurements. */
export function automatonSizes(tree: Node): [number, number] {
  const hir = toHir(tree);
  const sizes: number[] = [];
  for (const reverse of [false, true]) {
    const compiler = new Compiler(reverse, Number.MAX_SAFE_INTEGER);
    compiler.build(hir);
    sizes.push(compiler.bytes());
  }
  return [sizes[0], sizes[1]];
}
