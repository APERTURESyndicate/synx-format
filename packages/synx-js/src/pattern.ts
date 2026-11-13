/**
 * `[pattern:…]` in the dialect of the Rust `regex` crate, run in time linear
 * in the value.
 *
 * safety: `RegExp` backtracks. `^(a|aa)+$` against 41 characters held the
 * event loop for longer than 20 s here, and synx-core answers at once (S3-06).
 * A pattern is compiled into a small program and run as a Pike VM — every
 * thread advances one character at a time, so the cost is pattern size ×
 * value length, as with the crate.
 *
 * spec: the dialect is the crate's (E16), read by `pattern-syntax.ts` the way
 * `regex-syntax` reads it. A pattern the crate would not compile — for its
 * syntax, or for its size (`pattern-size.ts`) — is `null`, and the constraint
 * is not checked, as for an invalid pattern.
 */
import {
  A_TEXT_END, A_TEXT_START, Node, Pred, Unsupported, holds, parsePattern,
} from './pattern-syntax';

// ─── Program ──────────────────────────────────────────────

const I_CHAR = 0;
const I_SPLIT = 1;
const I_JMP = 2;
const I_ASSERT = 3;
const I_MATCH = 4;

/**
 * safety: no pattern the crate compiles comes near this many instructions —
 * its own limit stops at some 330 000 states. A program past it means the
 * size check let something through, and the pattern is refused.
 */
const MAX_PROGRAM = 1_000_000;

/** A program while it is being written. */
interface Draft {
  op: number[];
  x: number[];
  y: number[];
  pred: (Pred | null)[];
}

/**
 * A compiled pattern, with the scratch of its runs.
 *
 * perf: a pattern is run for every value it checks, most of them a dozen
 * characters long. Each run used to allocate its four work arrays and a
 * closure, which cost more than the match; they belong to the program now and
 * the next run takes them as the last one left them.
 */
export interface Program {
  op: Int32Array;
  x: Int32Array;
  y: Int32Array;
  pred: (Pred | null)[];
  /** The step an instruction was last expanded in. */
  mark: Int32Array;
  clist: Int32Array;
  nlist: Int32Array;
  stack: Int32Array;
  /** The last step counted in `mark`, over every run so far. */
  gen: number;
  /** The pattern opens with `^`: a match starts at the first character or nowhere. */
  anchored: boolean;
  /** What each character step answers for an ASCII character: 0 no, 1 yes, 2 not asked yet. */
  ascii: Uint8Array | null;
  /** The table of {@link Dfa}, grown as values are checked; `null` when the pattern has none. */
  dfa: Dfa | null;
  /** The pattern can have a table: its only assertions are the two ends of the text. */
  tabled: boolean;
}

/**
 * The states the threads of a program were found in, and where each ASCII
 * character takes them: one array read per character instead of a step of
 * every thread.
 *
 * perf: checking `^[a-z0-9-]+$` against a dozen characters cost twice what the
 * backtracking `RegExp` of 3.8.0 did. The table is still linear in the value —
 * it is the same machine with its steps remembered — and bounded: a pattern
 * that needs more than MAX_DFA_STATES states goes back to stepping.
 */
interface Dfa {
  ids: Map<string, number>;
  /** Per state: the threads it starts from, sorted. */
  threads: number[][];
  /** Per state: the character steps those threads stand at inside the text. */
  steps: Int32Array[];
  /** Per state: the next state for each ASCII character, DFA_UNKNOWN or DFA_DEAD. */
  next: Int16Array[];
  /** Per state: 1 when the pattern has matched whatever follows. */
  matched: number[];
  /** Per state: 1 when it matches if the text ends here, 0 when not, -1 until asked. */
  matchedAtEnd: number[];
}

/** The transition has not been worked out. */
const DFA_UNKNOWN = -1;
/** No thread is left and none can start: the text does not match. */
const DFA_DEAD = -2;
/** The table is full. */
const DFA_FULL = -3;
/** States one pattern may have in its table: 256 bytes of transitions each. */
const MAX_DFA_STATES = 64;

/** Programs up to this size remember their answers for ASCII (128 bytes an instruction). */
const MAX_ASCII_MEMO_PROGRAM = 1024;
/** `gen` is kept below this, so it always fits the `mark` array. */
const GEN_LIMIT = 0x7fffffff;

function emit(p: Draft, op: number, x = 0, y = 0, pred: Pred | null = null): number {
  if (p.op.length >= MAX_PROGRAM) throw new Unsupported();
  p.op.push(op); p.x.push(x); p.y.push(y); p.pred.push(pred);
  return p.op.length - 1;
}

function compileNode(p: Draft, n: Node): void {
  switch (n.t) {
    case 'empty': return;
    case 'lit': {
      const c = n.c;
      emit(p, I_CHAR, 0, 0, x => x === c);
      return;
    }
    case 'set': emit(p, I_CHAR, 0, 0, n.set.has); return;
    case 'assert': emit(p, I_ASSERT, n.kind); return;
    case 'cap': compileNode(p, n.node); return;
    case 'cat': for (const item of n.items) compileNode(p, item); return;
    case 'alt': {
      const jumps: number[] = [];
      for (let i = 0; i < n.items.length; i++) {
        if (i < n.items.length - 1) {
          const split = emit(p, I_SPLIT);
          p.x[split] = split + 1;
          compileNode(p, n.items[i]);
          jumps.push(emit(p, I_JMP));
          p.y[split] = p.op.length;
        } else {
          compileNode(p, n.items[i]);
        }
      }
      for (const j of jumps) p.x[j] = p.op.length;
      return;
    }
    case 'rep': {
      for (let i = 0; i < n.min; i++) compileNode(p, n.node);
      if (n.max === -1) {
        const split = emit(p, I_SPLIT);
        p.x[split] = split + 1;
        compileNode(p, n.node);
        emit(p, I_JMP, split);
        p.y[split] = p.op.length;
      } else {
        const exits: number[] = [];
        for (let i = n.min; i < n.max; i++) {
          const split = emit(p, I_SPLIT);
          p.x[split] = split + 1;
          exits.push(split);
          compileNode(p, n.node);
        }
        for (const s of exits) p.y[s] = p.op.length;
      }
      return;
    }
  }
}

/** The pattern can match nothing but the empty string: repeating it adds no instruction worth having. */
function matchesOnlyEmpty(n: Node): boolean {
  switch (n.t) {
    case 'empty': case 'assert': return true;
    case 'lit': case 'set': return false;
    case 'cap': case 'rep': return matchesOnlyEmpty(n.node);
    case 'cat': case 'alt': return n.items.every(matchesOnlyEmpty);
  }
}

/**
 * The tree the program is written from: a repetition of something that only
 * matches the empty string is that thing once, or not at all — the crate
 * makes the same cut, and `\b{4000000000}` would otherwise be four thousand
 * million instructions.
 */
function trimmed(n: Node): Node {
  switch (n.t) {
    case 'cap': return { ...n, node: trimmed(n.node) };
    case 'cat': case 'alt': return { ...n, items: n.items.map(trimmed) };
    case 'rep': {
      const node = trimmed(n.node);
      if (!matchesOnlyEmpty(node)) return { ...n, node };
      return { ...n, node, min: Math.min(n.min, 1), max: n.max === -1 ? 1 : Math.min(n.max, 1) };
    }
    default: return n;
  }
}

let sizeModule: typeof import('./pattern-size') | undefined;

/** Compile a pattern, or `null` when it is not checked (see the file comment). */
export function compileLinear(pattern: string): Program | null {
  const tree = parsePattern(pattern);
  if (tree === null) return null;
  try {
    // perf: the module that counts what the crate would build is asked for
    // by the first pattern that has a counted repetition — the only way a
    // pattern of 256 characters comes anywhere near the crate's limit.
    if (pattern.indexOf('{') !== -1 || pattern.length > 256) {
      // eslint-disable-next-line @typescript-eslint/no-var-requires
      sizeModule ??= require('./pattern-size') as typeof import('./pattern-size');
      if (sizeModule.exceedsSizeLimit(tree)) return null;
    }
    const draft: Draft = { op: [], x: [], y: [], pred: [] };
    compileNode(draft, trimmed(tree));
    emit(draft, I_MATCH);
    const size = draft.op.length;
    return {
      op: Int32Array.from(draft.op),
      x: Int32Array.from(draft.x),
      y: Int32Array.from(draft.y),
      pred: draft.pred,
      mark: new Int32Array(size).fill(-1),
      clist: new Int32Array(size),
      nlist: new Int32Array(size),
      // Each instruction is expanded once per step and pushes at most two.
      stack: new Int32Array(2 * size + 2),
      gen: 0,
      anchored: draft.op[0] === I_ASSERT && draft.x[0] === A_TEXT_START,
      ascii: null,
      dfa: null,
      // `\b`, `\<` and `(?m)^` read the characters on both sides: no table for those.
      tabled: draft.op.every((op, at) => op !== I_ASSERT || draft.x[at] === A_TEXT_START || draft.x[at] === A_TEXT_END),
    };
  } catch (e) {
    if (e instanceof Unsupported) return null;
    throw e;
  }
}

// ─── Pike VM ──────────────────────────────────────────────

/** Set by `follow` when it reaches the end of the program. */
let matched = false;

/**
 * Follow jumps, splits and assertions from `pc` in step `gen`; the character
 * steps it arrives at are appended to `list`, whose new length is returned.
 */
function follow(p: Program, list: Int32Array, count: number, pc: number, prev: number, next: number, gen: number): number {
  const op = p.op;
  const mark = p.mark;
  // perf: a thread that stepped over a character most often stands at the
  // next character step — no stack for that.
  if (op[pc] === I_CHAR) {
    if (mark[pc] !== gen) {
      mark[pc] = gen;
      list[count++] = pc;
    }
    return count;
  }
  const stack = p.stack;
  let sp = 0;
  stack[sp++] = pc;
  while (sp > 0) {
    const at = stack[--sp];
    if (mark[at] === gen) continue;
    mark[at] = gen;
    switch (op[at]) {
      case I_CHAR: list[count++] = at; break;
      case I_JMP: stack[sp++] = p.x[at]; break;
      case I_SPLIT: stack[sp++] = p.y[at]; stack[sp++] = p.x[at]; break;
      case I_ASSERT: if (holds(p.x[at], prev, next)) stack[sp++] = at + 1; break;
      default: matched = true; // I_MATCH
    }
  }
  return count;
}

/** Whether `value` contains a match of the program — `Regex::is_match`. */
export function runLinear(p: Program, value: string): boolean {
  if (p.tabled) {
    const answer = runTable(p, value);
    if (answer !== TABLE_NO_ANSWER) return answer === 1;
  }
  return runThreads(p, value);
}

// ─── The table ────────────────────────────────────────────

/** `runTable` cannot tell: the value has a character beyond ASCII, or the table is full. */
const TABLE_NO_ANSWER = -1;

/** A step of its own in `mark` for one expansion of a state. */
function nextGen(p: Program): number {
  if (p.gen >= GEN_LIMIT - 2) {
    p.mark.fill(-1);
    p.gen = 0;
  }
  return ++p.gen;
}

/**
 * Expand the threads of a state into `p.nlist`, as one step of `runThreads`
 * does; the number of character steps is returned and `matched` is set.
 * Only the two ends of the text matter to a tabled program: `first` is the
 * position before any character, `last` the one after all of them.
 */
function expand(p: Program, threads: number[], first: boolean, last: boolean): number {
  const gen = nextGen(p);
  const prev = first ? -1 : 0;
  const next = last ? -1 : 0;
  matched = false;
  let count = 0;
  for (let k = 0; k < threads.length; k++) count = follow(p, p.nlist, count, threads[k], prev, next, gen);
  if (first || !p.anchored) count = follow(p, p.nlist, count, 0, prev, next, gen);
  return count;
}

/** The state of `threads` (sorted), added to the table when it is new; DFA_FULL when there is no room. */
function tableState(p: Program, dfa: Dfa, threads: number[], first: boolean): number {
  // The state before the first character is the only one `^` holds in.
  const key = first ? '^' : threads.join(',');
  const known = dfa.ids.get(key);
  if (known !== undefined) return known;
  const id = dfa.threads.length;
  if (id >= MAX_DFA_STATES) return DFA_FULL;
  const count = expand(p, threads, first, false);
  dfa.ids.set(key, id);
  dfa.threads.push(threads);
  dfa.steps.push(p.nlist.slice(0, count));
  dfa.next.push(new Int16Array(128).fill(DFA_UNKNOWN));
  dfa.matched.push(matched ? 1 : 0);
  dfa.matchedAtEnd.push(-1);
  return id;
}

/** Work out where character `c` takes state `s`, and remember it. */
function tableStep(p: Program, dfa: Dfa, s: number, c: number): number {
  const steps = dfa.steps[s];
  const preds = p.pred;
  const threads: number[] = [];
  for (let k = 0; k < steps.length; k++) {
    if (preds[steps[k]]!(c)) threads.push(steps[k] + 1);
  }
  let to: number;
  if (threads.length === 0 && p.anchored) {
    to = DFA_DEAD;
  } else {
    threads.sort((a, b) => a - b);
    to = tableState(p, dfa, threads, false);
    if (to === DFA_FULL) return DFA_FULL;
  }
  dfa.next[s][c] = to;
  return to;
}

/** 1 or 0 for a match or none; TABLE_NO_ANSWER when the threads have to be stepped. */
function runTable(p: Program, value: string): number {
  let dfa = p.dfa;
  if (dfa === null) {
    dfa = p.dfa = { ids: new Map(), threads: [], steps: [], next: [], matched: [], matchedAtEnd: [] };
    tableState(p, dfa, [], true);
  }
  const n = value.length;
  let s = 0;
  for (let i = 0; i < n; i++) {
    if (dfa.matched[s] === 1) return 1;
    const c = value.charCodeAt(i);
    if (c >= 128) return TABLE_NO_ANSWER;
    let to = dfa.next[s][c];
    if (to === DFA_UNKNOWN) {
      to = tableStep(p, dfa, s, c);
      if (to === DFA_FULL) {
        // A pattern with this many states is stepped from now on.
        p.dfa = null;
        p.tabled = false;
        return TABLE_NO_ANSWER;
      }
    }
    if (to === DFA_DEAD) return 0;
    s = to;
  }
  let atEnd = dfa.matchedAtEnd[s];
  if (atEnd === -1) {
    expand(p, dfa.threads[s], s === 0, true);
    atEnd = dfa.matchedAtEnd[s] = matched ? 1 : 0;
  }
  return atEnd;
}

// ─── The threads ──────────────────────────────────────────

/** `runLinear` by stepping every thread over every character. */
function runThreads(p: Program, value: string): boolean {
  const n = value.length;
  // One step per character and one more; `mark` starts over before `gen` could wrap.
  if (p.gen >= GEN_LIMIT - n - 2) {
    p.mark.fill(-1);
    p.gen = 0;
  }
  const op = p.op;
  const preds = p.pred;
  const clist = p.clist;
  const nlist = p.nlist;
  const anchored = p.anchored;
  let ascii = p.ascii;
  if (ascii === null && op.length <= MAX_ASCII_MEMO_PROGRAM) ascii = p.ascii = new Uint8Array(op.length * 128).fill(2);
  let ccount = 0;
  let ncount = 0;
  let gen = p.gen;
  let prev = -1;
  let i = 0;
  matched = false;
  for (;;) {
    let next = -1;
    let width = 0;
    if (i < n) {
      next = value.codePointAt(i)!;
      width = next > 0xffff ? 2 : 1;
    }
    gen++;
    ncount = 0;
    for (let k = 0; k < ccount; k++) ncount = follow(p, nlist, ncount, clist[k], prev, next, gen);
    // Unanchored: a thread starts at every position, after the ones already
    // running. Behind `^` only the first position can start one.
    if (i === 0 || !anchored) ncount = follow(p, nlist, ncount, 0, prev, next, gen);
    if (matched || next === -1 || (ncount === 0 && anchored)) {
      p.gen = gen;
      return matched;
    }
    // Step every thread over `next`; `clist` was read above and is free again.
    ccount = 0;
    if (next < 128 && ascii !== null) {
      for (let k = 0; k < ncount; k++) {
        const at = nlist[k];
        const slot = at * 128 + next;
        let known = ascii[slot];
        if (known === 2) known = ascii[slot] = preds[at]!(next) ? 1 : 0;
        if (known === 1) clist[ccount++] = at + 1;
      }
    } else {
      for (let k = 0; k < ncount; k++) {
        const at = nlist[k];
        if (preds[at]!(next)) clist[ccount++] = at + 1;
      }
    }
    prev = next;
    i += width;
  }
}

// ─── Patterns of a document ───────────────────────────────

/**
 * Test `value` against `pattern` in the dialect of the `regex` crate, in time
 * linear in the value.
 *
 * Returns `null` for a refused or uncompilable pattern, and the caller then
 * skips the constraint — what synx-core does with a pattern it cannot
 * compile. A pattern that repeats a repetition (`^(a+)+$`) is refused by every
 * implementation, synx-core included (conformance 105).
 *
 * spec: `RegExp` read another dialect — `(?i)^abc$` did not compile (B-048.5),
 * `\p{L}` was the letter `p` (B-048.6), a lone `{` a literal brace (B-048.4) —
 * and backtracked: `^(a|aa)+$` against 41 characters ran past 20 s (S3-06).
 */
export function matchesLinear(pattern: string, value: string): boolean | null {
  let program = PATTERN_CACHE.get(pattern);
  if (program === undefined) {
    program = hasNestedQuantifier(pattern) ? null : compileLinear(pattern);
    const size = program === null ? 1 : program.op.length;
    // safety: bounded twice over, so a document of distinct patterns cannot
    // grow it forever — by their number, and by the instructions they hold
    // (one pattern may be a third of a million).
    if (PATTERN_CACHE.size >= 256 || cachedInstructions + size > MAX_CACHED_INSTRUCTIONS) {
      PATTERN_CACHE.clear();
      cachedInstructions = 0;
    }
    PATTERN_CACHE.set(pattern, program);
    cachedInstructions += size;
  }
  return program === null ? null : runLinear(program, value);
}

/**
 * Compiled `[pattern:…]` constraints; `null` marks a refused or invalid one.
 * perf: a pattern was compiled for every cell it checked.
 */
const PATTERN_CACHE = new Map<string, Program | null>();
/** Instructions of the programs in the cache; each costs some 50 bytes. */
let cachedInstructions = 0;
const MAX_CACHED_INSTRUCTIONS = 1_000_000;

/**
 * Does the pattern repeat something that already repeats — the `(a+)+` shape
 * behind exponential backtracking?
 *
 * Deliberately conservative: it looks for a quantified group whose body also
 * contains a quantifier, which covers the classic blow-ups without rejecting
 * the flat patterns constraints normally use.
 */
function hasNestedQuantifier(pattern: string): boolean {
  const opens: number[] = [];
  for (let i = 0; i < pattern.length; i++) {
    const ch = pattern[i];
    if (ch === '\\') { i++; continue; }
    if (ch === '[') {
      // Skip a character class wholesale; quantifiers inside it are literal.
      while (i < pattern.length && pattern[i] !== ']') {
        if (pattern[i] === '\\') i++;
        i++;
      }
      continue;
    }
    if (ch === '(') { opens.push(i); continue; }
    if (ch === ')') {
      const start = opens.pop();
      if (start === undefined) continue;
      const next = pattern[i + 1];
      if (next === '*' || next === '+' || next === '{') {
        const body = pattern.slice(start + 1, i).replace(/\\./g, '');
        if (/[*+]|\{\d/.test(body)) return true;
      }
    }
  }
  return false;
}
