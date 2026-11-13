#!/usr/bin/env node
'use strict';
/**
 * Compatibility snapshot for @aperturesyndicate/synx-format.
 *
 * Parses a corpus of real `.synx` / `.synxl` files with one build of the
 * package and stores everything that build makes of each file. Two snapshots
 * (a published release and the working tree) are then compared entry by entry,
 * so a change that reads an existing document differently is seen before it
 * ships.
 *
 *   node scripts/compat-snapshot.js mirror   --out <corpus> --root name=<dir> [--root name=<dir> ...]
 *   node scripts/compat-snapshot.js snapshot --impl <package dir> --corpus <corpus> --out <snapshot>
 *   node scripts/compat-snapshot.js diff     <snapshot A> <snapshot B> [--allow <rules>]... [--max 40]
 *
 * Runs under Node and Bun. Nothing here is specific to a release: `--impl`
 * is any directory holding `dist/index.js` and `dist/parser.js`.
 *
 * The corpus is a copy (`mirror`), never the original tree: `:once` writes a
 * `.synx.lock` beside the document it resolves, and a copy keeps the inputs
 * fixed while other work goes on in the source directories.
 */

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const zlib = require('zlib');

const SKIP_DIRS = new Set(['node_modules', '.git', 'target', 'dist', '.godot', 'worktrees', 'bin', 'obj']);
/** A result longer than this is stored as a digest and its two ends. */
const INLINE_LIMIT = 256 * 1024;

function sha(data) {
  return crypto.createHash('sha256').update(data).digest('hex');
}

function parseArgs(argv) {
  const out = { _: [], root: [], allow: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith('--')) {
      const name = a.slice(2);
      const next = argv[i + 1];
      const value = next === undefined || next.startsWith('--') ? true : (i++, next);
      if (name === 'root' || name === 'allow') out[name].push(value);
      else out[name] = value;
    } else {
      out._.push(a);
    }
  }
  return out;
}

function walk(dir, visit) {
  let entries;
  try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { return; }
  entries.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  for (const e of entries) {
    const full = path.join(dir, e.name);
    // A link is never followed: it can leave the tree or loop back into it.
    if (e.isSymbolicLink()) continue;
    if (e.isDirectory()) {
      if (SKIP_DIRS.has(e.name) || e.name.startsWith('.next')) continue;
      walk(full, visit);
    } else if (e.isFile() && (e.name.endsWith('.synx') || e.name.endsWith('.synxl'))) {
      visit(full);
    }
  }
}

// ─── mirror ───────────────────────────────────────────────

function cmdMirror(args) {
  const out = path.resolve(String(args.out));
  const manifest = { created: new Date().toISOString(), roots: {}, files: {} };
  let count = 0;
  let bytes = 0;
  for (const spec of args.root) {
    const eq = String(spec).indexOf('=');
    if (eq === -1) throw new Error(`--root expects name=<dir>, got '${spec}'`);
    const name = spec.slice(0, eq);
    const src = path.resolve(spec.slice(eq + 1));
    manifest.roots[name] = true;
    walk(src, (file) => {
      const rel = path.relative(src, file).split(path.sep).join('/');
      const dest = path.join(out, name, rel);
      fs.mkdirSync(path.dirname(dest), { recursive: true });
      const data = fs.readFileSync(file);
      fs.writeFileSync(dest, data);
      manifest.files[`${name}/${rel}`] = { bytes: data.length, sha256: sha(data) };
      count++;
      bytes += data.length;
    });
  }
  fs.writeFileSync(path.join(out, 'MANIFEST.json'), JSON.stringify(manifest, null, 1));
  console.log(`mirror: ${count} files, ${(bytes / 1048576).toFixed(1)} MiB -> ${out}`);
}

// ─── value rendering ──────────────────────────────────────

const SECRET_BRAND = Symbol.for('synx.secret');

/**
 * The declarations (markers, constraints, type hints) a build keeps for an
 * object's keys. 3.8.0 hid them on the object as `__synx`; later builds keep
 * them beside the tree and export an accessor.
 */
let declarationsOf = (obj) => {
  const d = Object.getOwnPropertyDescriptor(obj, '__synx');
  return d && d.enumerable === false && d.value && typeof d.value === 'object' ? d.value : undefined;
};

/**
 * A parsed value as plain JSON that keeps what `JSON.stringify` loses: the
 * exact number (`-0`, `NaN`, a bigint), the text behind a secret, key order,
 * a prototype that is not `Object.prototype`, the `__synx` metadata.
 */
function render(v, withMeta, depth) {
  if (depth > 600) return { $t: 'too-deep' };
  if (v === null) return null;
  if (v === undefined) return { $t: 'undefined' };
  switch (typeof v) {
    case 'string':
    case 'boolean':
      return v;
    case 'number':
      if (Number.isNaN(v)) return { $t: 'num', v: 'NaN' };
      if (!Number.isFinite(v)) return { $t: 'num', v: v > 0 ? 'Infinity' : '-Infinity' };
      if (Object.is(v, -0)) return { $t: 'num', v: '-0' };
      return v;
    case 'bigint':
      return { $t: 'bigint', v: v.toString() };
    case 'function':
      return { $t: 'function' };
    case 'symbol':
      return { $t: 'symbol' };
  }
  if (v[SECRET_BRAND] === true || (typeof v.reveal === 'function' && v.constructor && v.constructor.name === 'SynxSecret')) {
    return { $t: 'secret', v: v.reveal(), shown: String(v), json: JSON.stringify(v) };
  }
  if (Array.isArray(v)) {
    const items = [];
    for (let i = 0; i < v.length; i++) items.push(render(v[i], withMeta, depth + 1));
    const extra = Object.keys(v).filter((k) => !/^(0|[1-9]\d*)$/.test(k));
    if (extra.length === 0) return items;
    const props = {};
    for (const k of extra) props[k] = render(v[k], withMeta, depth + 1);
    return { $t: 'array+props', items, props };
  }
  const out = {};
  const proto = Object.getPrototypeOf(v);
  if (proto !== Object.prototype) out.$proto = proto === null ? 'null' : (proto.constructor && proto.constructor.name) || 'other';
  // Entries as a list: a JSON object would reorder integer-like keys again on
  // the way back in, and an own `__proto__` key would not survive at all.
  const entries = [];
  for (const k of Object.keys(v)) {
    const desc = Object.getOwnPropertyDescriptor(v, k);
    entries.push([k, render(desc && 'value' in desc ? desc.value : v[k], withMeta, depth + 1)]);
  }
  out.$e = entries;
  if (withMeta) {
    let meta = declarationsOf(v);
    if (meta instanceof Map) {
      // Listed as the object 3.8.0 kept them in would list them.
      const byKey = Object.create(null);
      for (const [k, one] of meta) byKey[k] = one;
      meta = byKey;
    }
    if (meta) {
      const m = [];
      for (const k of Object.keys(meta)) {
        const one = meta[k];
        if (!one) continue;
        const item = { markers: one.markers };
        if (one.args !== undefined) item.args = one.args;
        if (one.constraints !== undefined) item.constraints = render(one.constraints, false, 0);
        if (one.typeHint !== undefined) item.typeHint = one.typeHint;
        m.push([k, item]);
      }
      out.$meta = m;
    }
  }
  if (Object.getOwnPropertyDescriptor(v, '__synx_locked')) out.$locked = true;
  return out;
}

/** Seconds spent per result section, for `--timing`. */
const TIMING = {};
function timed(section, fn) {
  const t0 = process.hrtime.bigint();
  try {
    return fn();
  } finally {
    TIMING[section] = (TIMING[section] || 0) + Number(process.hrtime.bigint() - t0) / 1e9;
  }
}

function attempt(fn) {
  try {
    return fn();
  } catch (e) {
    const err = { $t: 'throw', name: e && e.name, message: e && e.message };
    if (e && e.code !== undefined) err.code = e.code;
    if (e && e.condition !== undefined) err.condition = e.condition;
    // Only the package's own line: JavaScriptCore puts a `line` on every Error.
    if (e && e.condition !== undefined && e.line !== undefined) err.line = e.line;
    return err;
  }
}

// ─── determinism ──────────────────────────────────────────

function seedRandom() {
  let a = 0x9e3779b9;
  Math.random = () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function pinClock() {
  let now = 1767225600000; // 2026-01-01T00:00:00Z
  Date.now = () => now;
  let n = 0;
  const uuid = () => `00000000-0000-4000-8000-${String(++n).padStart(12, '0')}`;
  try {
    if (globalThis.crypto) Object.defineProperty(globalThis.crypto, 'randomUUID', { value: uuid, configurable: true, writable: true });
  } catch { /* a runtime that seals crypto keeps its own */ }
  // An hour on between parses: `:spam` windows opened by one document have
  // closed before the next is read, so no result depends on corpus order.
  return () => { n = 0; now += 3600000; };
}

// ─── snapshot ─────────────────────────────────────────────

/** Names an active document reads through `:env`, from the parsed metadata. */
function envNames(tree, into, depth) {
  if (depth > 600 || tree === null || typeof tree !== 'object') return;
  if (Array.isArray(tree)) {
    for (const item of tree) envNames(item, into, depth + 1);
    return;
  }
  const meta = declarationsOf(tree);
  for (const k of Object.keys(tree)) {
    const v = tree[k];
    const one = !meta ? undefined
      : meta instanceof Map ? meta.get(k)
      : Object.prototype.hasOwnProperty.call(meta, k) ? meta[k] : undefined;
    if (one && Array.isArray(one.markers) && one.markers.includes('env') && typeof v === 'string') into.add(v);
    envNames(v, into, depth + 1);
  }
}

function digestIfLarge(value) {
  const text = JSON.stringify(value);
  if (text.length <= INLINE_LIMIT) return value;
  return { $t: 'digest', sha256: sha(text), length: text.length, head: text.slice(0, 400), tail: text.slice(-400) };
}

function textResult(s) {
  if (typeof s !== 'string') return s;
  if (s.length <= INLINE_LIMIT) return s;
  return { $t: 'digest', sha256: sha(s), length: s.length, head: s.slice(0, 400), tail: s.slice(-400) };
}

function snapshotSynx(impl, text, dir, full) {
  const { Synx, parseData } = impl;
  const keep = full ? (x) => x : digestIfLarge;
  const keepText = full ? (x) => x : textResult;
  const out = {};
  const lock = path.join(dir, '.synx.lock');
  const fresh = () => { try { fs.unlinkSync(lock); } catch { /* none */ } seedRandom(); impl.resetClock(); };

  out.parseData = timed('parseData', () => keep(attempt(() => {
    const r = parseData(text);
    const head = { mode: r.mode, locked: r.locked === true };
    for (const flag of ['llm', 'tool', 'schema']) if (r[flag] !== undefined) head[flag] = r[flag];
    if (r.includes !== undefined) head.includes = r.includes;
    head.root = render(r.root, true, 0);
    return head;
  })));

  const names = new Set();
  try { envNames(parseData(text).root, names, 0); } catch { /* reported above */ }
  // `compile`, `schema` and `parseTool` take no `env`: they read the process
  // environment, so the names this document asks for are unset while it is read.
  const saved = {};
  for (const n of names) {
    if (Object.prototype.hasOwnProperty.call(process.env, n)) { saved[n] = process.env[n]; delete process.env[n]; }
  }
  const profiles = { unset: {} };
  if (names.size > 0) {
    const str = {}; const num = {}; const bool = {};
    for (const n of names) { str[n] = `v_${n}`; num[n] = '8080'; bool[n] = 'true'; }
    profiles.text = str; profiles.number = num; profiles.bool = bool;
  }
  out.parse = {};
  out.strict = {};
  for (const name of Object.keys(profiles)) {
    fresh();
    out.parse[name] = timed('parse', () => keep(attempt(() => render(Synx.parse(text, { env: profiles[name], basePath: dir }), false, 0))));
    fresh();
    out.strict[name] = timed('strict', () => attempt(() => { Synx.parse(text, { env: profiles[name], basePath: dir, strict: true }); return 'ok'; }));
  }
  fresh();
  out.toJSON = timed('toJSON', () => keepText(attempt(() => Synx.toJSON(Synx.parse(text, { env: {}, basePath: dir }), false))));
  fresh();
  out.stringify = timed('stringify', () => keepText(attempt(() => Synx.stringify(Synx.parse(text, { env: {}, basePath: dir })))));
  out.format = timed('format', () => keepText(attempt(() => Synx.format(text))));
  out.schema = timed('schema', () => keep(attempt(() => Synx.schema(text))));
  fresh();
  const packed = timed('compile', () => attempt(() => Synx.compile(text, false)));
  if (packed && packed.$t === 'throw') {
    out.compile = packed;
  } else {
    // The deflate stream differs between runtimes (each ships its own zlib);
    // the payload inside it is what the format defines.
    out.compilePacked = { length: packed.length, sha256: sha(Buffer.from(packed)) };
    out.compile = timed('decompile', () => ({
      header: Buffer.from(packed.slice(0, 11)).toString('hex'),
      payload: attempt(() => sha(zlib.inflateRawSync(Buffer.from(packed.slice(11))))),
      back: keepText(attempt(() => Synx.decompile(packed))),
    }));
  }
  if (/(^|\n)[ \t]*!tool[ \t\r]*(\n|$)/.test(text)) {
    fresh();
    out.tool = timed('tool', () => keep(attempt(() => render(Synx.parseTool(text, { env: {}, basePath: dir }), false, 0))));
  }
  try { fs.unlinkSync(lock); } catch { /* none */ }
  for (const n of Object.keys(saved)) process.env[n] = saved[n];
  return out;
}

function snapshotSynxl(impl, text, full) {
  const { Synx } = impl;
  const keep = full ? (x) => x : digestIfLarge;
  const keepText = full ? (x) => x : textResult;
  const out = {};
  const shape = (doc) => ({
    version: doc.version,
    fieldLists: doc.fieldLists.map((fl) => ({ arity: fl.arity, line: fl.line, fields: fl.fields.map((f) => render(f, false, 0)) })),
    records: doc.records.map((r) => ({ index: r.index, line: r.line, values: render(r.values, false, 0), diagnostics: r.diagnostics })),
    diagnostics: doc.diagnostics,
  });
  out.parse = timed('synxl.parse', () => keep(attempt(() => shape(Synx.parseSynxl(text)))));
  out.validate = timed('synxl.validate', () => keep(attempt(() => shape(Synx.parseSynxl(text, { validate: true, reportTruncatedTail: true })))));
  out.ndjson = timed('synxl.ndjson', () => keepText(attempt(() => Synx.synxlToNDJSON(text))));
  out.json = timed('synxl.json', () => keepText(attempt(() => Synx.synxlToJSON(text))));
  out.write = timed('synxl.write', () => keepText(attempt(() => Synx.writeSynxl(Synx.parseSynxl(text)))));
  out.stream = timed('synxl.stream', () => keep(attempt(() => {
    let n = 0;
    const h = crypto.createHash('sha256');
    for (const r of Synx.streamSynxl(text)) { n++; h.update(JSON.stringify(render(r.values, false, 0))); h.update('\n'); }
    return { records: n, sha256: h.digest('hex') };
  })));
  return out;
}

function loadImpl(dir) {
  const root = path.resolve(dir);
  const Synx = require(path.join(root, 'dist', 'index.js'));
  const parser = require(path.join(root, 'dist', 'parser.js'));
  if (typeof parser.metaMapOf === 'function') declarationsOf = parser.metaMapOf;
  let version = 'unknown';
  try { version = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8').replace(/^﻿/, '')).version; } catch { /* unversioned build */ }
  return { Synx: Synx.Synx || Synx.default || Synx, parseData: parser.parseData, version, resetClock: () => {} };
}

function cmdSnapshot(args) {
  const impl = loadImpl(String(args.impl));
  const corpus = path.resolve(String(args.corpus));
  const out = path.resolve(String(args.out));
  const only = typeof args.only === 'string' ? args.only : null;
  const full = args.full === true;
  impl.resetClock = pinClock();
  const realRandom = Math.random;
  const startDir = process.cwd();

  const index = {
    impl: path.resolve(String(args.impl)),
    version: impl.version,
    runtime: typeof Bun !== 'undefined' ? `bun ${Bun.version}` : `node ${process.version}`,
    files: {},
  };
  let count = 0;
  const started = process.hrtime.bigint();
  walk(corpus, (file) => {
    const rel = path.relative(corpus, file).split(path.sep).join('/');
    if (only && !rel.includes(only)) return;
    const text = fs.readFileSync(file, 'utf8');
    // `compile` and `:once` resolve against the working directory when no
    // base is given; inside the corpus copy that is the document's own folder.
    process.chdir(path.dirname(file));
    const result = file.endsWith('.synxl')
      ? snapshotSynxl(impl, text, full)
      : snapshotSynx(impl, text, path.dirname(file), full);
    const body = JSON.stringify(result);
    const dest = path.join(out, rel + '.json');
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    fs.writeFileSync(dest, body);
    index.files[rel] = sha(body);
    count++;
  });
  Math.random = realRandom;
  process.chdir(startDir);
  index.seconds = Number(process.hrtime.bigint() - started) / 1e9;
  if (!only) fs.writeFileSync(path.join(out, 'INDEX.json'), JSON.stringify(index, null, 1));
  console.log(`snapshot: ${count} files, ${index.version} on ${index.runtime}, ${index.seconds.toFixed(1)} s -> ${out}`);
  if (args.timing === true) {
    for (const k of Object.keys(TIMING).sort((x, y) => TIMING[y] - TIMING[x])) console.log(`  ${k.padEnd(16)} ${TIMING[k].toFixed(2)} s`);
  }
}

// ─── diff ─────────────────────────────────────────────────

/**
 * Every leaf at which two rendered results differ, as `{ at, a, b }`.
 *
 * Text that holds structure is opened up first — canonical JSON is parsed and
 * multi-line text is compared line by line — so one changed value shows as one
 * leaf instead of a whole document that differs somewhere.
 */
function differences(a, b, at, out) {
  if (a === b) return;
  if (typeof a === 'string' && typeof b === 'string') {
    const first = a.charCodeAt(0);
    if ((first === 123 || first === 91) && (b.charCodeAt(0) === 123 || b.charCodeAt(0) === 91)) {
      let pa;
      let pb;
      try { pa = JSON.parse(a); pb = JSON.parse(b); } catch { pa = undefined; }
      if (pa !== undefined && pb !== undefined) return differences(pa, pb, `${at}:json`, out);
    }
    if (a.indexOf('\n') !== -1 || b.indexOf('\n') !== -1) {
      const la = a.split('\n');
      const lb = b.split('\n');
      if (la.length === lb.length) {
        for (let i = 0; i < la.length; i++) if (la[i] !== lb[i]) out.push({ at: `${at}:line ${i + 1}`, a: la[i], b: lb[i] });
        return;
      }
    }
    out.push({ at, a, b });
    return;
  }
  if (a === null || b === null || typeof a !== 'object' || typeof b !== 'object' || Array.isArray(a) !== Array.isArray(b)) {
    out.push({ at, a, b });
    return;
  }
  if (Array.isArray(a)) {
    if (a.length !== b.length) { out.push({ at: `${at}.length`, a: a.length, b: b.length }); return; }
    for (let i = 0; i < a.length; i++) differences(a[i], b[i], `${at}[${i}]`, out);
    return;
  }
  const entries = Array.isArray(a.$e) && Array.isArray(b.$e);
  if (entries) {
    // A rendered object: compared key by key, in order.
    const same = a.$e.length === b.$e.length && a.$e.every((e, i) => e[0] === b.$e[i][0]);
    if (!same) {
      out.push({ at: `${at}/(keys)`, a: a.$e.map((e) => e[0]).join(','), b: b.$e.map((e) => e[0]).join(',') });
      return;
    }
    for (let i = 0; i < a.$e.length; i++) differences(a.$e[i][1], b.$e[i][1], `${at}/${a.$e[i][0]}`, out);
  }
  for (const k of new Set([...Object.keys(a), ...Object.keys(b)])) {
    if (k === '$e' && entries) continue;
    differences(a[k], b[k], `${at}/${k}`, out);
  }
}

function clip(v) {
  const s = v === undefined ? 'undefined' : JSON.stringify(v);
  return s.length > 300 ? `${s.slice(0, 300)}… (${s.length} chars)` : s;
}

/**
 * Rules of an allow file, one per line, tab-separated:
 *
 *   <file prefix> <TAB> <path prefix> [<TAB> <A pattern> <TAB> <B pattern>]
 *
 * A leaf is waived when its file and path start with the first two fields and
 * the JSON text of each side matches its pattern — a regular expression tried
 * at the start of the text (left empty, it matches anything). A rule whose path begins with `~` is *derived*: it applies only to
 * a file that already has a waived leaf — for digests, which cannot say what
 * changed inside them.
 */
function readAllow(files) {
  const rules = [];
  for (const file of files) readAllowFile(file, rules);
  return rules;
}

function readAllowFile(file, rules) {
  if (typeof file !== 'string') return;
  for (const line of fs.readFileSync(file, 'utf8').split(/\r?\n/)) {
    if (!line.trim() || line.startsWith('#')) continue;
    const [f, p, a, b] = line.split('\t');
    const derived = (p || '').startsWith('~');
    rules.push({
      file: f.trim(),
      path: derived ? p.slice(1) : (p || ''),
      a: new RegExp('^(?:' + (a || '') + ')'),
      b: new RegExp('^(?:' + (b || '') + ')'),
      derived,
    });
  }
}

function cmdDiff(args) {
  const [dirA, dirB] = args._.slice(1).map((d) => path.resolve(d));
  const a = JSON.parse(fs.readFileSync(path.join(dirA, 'INDEX.json'), 'utf8'));
  const b = JSON.parse(fs.readFileSync(path.join(dirB, 'INDEX.json'), 'utf8'));
  const max = args.max ? Number(args.max) : 40;
  const rules = readAllow(args.allow);
  const names = [...new Set([...Object.keys(a.files), ...Object.keys(b.files)])].sort();
  let same = 0;
  let waived = 0;
  const open = [];
  const matches = (rule, name, d) => (rule.file === '*' || name.startsWith(rule.file))
    && d.at.startsWith(rule.path)
    && rule.a.test(JSON.stringify(d.a === undefined ? null : d.a))
    && rule.b.test(JSON.stringify(d.b === undefined ? null : d.b));
  for (const name of names) {
    if (a.files[name] === b.files[name]) { same++; continue; }
    if (a.files[name] === undefined || b.files[name] === undefined) {
      open.push({ name, at: '(file)', a: a.files[name] ? 'present' : 'absent', b: b.files[name] ? 'present' : 'absent' });
      continue;
    }
    const ra = JSON.parse(fs.readFileSync(path.join(dirA, name + '.json'), 'utf8'));
    const rb = JSON.parse(fs.readFileSync(path.join(dirB, name + '.json'), 'utf8'));
    const leaves = [];
    for (const section of new Set([...Object.keys(ra), ...Object.keys(rb)])) differences(ra[section], rb[section], section, leaves);
    const rest = leaves.filter((d) => !rules.some((r) => !r.derived && matches(r, name, d)));
    const direct = leaves.length - rest.length;
    const left = direct > 0 ? rest.filter((d) => !rules.some((r) => r.derived && matches(r, name, d))) : rest;
    waived += leaves.length - left.length;
    for (const d of left) open.push({ name, ...d });
    if (left.length === 0) same++;
  }
  console.log(`A: ${a.version} on ${a.runtime} (${Object.keys(a.files).length} files)`);
  console.log(`B: ${b.version} on ${b.runtime} (${Object.keys(b.files).length} files)`);
  console.log(`files without open differences: ${same}   waived leaves: ${waived}   open leaves: ${open.length}`);
  for (const d of open.slice(0, max)) {
    console.log(`\n${d.name}\n  at ${d.at}\n  A: ${clip(d.a)}\n  B: ${clip(d.b)}`);
  }
  if (open.length > max) console.log(`\n… ${open.length - max} more`);
  process.exitCode = open.length === 0 ? 0 : 1;
}

// ─── entry ────────────────────────────────────────────────

function main() {
  const args = parseArgs(process.argv.slice(2));
  const cmd = args._[0];
  if (cmd === 'mirror' && args.out && args.root.length > 0) return cmdMirror(args);
  if (cmd === 'snapshot' && args.impl && args.corpus && args.out) return cmdSnapshot(args);
  if (cmd === 'diff' && args._.length === 3) return cmdDiff(args);
  console.error([
    'usage:',
    '  compat-snapshot.js mirror   --out <corpus> --root name=<dir> [--root name=<dir> ...]',
    '  compat-snapshot.js snapshot --impl <package dir> --corpus <corpus> --out <snapshot> [--only <substring>] [--full] [--timing]',
    '  compat-snapshot.js diff     <snapshot A> <snapshot B> [--allow <file>]... [--max 40]',
  ].join('\n'));
  process.exitCode = 2;
}

main();
