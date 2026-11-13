/**
 * SYNX benchmark for the JavaScript package — Node and Bun.
 *
 *   node benchmarks/bench_synx_js.js
 *   bun  benchmarks/bench_synx_js.js --impl base=<dir> --impl tree=packages/synx-js
 *   node benchmarks/bench_synx_js.js --native bindings/node --json out.json
 *   node benchmarks/bench_synx_js.js --impl base=<dir> --impl tree=<dir> --cold --set configs=<dir>
 *   node benchmarks/bench_synx_js.js --dump <dir>     write the generated inputs and stop
 *
 * Each `--impl name=<dir>` is a build of the package (a directory holding
 * `dist/index.js`). The first one is the base, the others are compared with
 * it. `--native <dir | file.node>` adds the Rust addon. `JSON.parse` of the
 * same data is the reference row.
 *
 * Inputs are `benchmarks/config.synx` plus documents generated here, so the
 * numbers can be reproduced from the repository alone. More can be named:
 *
 *   --file name=<path>             one document (`.synx`, or `.synxl` for the record reader)
 *   --set  name=<dir>[::<regex>]   every `.synx` under <dir> whose relative path matches,
 *                                  read one after another as a single operation
 *   --binary name=<path>           a `.synx` document compiled to `.synxb` and written back
 *
 * Variables a document reads with `:env` are given fixed values (a number
 * where the key says `type:int`), never the environment of this process.
 *
 * How a figure is taken
 * ---------------------
 * The machine is rarely idle, so no row is timed on its own. After a warm-up
 * every row of a case runs one timed batch per round, the rows taking turns in
 * a different order each round; a row's figure is the median of its rounds,
 * shown with the fastest and the slowest. The change against the base is the
 * median of the per-round ratios — both sides of a ratio ran within the same
 * few milliseconds — shown with the middle half of those ratios and the number
 * of rounds that agree on the direction. It is marked `noise` when the middle
 * half spans zero or fewer than 85 % of the rounds agree.
 *
 * A build whose result differs from the last one's is marked `other result`:
 * a parser that drops part of a document is not faster for it.
 *
 * `--cold` measures what a service pays once at start: a new process per
 * sample loads the package and reads one document, the builds taking turns.
 *
 * Options: --rounds N (9)  --batch-ms N (50)  --samples N (cold, 15)
 *          --only TEXT     --json FILE
 */

'use strict';

const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');

const hr = () => Number(process.hrtime.bigint());

// ── the child of --cold: one process, one load, one read ────────────────────

if (process.argv[2] === '--cold-child') {
  const job = JSON.parse(fs.readFileSync(process.argv[4], 'utf-8'));
  const texts = job.docs.map((file) => fs.readFileSync(file, 'utf-8'));
  const t0 = hr();
  const mod = require(path.join(process.argv[3], 'dist', 'index.js'));
  const Synx = mod.Synx || mod.default || mod;
  const t1 = hr();
  let keys = 0;
  for (const text of texts) keys += Object.keys(Synx.parse(text, { env: job.env })).length;
  const t2 = hr();
  process.stdout.write(JSON.stringify({ loadNs: t1 - t0, parseNs: t2 - t1, keys }) + '\n');
  process.exit(0);
}

// ── arguments ───────────────────────────────────────────────────────────────

const args = {
  impl: [], native: null, rounds: 9, batchMs: 50, samples: 15, json: null, only: null, dump: null,
  cold: false, files: [], sets: [], binaries: [],
};
for (let i = 2; i < process.argv.length; i++) {
  const a = process.argv[i];
  if (a === '--impl') args.impl.push(process.argv[++i]);
  else if (a === '--native') args.native = process.argv[++i];
  else if (a === '--rounds') args.rounds = Number(process.argv[++i]);
  else if (a === '--batch-ms') args.batchMs = Number(process.argv[++i]);
  else if (a === '--samples') args.samples = Number(process.argv[++i]);
  else if (a === '--json') args.json = process.argv[++i];
  else if (a === '--only') args.only = process.argv[++i];
  else if (a === '--dump') args.dump = process.argv[++i];
  else if (a === '--cold') args.cold = true;
  else if (a === '--file') args.files.push(process.argv[++i]);
  else if (a === '--set') args.sets.push(process.argv[++i]);
  else if (a === '--binary') args.binaries.push(process.argv[++i]);
  else {
    console.error(`unknown argument: ${a}`);
    process.exit(2);
  }
}
if (args.impl.length === 0) args.impl.push('tree=' + path.resolve(__dirname, '..', 'packages', 'synx-js'));

/** `name=value` → [name, value]; a bare value is named after its last path segment. */
function named(spec) {
  const eq = spec.indexOf('=');
  return eq === -1 ? [path.basename(spec), spec] : [spec.slice(0, eq), spec.slice(eq + 1)];
}

const impls = args.impl.map((spec) => {
  const [name, dir] = named(spec);
  const abs = path.resolve(dir);
  const mod = require(path.join(abs, 'dist', 'index.js'));
  return { name, dir: abs, Synx: mod.Synx || mod.default || mod };
});

let native = null;
if (args.native) {
  try { native = require(path.resolve(args.native)); } catch (e) { console.error(`native addon not loaded: ${e.message}`); }
}

// ── inputs ──────────────────────────────────────────────────────────────────

/** A small deterministic generator, so every run builds the same documents. */
function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const WORDS = ['alpha', 'bravo', 'charlie', 'delta', 'echo', 'foxtrot', 'golf', 'hotel', 'india', 'juliet',
  'kilo', 'lima', 'mike', 'november', 'oscar', 'papa', 'quebec', 'romeo', 'sierra', 'tango'];

/** Sections of scalar keys, like a service config. */
function genConfig(sections, keys) {
  const r = rng(1);
  let out = '# generated\n';
  for (let s = 0; s < sections; s++) {
    out += `section_${s}\n`;
    for (let k = 0; k < keys; k++) {
      const kind = k % 5;
      const name = `${WORDS[(s + k) % WORDS.length]}_${k}`;
      if (kind === 0) out += `  ${name} ${Math.floor(r() * 100000)}\n`;
      else if (kind === 1) out += `  ${name} ${(r() * 1000).toFixed(3)}\n`;
      else if (kind === 2) out += `  ${name} ${r() < 0.5 ? 'true' : 'false'}\n`;
      else if (kind === 3) out += `  ${name} https://${WORDS[k % WORDS.length]}.example.com/api/v${k}  # endpoint\n`;
      else out += `  ${name} ${WORDS[(k * 7) % WORDS.length]} ${WORDS[(k * 3) % WORDS.length]} value\n`;
    }
  }
  return out;
}

/** A list of objects, like a catalog or a balance table; `nested` puts a list inside each item. */
function genCatalog(items, nested) {
  const r = rng(2);
  let out = 'catalog\n';
  for (let i = 0; i < items; i++) {
    out += `  - id item_${i}\n`;
    out += `    title ${WORDS[i % WORDS.length]} ${WORDS[(i * 3) % WORDS.length]} ${i}\n`;
    out += `    price ${Math.floor(r() * 10000)}\n`;
    out += `    weight ${(r() * 50).toFixed(2)}\n`;
    out += `    enabled ${i % 3 === 0 ? 'false' : 'true'}\n`;
    if (nested) {
      out += '    tags\n';
      out += `      - ${WORDS[i % WORDS.length]}\n`;
      out += `      - ${WORDS[(i + 5) % WORDS.length]}\n`;
    } else {
      out += `    tag ${WORDS[i % WORDS.length]}\n`;
    }
  }
  return out;
}

/** Long multiline text values, like a prompt library. */
function genPrompts(blocks, lines) {
  let out = '';
  for (let b = 0; b < blocks; b++) {
    out += `prompt_${b} |\n`;
    for (let l = 0; l < lines; l++) {
      out += `  ${WORDS[(b + l) % WORDS.length]} ${WORDS[(b * l) % WORDS.length]} — line ${l} of the block, with a # hash and // slashes kept as text.\n`;
    }
    out += `title_${b} ${WORDS[b % WORDS.length]} prompt\n`;
  }
  return out;
}

/** An active document: markers, constraints, interpolation. */
function genActive(sections) {
  let out = '!active\napp_name[required, min:3, max:30] Benchmark\nbase_hp 100\nbase_rate 1.5\n';
  for (let s = 0; s < sections; s++) {
    out += `svc_${s}\n`;
    out += `  port[type:int, min:1, max:65535]:env:default:${7000 + s} SVC_${s}_PORT\n`;
    out += `  host:env:default:0.0.0.0 SVC_${s}_HOST\n`;
    out += `  token[required]:secret:env SVC_${s}_TOKEN\n`;
    out += `  workers[type:int, min:1, max:32] ${1 + (s % 8)}\n`;
    out += '  boss_hp:calc base_hp * 5\n';
    out += `  url http://{svc_${s}.host}:{svc_${s}.port}/api\n`;
    out += '  mode[enum:dev|staging|prod] prod\n';
  }
  return out;
}

/**
 * An active document whose keys carry `[pattern:…]` — the constraint that used to run on `RegExp`.
 * No comma in a pattern: constraints are split at commas, and what is left of `{2,}` does not compile.
 */
function genPatterns(keys) {
  let out = '!active\n';
  for (let k = 0; k < keys; k++) {
    out += `slug_${k}[pattern:^[a-z0-9-]+$] ${WORDS[k % WORDS.length]}-${k}\n`;
    out += `mail_${k}[pattern:^[^@\\s]+@[^@\\s]+\\.[a-z]+$] ${WORDS[k % WORDS.length]}@example.com\n`;
    out += `port_${k}[type:int, min:1, max:65535] ${7000 + k}\n`;
  }
  return out;
}

/** A SYNXL record stream: a typed header and rows of five fields. */
function genSynxl(rows) {
  const r = rng(3);
  let out = '!synxl 1\n!fields id[type:int] ; name ; price[type:float] ; enabled[type:bool] ; note\n';
  for (let i = 0; i < rows; i++) {
    out += `${i} ; ${WORDS[i % WORDS.length]} ${i} ; ${(r() * 1000).toFixed(2)} ; ${i % 3 === 0 ? 'false' : 'true'} ; `
      + `${WORDS[(i * 7) % WORDS.length]} ${WORDS[(i * 3) % WORDS.length]} note\n`;
  }
  return out;
}

/**
 * Fixed values for every variable `text` reads with `:env`: a number where the
 * key declares `type:int` / `type:float`, `true` for `type:bool`, text otherwise.
 */
function envFor(texts) {
  const env = {};
  const line = /^\s*(?:-\s+)?[^\s[(:]+(?:\(([^)]*)\))?(\[[^\]]*\])?:(\S+)[ \t]+(\S+)/;
  for (const text of texts) {
    if (text.indexOf(':env') === -1) continue;
    for (const raw of text.split('\n')) {
      const m = line.exec(raw);
      if (!m || !m[3].split(':').includes('env')) continue;
      const type = m[1] || (/type:\s*(\w+)/.exec(m[2] || '') || [])[1] || '';
      env[m[4]] = type === 'int' ? '4242' : type === 'float' ? '1.5' : type === 'bool' ? 'true' : `v-${m[4]}`;
    }
  }
  return env;
}

/** Every file under `dir` whose path relative to it matches `pattern`, sorted. */
function walk(dir, pattern, rel = '') {
  const out = [];
  let entries;
  try { entries = fs.readdirSync(path.join(dir, rel), { withFileTypes: true }); } catch { return out; }
  for (const e of entries) {
    const child = rel ? `${rel}/${e.name}` : e.name;
    if (e.isDirectory()) {
      if (e.name !== 'node_modules' && e.name !== '.git') out.push(...walk(dir, pattern, child));
    } else if (pattern.test(child)) out.push(child);
  }
  return out.sort();
}

function sizeLabel(chars) {
  return chars < 10240 ? `${(chars / 1024).toFixed(1)} KB` : `${Math.round(chars / 1024)} KB`;
}

/** A case is a list of documents read one after another as one operation. */
const cases = [];
function addDocs(name, texts, kind, files) {
  const chars = texts.reduce((n, t) => n + t.length, 0);
  cases.push({ name: `${name} — ${texts.length > 1 ? `${texts.length} documents, ` : ''}${sizeLabel(chars)}`, kind, texts, chars, files });
}

addDocs('config (benchmarks/config.synx)', [fs.readFileSync(path.join(__dirname, 'config.synx'), 'utf-8')], 'synx');
addDocs('config, sections of scalars', [genConfig(110, 28)], 'synx');
addDocs('catalog, list of objects', [genCatalog(3300, false)], 'synx');
addDocs('catalog, a list in each object', [genCatalog(3300, true)], 'synx');
addDocs('prompts, multiline blocks', [genPrompts(60, 55)], 'synx');
addDocs('active, markers and constraints', [genActive(12)], 'synx');
addDocs('active, markers and constraints, large', [genActive(240)], 'synx');
addDocs('active, [pattern] on every other key', [genPatterns(100)], 'synx');
addDocs('synxl, typed records', [genSynxl(4000)], 'synxl');
addDocs('synxb of the catalog', [genCatalog(3300, false)], 'synxb');

if (args.dump) {
  // The generated inputs, written out for a profiler or another implementation.
  fs.mkdirSync(args.dump, { recursive: true });
  cases.forEach((c, i) => fs.writeFileSync(path.join(args.dump, `case-${i + 1}.${c.kind === 'synxl' ? 'synxl' : 'synx'}`), c.texts[0]));
  console.log(`inputs written to ${args.dump}`);
  process.exit(0);
}

for (const spec of args.files) {
  const [name, file] = named(spec);
  addDocs(name, [fs.readFileSync(file, 'utf-8')], file.toLowerCase().endsWith('.synxl') ? 'synxl' : 'synx', [path.resolve(file)]);
}
for (const spec of args.sets) {
  const [name, rest] = named(spec);
  const cut = rest.indexOf('::');
  const dir = path.resolve(cut === -1 ? rest : rest.slice(0, cut));
  const files = walk(dir, new RegExp(cut === -1 ? '[.]synx$' : rest.slice(cut + 2))).map((f) => path.join(dir, f));
  if (files.length === 0) {
    console.error(`--set ${name}: no document under ${dir}`);
    process.exit(2);
  }
  addDocs(name, files.map((f) => fs.readFileSync(f, 'utf-8')), 'synx', files);
}
for (const spec of args.binaries) {
  const [name, file] = named(spec);
  addDocs(`synxb of ${name}`, [fs.readFileSync(file, 'utf-8')], 'synxb', [path.resolve(file)]);
}

// ── statistics ──────────────────────────────────────────────────────────────

function median(values) {
  const s = values.slice().sort((x, y) => x - y);
  const mid = s.length >> 1;
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
}

function summary(values) {
  return { median: median(values), min: Math.min(...values), max: Math.max(...values) };
}

function fmt(ns) {
  if (ns < 1e3) return `${ns.toFixed(0)} ns`;
  if (ns < 1e6) return `${(ns / 1e3).toFixed(ns < 1e5 ? 2 : 1)} µs`;
  return `${(ns / 1e6).toFixed(2)} ms`;
}

function pct(ratio) {
  const p = (ratio - 1) * 100;
  return `${p > 0 ? '+' : ''}${p.toFixed(Math.abs(p) < 10 ? 1 : 0)} %`;
}

/**
 * The change `candidate / base`, round by round: the median ratio, the middle
 * half of the ratios, and the share of rounds on the side of the median.
 */
function change(base, candidate) {
  const ratios = base.map((b, i) => candidate[i] / b).sort((x, y) => x - y);
  const at = (p) => ratios[Math.round(p * (ratios.length - 1))];
  const faster = ratios.filter((r) => r < 1).length;
  const agree = Math.max(faster, ratios.length - faster);
  const q1 = at(0.25);
  const q3 = at(0.75);
  return {
    median: median(ratios), q1, q3, min: ratios[0], max: ratios[ratios.length - 1],
    agree, rounds: ratios.length, noise: agree / ratios.length < 0.85 || (q1 <= 1 && q3 >= 1),
  };
}

function changeText(c) {
  return `${pct(c.median)}  (${pct(c.q1)} … ${pct(c.q3)}, ${c.agree}/${c.rounds})${c.noise ? '  noise' : ''}`;
}

const runtime = typeof Bun !== 'undefined' ? `bun ${Bun.version}` : `node ${process.version}`;
const report = { runtime, platform: process.platform, arch: process.arch, cpus: os.cpus().length, mode: args.cold ? 'cold' : 'hot', cases: [] };

// ── cold start ──────────────────────────────────────────────────────────────

function coldSample(impl, jobFile) {
  const r = spawnSync(process.execPath, [__filename, '--cold-child', impl.dir, jobFile], { encoding: 'utf-8', windowsHide: true });
  if (r.status !== 0) throw new Error(`cold sample failed for ${impl.name}: ${(r.stderr || '').trim().split('\n').pop()}`);
  return JSON.parse(r.stdout.trim().split('\n').pop());
}

function runCold() {
  console.log(`SYNX cold start — ${runtime}, ${process.platform} ${process.arch}; a new process per sample, ${args.samples} samples, builds in turn`);
  console.log('load = require of the package, parse = the first Synx.parse of the document\n');
  const jobFile = path.join(os.tmpdir(), `synx-bench-cold-${process.pid}.json`);
  // One process reads one document: a set is as many cases as it has files.
  const docs = [];
  for (const c of cases) {
    if (c.kind !== 'synx' || !c.files) continue;
    if (args.only && !c.name.includes(args.only)) continue;
    c.files.forEach((file, i) => docs.push({ set: c.name, file, text: c.texts[i] }));
  }
  if (docs.length === 0) {
    console.error('--cold needs documents on disk: name them with --file or --set');
    process.exit(2);
  }
  const totals = new Map();
  for (const doc of docs) {
    fs.writeFileSync(jobFile, JSON.stringify({ docs: [doc.file], env: envFor([doc.text]) }));
    const samples = impls.map(() => ({ load: [], parse: [], total: [] }));
    for (let i = 0; i < args.samples; i++) {
      // The first sample of a build only warms the file cache.
      for (let k = 0; k < impls.length; k++) {
        const at = (k + i) % impls.length;
        const s = coldSample(impls[at], jobFile);
        if (i === 0) continue;
        samples[at].load.push(s.loadNs);
        samples[at].parse.push(s.parseNs);
        samples[at].total.push(s.loadNs + s.parseNs);
      }
    }
    const rows = impls.map((impl, k) => ({
      impl: impl.name,
      load: summary(samples[k].load), parse: summary(samples[k].parse), total: summary(samples[k].total),
      totals: samples[k].total,
    }));
    report.cases.push({ name: path.basename(path.dirname(path.dirname(doc.file))) + '/' + path.basename(doc.file), set: doc.set, chars: doc.text.length, rows });
    let sum = totals.get(doc.set);
    if (!sum) totals.set(doc.set, sum = { docs: 0, chars: 0, rounds: impls.map(() => new Array(args.samples - 1).fill(0)), load: impls.map(() => 0), parse: impls.map(() => 0) });
    sum.docs++;
    sum.chars += doc.text.length;
    rows.forEach((row, k) => {
      row.totals.forEach((ns, i) => { sum.rounds[k][i] += ns; });
      sum.load[k] += row.load.median;
      sum.parse[k] += row.parse.median;
    });
    if (docs.length <= 12) {
      console.log(`${doc.file} — ${doc.text.length} chars`);
      for (const row of rows) {
        console.log(`  ${row.impl.padEnd(12)} load ${fmt(row.load.median).padStart(9)}  parse ${fmt(row.parse.median).padStart(9)}  total ${fmt(row.total.median).padStart(9)}  (${fmt(row.total.min)} … ${fmt(row.total.max)})`);
      }
      console.log();
    }
  }
  try { fs.unlinkSync(jobFile); } catch { /* scratch only */ }
  report.sets = [];
  for (const [name, sum] of totals) {
    console.log(`${name}: every document read once, each by a process of its own (${sum.docs} processes)`);
    const baseRounds = sum.rounds[0];
    impls.forEach((impl, k) => {
      const s = summary(sum.rounds[k]);
      const c = k === 0 ? null : change(baseRounds, sum.rounds[k]);
      console.log(`  ${impl.name.padEnd(12)} load ${fmt(sum.load[k]).padStart(9)}  parse ${fmt(sum.parse[k]).padStart(9)}  total ${fmt(s.median).padStart(9)}  (${fmt(s.min)} … ${fmt(s.max)})`
        + (c ? `   ${changeText(c)}` : ''));
      report.sets.push({ set: name, impl: impl.name, docs: sum.docs, chars: sum.chars, loadNs: Math.round(sum.load[k]), parseNs: Math.round(sum.parse[k]), total: s, change: c });
    });
    console.log();
  }
}

// ── steady state ────────────────────────────────────────────────────────────

let sink = 0;
const collect = typeof Bun !== 'undefined' ? () => Bun.gc(true) : typeof global.gc === 'function' ? () => global.gc() : () => {};

/** Rows of one case, timed in turn: `rounds` batches each, nanoseconds per call. */
function measureTogether(rows) {
  for (const row of rows) {
    // Warm up until the optimizing tiers have seen the code, and size the batch.
    const warmEnd = hr() + 200e6;
    let calls = 0;
    while (hr() < warmEnd) { sink ^= row.fn() ? 1 : 0; calls++; }
    row.batch = Math.max(1, Math.round((args.batchMs * 1e6) / (200e6 / Math.max(calls, 1))));
    row.ns = [];
  }
  for (let r = 0; r < args.rounds; r++) {
    for (let k = 0; k < rows.length; k++) {
      const row = rows[(k + r) % rows.length];
      collect();
      const t0 = hr();
      for (let i = 0; i < row.batch; i++) sink ^= row.fn() ? 1 : 0;
      row.ns.push((hr() - t0) / row.batch);
    }
  }
}

function buildRows(c) {
  const rows = [];
  const options = { env: envFor(c.texts) };
  const texts = c.texts;
  const each = (one) => (texts.length === 1 ? () => one(texts[0]) : () => { let last; for (let i = 0; i < texts.length; i++) last = one(texts[i]); return last; });
  if (c.kind === 'synx') {
    // The data as the last build reads it — the one under test.
    const jsonOf = (impl) => texts.map((t) => impl.Synx.toJSON(impl.Synx.parse(t, options), false));
    const jsons = jsonOf(impls[impls.length - 1]);
    rows.push({ op: 'json', label: 'JSON.parse (same data)', fn: jsons.length === 1 ? () => JSON.parse(jsons[0]) : () => { let last; for (let i = 0; i < jsons.length; i++) last = JSON.parse(jsons[i]); return last; } });
    for (const impl of impls) {
      const differs = impl !== impls[impls.length - 1] && jsonOf(impl).some((j, i) => j !== jsons[i]);
      rows.push({ op: 'parse', impl: impl.name, label: `Synx.parse — ${impl.name}`, differs, fn: each((t) => impl.Synx.parse(t, options)) });
    }
    if (native) {
      const active = texts.some((t) => /^\s*(!active|#!mode:active)\s*$/m.test(t));
      if (active) {
        rows.push({ op: 'native', label: 'addon parseActive', fn: each((t) => native.parseActive(t, options)) });
      } else {
        rows.push({ op: 'native', label: 'addon parse', fn: each((t) => native.parse(t)) });
        rows.push({ op: 'native-json', label: 'addon parseToJson', fn: each((t) => native.parseToJson(t)) });
        rows.push({ op: 'native-json-parse', label: 'addon parseToJson + JSON.parse', fn: each((t) => JSON.parse(native.parseToJson(t))) });
      }
    }
  } else if (c.kind === 'synxl') {
    for (const impl of impls) rows.push({ op: 'parse', impl: impl.name, label: `Synx.parseSynxl — ${impl.name}`, fn: each((t) => impl.Synx.parseSynxl(t)) });
    for (const impl of impls) {
      rows.push({ op: 'stream', impl: impl.name, label: `Synx.streamSynxl — ${impl.name}`, fn: each((t) => { let n = 0; for (const rec of impl.Synx.streamSynxl(t)) n += rec ? 1 : 0; return n; }) });
    }
  } else {
    for (const impl of impls) rows.push({ op: 'compile', impl: impl.name, label: `Synx.compile — ${impl.name}`, fn: each((t) => impl.Synx.compile(t)) });
    for (const impl of impls) {
      const files = texts.map((t) => impl.Synx.compile(t));
      rows.push({ op: 'decompile', impl: impl.name, label: `Synx.decompile — ${impl.name}`, fn: files.length === 1 ? () => impl.Synx.decompile(files[0]) : () => { let last; for (const f of files) last = impl.Synx.decompile(f); return last; } });
    }
    if (native) {
      rows.push({ op: 'native-compile', label: 'addon compile', fn: each((t) => native.compile(t)) });
      const files = texts.map((t) => native.compile(t));
      rows.push({ op: 'native-decompile', label: 'addon decompile', fn: () => { let last; for (const f of files) last = native.decompile(f); return last; } });
    }
  }
  return rows;
}

function runHot() {
  console.log(`SYNX benchmark — ${runtime}, ${process.platform} ${process.arch}; median of ${args.rounds} rounds (fastest … slowest), rows of a case timed in turn`);
  console.log();
  const baseName = impls[0].name;
  for (const c of cases) {
    if (args.only && !c.name.includes(args.only)) continue;
    let rows;
    try {
      rows = buildRows(c);
      measureTogether(rows);
    } catch (e) {
      console.log(`${c.name}\n  skipped: ${e.message}\n`);
      continue;
    }
    console.log(c.name);
    const out = [];
    for (const row of rows) {
      const s = summary(row.ns);
      const base = row.impl && row.impl !== baseName ? rows.find((x) => x.op === row.op && x.impl === baseName) : null;
      const c2 = base ? change(base.ns, row.ns) : null;
      const mbps = (c.chars / 1048576) / (s.median / 1e9);
      console.log(`  ${row.label.padEnd(34)} ${fmt(s.median).padStart(10)}  (${fmt(s.min)} … ${fmt(s.max)})  ${mbps.toFixed(0).padStart(5)} MB/s`
        + (c2 ? `   ${changeText(c2)}` : '') + (row.differs ? '   other result' : ''));
      out.push({ label: row.label, op: row.op, impl: row.impl, medianNs: Math.round(s.median), minNs: Math.round(s.min), maxNs: Math.round(s.max), change: c2, differs: row.differs === true });
    }
    console.log();
    report.cases.push({ name: c.name, chars: c.chars, documents: c.texts.length, rows: out });
  }
}

if (args.cold) runCold(); else runHot();

if (args.json) {
  fs.writeFileSync(args.json, JSON.stringify(report, null, 2));
  console.log(`results written to ${args.json}`);
}
if (sink === 2) console.log('');
