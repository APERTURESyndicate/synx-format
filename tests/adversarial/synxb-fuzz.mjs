#!/usr/bin/env node
/**
 * Mutation fuzzer for the `.synxb` decoder. Valid containers are compiled by
 * synx-core from the conformance documents (with and without declarations),
 * their payload is inflated, mutated (bytes flipped, inserted, deleted,
 * ranges copied, varints blown up, tags swapped, truncated), deflated again and
 * handed to both decoders.
 *
 *   node tests/adversarial/synxb-fuzz.mjs [--seconds 120] [--seed N]
 *
 * Reported:
 *   - a crash, panic, hang or untyped exception, or a decode over 1 s;
 *   - one implementation decoding a container the other refuses;
 *   - both decoding it to different trees (through the text `decompile` writes);
 *   - in Rust, the text `decompile` writes reading back as another tree, as an
 *     active document when the container is static, or with an `!include`.
 *
 * Exit code 1 when anything was found.
 */
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { REPO, normJson } from './lib/harness.mjs';
import { worker } from './lib/impls.mjs';
import { varint } from './lib/synxb.mjs';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(name); return i === -1 ? fallback : argv[i + 1]; };
const seconds = Number(opt('--seconds', 120));
let seed = Number(opt('--seed', Date.now() % 1e9));
const firstSeed = seed;
function rnd() {
  seed = (seed + 0x6d2b79f5) | 0;
  let t = seed;
  t = Math.imul(t ^ (t >>> 15), t | 1);
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}
const ri = (n) => Math.floor(rnd() * n);
const pick = (a) => a[ri(a.length)];

const rust = worker('rust');
const ts = worker('ts');

// ── Seeds ─────────────────────────────────────────────────────
function* files(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) yield* files(p);
    else if (e.name.endsWith('.synx')) yield p;
  }
}
const docs = [...files(path.join(REPO, 'tests', 'conformance'))].map((f) => fs.readFileSync(f, 'utf-8'));
docs.push(
  '!active\nport[type:int, min:1]:env:default:8080 PORT\nname(string) app\nlist:join:,\n  - a\n  - b\nsecret:secret hunter2\n',
  '!active\n!include other.synx o\ng:inherit base\nbase\n  x 1\nc:calc 2 + 3\n',
  'a\n  b\n    c\n      - 1\n      - x: 2\n',
  '!active\nm:map:sel\n  - a 1\nsel a\nk:alias m\n',
);
const seeds = [];
for (const d of docs) {
  for (const resolved of [false, true]) {
    const r = await rust.call({ op: 'compile', text: d, resolved }, 20000);
    if (r.ok) seeds.push(Buffer.from(r.out.b64, 'base64'));
  }
}

// ── Mutation ──────────────────────────────────────────────────
function mutatePayload(p) {
  let b = Buffer.from(p);
  const n = 1 + ri(4);
  for (let k = 0; k < n; k++) {
    const r = rnd();
    const at = b.length ? ri(b.length) : 0;
    if (r < 0.25 && b.length) b[at] ^= 1 << ri(8);
    else if (r < 0.35 && b.length) b[at] = pick([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0x7f, 0x80, 0xff]);
    else if (r < 0.45) b = Buffer.concat([b.subarray(0, at), Buffer.from([ri(256)]), b.subarray(at)]);
    else if (r < 0.55 && b.length) b = Buffer.concat([b.subarray(0, at), b.subarray(at + 1)]);
    else if (r < 0.65 && b.length > 2) { const len = 1 + ri(Math.min(64, b.length - at)); b = Buffer.concat([b.subarray(0, at), b.subarray(at, at + len), b.subarray(at)]); }
    else if (r < 0.75) b = Buffer.concat([b.subarray(0, at), varint(pick([2 ** 31, 2 ** 32, 2 ** 53 + 1, (1n << 63n), (1n << 64n) - 1n, 1 << 20, 255, 128])), b.subarray(at + 1)]);
    else if (r < 0.82 && b.length) b = b.subarray(0, at);
    else if (r < 0.9) b = Buffer.concat([b, Buffer.from(Array.from({ length: 1 + ri(8) }, () => ri(256)))]);
    else if (b.length > 4) { const a = ri(b.length), c = ri(b.length); const t = b[a]; b[a] = b[c]; b[c] = t; }
  }
  return b;
}

function mutate(container) {
  const head = Buffer.from(container.subarray(0, 11));
  let payload;
  try { payload = zlib.inflateRawSync(container.subarray(11)); } catch { payload = Buffer.alloc(0); }
  const p2 = rnd() < 0.9 ? mutatePayload(payload) : payload;
  if (rnd() < 0.15) head[6] = ri(256);
  const body = zlib.deflateRawSync(p2, { level: 6 });
  // Usually a header that tells the truth about the size; sometimes not.
  head.writeUInt32LE(rnd() < 0.9 ? p2.length : ri(1 << 20), 7);
  return Buffer.concat([head, body]);
}

// ── Compare ───────────────────────────────────────────────────
const failure = (r) => (!r ? 'no answer' : r.timeout ? 'TIMEOUT' : r.crash ? `CRASH ${r.crash}` : r.panic !== undefined ? `PANIC ${r.panic}` : r.thrown ? `THROWN ${r.name}: ${r.err}` : null);
const groups = new Map();
function note(kind, input, detail) {
  const g = groups.get(kind) || { count: 0, input, detail };
  g.count++;
  if (input.length < g.input.length) { g.input = input; g.detail = detail; }
  groups.set(kind, g);
}

const started = Date.now();
let runs = 0;
let decodedBoth = 0;
while (Date.now() - started < seconds * 1000) {
  const input = mutate(pick(seeds));
  const req = { op: 'decompile_full', b64: input.toString('base64') };
  const [a, b] = await Promise.all([rust.call(req, 10000), ts.call(req, 10000)]);
  runs++;
  const fa = failure(a), fb = failure(b);
  if (fa || fb) { note(`failure: rust ${fa ? fa.slice(0, 50) : 'ok'} / ts ${fb ? fb.slice(0, 50) : 'ok'}`, input, `${fa || ''} ${fb || ''}`); continue; }
  for (const [name, r] of [['rust', a], ['ts', b]]) if (r.ms > 1000) note(`${name} over 1 s`, input, `${r.ms} ms, ${r.peak_mb} MiB`);
  if (a.ok !== b.ok) {
    const err = (a.ok ? b.err : a.err) || '';
    note(`${a.ok ? 'rust decodes, ts refuses' : 'ts decodes, rust refuses'}: ${err.replace(/\d+/g, 'N').slice(0, 60)}`, input, `rust ${a.ok ? 'ok' : a.err} / ts ${b.ok ? 'ok' : b.err}`);
    continue;
  }
  if (!a.ok) continue;
  decodedBoth++;
  const o = a.out;
  const rootKind = o.tree.startsWith('{') ? 'object' : o.tree.startsWith('[') ? 'list' : 'scalar';
  if (normJson(o.tree) !== normJson(o.text_tree)) note(`rust: decompiled text reads as another tree (root ${rootKind})`, input, `${o.tree.slice(0, 120)} → ${o.text_tree.slice(0, 120)}`);
  if (o.text_active && !o.active) note(`rust: static container decompiles to an active document (root ${rootKind})`, input, o.text.slice(0, 160));
  if (o.text_includes > o.includes) note(`rust: decompiled text has more !include than the container (root ${rootKind})`, input, o.text.slice(0, 160));
  if (b.out.text_active && !o.active) note(`ts: static container decompiles to an active document (root ${rootKind})`, input, b.out.text.slice(0, 160));
  if (b.out.text_includes > o.includes) note(`ts: decompiled text has more !include than the container (root ${rootKind})`, input, b.out.text.slice(0, 160));
  if (normJson(o.text_tree) !== normJson(b.out.text_tree)) note('rust and ts decompile to different trees', input, `rust ${o.text_tree.slice(0, 100)} / ts ${b.out.text_tree.slice(0, 100)}`);
}
rust.close();
ts.close();

console.log(`seed ${firstSeed}: ${seeds.length} seed containers, ${runs} mutants in ${seconds} s, ${decodedBoth} decoded by both; ${groups.size} classes`);
const out = [];
for (const [kind, g] of [...groups.entries()].sort((x, y) => y[1].count - x[1].count)) {
  console.log(`${String(g.count).padStart(6)}  ${kind}\n        ${g.detail.replace(/\n/g, '\\n').slice(0, 220)}\n        b64 ${g.input.toString('base64').slice(0, 200)}`);
  out.push({ kind, count: g.count, detail: g.detail, b64: g.input.toString('base64') });
}
fs.writeFileSync(path.join(REPO, 'tests', 'adversarial', 'tmp', 'synxb-findings.json'), JSON.stringify(out, null, 1));
process.exit(groups.size ? 1 : 0);
