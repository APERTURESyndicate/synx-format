#!/usr/bin/env node
/**
 * The other implementations against synx-core: every conformance case and
 * every `parse` / `active` regression case goes to each port that builds on
 * this machine, and the canonical JSON is compared with the core's.
 *
 *   node tests/adversarial/ports.mjs                  go, java, dart, dotnet, cpp, swift
 *   node tests/adversarial/ports.mjs --only go,java
 *   node tests/adversarial/ports.mjs --max 40         differences listed per port
 *   node tests/adversarial/ports.mjs --hostile        inputs built to exhaust a port: deep nesting,
 *                                                     long expressions, backtracking patterns, huge
 *                                                     widths — one process per input, 20 s each
 *   node tests/adversarial/ports.mjs --binary         .synxb containers with lying counts, lengths
 *                                                     and sizes, the same way (the C++ worker is
 *                                                     built without zlib and has no decoder)
 *
 * Also reports where the default string form of a resolved tree (`%v`,
 * `toString()`, string interpolation) contains the value of a `:secret` key.
 *
 * Workers: ports/<name>/ (one request per line, see ports/go/main.go). They are
 * built into tests/adversarial/tmp/ports; a port whose toolchain is missing or
 * whose build fails is reported and skipped.
 *
 * Exit code 1 when a port disagrees with the core or prints a masked value.
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { HERE, REPO, TMP, b64, ensureDir, normJson } from './lib/harness.mjs';
import { worker } from './lib/impls.mjs';
import { container, counted, stringTable, T, U64_MAX, varint } from './lib/synxb.mjs';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(name); return i === -1 ? fallback : argv[i + 1]; };
if (argv.includes('--help')) {
  process.stdout.write(fs.readFileSync(new URL(import.meta.url), 'utf-8').split('*/')[0].replace(/^#!.*\n\/\*\*\n?/, '').replace(/^ \* ?/gm, ''));
  process.exit(0);
}
const only = opt('--only', 'go,java,dart,dotnet,cpp,swift').split(',');
const maxShown = Number(opt('--max', '15'));
const out = ensureDir(path.join(TMP, 'ports'));
const exe = process.platform === 'win32' ? '.exe' : '';
const sh = (cmd, args, o = {}) => spawnSync(cmd, args, { encoding: 'utf-8', maxBuffer: 1 << 28, windowsHide: true, ...o });

// ─── inputs ──────────────────────────────────────────────────

const inputs = [];
const confDir = path.join(REPO, 'tests', 'conformance', 'cases');
for (const f of fs.readdirSync(confDir).filter((x) => x.endsWith('.synx')).sort()) {
  inputs.push({ id: `conformance/${f}`, op: 'active', text: fs.readFileSync(path.join(confDir, f), 'utf-8'), env: {} });
}
for (const file of fs.readdirSync(path.join(HERE, 'cases')).filter((f) => f.endsWith('.mjs')).sort()) {
  const mod = await import(pathToFileURL(path.join(HERE, 'cases', file)).href);
  for (const c of mod.default) {
    if (!['parse', 'active'].includes(c.op) || typeof c.input !== 'string' || c.setup || c.once) continue;
    if (/:random\b|:once\b|\(random/.test(c.input)) continue;
    inputs.push({ id: c.id, op: c.op, text: c.input, env: (typeof c.req === 'object' && c.req && c.req.env) || {} });
  }
}
// A marker value no document would contain by accident: it must never be printed.
const NEEDLE = 'masked-value-7f3a';
for (const [i, doc] of [`!active\npassword:secret ${NEEDLE}\n`, '!active\ndb\n  password:secret:env PW\n', `!active\nusers\n  - name bob\n    password:secret ${NEEDLE}\n`].entries()) {
  inputs.push({ id: `masked-${i + 1}`, op: 'active', text: doc, env: { PW: NEEDLE }, masked: true });
}

const base = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-ports-'));
process.on('exit', () => { try { fs.rmSync(base, { recursive: true, force: true }); } catch { /* scratch only */ } });
const feed = inputs.map((x) => [x.id, x.op, b64(x.text), b64(Object.entries(x.env).map(([k, v]) => `${k}=${v}`).join('\n')), base].join('\t')).join('\n') + '\n';

// ─── building the workers ────────────────────────────────────

const BUILD = {
  go() {
    const dir = ensureDir(path.join(out, 'go'));
    fs.copyFileSync(path.join(HERE, 'ports', 'go', 'main.go'), path.join(dir, 'main.go'));
    fs.writeFileSync(path.join(dir, 'go.mod'), `module advport\n\ngo 1.21\n\nrequire github.com/aperturesyndicate/synx v0.0.0\n\nreplace github.com/aperturesyndicate/synx => ${path.join(REPO, 'parsers', 'go').replace(/\\/g, '/')}\n`);
    const r = sh('go', ['build', '-o', 'port' + exe, '.'], { cwd: dir, env: { ...process.env, GOFLAGS: '-mod=mod', GOPROXY: 'off' } });
    if (r.status !== 0) throw new Error((r.stderr || r.error?.message || '').slice(0, 400));
    return [path.join(dir, 'port' + exe), []];
  },
  java() {
    const dir = ensureDir(path.join(out, 'java'));
    const src = path.join(REPO, 'parsers', 'java', 'src', 'main', 'java');
    const walk = (d) => fs.readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(path.join(d, e.name)) : e.name.endsWith('.java') ? [path.join(d, e.name)] : []));
    const r = sh('javac', ['-encoding', 'UTF-8', '-nowarn', '-d', path.join(dir, 'classes'), ...walk(src), path.join(HERE, 'ports', 'java', 'Main.java')]);
    if (r.status !== 0) throw new Error((r.stderr || r.error?.message || '').slice(0, 400));
    return ['java', ['-Dfile.encoding=UTF-8', '-cp', path.join(dir, 'classes'), 'Main']];
  },
  dart() {
    const dir = ensureDir(path.join(out, 'dart'));
    const lib = pathToFileURL(path.join(REPO, 'parsers', 'dart', 'lib', 'synx.dart')).href;
    fs.writeFileSync(path.join(dir, 'main.dart'), fs.readFileSync(path.join(HERE, 'ports', 'dart', 'main.dart'), 'utf-8').replace("'SYNX_DART_LIB'", () => `'${lib}'`));
    const r = sh('dart', ['compile', 'exe', path.join(dir, 'main.dart'), '-o', path.join(dir, 'port' + exe)], { shell: process.platform === 'win32' });
    if (r.status !== 0) throw new Error((r.stderr || r.stdout || r.error?.message || '').slice(0, 400));
    return [path.join(dir, 'port' + exe), []];
  },
  dotnet() {
    const dir = ensureDir(path.join(out, 'dotnet'));
    const lib = sh('dotnet', ['build', path.join(REPO, 'parsers', 'dotnet', 'src', 'Synx.Core', 'Synx.Core.csproj'), '-c', 'Release', '-o', path.join(dir, 'lib'), '--nologo', '-v', 'q']);
    if (lib.status !== 0) throw new Error((lib.stdout || '').slice(-400));
    for (const f of ['Program.cs', 'port.csproj']) fs.copyFileSync(path.join(HERE, 'ports', 'dotnet', f), path.join(dir, f));
    const r = sh('dotnet', ['build', path.join(dir, 'port.csproj'), '-c', 'Release', '-o', path.join(dir, 'bin'), `-p:SynxDll=${path.join(dir, 'lib', 'Synx.Core.dll')}`, '--nologo', '-v', 'q']);
    if (r.status !== 0) throw new Error((r.stdout || '').slice(-400));
    return [path.join(dir, 'bin', 'port' + exe), []];
  },
  cpp() {
    const dir = ensureDir(path.join(out, 'cpp'));
    const src = path.join(REPO, 'parsers', 'cpp');
    const units = fs.readdirSync(path.join(src, 'src')).filter((f) => f.endsWith('.cpp')).map((f) => path.join(src, 'src', f));
    // As the port's own build: no exceptions, no RTTI. Without zlib the .synxb half is left out.
    const r = sh('g++', ['-std=c++17', '-O2', '-fno-exceptions', '-fno-rtti', '-D_GLIBCXX_ASSERTIONS', '-I', path.join(src, 'include'), '-I', path.join(src, 'src'),
      ...units, path.join(HERE, 'ports', 'cpp', 'main.cpp'), '-o', path.join(dir, 'port' + exe), ...(process.platform === 'win32' ? ['-static'] : [])]);
    if (r.status !== 0) throw new Error((r.stderr || r.error?.message || '').slice(0, 400));
    return [path.join(dir, 'port' + exe), []];
  },
  swift() {
    // SwiftPM names a package after its directory: `swift` here would be taken for the port itself.
    const dir = ensureDir(path.join(out, 'swift-worker'));
    ensureDir(path.join(dir, 'Sources', 'port'));
    fs.copyFileSync(path.join(HERE, 'ports', 'swift', 'main.swift'), path.join(dir, 'Sources', 'port', 'main.swift'));
    fs.writeFileSync(path.join(dir, 'Package.swift'), [
      '// swift-tools-version: 5.9',
      'import PackageDescription',
      'let package = Package(',
      '    name: "port",',
      `    dependencies: [.package(name: "Synx", path: "${path.join(REPO, 'parsers', 'swift').replace(/\\/g, '/')}")],`,
      '    targets: [.executableTarget(name: "port", dependencies: [.product(name: "Synx", package: "Synx")], path: "Sources/port")]',
      ')', ''].join('\n'));
    const r = sh('swift', ['build', '-c', 'release'], { cwd: dir, timeout: 1200000 });
    if (r.status !== 0) throw new Error(((r.stdout || '') + (r.stderr || '') || r.error?.message || '').slice(-400));
    return [path.join(dir, '.build', 'release', 'port' + exe), []];
  },
};

// ─── hostile inputs: one process per input ───────────────────

/** Inputs sized to cost the core a few milliseconds; a port has 20 s for each. */
function hostileInputs() {
  const A = '!active\n';
  const rep = (s, n) => s.repeat(n);
  const lines = (n, f) => Array.from({ length: n }, (_, i) => f(i)).join('\n') + '\n';
  return [
    ['nest-3000', 'parse', lines(3000, (i) => `${rep(' ', i)}k`)],
    ['nest-list-3000', 'parse', 'k\n' + lines(3000, (i) => `${rep(' ', i + 1)}- `)],
    ['calc-parens-100k', 'active', `${A}x:calc ${rep('(', 100000)}1${rep(')', 100000)}\n`],
    ['calc-minus-200k', 'active', `${A}x:calc ${rep('-', 200000)}1\n`],
    ['calc-long-sum-200k', 'active', `${A}x:calc 1${rep(' + 1', 200000)}\n`],
    ['alias-chain-5000', 'active', A + 'a0 1\n' + lines(5000, (i) => `a${i + 1}:alias a${i}`)],
    ['alias-chain-reversed-5000', 'active', A + lines(5000, (i) => `a${5000 - i}:alias a${4999 - i}`) + 'a0 1\n'],
    ['alias-cycle', 'active', `${A}a:alias b\nb:alias c\nc:alias a\n`],
    ['inherit-chain-2000', 'active', A + 'b0\n  v 0\n' + lines(2000, (i) => `b${i + 1}:inherit b${i}\n  w${i} 1`).replace(/\n\n/g, '\n')],
    ['interp-1000x1000x1KiB', 'active', `${A}a ${rep('x', 1024)}\nb ${rep('{a}', 1000)}\nc ${rep('{b}', 1000)}\n`],
    ['interp-self', 'active', `${A}a {a}{a}\nb {a}{b}\n`],
    ['pattern-nested-plus', 'active', `${A}k[pattern:^(a+)+$] ${rep('a', 40)}!\n`],
    ['pattern-alternation', 'active', `${A}k[pattern:^(a|aa)+$] ${rep('a', 60)}!\n`],
    ['format-width', 'active', `${A}x:format:%2000000000d 5\n`],
    ['round-digits', 'active', `${A}x:round:2000000000 1.5\ny:round:-2000000000 1.5\n`],
    ['split-1MiB', 'active', `${A}x:split:, ${rep('a,', 500000)}\n`],
    ['digits-1MiB', 'parse', `x ${rep('9', 1 << 20)}\ny(int) ${rep('9', 1 << 20)}\nz(float) ${rep('9', 1 << 20)}\n`],
    ['float-exponent', 'parse', 'a 1e999999999\nb -1e999999999\nc 1e-999999999\nd(float) 0x1p9999\n'],
    ['key-8MiB', 'parse', `${rep('k', 8 << 20)} v\n`],
    ['same-key-300k', 'parse', rep('k v\n', 300000)],
    ['markers-on-one-key-20k', 'active', `${A}x${rep(':upper', 20000)} v\n`],
    ['constraints-20k', 'active', `${A}x[${rep('min:1, ', 20000)}max:9] 5\n`],
  ].map(([id, op, text]) => ({ id, op, text }));
}

/** `.synxb` containers whose header and deflate wrapping are right and whose payload lies. */
function binaryInputs() {
  const tree = (strings, ...rest) => container(Buffer.concat([stringTable(strings), ...rest.map((p) => (Buffer.isBuffer(p) ? p : Buffer.from(p)))]));
  const one = (strings, ...rest) => tree(strings, [T.obj], varint(1), ...rest);
  const good = Buffer.concat([stringTable(['k', 'v']), Buffer.from([T.obj]), varint(1), varint(0), Buffer.from([T.str]), varint(1)]);
  const nested = (n) => tree([], Buffer.alloc(2 * n, Buffer.from([T.arr, 1])), [T.nil]);
  const nan = Buffer.alloc(8);
  nan.writeDoubleLE(NaN);
  return [
    ['valid', container(good)],
    ['string-length-2^64-1', container(Buffer.concat([varint(1), varint(U64_MAX)]))],
    ['string-count-2^40', container(varint(1n << 40n))],
    ['varint-of-11-bytes', container(Buffer.from([0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01]))],
    ['string-not-utf8', tree([Buffer.from([0xff, 0xfe])], [T.obj], varint(1), varint(0), [T.nil])],
    ['deflate-cut-short', Buffer.from('SYNXB\x01\x00\x10\x00\x00\x00', 'latin1')],
    ['string-index-2^32', one(['zero', 'k'], varint(1), [T.str], varint(1n << 32n))],
    ['array-count-2^40', container(counted(T.arr, 1n << 40n))],
    ['object-count-2^40', container(counted(T.obj, 1n << 40n, Buffer.alloc(0), ['k']))],
    ['array-count-2^31-then-16-bytes', one(['k'], varint(0), [T.arr], varint((1n << 31n) - 1n), Buffer.alloc(16))],
    ['arrays-nested-5000', nested(5000)],
    ['arrays-nested-200000', nested(200000)],
    ['declared-size-4GiB', container(good, { declared: 0xffffffff })],
    ['declared-size-0', container(good, { declared: 0 })],
    ['64MiB-of-zeros', container(Buffer.alloc(64 << 20))],
    ['unknown-tag', tree([], [9])],
    ['bytes-after-the-tree', tree(['k', 'v'], [T.obj], varint(1), varint(0), [T.str], varint(1), [1, 2, 3])],
    ['version-2', container(good, { version: 2 })],
    ['flags-255', container(good, { flags: 255 })],
    ['key-with-a-line-break', one(['a\n!active\nleak:env PATH', 'v'], varint(0), [T.str], varint(1))],
    ['same-key-twice', tree(['k', 'a', 'b'], [T.obj], varint(2), varint(0), [T.str], varint(1), varint(0), [T.str], varint(2))],
    ['key-__proto__', one(['__proto__', 'v'], varint(0), [T.str], varint(1))],
    ['root-is-an-integer', tree([], [T.int], varint(2))],
    ['integer-2^64-1', one(['k'], varint(0), [T.int], varint(U64_MAX))],
    ['float-nan', one(['k'], varint(0), [T.float], nan)],
    ['secret-value', one(['k', 'masked-value-7f3a'], varint(0), [T.secret], varint(1))],
  ].map(([id, bytes]) => ({ id, op: 'decompile', bytes }));
}

function hostileRun(cmd, x) {
  const started = Date.now();
  const r = sh(cmd[0], cmd[1], { input: [x.id, x.op, b64(x.bytes ?? x.text), '', base].join('\t') + '\n', timeout: 20000 });
  const ms = Date.now() - started;
  if (r.error && r.error.code === 'ETIMEDOUT') return { state: 'no answer in 20 s', ms };
  const line = (r.stdout || '').split('\n').find((l) => l.startsWith(x.id + '\t'));
  if (!line) {
    const code = r.status === null ? r.signal : r.status > 0x7fffffff || r.status < 0 ? `0x${(r.status >>> 0).toString(16)}` : r.status;
    return { state: `died (exit ${code})`, tail: ((r.stderr || '').trim().split('\n').filter(Boolean).pop() || '').slice(0, 150), ms };
  }
  return { state: 'ok', out: Buffer.from(line.replace(/\r$/, '').split('\t')[1], 'base64').toString('utf-8'), ms };
}

// ─── the core's answers ──────────────────────────────────────

const core = worker('rust', { quiet: false });
const want = new Map();
for (const x of inputs) {
  const r = await core.call({ op: x.op, b64: b64(x.text), env: x.env, base }, 20000);
  want.set(x.id, r.ok ? normJson(r.out.json) : `ERROR: ${r.err || r.panic || r.crash}`);
}
core.close();

// ─── each port ───────────────────────────────────────────────

let bad = 0;
const hostile = argv.includes('--hostile') ? hostileInputs() : argv.includes('--binary') ? binaryInputs() : null;
const coreOn = new Map();
// Two refusals are one answer, whatever the wording.
const reading = (x, s) => (s.startsWith('ERROR') ? 'ERROR' : x.op === 'decompile' ? s : normJson(s));
if (hostile) {
  const ref = worker('rust', { quiet: false });
  for (const x of hostile) {
    const started = Date.now();
    const r = await ref.call({ op: x.op, b64: b64(x.bytes ?? x.text), env: {}, base, brief: false }, 60000);
    coreOn.set(x.id, { ms: Date.now() - started, out: r.ok ? (r.out.json ?? r.out.text) : `ERROR: ${r.err || r.panic || r.crash}` });
  }
  ref.close();
}
for (const name of only) {
  let cmd;
  try { cmd = BUILD[name](); } catch (e) { console.log(`\n${name}: not built — ${String(e.message).split('\n').slice(0, 3).join(' | ')}`); continue; }
  if (hostile) {
    console.log(`\n${name}:`);
    for (const x of hostile) {
      const got = hostileRun(cmd, x);
      const c = coreOn.get(x.id);
      const same = got.state === 'ok' && reading(x, got.out) === reading(x, c.out);
      if (got.state !== 'ok') bad++;
      const brief = (s) => `${s.length} bytes: ${JSON.stringify(s.slice(0, 70))}`;
      const note = got.state === 'ok' ? `${same ? 'as the core' : `differs from the core (${brief(got.out)}; core ${brief(c.out)})`}` : (got.tail || '');
      console.log(`  ${got.state === 'ok' ? 'ok  ' : 'FAIL'}  ${x.id.padEnd(28)} ${String(got.ms).padStart(6)} ms (core ${c.ms} ms)  ${got.state === 'ok' ? '' : got.state + ' '}${note}`);
    }
    continue;
  }
  const r = sh(cmd[0], cmd[1], { input: feed, timeout: 300000 });
  const got = new Map();
  for (const line of (r.stdout || '').split('\n')) {
    const f = line.replace(/\r$/, '').split('\t');
    if (f.length >= 3) got.set(f[0], [Buffer.from(f[1], 'base64').toString('utf-8'), Buffer.from(f[2], 'base64').toString('utf-8')]);
  }
  const diffs = [];
  const leaks = [];
  for (const x of inputs) {
    const g = got.get(x.id);
    if (!g) { diffs.push(`${x.id}: no answer${r.status ? ` (worker exit ${r.status}: ${(r.stderr || '').trim().split('\n').pop()?.slice(0, 120)})` : ''}`); continue; }
    if (g[1].includes(NEEDLE) || (x.masked && g[0].includes(NEEDLE))) leaks.push(`${x.id}: ${(g[0].includes(NEEDLE) ? g[0] : g[1]).slice(0, 110)}`);
    const a = g[0].startsWith('ERROR') ? g[0] : normJson(g[0]);
    if (a !== want.get(x.id)) {
      const w = want.get(x.id);
      let i = 0;
      while (i < a.length && i < w.length && a[i] === w[i]) i++;
      diffs.push(`${x.id}\n      ${name}: …${a.slice(Math.max(0, i - 30), i + 90)}\n      core: …${w.slice(Math.max(0, i - 30), i + 90)}`);
    }
  }
  bad += diffs.length + leaks.length;
  console.log(`\n${name}: ${inputs.length - diffs.length} of ${inputs.length} inputs read as the core reads them, ${diffs.length} differ, ${leaks.length} print a masked value`);
  for (const d of diffs.slice(0, maxShown)) console.log(`  ${d}`);
  if (diffs.length > maxShown) console.log(`  … and ${diffs.length - maxShown} more: ${diffs.slice(maxShown).map((d) => d.split('\n')[0]).join(' ')}`);
  for (const l of leaks) console.log(`  MASKED VALUE SHOWN ${l}`);
}
process.exit(bad ? 1 : 0);
