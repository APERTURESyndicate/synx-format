/**
 * The syntax of `[pattern:…]`: the dialect of the Rust `regex` crate, read
 * the way `regex-syntax` reads it.
 *
 * spec: synx-core hands a pattern to `regex::Regex::new` and skips the
 * constraint when that fails, so what a pattern means — and whether it means
 * anything — is whatever the crate says (E16). This reader follows the
 * crate's parser and translator step for step: flags and their scope, verbose
 * mode, classes with nesting and set operations, POSIX and Unicode classes
 * with their loose names, simple case folding, the ASCII world of `(?-u)`,
 * the nesting limit. A pattern the crate refuses is refused here (`null` from
 * `parsePattern`), for the same reason.
 *
 * What the crate knows and a JavaScript runtime does not is refused too, and
 * the constraint goes unchecked: `\p{Age=…}`, the three break properties, and
 * the binary properties outside ECMA-262 (`Other_Alphabetic`, `Hyphen`…).
 */

/** A test of one character, by code point. */
export type Pred = (c: number) => boolean;

/** Thrown inside the reader for any pattern that is not compiled. */
export class Unsupported extends Error {}

// ─── Ranges ───────────────────────────────────────────────

const MAX_SCALAR = 0x10ffff;
const SURROGATE_LO = 0xd800;
const SURROGATE_HI = 0xdfff;

/** Inclusive ranges `[lo0, hi0, lo1, hi1, …]` put in order, with overlapping and touching ones joined. */
export function normalizeRanges(pairs: number[]): number[] {
  if (pairs.length <= 2) return pairs;
  const order: number[] = [];
  for (let i = 0; i < pairs.length; i += 2) order.push(i);
  order.sort((a, b) => pairs[a] - pairs[b] || pairs[a + 1] - pairs[b + 1]);
  const out: number[] = [];
  for (const i of order) {
    const lo = pairs[i];
    const hi = pairs[i + 1];
    const last = out.length - 1;
    // Two ranges on either side of the surrogates do not touch: no character lies between them, yet none joins them.
    if (last >= 0 && lo <= out[last] + 1) {
      if (hi > out[last]) out[last] = hi;
    } else {
      out.push(lo, hi);
    }
  }
  return out;
}

function containsRange(ranges: number[], c: number): boolean {
  let lo = 0;
  let hi = (ranges.length >> 1) - 1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (c < ranges[2 * mid]) hi = mid - 1;
    else if (c > ranges[2 * mid + 1]) lo = mid + 1;
    else return true;
  }
  return false;
}

function rangesPred(ranges: number[]): Pred {
  if (ranges.length === 0) return () => false;
  if (ranges.length === 2) {
    const lo = ranges[0];
    const hi = ranges[1];
    return lo === hi ? c => c === lo : c => c >= lo && c <= hi;
  }
  if (ranges.length <= 8) {
    return c => {
      for (let i = 0; i < ranges.length; i += 2) if (c >= ranges[i] && c <= ranges[i + 1]) return true;
      return false;
    };
  }
  return c => containsRange(ranges, c);
}

/** Every scalar value outside `ranges`: the surrogates belong to neither side. */
function complementRanges(ranges: number[]): number[] {
  const out: number[] = [];
  const gap = (lo: number, hi: number): void => {
    if (lo > hi) return;
    if (lo < SURROGATE_LO && hi > SURROGATE_HI) out.push(lo, SURROGATE_LO - 1, SURROGATE_HI + 1, hi);
    else if (lo >= SURROGATE_LO && hi <= SURROGATE_HI) return;
    else if (lo < SURROGATE_LO && hi >= SURROGATE_LO) out.push(lo, SURROGATE_LO - 1);
    else if (lo <= SURROGATE_HI && hi > SURROGATE_HI) out.push(SURROGATE_HI + 1, hi);
    else out.push(lo, hi);
  };
  let next = 0;
  for (let i = 0; i < ranges.length; i += 2) {
    gap(next, ranges[i] - 1);
    next = ranges[i + 1] + 1;
  }
  gap(next, MAX_SCALAR);
  return out;
}

function intersectRanges(a: number[], b: number[]): number[] {
  const out: number[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    const lo = Math.max(a[i], b[j]);
    const hi = Math.min(a[i + 1], b[j + 1]);
    if (lo <= hi) out.push(lo, hi);
    if (a[i + 1] < b[j + 1]) i += 2; else j += 2;
  }
  return out;
}

function subtractRanges(a: number[], b: number[]): number[] {
  return intersectRanges(a, complementRanges(b));
}

// ─── Character sets ───────────────────────────────────────

/**
 * A set of characters: a test for the matcher, and its members as ranges for
 * whoever asks what the pattern costs the crate to compile.
 *
 * perf: a class named by a Unicode property is tested through the runtime's
 * own tables, one character at a time. Its ranges take a pass over every
 * code point, so they are worked out only when asked for.
 */
export interface CharSet {
  has: Pred;
  /** The members, as normalized inclusive ranges of scalar values. */
  ranges(): number[];
  /** The ranges, when they are known without asking the runtime; else `null`. */
  known: number[] | null;
  /** For a set whose ranges are not known: the most the crate spends on it, in bytes. */
  ceiling: number;
}

/** What a class of unknown shape is taken to cost at most (the largest found, six categories combined, is 60 KB). */
const CEILING_UNKNOWN = 400_000;

function setOfRanges(pairs: number[]): CharSet {
  const ranges = normalizeRanges(pairs);
  return { has: rangesPred(ranges), ranges: () => ranges, known: ranges, ceiling: 0 };
}

function lazySet(has: Pred, compute: () => number[], ceiling = CEILING_UNKNOWN): CharSet {
  let ranges: number[] | null = null;
  return { has, ranges: () => ranges ?? (ranges = compute()), known: null, ceiling };
}

const EMPTY_SET = setOfRanges([]);

function unionSets(sets: CharSet[]): CharSet {
  if (sets.length === 0) return EMPTY_SET;
  if (sets.length === 1) return sets[0];
  if (sets.every(s => s.known !== null)) {
    const all: number[] = [];
    for (const s of sets) for (const v of s.known!) all.push(v);
    return setOfRanges(all);
  }
  let ceiling = 0;
  for (const s of sets) ceiling += s.known !== null ? 64 + 3000 * (s.known.length >> 1) : s.ceiling;
  return lazySet(
    c => {
      for (let i = 0; i < sets.length; i++) if (sets[i].has(c)) return true;
      return false;
    },
    () => {
      const all: number[] = [];
      for (const s of sets) for (const v of s.ranges()) all.push(v);
      return normalizeRanges(all);
    },
    Math.max(ceiling, CEILING_UNKNOWN),
  );
}

function negateSet(set: CharSet): CharSet {
  if (set.known !== null) return setOfRanges(complementRanges(set.known));
  return lazySet(c => !set.has(c), () => complementRanges(set.ranges()));
}

const SET_AND = 1;
const SET_MINUS = 2;
const SET_XOR = 3;

function combineSets(op: number, a: CharSet, b: CharSet): CharSet {
  const onRanges = (x: number[], y: number[]): number[] => {
    if (op === SET_AND) return intersectRanges(x, y);
    if (op === SET_MINUS) return subtractRanges(x, y);
    return normalizeRanges(subtractRanges(x, y).concat(subtractRanges(y, x)));
  };
  if (a.known !== null && b.known !== null) return setOfRanges(onRanges(a.known, b.known));
  const has: Pred = op === SET_AND ? c => a.has(c) && b.has(c)
    : op === SET_MINUS ? c => a.has(c) && !b.has(c)
      : c => a.has(c) !== b.has(c);
  return lazySet(has, () => onRanges(a.ranges(), b.ranges()));
}

// ─── Unicode tables of the runtime ────────────────────────

/** Ranges of the classes already asked of the runtime, by the source of their test. */
const ENUMERATED = new Map<string, number[]>();

/**
 * Every scalar value a runtime class takes in, as ranges.
 *
 * perf: one regular expression run over the code points in order finds whole
 * runs at a time — a test per code point took ten times as long.
 */
function enumerate(source: string): number[] {
  const cached = ENUMERATED.get(source);
  if (cached !== undefined) return cached;
  const re = new RegExp(`(?:${source})+`, 'gu');
  const out: number[] = [];
  const scan = (first: number, last: number, width: number): void => {
    const STEP = 0x2000;
    for (let from = first; from <= last; from += STEP) {
      const to = Math.min(from + STEP - 1, last);
      const codes: number[] = [];
      for (let c = from; c <= to; c++) codes.push(c);
      const text = String.fromCodePoint(...codes);
      re.lastIndex = 0;
      for (let m = re.exec(text); m !== null; m = re.exec(text)) {
        const lo = from + m.index / width;
        const hi = lo + m[0].length / width - 1;
        const end = out.length - 1;
        if (end >= 0 && out[end] + 1 === lo) out[end] = hi; else out.push(lo, hi);
      }
    }
  };
  // U+D7FF and U+E000 are not neighbours, so a range never runs across the surrogates.
  scan(0, SURROGATE_LO - 1, 1);
  scan(SURROGATE_HI + 1, 0xffff, 1);
  scan(0x10000, MAX_SCALAR, 2);
  ENUMERATED.set(source, out);
  return out;
}

/** A class of the runtime by the body of its `\p{…}`, or `null` when the runtime has no such class. */
function runtimeClass(body: string, ceiling = CEILING_UNKNOWN): CharSet | null {
  let re: RegExp;
  try {
    re = new RegExp(`^\\p{${body}}$`, 'u');
  } catch {
    return null;
  }
  return lazySet(c => re.test(String.fromCodePoint(c)), () => enumerate(`\\p{${body}}`), ceiling);
}

// perf: the Unicode tables behind these are built when a character beyond
// ASCII first asks for them — a value of a config rarely has one.
let DIGIT: RegExp | undefined;
let SPACE: RegExp | undefined;
let WORD: RegExp | undefined;

const WORD_SOURCE = '[\\p{Alphabetic}\\p{M}\\p{Nd}\\p{Pc}\\p{Join_Control}]';

function isAsciiDigit(c: number): boolean {
  return c >= 48 && c <= 57;
}

function isAsciiSpace(c: number): boolean {
  return c === 32 || (c >= 9 && c <= 13);
}

export function isAsciiWord(c: number): boolean {
  return (c >= 48 && c <= 57) || (c >= 65 && c <= 90) || (c >= 97 && c <= 122) || c === 95;
}

function isDigit(c: number): boolean {
  return c < 128 ? isAsciiDigit(c) : (DIGIT ??= /^\p{Nd}$/u).test(String.fromCodePoint(c));
}

function isSpace(c: number): boolean {
  return c < 128 ? isAsciiSpace(c) : (SPACE ??= /^\p{White_Space}$/u).test(String.fromCodePoint(c));
}

export function isWord(c: number): boolean {
  if (c < 128) return isAsciiWord(c);
  return (WORD ??= new RegExp(`^${WORD_SOURCE}$`, 'u')).test(String.fromCodePoint(c));
}

// The crate spends a seventh less than this on one copy of each
// (regex-automata 0.4, Unicode 16, measured through `regex::Regex::new`): a
// pattern that stays under the limit with these is not looked at any closer.
const DIGIT_SET = lazySet(isDigit, () => enumerate('\\p{Nd}'), 6_200);
const NOT_DIGIT_SET = lazySet(c => !isDigit(c), () => complementRanges(enumerate('\\p{Nd}')), 15_000);
const SPACE_SET = lazySet(isSpace, () => enumerate('\\p{White_Space}'), 800);
const NOT_SPACE_SET = lazySet(c => !isSpace(c), () => complementRanges(enumerate('\\p{White_Space}')), 2_400);
const WORD_SET = lazySet(isWord, () => enumerate(WORD_SOURCE), 58_000);
const NOT_WORD_SET = lazySet(c => !isWord(c), () => complementRanges(enumerate(WORD_SOURCE)), 54_000);

/** Whether an assertion holds between `prev` and `next` (-1 at an end of the text). */
export function holds(kind: number, prev: number, next: number): boolean {
  const word = (kind & A_ASCII) !== 0 ? isAsciiWord : isWord;
  switch (kind & 15) {
    case A_TEXT_START: return prev === -1;
    case A_TEXT_END: return next === -1;
    case A_LINE_START: return prev === -1 || prev === 10;
    case A_LINE_END: return next === -1 || next === 10;
    // `\r\n` is one line break: nothing starts or ends inside it.
    case A_LINE_START_CRLF: return prev === -1 || prev === 10 || (prev === 13 && next !== 10);
    case A_LINE_END_CRLF: return next === -1 || next === 13 || (next === 10 && prev !== 13);
    case A_WORD: return (prev !== -1 && word(prev)) !== (next !== -1 && word(next));
    case A_NOT_WORD: return (prev !== -1 && word(prev)) === (next !== -1 && word(next));
    case A_WORD_START: return !(prev !== -1 && word(prev)) && next !== -1 && word(next);
    case A_WORD_END: return prev !== -1 && word(prev) && !(next !== -1 && word(next));
    case A_WORD_START_HALF: return !(prev !== -1 && word(prev));
    case A_WORD_END_HALF: return !(next !== -1 && word(next));
    default: return false;
  }
}

// ─── Syntax tree ──────────────────────────────────────────

/**
 * The pattern as the crate's translator leaves it, before its own
 * simplifications. `h` is how deep the crate's syntax tree is under the node:
 * the crate refuses a pattern nested deeper than {@link NEST_LIMIT}.
 */
export type Node =
  | { t: 'lit'; c: number; h: number }
  | { t: 'set'; set: CharSet; h: number }
  | { t: 'cat'; items: Node[]; h: number }
  | { t: 'alt'; items: Node[]; h: number }
  | { t: 'rep'; node: Node; min: number; max: number; h: number } // max -1: unbounded
  | { t: 'cap'; node: Node; h: number }
  | { t: 'assert'; kind: number; h: number }
  // An empty group, or flags on their own (`(?i)`).
  | { t: 'empty'; h: number };

export const A_TEXT_START = 0;
export const A_TEXT_END = 1;
export const A_LINE_START = 2;
export const A_LINE_END = 3;
export const A_LINE_START_CRLF = 4;
export const A_LINE_END_CRLF = 5;
export const A_WORD = 6;
export const A_NOT_WORD = 7;
export const A_WORD_START = 8;
export const A_WORD_END = 9;
export const A_WORD_START_HALF = 10;
export const A_WORD_END_HALF = 11;
/** Added to a word assertion under `(?-u)`: a word character is `[0-9A-Za-z_]`. */
export const A_ASCII = 16;

// ─── Simple case folding ──────────────────────────────────

const NO_ORBIT: number[] = [];
const ASCII_ORBIT: Record<number, number[]> = {
  0x4b: [0x6b, 0x212a], 0x6b: [0x4b, 0x212a], // K, k, KELVIN SIGN
  0x53: [0x73, 0x17f], 0x73: [0x53, 0x17f],   // S, s, LONG S
};

/** For every character with a case partner: the group it was found in, until checked. */
let ORBIT_CANDIDATES: Map<number, number[]> | null = null;
/** The orbits already checked against the runtime's own case folding. */
const ORBITS = new Map<number, number[]>();

/**
 * Every character that lower- or upper-cases to another, grouped with the
 * characters it reaches — a superset of the simple case folding orbits.
 *
 * perf: one pass over the planes that have cased letters, taken the first
 * time a case-insensitive pattern meets a character beyond ASCII.
 */
function orbitCandidates(): Map<number, number[]> {
  const parent = new Map<number, number>();
  const find = (x: number): number => {
    let root = x;
    for (let p = parent.get(root); p !== undefined && p !== root; p = parent.get(root)) root = p;
    return root;
  };
  const link = (a: number, b: number): void => {
    if (!parent.has(a)) parent.set(a, a);
    if (!parent.has(b)) parent.set(b, b);
    const ra = find(a);
    const rb = find(b);
    if (ra !== rb) parent.set(ra, rb);
  };
  for (let c = 0x41; c < 0x1f000; c++) {
    if (c >= SURROGATE_LO && c <= SURROGATE_HI) continue;
    const s = String.fromCodePoint(c);
    const lower = s.toLowerCase();
    if (lower !== s) {
      const l = lower.codePointAt(0)!;
      if (lower.length === (l > 0xffff ? 2 : 1)) link(c, l);
    }
    const upper = s.toUpperCase();
    if (upper !== s) {
      const u = upper.codePointAt(0)!;
      if (upper.length === (u > 0xffff ? 2 : 1)) link(c, u);
    }
  }
  // Folded together without lower- or upper-casing to one another.
  link(0x390, 0x1fd3);
  link(0x3b0, 0x1fe3);
  link(0xfb05, 0xfb06);
  const groups = new Map<number, number[]>();
  for (const c of parent.keys()) {
    const root = find(c);
    const group = groups.get(root);
    if (group === undefined) groups.set(root, [c]); else group.push(c);
  }
  const out = new Map<number, number[]>();
  for (const group of groups.values()) for (const c of group) out.set(c, group);
  return out;
}

/**
 * The other characters `c` is equal to when case is ignored — its orbit under
 * Unicode simple case folding, as the crate's table has it.
 *
 * The candidates come from the runtime's case mappings; what makes two of
 * them equal is the runtime's own case-insensitive comparison, which is that
 * folding (ECMA-262 `Canonicalize` under `/iu`). It drops the one pair the
 * mappings invent: `ı` upper-cases to `I` and folds to itself.
 */
function orbit(c: number): number[] {
  if (c < 128) {
    const special = ASCII_ORBIT[c];
    if (special !== undefined) return special;
    if (c >= 65 && c <= 90) return [c + 32];
    if (c >= 97 && c <= 122) return [c - 32];
    return NO_ORBIT;
  }
  const known = ORBITS.get(c);
  if (known !== undefined) return known;
  const group = (ORBIT_CANDIDATES ?? (ORBIT_CANDIDATES = orbitCandidates())).get(c);
  let found = NO_ORBIT;
  if (group !== undefined) {
    const same = new RegExp(`^\\u{${c.toString(16)}}$`, 'iu');
    const others = group.filter(d => d !== c && same.test(String.fromCodePoint(d)));
    if (others.length > 0) found = others;
  }
  ORBITS.set(c, found);
  return found;
}

/** The members of `ranges` and every character equal to one of them when case is ignored. */
function foldRanges(ranges: number[]): number[] {
  const out = ranges.slice();
  const add = (c: number): void => {
    if (!containsRange(ranges, c)) return;
    for (const d of orbit(c)) out.push(d, d);
  };
  for (let c = 65; c <= 90; c++) { add(c); add(c + 32); }
  for (const c of (ORBIT_CANDIDATES ?? (ORBIT_CANDIDATES = orbitCandidates())).keys()) if (c >= 128) add(c);
  return normalizeRanges(out);
}

/** A set with every character equal to one of its members when case is ignored (Unicode simple folding). */
function foldSet(set: CharSet): CharSet {
  if (set.known !== null && (set.known.length === 0 || set.known[set.known.length - 1] < 128)) {
    // Only ASCII: the letters, and the two beyond ASCII that fold onto `k` and `s`.
    const out = set.known.slice();
    for (let c = 65; c <= 90; c++) {
      const upper = containsRange(set.known, c);
      const lower = containsRange(set.known, c + 32);
      if (!upper && !lower) continue;
      out.push(c, c, c + 32, c + 32);
      if (c === 0x4b) out.push(0x212a, 0x212a);
      if (c === 0x53) out.push(0x17f, 0x17f);
    }
    return setOfRanges(out);
  }
  const has: Pred = c => {
    if (set.has(c)) return true;
    const others = orbit(c);
    for (let i = 0; i < others.length; i++) if (set.has(others[i])) return true;
    return false;
  };
  return lazySet(has, () => foldRanges(set.ranges()), Math.max(set.ceiling, CEILING_UNKNOWN));
}

/** A set of ASCII characters with its letters in both cases: all `(?i-u)` folds. */
function foldAscii(set: CharSet): CharSet {
  const ranges = set.ranges();
  const out = ranges.slice();
  for (let c = 65; c <= 90; c++) {
    if (containsRange(ranges, c) || containsRange(ranges, c + 32)) out.push(c, c, c + 32, c + 32);
  }
  return setOfRanges(out);
}

// ─── Property names ───────────────────────────────────────

// The names `\p{…}` goes by, from the tables of regex-syntax 0.8 (Unicode
// 16): `alias=Canonical`, or a bare canonical name whose alias is that name
// in lower case without `_`.
const BOOLS = 'ahex=ASCII_Hex_Digit alpha=Alphabetic Alphabetic ASCII_Hex_Digit bidic=Bidi_Control Bidi_Control bidim=Bidi_Mirrored Bidi_Mirrored Cased Case_Ignorable Changes_When_Casefolded Changes_When_Casemapped Changes_When_Lowercased Changes_When_Titlecased Changes_When_Uppercased ci=Case_Ignorable cwcf=Changes_When_Casefolded cwcm=Changes_When_Casemapped cwl=Changes_When_Lowercased cwt=Changes_When_Titlecased cwu=Changes_When_Uppercased Dash Default_Ignorable_Code_Point dep=Deprecated Deprecated di=Default_Ignorable_Code_Point dia=Diacritic Diacritic ebase=Emoji_Modifier_Base ecomp=Emoji_Component emod=Emoji_Modifier Emoji Emoji_Component Emoji_Modifier Emoji_Modifier_Base Emoji_Presentation epres=Emoji_Presentation ext=Extender Extended_Pictographic Extender extpict=Extended_Pictographic Grapheme_Base Grapheme_Extend Grapheme_Link grbase=Grapheme_Base grext=Grapheme_Extend grlink=Grapheme_Link hex=Hex_Digit Hex_Digit Hyphen idc=ID_Continue ID_Compat_Math_Continue ID_Compat_Math_Start ID_Continue ideo=Ideographic Ideographic ids=ID_Start idsb=IDS_Binary_Operator IDS_Binary_Operator idst=IDS_Trinary_Operator ID_Start IDS_Trinary_Operator idsu=IDS_Unary_Operator IDS_Unary_Operator joinc=Join_Control Join_Control loe=Logical_Order_Exception Logical_Order_Exception lower=Lowercase Lowercase Math mcm=Modifier_Combining_Mark Modifier_Combining_Mark nchar=Noncharacter_Code_Point Noncharacter_Code_Point oalpha=Other_Alphabetic odi=Other_Default_Ignorable_Code_Point ogrext=Other_Grapheme_Extend oidc=Other_ID_Continue oids=Other_ID_Start olower=Other_Lowercase omath=Other_Math Other_Alphabetic Other_Default_Ignorable_Code_Point Other_Grapheme_Extend Other_ID_Continue Other_ID_Start Other_Lowercase Other_Math Other_Uppercase oupper=Other_Uppercase patsyn=Pattern_Syntax Pattern_Syntax Pattern_White_Space patws=Pattern_White_Space pcm=Prepended_Concatenation_Mark Prepended_Concatenation_Mark qmark=Quotation_Mark Quotation_Mark Radical Regional_Indicator ri=Regional_Indicator sd=Soft_Dotted Sentence_Terminal Soft_Dotted space=White_Space sterm=Sentence_Terminal term=Terminal_Punctuation Terminal_Punctuation uideo=Unified_Ideograph Unified_Ideograph upper=Uppercase Uppercase Variation_Selector vs=Variation_Selector White_Space wspace=White_Space xidc=XID_Continue XID_Continue xids=XID_Start XID_Start';
const GENCATS = 'c=Other Cased_Letter cc=Control cf=Format Close_Punctuation cn=Unassigned cntrl=Control co=Private_Use combiningmark=Mark Connector_Punctuation Control cs=Surrogate Currency_Symbol Dash_Punctuation Decimal_Number digit=Decimal_Number Enclosing_Mark Final_Punctuation Format Initial_Punctuation l=Letter lc=Cased_Letter Letter Letter_Number Line_Separator ll=Lowercase_Letter lm=Modifier_Letter lo=Other_Letter Lowercase_Letter lt=Titlecase_Letter lu=Uppercase_Letter m=Mark Mark Math_Symbol mc=Spacing_Mark me=Enclosing_Mark mn=Nonspacing_Mark Modifier_Letter Modifier_Symbol n=Number nd=Decimal_Number nl=Letter_Number no=Other_Number Nonspacing_Mark Number Open_Punctuation Other Other_Letter Other_Number Other_Punctuation Other_Symbol p=Punctuation Paragraph_Separator pc=Connector_Punctuation pd=Dash_Punctuation pe=Close_Punctuation pf=Final_Punctuation pi=Initial_Punctuation po=Other_Punctuation Private_Use ps=Open_Punctuation punct=Punctuation Punctuation s=Symbol sc=Currency_Symbol Separator sk=Modifier_Symbol sm=Math_Symbol so=Other_Symbol Space_Separator Spacing_Mark Surrogate Symbol Titlecase_Letter Unassigned Uppercase_Letter z=Separator zl=Line_Separator zp=Paragraph_Separator zs=Space_Separator';
// Long names only: a four-letter code (`Grek`) is asked of the runtime as it is.
const SCRIPTS = 'Adlam Ahom Anatolian_Hieroglyphs Arabic Armenian Avestan Balinese Bamum Bassa_Vah Batak Bengali Bhaiksuki Bopomofo Brahmi Braille Buginese Buhid Canadian_Aboriginal Carian Caucasian_Albanian Chakma Cham Cherokee Chorasmian Common Coptic Cuneiform Cypriot Cypro_Minoan Cyrillic Deseret Devanagari Dives_Akuru Dogra Duployan Egyptian_Hieroglyphs Elbasan Elymaic Ethiopic Garay Georgian Glagolitic Gothic Grantha Greek Gujarati Gunjala_Gondi Gurmukhi Gurung_Khema Han Hangul Hanifi_Rohingya Hanunoo Hatran Hebrew Hiragana Imperial_Aramaic Inherited Inscriptional_Pahlavi Inscriptional_Parthian Javanese Kaithi Kannada Katakana Katakana_Or_Hiragana Kawi Kayah_Li Kharoshthi Khitan_Small_Script Khmer Khojki Khudawadi Kirat_Rai Lao Latin Lepcha Limbu Linear_A Linear_B Lisu Lycian Lydian Mahajani Makasar Malayalam Mandaic Manichaean Marchen Masaram_Gondi Medefaidrin Meetei_Mayek Mende_Kikakui Meroitic_Cursive Meroitic_Hieroglyphs Miao Modi Mongolian Mro Multani Myanmar Nabataean Nag_Mundari Nandinagari Newa New_Tai_Lue Nko Nushu Nyiakeng_Puachue_Hmong Ogham Ol_Chiki Old_Hungarian Old_Italic Old_North_Arabian Old_Permic Old_Persian Old_Sogdian Old_South_Arabian Old_Turkic Old_Uyghur Ol_Onal Oriya Osage Osmanya Pahawh_Hmong Palmyrene Pau_Cin_Hau Phags_Pa Phoenician Psalter_Pahlavi Rejang Runic Samaritan Saurashtra Sharada Shavian Siddham SignWriting Sinhala Sogdian Sora_Sompeng Soyombo Sundanese Sunuwar Syloti_Nagri Syriac Tagalog Tagbanwa Tai_Le Tai_Tham Tai_Viet Takri Tamil Tangsa Tangut Telugu Thaana Thai Tibetan Tifinagh Tirhuta Todhri Toto Tulu_Tigalari Ugaritic Unknown Vai Vithkuqi Wancho Warang_Citi Yezidi Yi Zanabazar_Square';
/** The properties that take a value, by every spelling the crate knows. */
const VALUED: Record<string, string> = {
  gc: 'General_Category', generalcategory: 'General_Category', category: 'General_Category',
  sc: 'Script', script: 'Script',
  scx: 'Script_Extensions', scriptextensions: 'Script_Extensions',
  // Known to the crate, with no counterpart in a JavaScript runtime.
  age: '', gcb: '', graphemeclusterbreak: '', sb: '', sentencebreak: '', wb: '', wordbreak: '',
};

let NAMES: { bools: Map<string, string>; gencats: Map<string, string>; scripts: Map<string, string> } | null = null;

function names(): NonNullable<typeof NAMES> {
  return NAMES ?? (NAMES = { bools: unpack(BOOLS), gencats: unpack(GENCATS), scripts: unpack(SCRIPTS) });
}

function unpack(packed: string): Map<string, string> {
  const out = new Map<string, string>();
  for (const entry of packed.split(' ')) {
    const eq = entry.indexOf('=');
    if (eq === -1) out.set(normalizeName(entry), entry); else out.set(entry.substring(0, eq), entry.substring(eq + 1));
  }
  return out;
}

/**
 * A property name or value as it is looked up (UAX44-LM3, as the crate does
 * it): a leading `is` goes, then spaces, `_`, `-` and everything beyond
 * ASCII, and the rest is lower-cased.
 */
function normalizeName(name: string): string {
  let start = 0;
  let startsWithIs = false;
  if (name.length >= 2 && (name.charCodeAt(0) | 0x20) === 105 && (name.charCodeAt(1) | 0x20) === 115
      && name.charCodeAt(0) < 128 && name.charCodeAt(1) < 128) {
    startsWithIs = true;
    start = 2;
  }
  let out = '';
  for (let i = start; i < name.length; i++) {
    const b = name.charCodeAt(i);
    if (b === 32 || b === 95 || b === 45 || b > 0x7f) continue;
    out += b >= 65 && b <= 90 ? String.fromCharCode(b + 32) : name[i];
  }
  // `isc` is the abbreviation of ISO_Comment, not `is` + `c`.
  return startsWithIs && out === 'c' ? 'isc' : out;
}

const ANY_SET = setOfRanges([0, SURROGATE_LO - 1, SURROGATE_HI + 1, MAX_SCALAR]);
const ASCII_SET = setOfRanges([0, 127]);

function generalCategory(canonical: string): CharSet | null {
  switch (canonical) {
    case 'ASCII': return ASCII_SET;
    case 'Any': return ANY_SET;
    case 'Assigned': {
      const unassigned = runtimeClass('General_Category=Unassigned');
      return unassigned === null ? null : negateSet(unassigned);
    }
    case 'Decimal_Number': return DIGIT_SET;
    default: return runtimeClass(`General_Category=${canonical}`);
  }
}

function scriptName(normalized: string): string | undefined {
  const long = names().scripts.get(normalized);
  if (long !== undefined) return long;
  // A four-letter code, as ISO 15924 writes it.
  if (normalized.length === 4 && /^[a-z]{4}$/.test(normalized)) {
    const code = normalized[0].toUpperCase() + normalized.substring(1);
    if (runtimeClass(`Script=${code}`) !== null) return code;
  }
  return undefined;
}

/** `\p{name}` / `\pX` — a binary property, then a general category, then a script. */
function namedClass(name: string): CharSet | null {
  const norm = normalizeName(name);
  // `cf`, `sc` and `lc` are general categories here, though each is also the
  // abbreviation of a property.
  if (norm !== 'cf' && norm !== 'sc' && norm !== 'lc') {
    const bool = names().bools.get(norm);
    if (bool !== undefined) return bool === 'White_Space' ? SPACE_SET : runtimeClass(bool);
    // A property that takes a value is no class by its name alone.
    if (norm in VALUED) return null;
  }
  if (norm === 'any') return ANY_SET;
  if (norm === 'assigned') return generalCategory('Assigned');
  if (norm === 'ascii') return ASCII_SET;
  const gencat = names().gencats.get(norm);
  if (gencat !== undefined) return generalCategory(gencat);
  const script = scriptName(norm);
  return script === undefined ? null : runtimeClass(`Script=${script}`);
}

/** `\p{name=value}` and its `:` and `!=` forms. */
function valuedClass(name: string, value: string): CharSet | null {
  const property = VALUED[normalizeName(name)];
  if (property === undefined || property === '') return null;
  const norm = normalizeName(value);
  if (property === 'General_Category') {
    if (norm === 'any') return ANY_SET;
    if (norm === 'assigned') return generalCategory('Assigned');
    if (norm === 'ascii') return ASCII_SET;
    const gencat = names().gencats.get(norm);
    return gencat === undefined ? null : generalCategory(gencat);
  }
  const script = scriptName(norm);
  return script === undefined ? null : runtimeClass(`${property}=${script}`);
}

// ─── Reader ───────────────────────────────────────────────

const PERL_DIGIT = 0;
const PERL_SPACE = 1;
const PERL_WORD = 2;

/** The members of a POSIX class, as inclusive ranges; `undefined` for a name there is none of. */
function posixRanges(name: string): number[] | undefined {
  switch (name) {
    case 'alnum': return [48, 57, 65, 90, 97, 122];
    case 'alpha': return [65, 90, 97, 122];
    case 'ascii': return [0, 127];
    case 'blank': return [9, 9, 32, 32];
    case 'cntrl': return [0, 31, 127, 127];
    case 'digit': return [48, 57];
    case 'graph': return [33, 126];
    case 'lower': return [97, 122];
    case 'print': return [32, 126];
    case 'punct': return [33, 47, 58, 64, 91, 96, 123, 126];
    case 'space': return [9, 13, 32, 32];
    case 'upper': return [65, 90];
    case 'word': return [48, 57, 65, 90, 95, 95, 97, 122];
    case 'xdigit': return [48, 57, 65, 70, 97, 102];
    default: return undefined;
  }
}

/** What one primitive of the pattern is: a literal, a class, or an assertion. */
type Prim =
  // `byte`: written `\xNN`, which under `(?-u)` is that byte and not a character.
  | { k: 'lit'; c: number; byte: boolean }
  | { k: 'perl'; which: number; negated: boolean }
  | { k: 'uni'; set: CharSet; negated: boolean }
  | { k: 'assert'; kind: number; word: boolean };

interface Flags { i: boolean; m: boolean; s: boolean; R: boolean; u: boolean; x: boolean }

/** Deepest nesting the crate reads (`nest_limit`). */
export const NEST_LIMIT = 250;
/** A repetition count is a `u32`. */
const MAX_COUNT = 4294967295;

/** In a list of what a concatenation holds: flags on their own, which no quantifier can repeat. */
const FLAGS: Node = { t: 'empty', h: 0 };

function isHexDigit(c: number): boolean {
  return (c >= 48 && c <= 57) || (c >= 65 && c <= 70) || (c >= 97 && c <= 102);
}

/** `char::is_whitespace`: Unicode White_Space. */
function isPatternSpace(c: number): boolean {
  return c !== -1 && isSpace(c);
}

function deep(h: number): number {
  if (h > NEST_LIMIT) throw new Unsupported();
  return h;
}

/** A class while it is read: the set, and how deep its syntax is. */
interface ClassPart { set: CharSet; h: number }

class Parser {
  private pos = 0;
  private flags: Flags = { i: false, m: false, s: false, R: false, u: true, x: false };
  private readonly names = new Set<string>();

  constructor(private readonly cps: number[]) {}

  parse(): Node {
    const node = this.alternation();
    if (!this.eof()) throw new Unsupported(); // a `)` nothing opened
    return node;
  }

  // ── The cursor, as regex-syntax moves it ──

  private eof(): boolean { return this.pos >= this.cps.length; }
  private char(): number { return this.pos < this.cps.length ? this.cps[this.pos] : -1; }
  private peek(): number { return this.pos + 1 < this.cps.length ? this.cps[this.pos + 1] : -1; }

  /** Step over the current character; `false` when nothing follows. */
  private bump(): boolean {
    if (this.eof()) return false;
    this.pos++;
    return !this.eof();
  }

  /** Under `(?x)`: step over whitespace and `#` comments. */
  private bumpSpace(): void {
    if (!this.flags.x) return;
    while (!this.eof()) {
      const c = this.char();
      if (isPatternSpace(c)) {
        this.pos++;
      } else if (c === 0x23) {
        this.pos++;
        while (!this.eof()) {
          const d = this.char();
          this.pos++;
          if (d === 10) break;
        }
      } else {
        break;
      }
    }
  }

  private bumpAndBumpSpace(): boolean {
    if (!this.bump()) return false;
    this.bumpSpace();
    return !this.eof();
  }

  private bumpIf(prefix: string): boolean {
    for (let i = 0; i < prefix.length; i++) {
      if (this.cps[this.pos + i] !== prefix.charCodeAt(i)) return false;
    }
    this.pos += prefix.length;
    return true;
  }

  /**
   * The character after the current one, past whitespace under `(?x)`. As in
   * the crate, a `#` on the way is stepped over and what follows it is the
   * answer, comment or not; and when nothing but whitespace follows, the
   * answer is the character right after the current one.
   */
  private peekSpace(): number {
    if (!this.flags.x) return this.peek();
    if (this.eof()) return -1;
    let inComment = false;
    for (let i = this.pos + 1; i < this.cps.length; i++) {
      const c = this.cps[i];
      if (isPatternSpace(c)) continue;
      if (!inComment && c === 0x23) { inComment = true; continue; }
      return c;
    }
    return this.peek();
  }

  // ── Alternation and concatenation ──

  /** Everything up to the end of the pattern or the `)` that closes the group. */
  private alternation(): Node {
    const branches: Node[] = [];
    let items: Node[] = [];
    for (;;) {
      this.bumpSpace();
      if (this.eof()) break;
      const c = this.char();
      if (c === 0x29) break; // )
      if (c === 0x7c) { // |
        branches.push(concat(items));
        items = [];
        this.bump();
      } else if (c === 0x28) { // (
        items.push(this.group());
      } else if (c === 0x5b) { // [
        const part = this.bracketed();
        items.push({ t: 'set', set: part.set, h: part.h });
      } else if (c === 0x3f || c === 0x2a || c === 0x2b) { // ? * +
        this.uncounted(items, c);
      } else if (c === 0x7b) { // {
        this.counted(items);
      } else {
        items.push(this.primitiveNode());
      }
    }
    if (branches.length === 0) return concat(items);
    branches.push(concat(items));
    let h = 0;
    for (const b of branches) if (b.h > h) h = b.h;
    return { t: 'alt', items: branches, h: deep(h + 1) };
  }

  /** The expression a quantifier repeats: the last one, and not a bare `(?i)`. */
  private repeated(items: Node[]): Node {
    const last = items.pop();
    if (last === undefined || last === FLAGS) throw new Unsupported();
    return last;
  }

  private uncounted(items: Node[], op: number): void {
    const node = this.repeated(items);
    // A `?` after the operator makes it lazy, which a yes/no match does not see.
    if (this.bump() && this.char() === 0x3f) this.bump();
    items.push({ t: 'rep', node, min: op === 0x2b ? 1 : 0, max: op === 0x3f ? 1 : -1, h: deep(node.h + 1) });
  }

  /** `{n}`, `{n,}`, `{n,m}`, with whitespace around the numbers. */
  private counted(items: Node[]): void {
    const node = this.repeated(items);
    if (!this.bumpAndBumpSpace()) throw new Unsupported();
    const min = this.decimal();
    let max = min;
    if (this.eof()) throw new Unsupported();
    if (this.char() === 0x2c) { // ,
      if (!this.bumpAndBumpSpace()) throw new Unsupported();
      if (this.char() !== 0x7d) max = this.decimal(); else max = -1;
    }
    if (this.eof() || this.char() !== 0x7d) throw new Unsupported();
    if (this.bumpAndBumpSpace() && this.char() === 0x3f) this.bump();
    if (max !== -1 && min > max) throw new Unsupported();
    items.push({ t: 'rep', node, min, max, h: deep(node.h + 1) });
  }

  private decimal(): number {
    while (!this.eof() && isPatternSpace(this.char())) this.pos++;
    let digits = 0;
    let value = 0;
    while (!this.eof() && isAsciiDigit(this.char())) {
      // Past a `u32` the count is refused; the digits are read to the end all the same.
      if (value <= MAX_COUNT) value = value * 10 + (this.char() - 48);
      digits++;
      this.bumpAndBumpSpace();
    }
    while (!this.eof() && isPatternSpace(this.char())) this.bumpAndBumpSpace();
    if (digits === 0 || value > MAX_COUNT) throw new Unsupported();
    return value;
  }

  // ── Groups and flags ──

  private group(): Node {
    this.bump();
    this.bumpSpace();
    // Look-around is not part of the dialect.
    if (this.bumpIf('?=') || this.bumpIf('?!') || this.bumpIf('?<=') || this.bumpIf('?<!')) throw new Unsupported();
    const saved = { ...this.flags };
    let capture = true;
    if (this.bumpIf('?P<') || this.bumpIf('?<')) {
      this.captureName();
    } else if (this.bumpIf('?')) {
      if (this.eof()) throw new Unsupported();
      const set = this.flagItems();
      const end = this.char();
      this.bump();
      if (end === 0x29) { // (?flags)
        // `(?)` sets nothing: the crate reads it as a quantifier with nothing to repeat.
        if (set.count === 0) throw new Unsupported();
        this.applyFlags(set.on, set.off);
        return FLAGS;
      }
      this.applyFlags(set.on, set.off); // (?flags:…)
      capture = false;
    }
    const inner = this.alternation();
    if (this.char() !== 0x29) throw new Unsupported(); // unclosed
    this.bump();
    this.flags = saved;
    const h = deep(inner.h + 1);
    if (capture) return { t: 'cap', node: inner, h };
    // A group is one expression, whatever is in it: a quantifier after it repeats the group.
    return inner.t === 'empty' ? { t: 'empty', h } : { ...inner, h };
  }

  /** A group name: a letter or `_`, then letters, digits, `_`, `.`, `[`, `]`; no name twice. */
  private captureName(): void {
    if (this.eof()) throw new Unsupported();
    const start = this.pos;
    for (;;) {
      const c = this.char();
      if (c === 0x3e) break; // >
      const first = this.pos === start;
      const ok = c === 0x5f || (first ? isAlphabetic(c) : c === 0x2e || c === 0x5b || c === 0x5d || isAlphanumeric(c));
      if (!ok) throw new Unsupported();
      if (!this.bump()) throw new Unsupported();
    }
    const name = String.fromCodePoint(...this.cps.slice(start, this.pos));
    this.bump();
    if (name === '' || this.names.has(name)) throw new Unsupported();
    this.names.add(name);
  }

  /** The flags between `(?` and `:` or `)`: which are switched on, which off. */
  private flagItems(): { on: string; off: string; count: number } {
    let on = '';
    let off = '';
    let negated = false;
    let danglingNegation = false;
    let count = 0;
    while (this.char() !== 0x3a && this.char() !== 0x29) {
      const c = this.char();
      if (c === 0x2d) { // -
        if (negated) throw new Unsupported(); // a second `-`
        negated = true;
        danglingNegation = true;
      } else {
        const flag = c === -1 ? '' : String.fromCodePoint(c);
        if (flag === '' || 'imsUuRx'.indexOf(flag) === -1) throw new Unsupported();
        if (on.indexOf(flag) !== -1 || off.indexOf(flag) !== -1) throw new Unsupported(); // twice
        if (negated) off += flag; else on += flag;
        danglingNegation = false;
      }
      count++;
      if (!this.bump()) throw new Unsupported();
    }
    if (danglingNegation) throw new Unsupported();
    return { on, off, count };
  }

  private applyFlags(on: string, off: string): void {
    for (let pass = 0; pass < 2; pass++) {
      const letters = pass === 0 ? on : off;
      const value = pass === 0;
      for (let i = 0; i < letters.length; i++) {
        switch (letters[i]) {
          case 'i': this.flags.i = value; break;
          case 'm': this.flags.m = value; break;
          case 's': this.flags.s = value; break;
          case 'R': this.flags.R = value; break;
          case 'u': this.flags.u = value; break;
          case 'x': this.flags.x = value; break;
          default: break; // `U` swaps greedy and lazy: no difference to a yes/no match
        }
      }
    }
  }

  // ── Primitives ──

  private primitiveNode(): Node {
    const c = this.char();
    if (c === 0x5c) return this.primNode(this.escape()); // \
    this.bump();
    if (c === 0x2e) { // .
      // Without Unicode a dot is any byte, and a byte need not be a character: refused.
      if (!this.flags.u) throw new Unsupported();
      if (this.flags.s) return { t: 'set', set: ANY_SET, h: 0 };
      return { t: 'set', set: this.flags.R ? DOT_CRLF_SET : DOT_SET, h: 0 };
    }
    if (c === 0x5e) { // ^
      return { t: 'assert', kind: this.flags.m ? (this.flags.R ? A_LINE_START_CRLF : A_LINE_START) : A_TEXT_START, h: 0 };
    }
    if (c === 0x24) { // $
      return { t: 'assert', kind: this.flags.m ? (this.flags.R ? A_LINE_END_CRLF : A_LINE_END) : A_TEXT_END, h: 0 };
    }
    return this.literalNode(c, false);
  }

  private primNode(p: Prim): Node {
    switch (p.k) {
      case 'lit': return this.literalNode(p.c, p.byte);
      case 'assert': return { t: 'assert', kind: p.word && !this.flags.u ? p.kind | A_ASCII : p.kind, h: 0 };
      case 'perl': return { t: 'set', set: this.perlSet(p.which, p.negated), h: 0 };
      case 'uni': return { t: 'set', set: this.unicodeSet(p.set, p.negated), h: 0 };
    }
  }

  private literalNode(c: number, byte: boolean): Node {
    if (this.flags.u) {
      if (this.flags.i) {
        const others = orbit(c);
        if (others.length > 0) {
          const pairs = [c, c];
          for (const d of others) pairs.push(d, d);
          return { t: 'set', set: setOfRanges(pairs), h: 0 };
        }
      }
      return { t: 'lit', c, h: 0 };
    }
    // `(?-u)`: `\xNN` past 7F is a lone byte, which is not text.
    if (byte && c > 0x7f) throw new Unsupported();
    // Only ASCII letters fold without Unicode; any other character is itself.
    if (this.flags.i && ((c >= 65 && c <= 90) || (c >= 97 && c <= 122))) {
      const other = c ^ 0x20;
      return { t: 'set', set: setOfRanges([c, c, other, other]), h: 0 };
    }
    return { t: 'lit', c, h: 0 };
  }

  /** The escape at the cursor (on its `\`), consumed. */
  private escape(): Prim {
    if (!this.bump()) throw new Unsupported();
    const c = this.char();
    // Back-references and octal escapes are not part of the dialect.
    if (c >= 48 && c <= 57) throw new Unsupported();
    switch (c) {
      case 0x78: case 0x75: case 0x55: return this.hex(c); // x u U
      case 0x70: case 0x50: return this.unicodeClass(c === 0x50); // p P
      case 0x64: this.bump(); return { k: 'perl', which: PERL_DIGIT, negated: false };
      case 0x44: this.bump(); return { k: 'perl', which: PERL_DIGIT, negated: true };
      case 0x73: this.bump(); return { k: 'perl', which: PERL_SPACE, negated: false };
      case 0x53: this.bump(); return { k: 'perl', which: PERL_SPACE, negated: true };
      case 0x77: this.bump(); return { k: 'perl', which: PERL_WORD, negated: false };
      case 0x57: this.bump(); return { k: 'perl', which: PERL_WORD, negated: true };
      default: break;
    }
    this.bump();
    // Any ASCII character that is not a letter, a digit, `<` or `>` stands for itself.
    if (c < 128 && !(c >= 48 && c <= 57) && !(c >= 65 && c <= 90) && !(c >= 97 && c <= 122) && c !== 0x3c && c !== 0x3e) {
      return { k: 'lit', c, byte: false };
    }
    switch (c) {
      case 0x61: return { k: 'lit', c: 7, byte: false };  // \a
      case 0x66: return { k: 'lit', c: 12, byte: false }; // \f
      case 0x74: return { k: 'lit', c: 9, byte: false };  // \t
      case 0x6e: return { k: 'lit', c: 10, byte: false }; // \n
      case 0x72: return { k: 'lit', c: 13, byte: false }; // \r
      case 0x76: return { k: 'lit', c: 11, byte: false }; // \v
      case 0x41: return { k: 'assert', kind: A_TEXT_START, word: false }; // \A
      case 0x7a: return { k: 'assert', kind: A_TEXT_END, word: false };   // \z
      case 0x62: { // \b, \b{start}, \b{end}, \b{start-half}, \b{end-half}
        if (!this.eof() && this.char() === 0x7b) {
          const special = this.specialWordBoundary();
          if (special !== -1) return { k: 'assert', kind: special, word: true };
        }
        return { k: 'assert', kind: A_WORD, word: true };
      }
      case 0x42: return { k: 'assert', kind: A_NOT_WORD, word: true };   // \B
      case 0x3c: return { k: 'assert', kind: A_WORD_START, word: true }; // \<
      case 0x3e: return { k: 'assert', kind: A_WORD_END, word: true };   // \>
      default: throw new Unsupported();
    }
  }

  /**
   * `{start}` and its kin after `\b`, or -1 with the cursor back on `{` when
   * what follows is a repetition count for the `\b`.
   */
  private specialWordBoundary(): number {
    const start = this.pos;
    const isNameChar = (c: number): boolean => (c >= 65 && c <= 90) || (c >= 97 && c <= 122) || c === 0x2d;
    if (!this.bumpAndBumpSpace()) throw new Unsupported();
    if (!isNameChar(this.char())) {
      this.pos = start;
      return -1;
    }
    let name = '';
    while (!this.eof() && isNameChar(this.char())) {
      name += String.fromCharCode(this.char());
      this.bumpAndBumpSpace();
    }
    if (this.eof() || this.char() !== 0x7d) throw new Unsupported();
    this.bump();
    switch (name) {
      case 'start': return A_WORD_START;
      case 'end': return A_WORD_END;
      case 'start-half': return A_WORD_START_HALF;
      case 'end-half': return A_WORD_END_HALF;
      default: throw new Unsupported();
    }
  }

  /** `\xNN`, `\uNNNN`, `\UNNNNNNNN` and their `{…}` forms. */
  private hex(kind: number): Prim {
    if (!this.bumpAndBumpSpace()) throw new Unsupported();
    let value = 0;
    let digits = 0;
    let fixed = false;
    const digit = (): void => {
      const d = this.char();
      if (!isHexDigit(d)) throw new Unsupported();
      // Past the last code point the number is refused; its digits are read to the end all the same.
      if (value <= MAX_SCALAR) value = value * 16 + (d <= 57 ? d - 48 : (d | 0x20) - 87);
      digits++;
    };
    if (this.char() === 0x7b) {
      while (this.bumpAndBumpSpace() && this.char() !== 0x7d) digit();
      if (this.eof()) throw new Unsupported();
      this.bumpAndBumpSpace();
      if (digits === 0) throw new Unsupported();
    } else {
      fixed = true;
      const count = kind === 0x78 ? 2 : kind === 0x75 ? 4 : 8;
      for (let i = 0; i < count; i++) {
        if (i > 0 && !this.bumpAndBumpSpace()) throw new Unsupported();
        digit();
      }
      this.bumpAndBumpSpace();
    }
    if (value > MAX_SCALAR || (value >= SURROGATE_LO && value <= SURROGATE_HI)) throw new Unsupported();
    return { k: 'lit', c: value, byte: fixed && kind === 0x78 };
  }

  /** `\p{…}`, `\pX` and their negations. */
  private unicodeClass(upper: boolean): Prim {
    if (!this.bumpAndBumpSpace()) throw new Unsupported();
    let set: CharSet | null;
    let negated = upper;
    if (this.char() === 0x7b) {
      const chars: number[] = [];
      while (this.bumpAndBumpSpace() && this.char() !== 0x7d) chars.push(this.char());
      if (this.eof()) throw new Unsupported();
      this.bump();
      const body = String.fromCodePoint(...chars);
      let at = body.indexOf('!=');
      if (at !== -1) {
        set = valuedClass(body.substring(0, at), body.substring(at + 2));
        negated = !upper;
      } else if ((at = body.indexOf(':')) !== -1 || (at = body.indexOf('=')) !== -1) {
        set = valuedClass(body.substring(0, at), body.substring(at + 1));
      } else {
        set = namedClass(body);
      }
    } else {
      const c = this.char();
      if (c === 0x5c) throw new Unsupported();
      this.bumpAndBumpSpace();
      set = namedClass(String.fromCodePoint(c));
    }
    if (set === null) throw new Unsupported();
    return { k: 'uni', set, negated };
  }

  // ── Sets, in the Unicode world and in the world of bytes ──

  /**
   * Under `(?-u)` a set is a set of bytes, and one that could match a byte
   * past 7F could match half a character: the crate refuses it.
   */
  private asciiOnly(set: CharSet): CharSet {
    const ranges = set.ranges();
    if (ranges.length > 0 && ranges[ranges.length - 1] > 0x7f) throw new Unsupported();
    return set;
  }

  private perlSet(which: number, negated: boolean): CharSet {
    if (this.flags.u) {
      if (which === PERL_DIGIT) return negated ? NOT_DIGIT_SET : DIGIT_SET;
      if (which === PERL_SPACE) return negated ? NOT_SPACE_SET : SPACE_SET;
      return negated ? NOT_WORD_SET : WORD_SET;
    }
    // Negated, any of the three takes in every byte past 7F.
    if (negated) throw new Unsupported();
    return setOfRanges(which === PERL_DIGIT ? [48, 57] : which === PERL_SPACE ? [9, 13, 32, 32] : [48, 57, 65, 90, 95, 95, 97, 122]);
  }

  private unicodeSet(set: CharSet, negated: boolean): CharSet {
    // No Unicode classes without Unicode.
    if (!this.flags.u) throw new Unsupported();
    const folded = this.flags.i ? foldSet(set) : set;
    return negated ? negateSet(folded) : folded;
  }

  private posixSet(ranges: number[], negated: boolean): CharSet {
    const base = setOfRanges(ranges);
    if (this.flags.u) {
      const folded = this.flags.i ? foldSet(base) : base;
      return negated ? negateSet(folded) : folded;
    }
    // The complement of a set of bytes always has a byte past 7F.
    if (negated) throw new Unsupported();
    return this.flags.i ? foldAscii(base) : base;
  }

  /** A class member under `(?-u)`: an ASCII character, however it is written. */
  private classByte(p: Prim & { k: 'lit' }): number {
    if (p.c > 0x7f) throw new Unsupported();
    return p.c;
  }

  private literalSet(p: Prim & { k: 'lit' }): CharSet {
    const c = this.flags.u ? p.c : this.classByte(p);
    return setOfRanges([c, c]);
  }

  private fold(set: CharSet): CharSet {
    if (!this.flags.i) return set;
    return this.flags.u ? foldSet(set) : foldAscii(set);
  }

  /** `[…]` from its `[` through its `]`: folded when case is ignored, then negated. */
  private bracketed(): ClassPart {
    if (!this.bumpAndBumpSpace()) throw new Unsupported();
    let negated = false;
    if (this.char() === 0x5e) { // ^
      negated = true;
      if (!this.bumpAndBumpSpace()) throw new Unsupported();
    }
    let items: CharSet[] = [];
    // The deepest of the items read since the last operator.
    let itemsH = 0;
    // Any number of leading `-`, then a leading `]`, are themselves.
    while (this.char() === 0x2d) {
      items.push(this.literalSet({ k: 'lit', c: 0x2d, byte: false }));
      if (!this.bumpAndBumpSpace()) throw new Unsupported();
    }
    if (items.length === 0 && this.char() === 0x5d) {
      items.push(this.literalSet({ k: 'lit', c: 0x5d, byte: false }));
      if (!this.bumpAndBumpSpace()) throw new Unsupported();
    }
    // `a && b -- c` is `(a && b) -- c`: one precedence, left to right.
    let lhs: ClassPart | null = null;
    let op = 0;
    const operand = (): ClassPart => {
      const rhs: ClassPart = {
        set: unionSets(items),
        // Several items make a union of their own; one item is just itself.
        h: items.length >= 2 ? itemsH + 1 : itemsH,
      };
      items = [];
      itemsH = 0;
      if (lhs === null) return rhs;
      // With case ignored each side is folded before the two are combined.
      return { set: combineSets(op, this.fold(lhs.set), this.fold(rhs.set)), h: deep(Math.max(lhs.h, rhs.h) + 1) };
    };
    for (;;) {
      this.bumpSpace();
      if (this.eof()) throw new Unsupported();
      const c = this.char();
      if (c === 0x5b) { // [ — a POSIX class, else a class inside this one
        const posix = this.posixClass();
        if (posix !== null) {
          items.push(posix);
        } else {
          const nested = this.bracketed();
          items.push(nested.set);
          if (nested.h > itemsH) itemsH = nested.h;
        }
      } else if (c === 0x5d) { // ]
        this.bump();
        const whole = operand();
        let set = this.fold(whole.set);
        if (negated) {
          // The complement of a set of bytes always has a byte past 7F.
          if (!this.flags.u) throw new Unsupported();
          set = negateSet(set);
        }
        if (!this.flags.u) this.asciiOnly(set);
        return { set, h: deep(whole.h + 1) };
      } else if (c === 0x26 && this.peek() === 0x26) { // &&
        this.pos += 2;
        lhs = operand();
        op = SET_AND;
      } else if (c === 0x2d && this.peek() === 0x2d) { // --
        this.pos += 2;
        lhs = operand();
        op = SET_MINUS;
      } else if (c === 0x7e && this.peek() === 0x7e) { // ~~
        this.pos += 2;
        lhs = operand();
        op = SET_XOR;
      } else {
        items.push(this.classRange());
      }
    }
  }

  /** `[:alpha:]` at the cursor, or `null` with the cursor where it was. */
  private posixClass(): CharSet | null {
    const start = this.pos;
    const fail = (): null => { this.pos = start; return null; };
    if (!this.bump() || this.char() !== 0x3a) return fail(); // :
    if (!this.bump()) return fail();
    let negated = false;
    if (this.char() === 0x5e) {
      negated = true;
      if (!this.bump()) return fail();
    }
    const nameStart = this.pos;
    while (this.char() !== 0x3a && this.bump()) { /* to the closing colon */ }
    if (this.eof()) return fail();
    const name = String.fromCodePoint(...this.cps.slice(nameStart, this.pos));
    if (!this.bumpIf(':]')) return fail();
    const ranges = posixRanges(name);
    if (ranges === undefined) return fail();
    return this.posixSet(ranges, negated);
  }

  /** One member of a class: a character, a range, or an escape that is a class. */
  private classRange(): CharSet {
    const first = this.classItem();
    this.bumpSpace();
    if (this.eof()) throw new Unsupported();
    // A `-` before `]` is itself; before another `-` it begins the `--` operator.
    if (this.char() !== 0x2d || this.peekSpace() === 0x5d || this.peekSpace() === 0x2d) {
      switch (first.k) {
        case 'lit': return this.literalSet(first);
        case 'perl': return this.perlSet(first.which, first.negated);
        case 'uni': return this.unicodeSet(first.set, first.negated);
        default: throw new Unsupported(); // an assertion in a class
      }
    }
    if (!this.bumpAndBumpSpace()) throw new Unsupported();
    const last = this.classItem();
    // Both ends of a range are single characters.
    if (first.k !== 'lit' || last.k !== 'lit') throw new Unsupported();
    const lo = this.flags.u ? first.c : this.classByte(first);
    const hi = this.flags.u ? last.c : this.classByte(last);
    if (lo > hi) throw new Unsupported();
    return setOfRanges([lo, hi]);
  }

  private classItem(): Prim {
    if (this.char() === 0x5c) return this.escape();
    const c = this.char();
    this.bump();
    return { k: 'lit', c, byte: false };
  }
}

/** `.` without `(?s)`: any character but a line feed; with `(?R)`, nor a carriage return. */
const DOT_SET = setOfRanges([0, 9, 11, SURROGATE_LO - 1, SURROGATE_HI + 1, MAX_SCALAR]);
const DOT_CRLF_SET = setOfRanges([0, 9, 11, 12, 14, SURROGATE_LO - 1, SURROGATE_HI + 1, MAX_SCALAR]);

/** What a concatenation of `items` is: nothing, the one item, or all of them in a row. */
function concat(items: Node[]): Node {
  if (items.length === 0) return { t: 'empty', h: 0 };
  if (items.length === 1) return items[0] === FLAGS ? { t: 'empty', h: 0 } : items[0];
  let h = 0;
  for (const item of items) if (item.h > h) h = item.h;
  return { t: 'cat', items: items.filter(n => n !== FLAGS), h: deep(h + 1) };
}

let ALPHABETIC: RegExp | undefined;
let ALPHANUMERIC: RegExp | undefined;

/** `char::is_alphabetic`. */
function isAlphabetic(c: number): boolean {
  if (c < 128) return (c >= 65 && c <= 90) || (c >= 97 && c <= 122);
  return c !== -1 && (ALPHABETIC ??= /^\p{Alphabetic}$/u).test(String.fromCodePoint(c));
}

/** `char::is_alphanumeric`. */
function isAlphanumeric(c: number): boolean {
  if (c < 128) return isAlphabetic(c) || isAsciiDigit(c);
  return c !== -1 && (ALPHANUMERIC ??= /^[\p{Alphabetic}\p{N}]$/u).test(String.fromCodePoint(c));
}

/** The pattern as a tree, or `null` when the crate would not compile it or this reader cannot read it. */
export function parsePattern(pattern: string): Node | null {
  const cps: number[] = [];
  for (const ch of pattern) cps.push(ch.codePointAt(0)!);
  try {
    return new Parser(cps).parse();
  } catch (e) {
    if (e instanceof Unsupported) return null;
    throw e;
  }
}
