#!/usr/bin/env node
/**
 * `[pattern:…]` in TypeScript (packages/synx-js/src/pattern.ts, a Pike VM for
 * the dialect of the `regex` crate) against the crate itself. Random patterns
 * from the dialect's grammar, random values over a tricky alphabet (case
 * folding, scripts, digits and spaces outside ASCII, astral characters). A
 * pattern one side compiles and the other does not, or a value the two match
 * differently, is reduced and grouped.
 *
 *   node tests/adversarial/pattern-diff.mjs [--seconds 60] [--seed N] [--max 25] [--all]
 *
 * Only patterns a document can carry are generated: no `,` (the constraint
 * list is split on it) and `[`/`]` balanced (the key scanner looks for the
 * closing bracket). `--all` drops that filter.
 *
 * Exit code 1 when a difference is found.
 */
import fs from 'node:fs';
import { worker } from './lib/impls.mjs';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(name); return i === -1 ? fallback : argv[i + 1]; };
if (argv.includes('--help')) {
  process.stdout.write(fs.readFileSync(new URL(import.meta.url), 'utf-8').split('*/')[0].replace(/^#!.*\n\/\*\*\n?/, '').replace(/^ \* ?/gm, ''));
  process.exit(0);
}
const seconds = Number(opt('--seconds', 60));
const maxShown = Number(opt('--max', 25));
const reachableOnly = !argv.includes('--all');
let seed = Number(opt('--seed', Date.now() % 1e9));
const firstSeed = seed;
function rnd() {
  // mulberry32
  seed = (seed + 0x6d2b79f5) | 0;
  let t = seed;
  t = Math.imul(t ^ (t >>> 15), t | 1);
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}
const pick = (a) => a[Math.floor(rnd() * a.length)];
const chance = (p) => rnd() < p;

// Letters with case foldings beyond toLowerCase/toUpperCase, scripts that
// share characters, digits and spaces outside ASCII, marks, joiners, astral.
const TRICKY = [
  'a', 'b', 'k', 'K', 's', 'S', 'z', '0', '7', '_', '-', ' ', '.',
  'ſ', 'K', 'ß', 'ẞ', 'σ', 'ς', 'Σ', 'µ', 'μ', 'Μ',
  'İ', 'ı', 'i', 'I', 'ǅ', 'Ǆ', 'ǆ', 'Ω', 'Ω', 'ω', 'Å', 'Å', 'å',
  'ϑ', 'θ', 'Θ', 'ϐ', 'β', 'ẛ', 'ﬀ', 'Ꭰ', 'ꭰ', 'ᾈ', 'ᾀ', 'ͅ', 'ι',
  '٣', '①', '²', ' ', ' ', '\u0085', '　', '‍', '‌', '́', '͂', '、', 'ー',
  'é', 'É', 'Ж', 'ж', '中', 'अ', '॑', '😀', '𐐀', '𐐨', '\n', '\r', '\t',
];
const META = new Set('\\.+*?()|[]{}^$#&-~'.split(''));
const lit = (c) => (META.has(c) ? '\\' + c : c === '\n' ? '\\n' : c === '\r' ? '\\r' : c === '\t' ? '\\t' : c);

const CLASS_ESC = ['\\d', '\\D', '\\w', '\\W', '\\s', '\\S', '\\pL', '\\PL', '\\pN', '\\p{Lu}', '\\p{Ll}', '\\p{Greek}', '\\p{Han}',
  '\\p{Latin}', '\\p{Cyrillic}', '\\p{Script=Greek}', '\\p{sc=Grek}', '\\p{scx=Grek}', '\\p{gc=Lu}', '\\p{Letter}', '\\p{Uppercase}',
  '\\p{Lowercase}', '\\p{Alphabetic}', '\\p{White_Space}', '\\p{Any}', '\\p{ASCII}', '\\p{Nd}', '\\p{greek}', '\\p{lu}',
  '\\p{Uppercase_Letter}', '\\p{Decimal_Number}', '\\p{Emoji}', '\\p{Inherited}', '\\p{Common}', '\\p{Zs}', '\\p{Cf}', '\\p{M}', '\\P{Greek}'];
const ASSERT = ['^', '$', '\\b', '\\B', '\\A', '\\z', '\\<', '\\>', '\\b{start}', '\\b{end}', '\\b{start-half}', '\\b{end-half}'];
const ESC_LIT = ['\\x41', '\\x{3C3}', '\\x{1F600}', '\\u0041', '\\u{17F}', '\\U0001F600', '\\n', '\\t', '\\.', '\\-', '\\ ', '\\#',
  '\\&', '\\~', '\\%', '\\e', '\\0', '\\@', '\\"', '\\\'', '\\/', '\\=', '\\!', '\\:', '\\_'];
const FLAGS = ['(?i)', '(?-i)', '(?m)', '(?s)', '(?U)', '(?u)', '(?-u)', '(?x)', '(?is)', '(?i-u)', '(?R)'];
const GROUP_OPEN = ['(', '(?:', '(?i:', '(?-i:', '(?s:', '(?m:', '(?-u:', '(?P<n>', '(?<n>', '(?x:', '(?U:'];
const QUANT = ['*', '+', '?', '*?', '+?', '??', '{2}', '{0}', '{1}', '{3,}', '{1,2}', '{,2}', '{ 2 }', '{2}?'];

function klass(depth) {
  let s = '[';
  if (chance(0.3)) s += '^';
  if (chance(0.1)) s += ']';
  const n = 1 + Math.floor(rnd() * 4);
  for (let i = 0; i < n; i++) {
    const r = rnd();
    if (r < 0.35) s += lit(pick(TRICKY));
    else if (r < 0.55) s += pick(['a-z', 'A-Z', '0-9', 'а-я', 'α-ω', 'Α-Ω', 'k-s', 'J-L', 'Ā-ſ', 'ß-ẞ']);
    else if (r < 0.75) s += pick(CLASS_ESC);
    else if (r < 0.82) s += pick(['[:alpha:]', '[:digit:]', '[:^space:]', '[:word:]']);
    else if (r < 0.88 && depth < 2) s += klass(depth + 1);
    else if (r < 0.93) s += pick(['&&a-c', '--b', '~~a']);
    else s += pick(['-', '\\]', '\\[', '\\^', '\\\\']);
  }
  if (chance(0.1)) s += '-';
  return s + ']';
}

function atom(depth) {
  const r = rnd();
  if (r < 0.32) return lit(pick(TRICKY));
  if (r < 0.42) return '.';
  if (r < 0.55) return pick(CLASS_ESC);
  if (r < 0.65) return klass(0);
  if (r < 0.72) return pick(ASSERT);
  if (r < 0.77) return pick(ESC_LIT);
  if (r < 0.82) return pick(FLAGS);
  if (depth < 3) return pick(GROUP_OPEN) + alt(depth + 1) + ')';
  return lit(pick(TRICKY));
}

function seq(depth) {
  let s = '';
  const n = 1 + Math.floor(rnd() * (depth === 0 ? 5 : 3));
  for (let i = 0; i < n; i++) {
    let a = atom(depth);
    if (chance(0.25)) a += pick(QUANT);
    s += a;
  }
  return s;
}

function alt(depth) {
  let s = seq(depth);
  while (chance(depth === 0 ? 0.25 : 0.2)) s += '|' + (chance(0.1) ? '' : seq(depth));
  return s;
}

/** Whether a document can carry this pattern: no comma, brackets balanced, nothing to trim. */
function reachable(p) {
  if (p.includes(',') || p !== p.trim() || p.length === 0) return false;
  let depth = 1;
  for (const ch of p) {
    if (ch === '[') depth++;
    else if (ch === ']') { depth--; if (depth === 0) return false; }
  }
  return depth === 1 && Buffer.byteLength(p) <= 256;
}

function makeValues(pattern) {
  const local = [...pattern].filter((c) => !META.has(c) && c !== '\\');
  const alphabet = local.length ? [...TRICKY, ...local, ...local] : TRICKY;
  const out = new Set(['']);
  while (out.size < 16) {
    let v = '';
    const n = Math.floor(rnd() * 7);
    for (let i = 0; i < n; i++) v += pick(alphabet);
    out.add(v);
  }
  return [...out];
}

const rust = worker('rust');
const ts = worker('ts');

async function ask(pattern, values) {
  const [a, b] = await Promise.all([
    rust.call({ op: 'regex', pattern, values }, 20000),
    ts.call({ op: 'regex', pattern, values }, 20000),
  ]);
  return [a, b];
}

function failure(r) {
  if (!r) return 'no answer';
  if (r.timeout) return `TIMEOUT>${r.ms}ms`;
  if (r.crash) return `CRASH ${r.crash}`;
  if (r.panic !== undefined) return `PANIC ${r.panic}`;
  if (r.thrown) return `THROWN ${r.name}: ${r.err}`;
  if (!r.ok) return `ERR ${r.err}`;
  return null;
}

/** The difference between two answers about one pattern and one value list, or null. */
function differs(a, b) {
  const fa = failure(a);
  const fb = failure(b);
  if (fa || fb) return { kind: 'failure', rust: fa || 'ok', ts: fb || 'ok' };
  if (a.out.compiled !== b.out.compiled) return { kind: a.out.compiled ? 'rust compiles, ts does not' : 'ts compiles, rust does not', why: a.out.why };
  if (!a.out.compiled) return null;
  for (let i = 0; i < a.out.m.length; i++) {
    if (a.out.m[i] !== b.out.m[i]) return { kind: 'match differs', index: i, rust: a.out.m[i], ts: b.out.m[i] };
  }
  return null;
}

async function stillDiffers(pattern, values, kind) {
  const [a, b] = await ask(pattern, values);
  const d = differs(a, b);
  return d && d.kind === kind ? d : null;
}

/** Shrink the pattern (and, for a match difference, the value) while the same kind of difference remains. */
async function reduce(pattern, values, d) {
  let p = pattern;
  let vals = d.kind === 'match differs' ? [values[d.index]] : values;
  const kind = d.kind;
  let changed = true;
  while (changed) {
    changed = false;
    const cps = [...p];
    for (let len = Math.max(1, cps.length >> 1); len >= 1; len >>= 1) {
      for (let i = 0; i + len <= cps.length; i++) {
        const cand = [...cps.slice(0, i), ...cps.slice(i + len)].join('');
        if (!cand || (reachableOnly && !reachable(cand))) continue;
        if (await stillDiffers(cand, vals, kind)) { p = cand; changed = true; break; }
      }
      if (changed) break;
    }
    if (kind === 'match differs') {
      const v = [...vals[0]];
      for (let i = 0; i < v.length && !changed; i++) {
        const cand = [...v.slice(0, i), ...v.slice(i + 1)].join('');
        if (await stillDiffers(p, [cand], kind)) { vals = [cand]; changed = true; }
      }
    }
  }
  const final = await ask(p, vals);
  return { pattern: p, value: kind === 'match differs' ? vals[0] : undefined, d: differs(...final) };
}

const groups = new Map();
const started = Date.now();
let patterns = 0;
let compiledBoth = 0;
let comparisons = 0;
while (Date.now() - started < seconds * 1000) {
  const p = alt(0);
  if (reachableOnly && !reachable(p)) continue;
  const values = makeValues(p);
  const [a, b] = await ask(p, values);
  patterns++;
  if (!failure(a) && !failure(b) && a.out.compiled && b.out.compiled) { compiledBoth++; comparisons += values.length; }
  const d = differs(a, b);
  if (!d) continue;
  const red = await reduce(p, values, d);
  const key = `${red.d ? red.d.kind : d.kind} | ${red.pattern}`;
  const g = groups.get(key) || { count: 0, example: red, original: p };
  g.count++;
  groups.set(key, g);
}
rust.close();
ts.close();

const esc = (s) => JSON.stringify(s);
console.log(`seed ${firstSeed}: ${patterns} patterns in ${seconds} s, ${compiledBoth} compiled by both (${comparisons} values compared), ${groups.size} reduced differences`);
const sorted = [...groups.entries()].sort((x, y) => y[1].count - x[1].count);
for (const [key, g] of sorted.slice(0, maxShown)) {
  const e = g.example;
  const d = e.d || {};
  let line = `${String(g.count).padStart(5)}  ${d.kind || key.split(' | ')[0]}  pattern ${esc(e.pattern)}`;
  if (e.value !== undefined) line += `  value ${esc(e.value)}  rust ${d.rust} ts ${d.ts}`;
  if (d.why) line += `  (rust: ${d.why})`;
  if (d.kind === 'failure') line += `  rust ${d.rust} / ts ${d.ts}`;
  console.log(line);
}
process.exit(groups.size ? 1 : 0);
