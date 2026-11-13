#!/usr/bin/env node
/**
 * The native bindings against hostile values: the Node addon, the Python
 * module and the C library as the Kotlin binding reaches it (JNA).
 *
 *   node tests/adversarial/bindings.mjs                 node, python, jna
 *   node tests/adversarial/bindings.mjs --only node
 *
 * The three libraries are built from the working tree with cargo into
 * target-sy4/bind (link-time optimisation off, to keep the build short). A
 * binding whose toolchain is missing is reported and skipped: Python needs a
 * `python` / `py` on PATH, JNA needs `java` and a jna-*.jar in the Gradle or
 * Maven cache (or SYNX_JNA_JAR).
 *
 * Every probe runs in its own process — several of them end the process.
 * Exit code 1 when a probe fails.
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { HERE, REPO, TMP, ensureDir } from './lib/harness.mjs';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(name); return i === -1 ? fallback : argv[i + 1]; };
if (argv.includes('--help')) {
  process.stdout.write(fs.readFileSync(new URL(import.meta.url), 'utf-8').split('*/')[0].replace(/^#!.*\n\/\*\*\n?/, '').replace(/^ \* ?/gm, ''));
  process.exit(0);
}
const only = opt('--only', 'node,python,jna').split(',');
const sh = (cmd, args, o = {}) => spawnSync(cmd, args, { encoding: 'utf-8', maxBuffer: 1 << 26, windowsHide: true, ...o });
const out = ensureDir(path.join(TMP, 'bind'));
const win = process.platform === 'win32';
const libName = (stem) => (win ? `${stem}.dll` : process.platform === 'darwin' ? `lib${stem}.dylib` : `lib${stem}.so`);

function python() {
  for (const [cmd, args] of [['py', ['-3']], ['python3', []], ['python', []]]) {
    const r = sh(cmd, [...args, '-c', 'import sys; print(sys.executable)']);
    if (r.status === 0 && r.stdout.trim()) return r.stdout.trim();
  }
  return null;
}

function build(packages, env = {}) {
  const r = sh('cargo', ['build', '--release', '--offline', ...packages.flatMap((p) => ['-p', p])], {
    cwd: REPO,
    env: { ...process.env, CARGO_TARGET_DIR: path.join(REPO, 'target-sy4', 'bind'), CARGO_PROFILE_RELEASE_LTO: 'false', CARGO_PROFILE_RELEASE_CODEGEN_UNITS: '16', ...env },
  });
  if (r.status !== 0) throw new Error((r.stderr || r.error?.message || '').trim().split('\n').slice(-4).join(' | '));
  return path.join(REPO, 'target-sy4', 'bind', 'release');
}

let failed = 0;
const report = (name, title, problem) => {
  if (problem) failed++;
  console.log(`${problem ? 'FAIL' : 'pass'}  ${name}  ${title}${problem ? `\n      ${problem}` : ''}`);
};
const exitOf = (r) => (r.status === null ? `signal ${r.signal}` : `exit ${r.status > 0x7fffffff ? '0x' + (r.status >>> 0).toString(16) : r.status}`);

// ─── the Node addon ──────────────────────────────────────────

function nodeAddon() {
  const addon = path.join(out, 'synx_node.node');
  fs.copyFileSync(path.join(build(['synx-node']), libName('synx_node')), addon);
  const probe = (name) => {
    const r = sh(process.execPath, [path.join(HERE, 'ports', 'node-addon', 'probe.cjs'), addon, name], { timeout: 60000 });
    const line = (r.stdout || '').trim().split('\n').pop();
    try { return { ...JSON.parse(line), exit: r.status }; } catch { return { died: exitOf(r), note: (r.stderr || '').split('\n').find((l) => /memory allocation|Fatal error|Check failed|overflow/.test(l)) || '' }; }
  };
  // A value the addon cannot write has to come back as an exception.
  const survives = (title, name) => {
    const a = probe(name);
    report('node', title, a.died ? `the process ended (${a.died}) ${a.note}`.trim() : null);
  };
  survives('stringify of an array nested 12 000 deep (24 KB of JSON) leaves the process alive', 'deep-array');
  survives('stringify of an object nested 6 000 deep leaves the process alive', 'deep-object');
  survives('diff of an object nested 6 000 deep leaves the process alive', 'deep-diff');
  survives('stringify of a value that contains itself leaves the process alive', 'cycle');
  survives('diff of a value that contains itself leaves the process alive', 'cycle-diff');
  survives('stringify of an array whose length is 2^32-1 leaves the process alive', 'sparse');
  const exact = (title, name, want) => {
    const a = probe(name);
    report('node', title, a.died ? `the process ended (${a.died})` : a.ok && String(a.value).includes(want) ? null : `got ${a.ok ? JSON.stringify(a.value) : a.thrown}`);
  };
  exact('parse keeps 9007199254740993 digit for digit', 'int53', '9007199254740993');
  exact('parseActive keeps 9223372036854775807 digit for digit', 'int64', '9223372036854775807');
  exact('stringify writes a bigint', 'bigint-out', '9007199254740993');
  const two63 = probe('two-to-63');
  report('node', 'stringify does not turn 2^63 into 2^63-1', two63.ok && String(two63.value).includes('9223372036854775807') ? `got ${JSON.stringify(two63.value)}` : null);
  const nul = probe('nul-in-key');
  report('node', 'a key that contains U+0000 does not fail the whole document', nul.ok ? null : nul.thrown || nul.died);
  const lone = probe('surrogate-key');
  report('node', 'a key with a lone surrogate keeps its value or is refused', lone.ok && / null\n/.test(String(lone.value)) ? `got ${JSON.stringify(lone.value)}` : null);
}

// ─── the Python module ───────────────────────────────────────

function pythonModule() {
  const exe = python();
  if (!exe) throw new Error('no python on PATH');
  const built = build(['synx-python'], { PYO3_PYTHON: exe });
  fs.copyFileSync(path.join(built, libName('synx_native')), path.join(out, win ? 'synx_native.pyd' : 'synx_native.so'));
  const probe = (name) => {
    const r = sh(exe, [path.join(HERE, 'ports', 'python', 'probe.py'), out, name], { timeout: 60000 });
    const line = (r.stdout || '').trim().split('\n').pop() || '';
    return line.startsWith('ok ') || line.startsWith('raised ') ? line : `the interpreter ended (${exitOf(r)}) ${(r.stderr || '').trim().split('\n').pop() || ''}`;
  };
  const expect = (title, name, want) => {
    const got = probe(name);
    report('python', title, want.test(got) ? null : got.slice(0, 200));
  };
  expect('a list nested 200 000 deep is refused', 'deep', /^raised ValueError/);
  expect('a value that contains itself is refused', 'cycle', /^raised ValueError/);
  expect('diff of a value that contains itself is refused', 'cycle-diff', /^raised ValueError/);
  expect('parse keeps 9223372036854775807', 'int64', /^ok 9223372036854775807$/);
  expect('an integer beyond i64 is refused, not rounded', 'beyond-int64', /^raised (ValueError|OverflowError)/);
  expect('a :secret value is masked in the returned dict', 'masked', /^ok (?!.*masked-value-7f3a)/);
  expect('a file that is not UTF-8 is refused', 'not-utf8', /^raised /);
  expect('an environment value that is not a string is refused', 'wrong-env', /^raised TypeError/);
}

// ─── the C library through JNA, as the Kotlin binding loads it ─

function findJna() {
  if (process.env.SYNX_JNA_JAR) return process.env.SYNX_JNA_JAR;
  const roots = [path.join(os.homedir(), '.gradle', 'caches', 'modules-2', 'files-2.1', 'net.java.dev.jna', 'jna'), path.join(os.homedir(), '.m2', 'repository', 'net', 'java', 'dev', 'jna', 'jna')];
  const walk = (d, depth) => (depth < 0 || !fs.existsSync(d) ? [] : fs.readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(path.join(d, e.name), depth - 1) : /^jna-[\d.]+\.jar$/.test(e.name) ? [path.join(d, e.name)] : [])));
  return roots.flatMap((r) => walk(r, 3)).sort().pop() || null;
}

function jna() {
  const jar = findJna();
  if (!jar) throw new Error('no jna-*.jar in the Gradle or Maven cache; set SYNX_JNA_JAR');
  fs.copyFileSync(path.join(build(['synx-c']), libName('synx_c')), path.join(out, libName('synx_c')));
  const r = sh('java', ['-cp', jar, path.join(HERE, 'ports', 'jna', 'Main.java'), out], { timeout: 120000 });
  if (r.status !== 0) throw new Error((r.stderr || r.error?.message || '').trim().split('\n').slice(0, 3).join(' | '));
  const line = Object.fromEntries(r.stdout.split('\n').map((l) => l.replace(/\r$/, '').split(/\s*: /)).filter((p) => p.length >= 2).map((p) => [p[0].trim(), p.slice(1).join(': ')]));
  // The document is `name x`, `role guest<U+0000>`, `role admin`, `port 1`.
  // Through a bare `const char*` the C library sees the text up to the U+0000:
  // that is the contract of the C ABI, so the binding has to refuse such text
  // before the call (checked in the Kotlin sources below).
  console.log(`note  jna  C ABI, text cut at U+0000 by contract: parse ${line.parse} · format ${JSON.stringify(line.format)} · parse_active ${line.active}`);

  // The Kotlin binding itself (no Gradle here to run its tests): every String
  // that crosses into C goes through `whole(…)`, which refuses U+0000, and
  // every `size_t` is the platform-wide `SizeT`.
  const kt = path.join(REPO, 'bindings', 'kotlin', 'src', 'main', 'kotlin', 'com', 'aperturesyndicate', 'synx');
  const engine = fs.readFileSync(path.join(kt, 'SynxEngine.kt'), 'utf-8');
  const unguarded = [...engine.matchAll(/lib\.(synx_\w+)\(([^)]*)\)/g)]
    .filter(([, fn, args]) => !/decompile|is_synxb|free/.test(fn) && args.split(',')[0].trim() !== '' && !/^whole\(/.test(args.split(',')[0].trim()))
    .map(([, fn]) => fn);
  report('jna', 'Kotlin: every text passed to synx-c goes through whole() (refuses U+0000)', unguarded.length ? `unguarded: ${unguarded.join(', ')}` : null);
  const lib = fs.readFileSync(path.join(kt, 'internal', 'LibSynx.kt'), 'utf-8');
  const narrow = [...lib.matchAll(/fun (synx_\w+)\(([^)]*)\)/g)].filter(([, , args]) => /len\s*:\s*NativeLong|:\s*Long\b/.test(args)).map(([, fn]) => fn);
  report('jna', 'Kotlin: size_t is SizeT (Native.SIZE_T_SIZE), not NativeLong', narrow.length ? `narrow size_t in: ${narrow.join(', ')}` : null);
}

for (const [name, run] of [['node', nodeAddon], ['python', pythonModule], ['jna', jna]]) {
  if (!only.includes(name)) continue;
  try { run(); } catch (e) { console.log(`skip  ${name}  not built — ${String(e.message).slice(0, 300)}`); }
}
console.log(`\n${failed ? `${failed} probe${failed === 1 ? '' : 's'} fail` : 'every probe passes'}`);
process.exit(failed ? 1 : 0);
