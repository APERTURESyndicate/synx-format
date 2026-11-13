/**
 * Input shapes for cost measurements: each is a function of a size `n` that
 * returns one request. Shared by scale.mjs (curves) and the cost cases of the
 * regression suite.
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import zlib from 'node:zlib';
import { container, varint, stringTable, T } from './synxb.mjs';

const base = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-scale-'));
process.on('exit', () => { try { fs.rmSync(base, { recursive: true, force: true }); } catch { /* scratch only */ } });
fs.writeFileSync(path.join(base, 'small.synx'), 'k v\n');
fs.writeFileSync(path.join(base, 'data.json'), JSON.stringify({ a: { b: 1 }, list: Array.from({ length: 2000 }, (_, i) => ({ id: i, name: 'item' + i })) }));

const A = '!active\n';
const lines = (n, f) => { let s = ''; for (let i = 0; i < n; i++) s += f(i); return s; };

/** name → [start n, n => { op, input, req? }] */
export const SHAPES = {
  // ── static parser ──
  'parse: flat keys': [20000, (n) => ({ op: 'parse', input: lines(n, (i) => `key_${i} value ${i}\n`) })],
  'parse: keys sharing a 200-byte prefix': [5000, (n) => ({ op: 'parse', input: lines(n, (i) => `${'p'.repeat(200)}${i} ${i}\n`) })],
  'parse: nesting to the cap, then siblings': [10000, (n) => ({ op: 'parse', input: lines(127, (i) => `${' '.repeat(i)}k${i}\n`) + lines(n, (i) => `${' '.repeat(127)}c${i} ${i}\n`) })],
  'parse: one long value': [1 << 20, (n) => ({ op: 'parse', input: 'k ' + 'v'.repeat(n) + '\n' })],
  'parse: one long key': [1 << 20, (n) => ({ op: 'parse', input: 'k'.repeat(n) + ' v\n' })],
  'parse: long ancestor key, groups opening and closing under it': [5000, (n) => ({ op: 'parse', input: 'K'.repeat(200000) + '\n' + lines(n, () => '  a\n    x 1\n') })],
  'parse: 100 nested long keys, group per line at the bottom': [3000, (n) => ({ op: 'parse', input: lines(100, (i) => `${' '.repeat(i)}${'k'.repeat(2000)}${i}\n`) + lines(n, () => `${' '.repeat(100)}a\n${' '.repeat(101)}x 1\n`) })],
  'parse: ancestor key and its groups both growing': [2000, (n) => ({ op: 'parse', input: 'K'.repeat(n * 12) + '\n' + lines(n, () => '  a\n    x 1\n') })],
  'parse: list of scalars': [50000, (n) => ({ op: 'parse', input: 'l\n' + lines(n, (i) => `  - ${i}\n`) })],
  'parse: list of objects': [20000, (n) => ({ op: 'parse', input: 'l\n' + lines(n, (i) => `  - id ${i}\n    name n${i}\n`) })],
  'parse: blank lines between list items': [2000, (n) => ({ op: 'parse', input: 'l\n' + lines(n, (i) => `  - id ${i}\n${'\n'.repeat(200)}    name x\n`) })],
  'parse: block bodies': [20000, (n) => ({ op: 'parse', input: lines(n / 100, (i) => `b${i} |\n` + lines(100, (j) => `  line ${j} of block\n`)) })],
  'parse: the same key again and again': [100000, (n) => ({ op: 'parse', input: lines(n, () => 'a\n') })],
  'parse: comment toggles': [100000, (n) => ({ op: 'parse', input: lines(n, () => '###\n') })],
  'parse: non-ASCII indent': [30000, (n) => ({ op: 'parse', input: 'a\n' + lines(n, (i) => `　 k${i} v\n`) })],
  'parse: inline comments on long values': [3000, (n) => ({ op: 'parse', input: lines(n, (i) => `k${i} ${'x '.repeat(200)}# ${'c '.repeat(200)}\n`) })],

  // ── active engine ──
  'active: flat env keys': [20000, (n) => ({ op: 'active', input: A + lines(n, (i) => `k${i}:env:default:${i} V${i}\n`) })],
  'active: key redeclared, metadata elsewhere': [3000, (n) => ({ op: 'active', input: A + lines(n, (i) => `g${i}\n  x[min:1] 1\n`) + lines(n * 4, () => 'a\n') })],
  'active: marked keys under a long ancestor key': [5000, (n) => ({ op: 'active', input: A + 'K'.repeat(200000) + '\n' + lines(n, (i) => `  c${i}:env:default:1 V\n`) })],
  'active: ancestor key and its marked keys both growing': [2000, (n) => ({ op: 'active', input: A + 'K'.repeat(n * 12) + '\n' + lines(n, (i) => `  c${i}:env:default:1 V\n`) })],
  'active: { without }': [100000, (n) => ({ op: 'active', input: A + 'k ' + '{'.repeat(n) + '\n' })],
  'active: {a{a{a…': [100000, (n) => ({ op: 'active', input: A + 'k ' + '{a'.repeat(n) + '\n' })],
  'active: many values each with an unclosed {': [3000, (n) => ({ op: 'active', input: A + lines(n, (i) => `k${i} ${'{x'.repeat(100)}\n`) })],
  'active: {big} in many values': [1500, (n) => ({ op: 'active', input: A + 'big ' + 'v'.repeat(n * 10) + '\n' + lines(n, (i) => `k${i} {big}\n`) })],
  'active: {group} in many values': [1500, (n) => ({ op: 'active', input: A + 'g\n' + lines(n, (i) => `  f${i} ${i}\n`) + lines(n, (i) => `k${i} {g}\n`) })],
  'active: doubling chain of {…}': [14, (n) => ({ op: 'active', input: A + 'a0 0123456789\n' + lines(n, (i) => `a${i + 1} {a${i}}{a${i}}\n`) })],
  'active: :alias of one big group': [1500, (n) => ({ op: 'active', input: A + 'g\n' + lines(n, (i) => `  f${i} ${i}\n`) + lines(n, (i) => `k${i}:alias g\n`) })],
  'active: :ref of one big group': [1500, (n) => ({ op: 'active', input: A + 'g\n' + lines(n, (i) => `  f${i} ${i}\n`) + lines(n, (i) => `k${i}:ref g\n`) })],
  'active: alias chain': [20000, (n) => ({ op: 'active', input: A + 'k0 value\n' + lines(n, (i) => `k${i + 1}:alias k${i}\n`) })],
  'active: :inherit, many children of one wide parent': [1000, (n) => ({ op: 'active', input: A + 'p\n' + lines(n, (i) => `  f${i} ${i}\n`) + lines(n, (i) => `c${i}:inherit:p\n  own ${i}\n`) })],
  'active: :inherit with markers everywhere': [300, (n) => ({ op: 'active', input: A + 'p\n' + lines(n, (i) => `  f${i}:env:default:1 V\n`) + lines(n, (i) => `o${i}\n  x[min:1] 1\n`) + lines(n, (i) => `c${i}:inherit:p\n  own ${i}\n`) })],
  'active: :inherit chain': [2000, (n) => ({ op: 'active', input: A + 'c0\n  f 1\n' + lines(n, (i) => `c${i + 1}:inherit:c${i}\n  g${i} ${i}\n`) })],
  'active: :calc over many numeric keys': [5000, (n) => ({ op: 'active', input: A + lines(n, (i) => `v${i} ${i}\n`) + lines(n, (i) => `c${i}:calc v${i} + v${(i * 7) % n} * 2\n`) })],
  'active: :calc with a key containing a dash': [1000, (n) => ({ op: 'active', input: A + 'max-hp 5\n' + lines(n, (i) => `v${i} ${i}\n`) + lines(n, (i) => `c${i}:calc v${i} + 1\n`) })],
  'active: :calc dot-path into a big group': [1500, (n) => ({ op: 'active', input: A + 'g\n  sub\n' + lines(n, (i) => `    f${i} ${i}\n`) + lines(n, (i) => `c${i}:calc g.sub + 1\n`) })],
  'active: :replace, long replacement': [2000, (n) => ({ op: 'active', input: A + `k:replace:a:${'B'.repeat(n)} ${'a'.repeat(n)}\n` })],
  'active: :join, long separator': [2000, (n) => ({ op: 'active', input: A + `k:join:${'S'.repeat(n)}\n` + lines(n, (i) => `  - ${i}\n`) })],
  'active: :split of a long value': [200000, (n) => ({ op: 'active', input: A + 'k:split ' + 'a,'.repeat(n) + '\n' })],
  'active: :sort of mixed items': [50000, (n) => ({ op: 'active', input: A + 'l:sort\n' + lines(n, (i) => `  - ${i % 3 ? `"${(i * 7919) % 1000}"` : (i * 104729) % 1000}\n`) })],
  'active: :unique': [50000, (n) => ({ op: 'active', input: A + 'l:unique\n' + lines(n, (i) => `  - v${i % 1000}\n`) })],
  'active: :map with a long table per key': [300, (n) => ({ op: 'active', input: A + 'src 5\n' + lines(n, (i) => `m${i}:map:src\n` + lines(n, (j) => `  - ${j} r${j}\n`)) })],
  'active: :spam, one bucket hit from many groups': [5000, (n) => ({ op: 'active', input: A + 't 1\n' + lines(n, (i) => `g${i}\n  k:spam:100000000 t\n`) })],
  'active: :spam, a bucket per key': [5000, (n) => ({ op: 'active', input: A + lines(n, (i) => `k${i}:spam:5 t${i}\n`) })],
  'active: :once, many keys': [300, (n) => ({ op: 'active', input: A + lines(n, (i) => `id${i}:once\n`), req: { base: fs.mkdtempSync(path.join(base, 'once-')) } })],
  'active: :fallback, many keys': [1000, (n) => ({ op: 'active', input: A + lines(n, (i) => `k${i}:fallback:dflt nofile${i}.txt\n`), req: { base } })],
  'active: :include of a missing file, many keys': [1000, (n) => ({ op: 'active', input: A + lines(n, (i) => `k${i}:include nofile${i}.synx\n`), req: { base } })],
  'active: :watch of one JSON file, many keys': [200, (n) => ({ op: 'active', input: A + lines(n, (i) => `k${i}:watch:a.b data.json\n`), req: { base } })],
  'active: [pattern] with a counted class on long values': [20000, (n) => ({ op: 'active', input: A + `k[pattern:^(?:[ab]{200}c)?[ab]*$] ${'ab'.repeat(n)}\n` })],
  'active: [pattern] unanchored on long values': [20000, (n) => ({ op: 'active', input: A + `k[pattern:a[ab]{200}c] ${'ab'.repeat(n)}\n` })],
  'active: one constraint name, many fields': [20000, (n) => ({ op: 'active', input: A + 'x[min:1, max:9, pattern:^\\d$] 5\n' + lines(n, (i) => `g${i}\n  x 5\n`) })],
  'active: distinct patterns': [200, (n) => ({ op: 'active', input: A + lines(n, (i) => `k${i}[pattern:^[a-z]{1,${(i % 200) + 1}}x${i}$] v\n`) })],
  'active: :prompt of a group, via :ref, many times': [300, (n) => ({ op: 'active', input: A + 'g\n' + lines(n, (i) => `  f${i} ${i}\n`) + lines(n, (i) => `k${i}:ref:prompt g\n`) })],
  'active: :i18n with a count path': [5000, (n) => ({ op: 'active', input: A + 'cnt 5\n' + lines(n, (i) => `t${i}:i18n:cnt\n  en\n    one {count} a\n    other {count} b\n`) })],
  'active: nested groups with markers': [2000, (n) => ({ op: 'active', input: A + lines(n, (i) => `g${i}\n  a\n    b\n      c:env:default:1 V\n`) })],
  'active: list items with markers': [5000, (n) => ({ op: 'active', input: A + 'l\n' + lines(n, (i) => `  - id ${i}\n    port:env:default:1 V\n`) })],

  // ── format, stringify, binary ──
  'format: flat keys': [20000, (n) => ({ op: 'format', input: lines(n, (i) => `key_${(i * 7919) % n} value\n`) })],
  'format: nested groups': [20000, (n) => ({ op: 'format', input: lines(n, (i) => `${'  '.repeat(i % 60)}g${i}\n`) })],
  'format: towers deeper than the cap': [20, (n) => ({ op: 'format', input: lines(n, () => lines(200, (i) => `${' '.repeat(i)}g${i}\n`)) })],
  'format: lists': [20000, (n) => ({ op: 'format', input: lines(n / 10, (i) => `l${i}\n` + lines(10, (j) => `  - item ${j}\n`)) })],
  'stringify: round trip': [20000, (n) => ({ op: 'stringify_check', input: lines(n, (i) => `g${i}\n  a "${i}"\n  b |\n    x\n    y\n`) })],
  'compile: flat keys': [20000, (n) => ({ op: 'compile', input: lines(n, (i) => `key_${i} value ${i}\n`) })],
  'compile: active with metadata': [10000, (n) => ({ op: 'compile', input: A + lines(n, (i) => `k${i}[min:1, max:9]:env:default:5 V${i}\n`) })],
  'decompile: valid container': [20000, (n) => ({ op: 'decompile', input: validContainer(n) })],
  'decompile: array of nulls behind a zero bomb': [1 << 20, (n) => ({ op: 'decompile', input: container(Buffer.concat([varint(0), Buffer.from([T.arr]), varint(n), Buffer.alloc(n)])) })],
  'decompile: object count far above its content': [1 << 20, (n) => ({ op: 'decompile', input: container(Buffer.concat([varint(0), Buffer.from([T.obj]), varint(n), Buffer.alloc(n)])) })],
  'decompile: one long string referenced many times': [1500, (n) => ({ op: 'decompile', input: container(Buffer.concat([stringTable(['S'.repeat(n * 20)]), Buffer.from([T.arr]), varint(n), Buffer.concat(Array.from({ length: n }, () => Buffer.from([T.str, 0])))])) })],
  'decompile: declared size only': [1 << 22, (n) => ({ op: 'decompile', input: Buffer.concat([Buffer.from('SYNXB\x01\x00', 'latin1'), (() => { const b = Buffer.alloc(4); b.writeUInt32LE(n); return b; })(), Buffer.from([3, 0])]) })],
  'decompile: truncated stream of zeros': [1 << 20, (n) => ({ op: 'decompile', input: (() => { const raw = zlib.deflateRawSync(Buffer.alloc(n), { level: 9 }); const h = Buffer.alloc(11); h.write('SYNXB', 0, 'latin1'); h[5] = 1; h.writeUInt32LE(n, 7); return Buffer.concat([h, raw.subarray(0, raw.length - 4)]); })() })],

  // ── SYNXL ──
  'synxl: records': [20000, (n) => ({ op: 'synxl', input: '!synxl 1\n!fields id[type:int] ; name ; note\n' + lines(n, (i) => `${i} ; name ${i} ; note ${i}\n`) })],
  'synxl: wide field list': [1000, (n) => ({ op: 'synxl', input: '!synxl 1\n!fields ' + Array.from({ length: Math.min(n, 4096) }, (_, i) => `f${i}`).join(' ; ') + '\n' + lines(20, () => Array.from({ length: Math.min(n, 4096) }, (_, i) => i).join(' ; ') + '\n') })],
  'synxl: a field list per record': [5000, (n) => ({ op: 'synxl', input: '!synxl 1\n' + lines(n, (i) => `!fields a${i} ; b\n1 ; 2\n`) })],
  'synxl: block fields': [5000, (n) => ({ op: 'synxl', input: '!synxl 1\n!fields id ; body[block]\n' + lines(n, (i) => `${i}\n  body |\n    line one\n    line two\n`) })],
  'synxl: quoted fields with escapes': [5000, (n) => ({ op: 'synxl', input: '!synxl 1\n!fields a ; b\n' + lines(n, (i) => `"x ; y ${i}" ; 'q " q'\n`) })],
  'synxl: validate with pattern': [5000, (n) => ({ op: 'synxl', input: '!synxl 1\n!fields a[pattern:^[a-z0-9 ]+$] ; b[min:1, max:5]\n' + lines(n, (i) => `value number ${i} ; 3\n`), req: { validate: true } })],
  'synxl: round trip': [5000, (n) => ({ op: 'synxl_check', input: '!synxl 1\n!fields id[type:int] ; name ; body[block]\n' + lines(n, (i) => `${i} ; "n ; ${i}"\n  body |\n    text ${i}\n`) })],
};

function validContainer(n) {
  const strings = Array.from({ length: n }, (_, i) => `key_${i}`);
  const parts = [stringTable(strings), Buffer.from([T.obj]), varint(n)];
  for (let i = 0; i < n; i++) parts.push(varint(i), Buffer.from([T.int]), varint(i * 2));
  return container(Buffer.concat(parts));
}

