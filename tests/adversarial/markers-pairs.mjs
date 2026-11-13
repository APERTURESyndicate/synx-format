#!/usr/bin/env node
/**
 * Every marker next to every other one, and `:secret` with every pair: the two
 * implementations must resolve the key the same way, and no printed form may
 * hold the text of a value marked `:secret`.
 *
 *   node tests/adversarial/markers-pairs.mjs [--triples] [--max 40]
 *
 * Each chain is tried with a few values (a word, a number, a list, a group, a
 * path to a file in the scratch base, an environment variable name) and with
 * the helper keys the markers read (`base`, `src`, `sel`). The same request
 * goes to `active` (trees compared) and, when `:secret` is in the chain, to
 * `sinks` with the marked text as the needle.
 *
 * Exit code 1 when a chain disagrees or leaks.
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { normJson } from './lib/harness.mjs';
import { worker } from './lib/impls.mjs';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(name); return i === -1 ? fallback : argv[i + 1]; };
const triples = argv.includes('--triples');
const maxShown = Number(opt('--max', 40));

const MARKERS = ['alias', 'calc', 'clamp:1:5', 'default:7', 'env', 'env:default:7', 'fallback:alt.synx', 'format:%05.1f', 'i18n', 'include',
  'join:,', 'map:sel', 'once', 'prompt:x', 'random', 'ref', 'replace:a:b', 'round:1', 'secret', 'sort', 'spam:2:60', 'split:,', 'sum',
  'unique', 'version:>=:1.0', 'watch', 'inherit', 'import', 'geo'];
// The text the documents mark `:secret`; it must not come out anywhere. A
// chain without `:secret` gets PLAIN in its own value, files and environment,
// and can meet NEEDLE only through the helper keys, which are all `:secret`.
const NEEDLE = 'needle7q';
const PLAIN = 'plain5w';
function scratchBase(word) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-pairs-'));
  fs.writeFileSync(path.join(dir, 'inc.synx'), `x ${word}\ny 2\n`);
  fs.writeFileSync(path.join(dir, 'alt.synx'), 'z 3\n');
  fs.writeFileSync(path.join(dir, 'data.json'), JSON.stringify({ a: word, n: 4 }));
  return dir;
}
const bases = { [NEEDLE]: scratchBase(NEEDLE), [PLAIN]: scratchBase(PLAIN) };

const HELPERS = `base\n  hb 1\n  hs:secret ${NEEDLE}\nsrc:secret ${NEEDLE}\nsel a\nnum 3\n`;
const VALUES = [
  ['word', (w) => ` ${w}`],
  ['number', () => ' 2.25'],
  ['name', () => ' src'],
  ['env name', () => ' V'],
  ['file', () => ' inc.synx'],
  ['json file', () => ' data.json'],
  ['list', (w) => `\n  - ${w}\n  - b,c\n  - 3`],
  ['group', (w) => `\n  a ${w}\n  b 2`],
  ['map table', (w) => `\n  - a ${w}\n  - b no`],
  ['parent name', () => ' base'],
  ['none', () => ''],
];

const rust = worker('rust');
const ts = worker('ts');
const show = (r) => (!r ? 'no answer' : r.timeout ? 'TIMEOUT' : r.crash ? `CRASH ${r.crash}` : r.panic !== undefined ? `PANIC ${r.panic}` : r.thrown ? `THROWN ${r.name}: ${r.err}` : !r.ok ? `ERR ${r.err}` : null);

const chains = [];
for (const a of MARKERS) for (const b of MARKERS) if (a !== b) chains.push([a, b]);
if (triples) for (const a of MARKERS) for (const b of MARKERS) if (a !== b && a !== 'secret' && b !== 'secret') chains.push(['secret', a, b], [a, 'secret', b], [a, b, 'secret']);

const groups = new Map();
let runs = 0;
for (const chain of chains) {
  for (const [vname, valueOf] of VALUES) {
    const key = `k:${chain.join(':')}`;
    const marked = chain.includes('secret');
    const word = marked ? NEEDLE : PLAIN;
    const doc = `!active\n${HELPERS}${key}${valueOf(word)}\nafter:alias k\n`;
    const req = { op: 'sinks', text: doc, needle: NEEDLE, env: { V: word, N: '12' }, base: bases[word] };
    const [a, b] = await Promise.all([rust.call(req, 20000), ts.call(req, 20000)]);
    runs++;
    const fa = show(a), fb = show(b);
    let kind = null, detail = '';
    if (fa || fb) { kind = `failure rust=${fa ? fa.slice(0, 40) : 'ok'} ts=${fb ? fb.slice(0, 40) : 'ok'}`; detail = `${fa || ''} | ${fb || ''}`; }
    else {
      const ja = normJson(a.out.json), jb = normJson(b.out.json);
      // `:random` and `:once` differ by design; compare only the shape.
      const nd = chain.some((m) => m.startsWith('random') || m === 'once');
      const strip = (s) => (nd ? s.replace(/"[0-9a-f-]{36}"/g, '"U"').replace(/"k":[^,}]+/g, '"k":X').replace(/"after":[^,}]+/g, '"after":X') : s);
      if (strip(ja) !== strip(jb)) { kind = 'disagree'; detail = `rust ${ja.slice(0, 200)}\n          ts   ${jb.slice(0, 200)}`; }
      // `.synxb` keeps a secret's text for the reader (decompile masks it).
      const leaks = (r) => (r.out.hits || []).filter((h) => !h.startsWith('synxb_'));
      const la = leaks(a), lb = leaks(b);
      if (la.length || lb.length) { kind = `leak rust=${la.length ? 'yes' : 'no'} ts=${lb.length ? 'yes' : 'no'}`; detail = `rust ${ja.slice(0, 160)} ${JSON.stringify(la.slice(0, 3))}\n          ts   ${jb.slice(0, 160)} ${JSON.stringify(lb.slice(0, 3))}`; }
    }
    if (!kind) continue;
    const gk = `${kind} | ${chain.join(':')}`;
    const g = groups.get(gk) || { count: 0, doc, detail, vname };
    g.count++;
    groups.set(gk, g);
  }
}
rust.close();
ts.close();
for (const dir of Object.values(bases)) fs.rmSync(dir, { recursive: true, force: true });

fs.writeFileSync(new URL('./tmp/markers-findings.json', import.meta.url), JSON.stringify([...groups.entries()].map(([k, g]) => ({ key: k, ...g })), null, 1));
console.log(`${chains.length} chains × ${VALUES.length} values = ${runs} documents; ${groups.size} chain classes differ or leak (all in tmp/markers-findings.json)`);
const byKind = new Map();
for (const [k] of groups) { const kind = k.split(' | ')[0]; byKind.set(kind, (byKind.get(kind) || 0) + 1); }
for (const [k, n] of byKind) console.log(`  ${n} chains: ${k}`);
let shown = 0;
for (const [k, g] of [...groups.entries()].sort((x, y) => (x[0].startsWith('leak') ? -1 : 1) - (y[0].startsWith('leak') ? -1 : 1))) {
  if (shown++ >= maxShown) break;
  console.log(`\n${k}  (${g.count} of ${VALUES.length} values; e.g. ${g.vname})\n    ${JSON.stringify(g.doc.slice(HELPERS.length + 9))}\n          ${g.detail}`);
}
process.exit(groups.size ? 1 : 0);
