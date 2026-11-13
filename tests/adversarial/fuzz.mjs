#!/usr/bin/env node
/**
 * Differential fuzzer: random and mutated documents go to both
 * implementations; anything that crashes, hangs, throws an untyped exception,
 * breaks a round trip or is read differently is minimised and reported.
 *
 *   node tests/adversarial/fuzz.mjs                    60 seconds, fresh seed
 *   node tests/adversarial/fuzz.mjs --seconds 600 --seed 42
 *   node tests/adversarial/fuzz.mjs --checks parse,active,format,stringify,compile
 *   node tests/adversarial/fuzz.mjs --known            also report classes already in cases/
 *   node tests/adversarial/fuzz.mjs --impl rust,ts-bun
 *
 * Checks per document:
 *   parse      static tree, flags and include list agree
 *   active     resolved tree agrees (fixed environment, scratch base directory)
 *   format     format is idempotent and parses to the same tree, in each
 *   stringify  parse → stringify → parse gives the same tree, in each
 *   compile    text → .synxb → text gives the same tree, in each
 *   synxl      SYNXL records, hard errors and diagnostics agree; write and the
 *              streaming reader give the same records (run it on its own:
 *              --checks synxl)
 *
 * Findings are minimised line by line, then character by character, grouped
 * by a signature of the minimised input, and written to
 * tests/adversarial/tmp/fuzz-findings.json. Classes that already have a
 * regression case are counted but not listed unless --known is given.
 *
 * Exit code 1 when a new finding was made.
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { REPO, TMP, b64, ensureDir, isFailure, normJson, verdict } from './lib/harness.mjs';
import { worker } from './lib/impls.mjs';

const argv = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = argv.indexOf(name);
  return i === -1 ? fallback : argv[i + 1];
};
if (argv.includes('--help')) {
  process.stdout.write(fs.readFileSync(new URL(import.meta.url), 'utf-8').split('*/')[0].replace(/^#!.*\n\/\*\*\n?/, '').replace(/^ \* ?/gm, ''));
  process.exit(0);
}
const seconds = Number(opt('--seconds', '60'));
const seed0 = Number(opt('--seed', String(Date.now() % 1000000007)));
const implNames = opt('--impl', 'rust,ts').split(',');
const checks = opt('--checks', 'parse,active,format,stringify,compile').split(',');
const showKnown = argv.includes('--known');
const maxShown = Number(opt('--max', '25'));

// ─── randomness ──────────────────────────────────────────────

function rngOf(seed) {
  let a = seed >>> 0;
  const next = () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const r = {
    next,
    int: (n) => Math.floor(next() * n),
    pick: (list) => list[Math.floor(next() * list.length)],
    chance: (p) => next() < p,
  };
  return r;
}

// ─── generator ───────────────────────────────────────────────

const KEYS = ['a', 'b', 'c', 'name', 'port', 'host', 'x1', 'key_2', 'k-1', 'k.2', 'сервер', '名', 'Ünï', 'UPPER', '9lives', '_private', 'a.b', 'en', 'ru', 'one', 'other', 'count', 'base', 'child', 'items', 'list', 'value', 'min', 'max', 'true', 'null', '0'];
const SCALARS = ['1', '0', '-3', '42', '007', '-0', '3.14', '-0.5', '100.0', '1.50', '0.1', '1e5', '0x1F', '1_000', '9007199254740993', '9223372036854775807', '9223372036854775808', '-9223372036854775808', '12345678901234567890', '0.30000000000000004', '.5', '5.', '1.2.3', '+5', 'true', 'false', 'null', 'True', 'NULL', 'hello', 'hello world', 'a  b', '"quoted"', "'single'", '"', "''", '""', '"un"balanced', 'x # not a comment?', 'x #tight', 'a // b', 'http://a.b/c', 'path\\to\\file', '|', '|+', '| x', 'x |', '{a}', '{name}', '{a.b}', '{nope}', '{', '}', '{a}{a}', '- dash', '-', '!bang', '(paren)', '(int)', '[br]', ':colon', 'a:b', 'k v', 'tab\there', 'emoji 😀', 'мир', 'x y', 'trailing  ', '%', '100%', '%s', '$HOME', '~', '..', './x', '\\', '\\n', 'a,b,c', 'a, b , c', 'a|b', '1,2,3', '3 2 1', 'b a c'];
const HINTS = ['int', 'float', 'bool', 'string', 'weird', ''];
const CONSTRAINTS = ['required', 'readonly', 'min:1', 'max:5', 'min:3, max:30', 'type:int', 'type:string', 'type:float', 'type:bool', 'type:what', 'enum:a|b|c', 'enum:1|2', 'pattern:^[a-z]+$', 'pattern:^\\d+$', 'pattern:^.{1,3}$', 'min:1.5', 'max:-1', 'required, type:int', 'min:abc', ''];
const MARKERS = [
  'env', 'env:default:7', 'env:default:x y', 'env:default:0.0.0.0', 'default:5', 'default:text', 'default:', 'default:null', 'default:"q"',
  'calc', 'alias', 'ref', 'ref:calc:*2', 'ref:calc:+1', 'secret', 'template',
  'split', 'split:space', 'split:pipe', 'split:x', 'join', 'join:space', 'join:dash', 'join:, ',
  'clamp:1:10', 'clamp:10:1', 'clamp:0:0.5', 'round', 'round:2', 'round:0',
  'format:%.2f', 'format:%05d', 'format:%5d', 'format:%e', 'format:%d', 'format:%s', 'format:xyz',
  'replace:a:b', 'replace:a', 'replace::b', 'sort', 'sort:desc', 'unique', 'sum', 'map:a', 'map:name', 'map',
  'i18n', 'i18n:count', 'version:>=:1.2', 'version:<:2', 'version:==:1.2.3', 'version:what:1',
  'inherit:base', 'inherit:a:b', 'fallback:dflt', 'include', 'watch', 'watch:a.b', 'prompt', 'prompt:Label', 'vision', 'audio', 'unknownmarker', 'calc:round:2', 'env:secret', 'secret:env', 'alias:secret', 'default:5:clamp:1:3',
];
const CALC = ['1 + 2', '2 * (3 + 4)', 'a + 1', 'port * 2', 'a.b + 1', '10 / 4', '10 % 3', '1 / 0', '-5 + 3', '2 * -3', '(1', '1 +', 'x', '1.5 * 2', '0.1 + 0.2', '9007199254740993 + 0', '1 - -1', '  7  ', '', '3 3', 'a b', '2 ** 3', '1e3', '.5 + .5'];
const FILES = ['inside.synx', 'active.synx', 'data.json', 'missing.synx', 'sub/inner.synx', '../outside.synx', 'text.txt'];

function genLine(r, depth, active) {
  const ind = r.chance(0.04) ? '\t'.repeat(depth) : ' '.repeat(depth * (r.chance(0.08) ? 4 : 2) + (r.chance(0.03) ? 1 : 0));
  const roll = r.next();
  if (roll < 0.04) return [ind + r.pick(['# comment', '// comment', '#', '//x', '###', '# !active'])];
  if (roll < 0.06) return [''];
  let key = r.pick(KEYS);
  if (r.chance(0.12)) key += `(${r.pick(HINTS)})`;
  if (active && r.chance(0.2)) key += `[${r.pick(CONSTRAINTS)}]`;
  let marker = '';
  if (active && r.chance(0.55)) marker = r.pick(MARKERS);
  if (marker) key += ':' + marker;
  const head = marker.split(':')[0];
  const comment = r.chance(0.05) ? r.pick([' # c', ' // c', '  #c']) : '';
  if (roll < 0.26) {
    // group
    const out = [ind + key + comment];
    const n = 1 + r.int(3);
    for (let i = 0; i < n; i++) out.push(...genLine(r, depth + 1, active));
    return out;
  }
  if (roll < 0.40) {
    // list
    const out = [ind + key + comment];
    const n = r.int(4);
    for (let i = 0; i < n; i++) {
      if (r.chance(0.3)) {
        out.push(`${ind}  - ${r.pick(KEYS)}${active && r.chance(0.2) ? ':' + r.pick(MARKERS) : ''} ${r.pick(SCALARS)}`);
        const m = r.int(3);
        for (let j = 0; j < m; j++) out.push(`${ind}    ${r.pick(KEYS)} ${r.pick(SCALARS)}`);
      } else out.push(`${ind}  - ${r.pick(SCALARS)}`);
    }
    return out;
  }
  if (roll < 0.46) {
    const out = [ind + key + ' ' + r.pick(['|', '|+'])];
    const n = r.int(4);
    for (let i = 0; i < n; i++) out.push(`${ind}  ${' '.repeat(r.int(3))}${r.pick(['line', '# hash', '  indented', '- item', '!active', 'k v', '', 'x: y'])}`);
    return out;
  }
  let value = r.pick(SCALARS);
  if (head === 'calc') value = r.pick(CALC);
  else if (['alias', 'ref', 'map'].includes(head) && r.chance(0.7)) value = r.pick(KEYS);
  else if (['include', 'watch', 'fallback'].includes(head)) value = r.pick(FILES);
  else if (head === 'env' || marker.includes('env')) value = r.pick(['SET_NUM', 'SET_TEXT', 'SET_EMPTY', 'UNSET', 'SET_BOOL', 'SET_BIG', 'SET_QUOTED']);
  else if (head === 'version') value = r.pick(['1.2.3', '2', '1.2', 'v1', '1.x.3', '']);
  return [`${ind}${key}${r.pick([' ', ' ', ' ', '  ', '\t'])}${value}${comment}`];
}

function genDoc(r) {
  const active = r.chance(0.6);
  const lines = [];
  if (r.chance(0.03)) lines.push('﻿' + (active ? '!active' : '# bom'));
  else if (active) lines.push(r.chance(0.9) ? '!active' : '#!mode:active');
  if (r.chance(0.05)) lines.push(r.pick(['!lock', '!tool', '!llm', '!schema', '!include inside.synx', '!include inside.synx al', '!use @x/y', '!unknown x']));
  const n = 1 + r.int(8);
  for (let i = 0; i < n; i++) lines.push(...genLine(r, 0, active));
  return lines.join(r.chance(0.05) ? '\r\n' : '\n') + (r.chance(0.8) ? '\n' : '');
}

const FIELD_NAMES = ['id', 'name', 'note', 'body', 'a', 'b', 'x1', 'k-1', 'k.2', 'сервер', '名', '0', '10', 'true', 'null', 'toString', '__proto__', '_value'];
const FIELD_PARTS = SCALARS.concat(['"a ; b"', "'x ; y'", '"unterminated', '"a" trailing', '', ' ', 'a;b', ';', '"', '""', "''", '  padded  ', 'x ; ', ' ', 'a\tb']);

function genSynxl(r) {
  const lines = [];
  if (!r.chance(0.03)) lines.push(r.pick(['!synxl 1', '!synxl 1', '!synxl 1', '!synxl 1 ', '!synxl  1', '!synxl 2', '!synxl', '!SYNXL 1', ' !synxl 1', '!synxl 01', '!synxl 1 x']));
  if (r.chance(0.05)) lines.push(r.pick(['', '# c', '###', '!synxl 1', '!active']));
  const lists = 1 + r.int(2);
  for (let k = 0; k < lists; k++) {
    const nf = 1 + r.int(4);
    const fields = [];
    for (let i = 0; i < nf; i++) {
      let f = r.pick(FIELD_NAMES);
      if (r.chance(0.3)) f += '(' + r.pick(['int', 'float', 'bool', 'string', 'random', 'random:int', 'x', '']) + ')';
      if (r.chance(0.4)) f += '[' + r.pick(['block', 'block', 'required', 'min:1', 'max:3', 'type:int', 'type:string', 'type:float', 'type:bool', 'enum:a|b', 'pattern:^a+\x24', 'block, type:int', 'min:1, max:2', 'what', '', 'block, required']) + ']';
      if (r.chance(0.04)) f += ':' + r.pick(['env', 'calc', 'secret', 'a:b']);
      fields.push(f);
    }
    if (!r.chance(0.04)) lines.push('!fields ' + fields.join(r.pick([' ; ', ' ; ', ' ; ', ';', ' ;', '; ', '  ;  '])));
    const records = r.int(5);
    for (let j = 0; j < records; j++) {
      const np = Math.max(0, nf + r.pick([0, 0, 0, 0, 0, -1, 1]));
      const parts = [];
      for (let i = 0; i < np; i++) parts.push(r.pick(FIELD_PARTS));
      lines.push(parts.join(r.pick([' ; ', ' ; ', ' ; ', ';', ' ;  '])));
      if (r.chance(0.3)) {
        const name = fields[r.int(fields.length)].replace(/[(\[:].*/, '');
        lines.push('  ' + r.pick([name, name, name, 'unknown']) + r.pick(['', '', ' |', ' |+', ' inline']));
        const body = r.int(4);
        for (let b = 0; b < body; b++) lines.push('    ' + r.pick(['k v', '- item', 'x 1', '!active', '# c', 'deep', '  deeper 1', 'k:env V', '- a 1', '  b 2', 'n(random) x', '###', '!include x.synx', 'k(int) 0x10']));
      }
      if (r.chance(0.06)) lines.push(r.pick(['# comment', '###', '', '// c', '!active', '!include x', '!unknown', '  orphan 1', '!synxl 1', '!synxl 2']));
    }
  }
  return lines.join(r.chance(0.05) ? '\r\n' : '\n') + (r.chance(0.8) ? '\n' : '');
}

const SPLICE = ['!active', '#!mode:active', '###', '- ', '  - x', '|', '|+', ':env', ':calc', ':alias', '(int)', '[required]', '[min:1]', ' # c', ' //', '\t', '  ', '\r', '{a}', '"', "'", ' ', '　', ':', '(', '[', ']', ')', '-', '!', '#', '0', '9'.repeat(19), '.', 'null', 'true', '\n', '\n\n', '😀'];

function mutate(r, text) {
  let lines = text.split('\n');
  const n = 1 + r.int(3);
  for (let k = 0; k < n; k++) {
    const i = r.int(lines.length);
    switch (r.int(9)) {
      case 0: lines.splice(i, 1); break;
      case 1: lines.splice(i, 0, lines[r.int(lines.length)]); break;
      case 2: lines[i] = ' '.repeat(r.int(7)) + lines[i].trimStart(); break;
      case 3: { const j = r.int(lines[i].length + 1); lines[i] = lines[i].slice(0, j) + r.pick(SPLICE) + lines[i].slice(j); break; }
      case 4: { const j = r.int(lines.length); [lines[i], lines[j]] = [lines[j], lines[i]]; break; }
      case 5: { const a = r.int(lines[i].length + 1); lines[i] = lines[i].slice(0, a) + lines[i].slice(a + 1 + r.int(3)); break; }
      case 6: lines[i] = lines[i].replace(/\S+$/, r.pick(SCALARS)); break;
      case 7: lines[i] = lines[i].replace(/^(\s*)(\S+)/, (m, s, k2) => s + r.pick(KEYS) + k2.replace(/^[^:([]*/, '')); break;
      default: lines.splice(i, 0, ...genLine(r, r.int(3), true)); break;
    }
    if (lines.length === 0) lines = ['a 1'];
  }
  return lines.join('\n');
}

// ─── scratch base directory and corpus ───────────────────────

const base = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-fuzz-'));
process.on('exit', () => { try { fs.rmSync(base, { recursive: true, force: true }); } catch { /* scratch only */ } });
fs.mkdirSync(path.join(base, 'sub'));
fs.writeFileSync(path.join(base, 'inside.synx'), 'a 1\nname inside\nb\n  c 2\n');
fs.writeFileSync(path.join(base, 'active.synx'), '!active\na:env:default:9 UNSET\nname {a}\n');
fs.writeFileSync(path.join(base, 'sub', 'inner.synx'), 'a inner\n');
fs.writeFileSync(path.join(base, 'data.json'), '{"a":{"b":5},"name":"json"}');
fs.writeFileSync(path.join(base, 'text.txt'), 'plain text\n');
fs.writeFileSync(path.join(path.dirname(base), path.basename(base) + '-outside.synx'), 'a OUTSIDE\n');
const ENV = { SET_NUM: '42', SET_TEXT: 'hello', SET_EMPTY: '', SET_BOOL: 'true', SET_BIG: '9007199254740993', SET_QUOTED: '"7"' };

const corpus = [];
for (const dir of [path.join(REPO, 'tests', 'conformance', 'cases'), path.join(REPO, 'examples')]) {
  try {
    for (const f of fs.readdirSync(dir)) if (f.endsWith('.synx')) corpus.push(fs.readFileSync(path.join(dir, f), 'utf-8'));
  } catch { /* directory missing */ }
}

const corpusSynxl = [];
try {
  const dir = path.join(REPO, 'tests', 'conformance-synxl', 'cases');
  for (const f of fs.readdirSync(dir)) if (f.endsWith('.synxl')) corpusSynxl.push(fs.readFileSync(path.join(dir, f), 'utf-8'));
} catch { /* directory missing */ }
const synxlRun = checks.every((c) => c === 'synxl');

// ─── checks ──────────────────────────────────────────────────

const workers = implNames.map((n) => worker(n, { quiet: false }));

/** Returns null, or `{ kind, detail }` describing the first problem. */
async function examine(text, only) {
  const input = b64(text);
  for (const check of only || checks) {
    if (check === 'synxl') {
      const views = [];
      for (const w of workers) {
        const resp = await w.call({ op: 'synxl', b64: input, validate: true }, 5000);
        if (isFailure(resp)) return { kind: 'synxl:' + w.name + ':' + String(verdict(resp)).replace(/\d+/g, 'N').slice(0, 70), detail: String(verdict(resp)) };
        views.push(!resp.ok ? 'ERR ' + resp.err
          : resp.out.hard ? `hard ${resp.out.hard} at line ${resp.out.line}`
            : normJson(resp.out.json) + ' diagnostics ' + JSON.stringify(resp.out.diagnostics));
        const rt = await w.call({ op: 'synxl_check', b64: input }, 5000);
        if (isFailure(rt)) return { kind: 'synxl:' + w.name + ':round trip ' + String(verdict(rt)).replace(/\d+/g, 'N').slice(0, 60), detail: String(verdict(rt)) };
        if (rt.ok && !rt.out.hard) {
          if (normJson(rt.out.j0) !== normJson(rt.out.j1)) return { kind: 'synxl:' + w.name + ':write changes the records', detail: rt.out.j0 + ' → ' + rt.out.j1 + '  via ' + JSON.stringify(rt.out.written) };
          if (rt.out.stream_err === null && normJson(rt.out.stream) !== normJson(rt.out.j0)) return { kind: 'synxl:' + w.name + ':stream reader differs', detail: rt.out.j0 + ' vs ' + rt.out.stream };
        }
      }
      if (new Set(views).size > 1) return { kind: 'synxl:disagree', detail: views.map((v, i) => workers[i].name + ': ' + v).join('\n        ') };
      continue;
    }
    const op = { parse: 'parse', active: 'active', format: 'format_check', stringify: 'stringify_check', compile: 'compile_check' }[check];
    const answers = [];
    for (const w of workers) {
      const resp = await w.call({ op, b64: input, env: ENV, base }, 5000);
      answers.push(resp);
      if (isFailure(resp)) return { kind: `${check}:${w.name}:${String(verdict(resp)).replace(/\d+/g, 'N').slice(0, 70)}`, detail: String(verdict(resp)) };
      if (!resp.ok) continue;
      const o = resp.out;
      if (check === 'format') {
        if (!o.idempotent) return { kind: `format:${w.name}:not idempotent`, detail: `${JSON.stringify(o.f1)} → ${JSON.stringify(o.f2)}` };
        if (o.m0 !== o.m1) return { kind: `format:${w.name}:mode changed`, detail: `active ${o.m0} → ${o.m1}` };
        if (normJson(o.p0) !== normJson(o.p1)) return { kind: `format:${w.name}:data changed`, detail: `${o.p0} → ${o.p1}` };
      } else if (check === 'stringify') {
        if (normJson(o.p0) !== normJson(o.p1)) return { kind: `stringify:${w.name}:data changed`, detail: `${o.p0} → ${o.p1}  via ${JSON.stringify(o.text)}` };
      } else if (check === 'compile') {
        if (o.p1 !== undefined && normJson(o.p0) !== normJson(o.p1)) return { kind: `compile:${w.name}:tree changed`, detail: `${o.p0} → ${o.p1}` };
        if (normJson(o.p0) !== normJson(o.p2)) return { kind: `compile:${w.name}:decompiled text differs`, detail: `${o.p0} → ${o.p2}` };
      }
    }
    if (check === 'parse' || check === 'active') {
      const views = answers.map((a) => (a.ok ? normJson(a.out.json) + (check === 'parse' ? `|${a.out.active}${a.out.locked}${a.out.tool}${a.out.schema}${a.out.llm}|${JSON.stringify(a.out.includes)}` : '') : `ERR ${a.err}`));
      if (new Set(views).size > 1) return { kind: `${check}:disagree`, detail: views.map((v, i) => `${workers[i].name}: ${v}`).join('\n        ') };
      if (check === 'active' && answers.every((a) => a.ok)) {
        const ordered = answers.map((a) => normJson(a.out.ordered));
        if (new Set(ordered).size > 1) return { kind: 'active:key order', detail: ordered.map((v, i) => `${workers[i].name}: ${v}`).join('\n        ') };
      }
    }
  }
  return null;
}

async function minimise(text, kind) {
  const check = kind.split(':')[0];
  const still = async (t) => {
    const f = await examine(t, [check]);
    return f && f.kind === kind;
  };
  let lines = text.split('\n');
  for (let chunk = Math.max(1, lines.length >> 1); chunk >= 1; chunk = chunk >> 1) {
    for (let i = 0; i < lines.length;) {
      const trial = lines.slice(0, i).concat(lines.slice(i + chunk));
      if (trial.length && await still(trial.join('\n'))) lines = trial;
      else i += chunk;
    }
    if (chunk === 1) break;
  }
  let cur = lines.join('\n');
  let budgetLeft = 400;
  for (let size = 8; size >= 1 && budgetLeft > 0; size = size >> 1) {
    for (let i = 0; i < cur.length && budgetLeft > 0;) {
      const trial = cur.slice(0, i) + cur.slice(i + size);
      budgetLeft--;
      if (trial.length && await still(trial)) cur = trial;
      else i += size;
    }
  }
  return cur;
}

/** Classes that already have a regression case: pattern on the minimised input. */
const KNOWN = [
  [/﻿|\u0085/, 'B-007 U+FEFF / U+0085'],
  [/:round/, 'B-008…B-010 :round'],
  [/:random/, 'B-011 :random'],
  [/:format:%\d*i|:format:%\.\d+e/, 'B-013 :format'],
  [/:sum/, 'B-015 :sum'],
  [/\[(min|max):(0[xbo]|\]|inf|-inf|nan)/i, 'B-005 bounds'],
  [/\((int)\)\s+0[xbo]/, 'B-006 (int)'],
  [/__synx|__proto__|constructor|prototype|toString|valueOf|hasOwnProperty/, 'B-002…B-004 reserved names'],
  [/#!mode:\s*$/m, 'B-012 #!mode:'],
  [/\(int\)\S*\s*0[xbo]/i, 'B-006 (int)'],
  [/^\s*- *$/m, 'B-042 bare dash'],
  [/^\s*(#|\/\/).*\n\s*- /m, 'B-043 comment before the first item'],
  [/^\s*- [^\s:]+\S*:\S*(inherit|random)/m, 'B-044 list item :inherit / :random value'],
  [/^\s*- [^\s(]+\([^)]*\)\S*\s*$/m, 'B-045 list item hint without value'],
  [/^\s*\d+\s*$|^\s*\d+[\s:([]/m, 'B-047 integer-like keys'],
  [/\[pattern:/, 'B-048 [pattern] dialect'],
  [/\(float\)\S*\[type:int|\[type:int\]\S*\s+-?\d+\.0*\s*$/m, 'B-050 float against [type:int]'],
];

/** The features a minimised input still uses: findings with the same set are one class. */
function signature(text) {
  const feats = new Set();
  for (const m of text.matchAll(/:([a-z][a-z0-9]*)/g)) feats.add(':' + m[1]);
  for (const m of text.matchAll(/\[(\w+)/g)) feats.add('[' + m[1]);
  for (const m of text.matchAll(/\((\w*)\)/g)) feats.add('(' + m[1] + ')');
  if (/^\s*- /m.test(text)) feats.add('list');
  if (/\|\+?\s*$/m.test(text)) feats.add('block');
  if (/[{}]/.test(text)) feats.add('{}');
  if (/["']/.test(text)) feats.add('quote');
  if (/#|\/\//.test(text)) feats.add('comment');
  if (/\t/.test(text)) feats.add('tab');
  if (/\r/.test(text)) feats.add('CR');
  if (/[^\x00-\x7f]/.test(text)) feats.add('non-ascii');
  if (/^\s*!\w+/m.test(text)) feats.add('directive');
  if (/^\s*\d+\s*$/m.test(text) || /^\s*\d+[\s:([]/m.test(text)) feats.add('numeric key');
  return [...feats].sort().join(' ') || 'plain';
}

// ─── main loop ───────────────────────────────────────────────

const findings = new Map();
const knownHits = new Map();
const started = Date.now();
let docs = 0;
let seed = seed0;
console.log(`seed ${seed0}, ${seconds} s, implementations ${implNames.join(' + ')}, checks ${checks.join(' ')}`);
// A fixed document every worker must keep answering the same way. If the
// answer changes, some earlier request left state behind in that process.
const CANARY = '!active\ncanary:env:default:7 UNSET_CANARY\nsum:calc 1 + 2\nlist:join\n  - a\n  - b\n';
const CANARY_WANT = '{"canary":7,"list":"a,b","sum":3}';
const canaryOk = async (w) => {
  const r = await w.call({ op: 'active', b64: b64(CANARY), env: ENV, base }, 5000);
  return r.ok && normJson(r.out.json) === CANARY_WANT;
};
const recent = [];
/** Find the request after which worker `w` stops answering the canary. */
async function findPoison(w) {
  w.close();
  const order = (process.env.SYNX_FUZZ_OPS || 'parse,active,format_check,stringify_check,compile_check').split(',');
  for (const text of recent) {
    for (const op of order) {
      await w.call({ op, b64: b64(text), env: ENV, base }, 5000);
      if (!(await canaryOk(w))) {
        w.close();
        return { text, op };
      }
    }
  }
  w.close();
  return null;
}

while (Date.now() - started < seconds * 1000) {
  const r = rngOf(seed++);
  let text = synxlRun
    ? (corpusSynxl.length && r.chance(0.35) ? mutate(r, r.pick(corpusSynxl)) : genSynxl(r))
    : (corpus.length && r.chance(0.35) ? mutate(r, r.pick(corpus)) : genDoc(r));
  if (r.chance(0.3)) text = mutate(r, text);
  docs++;
  recent.push(text);
  if (recent.length >= 40) {
    for (const w of workers) {
      if (await canaryOk(w)) continue;
      const poison = await findPoison(w);
      const key = `state:${w.name} | ${poison ? signature(poison.text) : 'not reproduced'}`;
      if (!findings.has(key)) {
        findings.set(key, { kind: `state:${w.name}`, input: poison ? poison.text : null, detail: poison ? `after ${poison.op} on this input the same process resolves other documents differently` : 'canary failed, single replay did not reproduce', seed: seed - 1, count: 0 });
        console.log(`\n[${findings.size}] STATE LEFT BEHIND in ${w.name} (seed ≤ ${seed - 1})\n    after ${poison ? poison.op : '?'} on: ${JSON.stringify(poison ? poison.text : null)}\n        the canary ${JSON.stringify(CANARY)} no longer gives ${CANARY_WANT}`);
      }
      findings.get(key).count++;
    }
    recent.length = 0;
  }
  const found = await examine(text);
  if (!found) continue;
  const small = await minimise(text, found.kind);
  const again = (await examine(small, [found.kind.split(':')[0]])) || found;
  const known = KNOWN.find(([re]) => re.test(small));
  if (known && !showKnown) {
    knownHits.set(known[1], (knownHits.get(known[1]) || 0) + 1);
    continue;
  }
  const key = `${found.kind} | ${signature(small)}`;
  if (!findings.has(key)) {
    findings.set(key, { kind: found.kind, features: signature(small), input: small, detail: again.detail, seed: seed - 1, count: 0 });
  }
  const f = findings.get(key);
  f.count++;
  // Keep the shortest example of each class.
  if (small.length < f.input.length) { f.input = small; f.detail = again.detail; f.seed = seed - 1; }
}
for (const w of workers) w.close();

const out = path.join(ensureDir(TMP), 'fuzz-findings.json');
const ranked = [...findings.values()].sort((a, b) => b.count - a.count);
fs.writeFileSync(out, JSON.stringify(ranked, null, 1));
for (const f of ranked.slice(0, maxShown)) {
  console.log(`\n×${f.count}  ${f.kind}  [${f.features || ''}]  (seed ${f.seed})\n    input: ${JSON.stringify(f.input)}\n        ${String(f.detail).slice(0, 330)}`);
}
console.log(`\n${docs} documents in ${((Date.now() - started) / 1000).toFixed(0)} s; ${findings.size} new classes → ${out}`);
for (const [name, count] of knownHits) console.log(`  already covered ×${count}: ${name}`);
process.exit(findings.size ? 1 : 0);
