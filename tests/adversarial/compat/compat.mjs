#!/usr/bin/env node
/**
 * Backward-compatibility check: every real document must parse exactly as the
 * released version parsed it.
 *
 *   node tests/adversarial/compat/compat.mjs snapshot   record what the release does
 *   node tests/adversarial/compat/compat.mjs check      compare the working tree with it
 *
 * The corpus is the conformance cases and examples of this repository plus
 * whatever `tests/adversarial/local.json` lists under "corpus" (see
 * lib/impls.mjs). Documents are copied to a scratch directory first, so a
 * marker that writes (`:once`) never touches the originals.
 *
 * Per document the snapshot holds, for the released TypeScript package and for
 * synx-core at the release commit:
 *   parse            the static tree — canonical JSON and JSON in document order
 *   active:empty     the resolved tree with no environment variables set
 *   active:num       … with every variable the corpus names set to 4242
 *   active:text      … set to `v-<name>`
 *   format           canonical text from `format`
 * A document whose resolved tree changes from run to run (`:random`, `:once`)
 * is recorded as non-deterministic and only that fact is compared.
 * The host environment is never passed to either implementation.
 *
 * `check` compares TypeScript with TypeScript and Rust with Rust, byte for
 * byte, so a change of key order or of how a number is written shows up too.
 * It then lists where the two working-tree implementations disagree with each
 * other, marking what the release already disagreed on.
 *
 * Options:
 *   --impl ts,rust       what to check (default: both)
 *   --filter TEXT        only documents whose name contains TEXT
 *   --max N              differences printed per implementation (default 20)
 *   --rust-ref REV       release commit for the Rust reference (default: HEAD)
 *   --snapshot-dir DIR   where the snapshot lives (default: local.json
 *                        "snapshotDir", else tests/adversarial/tmp/compat)
 *
 * A difference that is a deliberate bug fix is recorded in `accepted.json` in
 * the snapshot directory:
 *   [ { "impl": "*", "file": "config/app.synx", "op": "active:empty",
 *       "detail": "→ null$", "reason": "why this change is right" } ]
 * `*` matches anything, `file` matches as a substring, `op` may end in `*`.
 * `detail` is a regular expression over one line of the report; with it, a
 * result is accepted only when every changed value in it matches some rule —
 * a rule cannot wave through an unrelated change in the same document.
 * Accepted differences are counted but do not fail the check.
 *
 * Exit code: 0 no unaccepted difference, 1 otherwise, 2 usage or setup error.
 */
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { REPO, TMP, TS_WORKER, Worker, b64, ensureDir, isFailure, normJson, verdict } from '../lib/harness.mjs';
import { command, localConfig } from '../lib/impls.mjs';

const argv = process.argv.slice(2);
const mode = argv[0];
if (!['snapshot', 'check'].includes(mode)) {
  process.stdout.write(fs.readFileSync(new URL(import.meta.url), 'utf-8').split('*/')[0].replace(/^#!.*\n\/\*\*\n?/, '').replace(/^ \* ?/gm, ''));
  process.exit(2);
}
const opt = (name, fallback) => {
  const i = argv.indexOf(name);
  return i === -1 ? fallback : argv[i + 1];
};
const local = localConfig();
const impls = opt('--impl', 'ts,rust').split(',');
const filter = opt('--filter', '');
const maxShown = Number(opt('--max', '20'));
const rustRef = opt('--rust-ref', local.referenceRustCommit || 'HEAD');
const snapDir = ensureDir(opt('--snapshot-dir', local.snapshotDir || path.join(TMP, 'compat')));

// ─── Corpus ──────────────────────────────────────────────────

function walk(dir, rel = '') {
  const out = [];
  let entries;
  try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { return out; }
  for (const e of entries) {
    if (e.name === 'node_modules' || e.name === '.git' || e.name === 'target') continue;
    const r = rel ? `${rel}/${e.name}` : e.name;
    if (e.isDirectory()) out.push(...walk(path.join(dir, e.name), r));
    else if (e.isFile()) out.push(r);
  }
  return out;
}

const roots = [
  { name: 'conformance', root: path.join(REPO, 'tests', 'conformance', 'cases'), match: '[.]synx$' },
  { name: 'examples', root: path.join(REPO, 'examples'), match: '[.]synx$' },
  ...(local.corpus || []),
];

const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-compat-'));
process.on('exit', () => { try { fs.rmSync(scratch, { recursive: true, force: true }); } catch { /* scratch only */ } });

const docs = [];
for (const r of roots) {
  const re = new RegExp(r.match || '[.]synx$');
  for (const rel of walk(r.root).filter((f) => re.test(f)).sort()) {
    const name = `${r.name}/${rel}`;
    if (filter && !name.includes(filter)) continue;
    const source = path.join(r.root, rel);
    let bytes;
    try { bytes = fs.readFileSync(source); } catch { continue; }
    const copy = path.join(scratch, r.name, rel);
    ensureDir(path.dirname(copy));
    fs.writeFileSync(copy, bytes);
    docs.push({ name, bytes, source, copy, sha256: crypto.createHash('sha256').update(bytes).digest('hex') });
  }
}
// Files a document may include sit next to it: copy the rest of its directory.
for (const dir of new Set(docs.map((d) => path.dirname(d.source)))) {
  const target = path.dirname(docs.find((d) => path.dirname(d.source) === dir).copy);
  let entries = [];
  try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { /* gone meanwhile */ }
  for (const e of entries) {
    const copy = path.join(target, e.name);
    try {
      if (e.isFile() && /[.](synx|json|txt)$/.test(e.name) && !fs.existsSync(copy) && fs.statSync(path.join(dir, e.name)).size < (1 << 20)) {
        fs.copyFileSync(path.join(dir, e.name), copy);
      }
    } catch { /* gone meanwhile */ }
  }
}
if (docs.length === 0) {
  console.error('no documents in the corpus');
  process.exit(2);
}

/** Names read through `:env` anywhere in the corpus. */
function envNames() {
  const names = new Set();
  for (const d of docs) {
    for (const line of d.bytes.toString('utf-8').split('\n')) {
      const t = line.trim().replace(/^- /, '');
      const m = /^[^\s:[(]+(?:\([^)]*\))?(?:\[[^\]]*\])?:(\S+)\s+(\S+)/.exec(t);
      if (m && m[1].split(':').includes('env')) names.add(m[2]);
    }
  }
  return [...names].sort();
}
const names = envNames();
const ENVS = {
  empty: {},
  num: Object.fromEntries(names.map((n) => [n, '4242'])),
  text: Object.fromEntries(names.map((n) => [n, `v-${n.toLowerCase()}`])),
};

// ─── Running one implementation over the corpus ──────────────

function cell(resp, fields) {
  if (isFailure(resp) || !resp.ok) return { fail: verdict(resp) };
  const out = {};
  for (const f of fields) out[f] = resp.out[f];
  return out;
}

async function runAll(label, cmd, args, subset = docs) {
  const w = new Worker(label, cmd, args);
  const results = {};
  for (const d of subset) {
    const base = path.dirname(d.copy);
    const input = d.bytes.toString('base64');
    const r = {};
    const parsed = await w.call({ op: 'parse', b64: input }, 60000);
    r.parse = cell(parsed, ['json', 'ordered', 'active', 'locked', 'tool', 'schema', 'llm']);
    if (parsed.ok && parsed.out.active) {
      // A document that draws random numbers or mints an id has no single
      // answer to compare; that fact is all that is recorded for it.
      const random = /:random\b|:once\b|\(random(?::\w+)?\)/.test(d.bytes.toString('utf-8'));
      for (const [envName, env] of Object.entries(ENVS)) {
        const runs = [];
        for (let k = 0; k < 2 && !random; k++) {
          runs.push(cell(await w.call({ op: 'active', b64: input, env, base }, 60000), ['json', 'ordered']));
          // `:once` leaves a lock file next to the copy; each run starts without it.
          fs.rmSync(path.join(base, '.synx.lock'), { force: true });
        }
        r[`active:${envName}`] = !random && JSON.stringify(runs[0]) === JSON.stringify(runs[1]) ? runs[0] : { nondeterministic: true };
      }
    }
    if (parsed.ok && parsed.out.tool) {
      r.tool = cell(await w.call({ op: 'tool', b64: input, env: {}, base }, 60000), ['json']);
    }
    r.format = cell(await w.call({ op: 'format', b64: input }, 60000), ['text']);
    results[d.name] = r;
  }
  w.close();
  return results;
}

/** The Rust worker built against synx-core as it was at `rev`. */
function buildRustAt(rev) {
  const dir = path.join(TMP, 'rust-ref');
  fs.rmSync(dir, { recursive: true, force: true });
  ensureDir(dir);
  const sha = spawnSync('git', ['-C', REPO, 'rev-parse', rev], { encoding: 'utf-8' }).stdout.trim();
  const tar = path.join(dir, 'core.tar');
  const ar = spawnSync('git', ['-C', REPO, 'archive', '--format=tar', '-o', tar, rev, 'crates/synx-core', 'Cargo.lock'], { encoding: 'utf-8' });
  if (ar.status !== 0) throw new Error(`git archive ${rev} failed: ${ar.stderr}`);
  // Relative name and cwd: GNU tar reads `C:\…` as a remote host.
  const ex = spawnSync('tar', ['-xf', 'core.tar'], { encoding: 'utf-8', cwd: dir });
  if (ex.status !== 0) throw new Error(`tar failed: ${ex.stderr}`);
  ensureDir(path.join(dir, 'fuzz', 'src'));
  fs.copyFileSync(path.join(REPO, 'fuzz', 'Cargo.toml'), path.join(dir, 'fuzz', 'Cargo.toml'));
  fs.copyFileSync(path.join(REPO, 'fuzz', 'src', 'main.rs'), path.join(dir, 'fuzz', 'src', 'main.rs'));
  fs.copyFileSync(path.join(dir, 'Cargo.lock'), path.join(dir, 'fuzz', 'Cargo.lock'));
  const target = path.join(process.env.CARGO_TARGET_DIR || path.join(REPO, 'fuzz', 'target'), 'ref');
  const r = spawnSync('cargo', ['build', '--release', '--offline', '--manifest-path', path.join(dir, 'fuzz', 'Cargo.toml')], {
    stdio: ['ignore', 'inherit', 'inherit'],
    env: { ...process.env, CARGO_TARGET_DIR: target },
  });
  if (r.status !== 0) throw new Error(`building the Rust reference at ${rev} failed`);
  return { exe: path.join(target, 'release', 'advharness' + (process.platform === 'win32' ? '.exe' : '')), sha };
}

function packageVersion(dist) {
  try {
    const p = JSON.parse(fs.readFileSync(path.join(dist, '..', 'package.json'), 'utf-8'));
    return `${p.name}@${p.version}`;
  } catch { return 'unknown'; }
}

const snapFile = path.join(snapDir, 'snapshot.json');

// ─── snapshot ────────────────────────────────────────────────

if (mode === 'snapshot') {
  const [refCmd, refArgs] = command('ref');
  const dist = refArgs[1];
  console.log(`reference TypeScript: ${packageVersion(dist)}  (${dist})`);
  const ts = await runAll('ref-ts', refCmd, refArgs);
  let rust = null;
  let rustSha = null;
  try {
    const built = buildRustAt(rustRef);
    rustSha = built.sha;
    console.log(`reference Rust: synx-core at ${rustSha.slice(0, 12)}`);
    rust = await runAll('ref-rust', built.exe, []);
  } catch (e) {
    console.log(`reference Rust not built: ${e.message}`);
  }
  const snapshot = {
    meta: {
      created: new Date().toISOString(),
      ts: packageVersion(dist),
      rustCommit: rustSha,
      node: process.version,
      envNames: names,
      documents: docs.length,
    },
    files: Object.fromEntries(docs.map((d) => [d.name, {
      sha256: d.sha256,
      bytes: d.bytes.length,
      ts: ts[d.name],
      rust: rust ? rust[d.name] : undefined,
    }])),
  };
  fs.writeFileSync(snapFile, JSON.stringify(snapshot));
  const fails = docs.flatMap((d) => ['ts', 'rust'].flatMap((k) => Object.entries(snapshot.files[d.name][k] || {})
    .filter(([, c]) => c.fail).map(([op, c]) => `${k} ${d.name} ${op}: ${c.fail}`)));
  console.log(`snapshot of ${docs.length} documents written to ${snapFile} (${(fs.statSync(snapFile).size / 1048576).toFixed(1)} MiB)`);
  if (fails.length) console.log(`the release itself fails on ${fails.length} operations:\n  ${fails.slice(0, 20).join('\n  ')}`);
  process.exit(0);
}


// ─── check ───────────────────────────────────────────────────

if (!fs.existsSync(snapFile)) {
  console.error(`no snapshot at ${snapFile} — run "compat.mjs snapshot" first`);
  process.exit(2);
}
const snapshot = JSON.parse(fs.readFileSync(snapFile, 'utf-8'));
const acceptedFile = path.join(snapDir, 'accepted.json');
const accepted = fs.existsSync(acceptedFile) ? JSON.parse(fs.readFileSync(acceptedFile, 'utf-8')) : [];
const ruleFits = (a, impl, file, op) =>
  (a.impl === '*' || a.impl === impl)
  && (a.op === '*' || a.op === op || (a.op.endsWith('*') && op.startsWith(a.op.slice(0, -1))))
  && (a.file === '*' || file.includes(a.file));
/** The rule covering every changed value of one result, if the rules cover them all. */
function acceptedBy(impl, file, op, details) {
  const fits = accepted.filter((a) => ruleFits(a, impl, file, op));
  if (fits.length === 0) return undefined;
  const whole = fits.find((a) => !a.detail);
  if (whole) return whole;
  const covers = (d) => fits.find((a) => new RegExp(a.detail).test(d));
  return details.every(covers) ? covers(details[0]) : undefined;
}

/** Every place two results part ways, one line each (at most 200). */
function differences(a, b) {
  if (typeof a !== 'string' || typeof b !== 'string') return [`${JSON.stringify(a)} → ${JSON.stringify(b)}`];
  const textual = (label) => {
    let i = 0;
    while (i < a.length && i < b.length && a[i] === b[i]) i++;
    const show = (s) => JSON.stringify(s.slice(Math.max(0, i - 30), i + 50));
    return [`${label} at character ${i}: ${show(a)} → ${show(b)}`];
  };
  let va;
  let vb;
  try { va = JSON.parse(a); vb = JSON.parse(b); } catch { return textual('text differs'); }
  const out = [];
  const walkDiff = (x, y, p) => {
    if (x === y || out.length >= 200) return;
    if (x && y && typeof x === 'object' && typeof y === 'object' && Array.isArray(x) === Array.isArray(y)) {
      const kx = Object.keys(x);
      const ky = Object.keys(y);
      for (const k of kx) {
        if (!(k in y)) out.push(`${p}.${k}: present → missing`);
        else walkDiff(x[k], y[k], `${p}.${k}`);
      }
      for (const k of ky) if (!(k in x)) out.push(`${p}.${k}: missing → ${JSON.stringify(y[k]).slice(0, 80)}`);
      const sharedX = kx.filter((k) => k in y);
      const sharedY = ky.filter((k) => k in x);
      if (sharedX.some((k, i) => k !== sharedY[i])) out.push(`${p}: key order ${JSON.stringify(kx).slice(0, 100)} → ${JSON.stringify(ky).slice(0, 100)}`);
      return;
    }
    out.push(`${p}: ${JSON.stringify(x).slice(0, 100)} → ${JSON.stringify(y).slice(0, 100)}`);
  };
  walkDiff(va, vb, '$');
  return out.length ? out : textual('same value, written differently');
}

const current = {};
for (const impl of impls) {
  const [cmd, args] = command(impl, { quiet: false });
  current[impl] = await runAll(impl, cmd, args);
}

// Documents edited since the snapshot (or new): ask the release again, live,
// when it is at hand; otherwise they are counted and skipped.
const staleDocs = docs.filter((d) => !snapshot.files[d.name] || snapshot.files[d.name].sha256 !== d.sha256);
let stale = staleDocs.length;
if (stale) {
  try {
    const [refCmd, refArgs] = command('ref');
    const liveTs = await runAll('ref-ts', refCmd, refArgs, staleDocs);
    const refExe = path.join(process.env.CARGO_TARGET_DIR || path.join(REPO, 'fuzz', 'target'), 'ref', 'release', 'advharness' + (process.platform === 'win32' ? '.exe' : ''));
    const liveRust = fs.existsSync(refExe) ? await runAll('ref-rust', refExe, [], staleDocs) : {};
    for (const d of staleDocs) snapshot.files[d.name] = { sha256: d.sha256, ts: liveTs[d.name], rust: liveRust[d.name] };
    console.log(`${stale} documents changed since the snapshot: the release was asked again for those`);
    stale = 0;
  } catch { /* no release at hand */ }
}

let unaccepted = 0;
for (const impl of impls) {
  const diffs = [];
  let compared = 0;
  for (const d of docs) {
    const snap = snapshot.files[d.name];
    if (!snap || snap.sha256 !== d.sha256) continue;
    const before = snap[impl];
    if (!before) continue;
    const now = current[impl][d.name];
    for (const op of new Set([...Object.keys(before), ...Object.keys(now)])) {
      const x = before[op] || {};
      const y = now[op] || {};
      for (const f of new Set([...Object.keys(x), ...Object.keys(y)])) {
        compared++;
        if (x[f] === y[f]) continue;
        const details = differences(x[f], y[f]);
        diffs.push({ file: d.name, op, field: f, details, acc: acceptedBy(impl, d.name, op, details) });
      }
    }
  }
  const bad = diffs.filter((x) => !x.acc);
  unaccepted += bad.length;
  console.log(`\n${impl}: ${compared} results compared with the release, ${diffs.length} differ (${diffs.length - bad.length} accepted)`);
  for (const x of bad.slice(0, maxShown)) {
    const more = x.details.length > 4 ? `\n      … ${x.details.length - 4} more places` : '';
    console.log(`  ${x.file}  ${x.op}.${x.field}\n      ${x.details.slice(0, 4).join('\n      ')}${more}`);
  }
  if (bad.length > maxShown) console.log(`  … and ${bad.length - maxShown} more`);
  const byReason = new Map();
  for (const x of diffs.filter((y) => y.acc)) byReason.set(x.acc.reason, (byReason.get(x.acc.reason) || 0) + 1);
  for (const [reason, count] of byReason) console.log(`  accepted ×${count}: ${reason}`);
}
if (stale) console.log(`\n${stale} documents changed since the snapshot or are new and were skipped — run "compat.mjs snapshot" again to cover them`);

if (impls.includes('ts') && impls.includes('rust')) {
  const cross = [];
  for (const d of docs) {
    const a = current.ts[d.name];
    const b = current.rust[d.name];
    const snap = snapshot.files[d.name] || {};
    for (const op of Object.keys(a)) {
      const f = op === 'format' ? 'text' : 'json';
      const pick = (c) => (c ? c.fail || (c.nondeterministic ? 'nondeterministic' : c[f]) : undefined);
      const x = pick(a[op]);
      const y = pick(b[op]);
      if (normJson(x) === normJson(y)) continue;
      const rx = pick(snap.ts && snap.ts[op]);
      const ry = pick(snap.rust && snap.rust[op]);
      cross.push({
        file: d.name,
        op,
        detail: differences(normJson(x), normJson(y)).slice(0, 3).join('\n      '),
        old: rx !== undefined && ry !== undefined && normJson(rx) !== normJson(ry),
      });
    }
  }
  console.log(`\nTypeScript against Rust on the same documents: ${cross.length} differences (${cross.filter((c) => c.old).length} already in the release)`);
  for (const c of cross.slice(0, maxShown)) console.log(`  ${c.old ? '(release too) ' : ''}${c.file}  ${c.op}\n      ${c.detail}`);
  if (cross.length > maxShown) console.log(`  … and ${cross.length - maxShown} more`);
}

console.log(unaccepted ? `\nFAIL: ${unaccepted} unaccepted differences from the release` : '\nOK: nothing unaccepted differs from the release');
process.exit(unaccepted ? 1 : 0);
