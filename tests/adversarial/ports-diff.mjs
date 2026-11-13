#!/usr/bin/env node
/**
 * The core against every port on inputs ports.mjs does not have: text built
 * from Unicode and layout edge cases (line endings, BOM, tabs against spaces,
 * Unicode spaces, combining marks, bidi controls, NFC/NFD twins), and — with
 * `--corpus` — every document of the compatibility corpus (production configs,
 * game files, conformance, examples), statically and resolved.
 *
 *   node tests/adversarial/ports-diff.mjs [--only go,java] [--corpus] [--max 12]
 *
 * Uses the port workers ports.mjs builds into tmp/ports (run it once first).
 * Differences are grouped by the generator's feature tags.
 *
 * Exit code 1 when a port reads an input differently from the core.
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { HERE, TMP, b64, normJson } from './lib/harness.mjs';
import { worker, localConfig } from './lib/impls.mjs';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(name); return i === -1 ? fallback : argv[i + 1]; };
const only = opt('--only', 'ts,go,java,dart,dotnet,cpp,swift').split(',');
const maxShown = Number(opt('--max', 12));
const exe = process.platform === 'win32' ? '.exe' : '';
const P = path.join(TMP, 'ports');
const PORTS = {
  go: [path.join(P, 'go', 'port' + exe), []],
  java: ['java', ['-Dfile.encoding=UTF-8', '-cp', path.join(P, 'java', 'classes'), 'Main']],
  dart: [path.join(P, 'dart', 'port' + exe), []],
  dotnet: [path.join(P, 'dotnet', 'bin', 'port' + exe), []],
  cpp: [path.join(P, 'cpp', 'port' + exe), []],
  swift: [path.join(P, 'swift-worker', '.build', 'release', 'port' + exe), []],
};

// ── Unicode and layout battery ───────────────────────────────
const cp = (...c) => String.fromCodePoint(...c);
const SPACES = { nbsp: cp(0xa0), ideo: cp(0x3000), en: cp(0x2002), nel: cp(0x85), ls: cp(0x2028), bom: cp(0xfeff), zwsp: cp(0x200b), tab: '\t', vt: '\v', ff: '\f' };
const inputs = [];
const add = (tags, text, op = 'parse') => inputs.push({ id: `u${inputs.length}`, tags, op, text });
for (const [eol, name] of [['\n', 'LF'], ['\r\n', 'CRLF'], ['\r', 'CR'], ['\n\r', 'LFCR']]) {
  add([`eol ${name}`], ['a 1', 'g', '  x 2', '  y', '    - 3', '    - 4', 'b |', '  line one', '  line two', 'c 5'].join(eol) + eol);
  add([`eol ${name} active`], ['!active', 'p:env:default:5 V', 'q:calc p * 2', 'l:join:,', '  - a', '  - b'].join(eol) + eol, 'active');
}
add(['eol mixed'], 'a 1\r\ng\n  x 2\r  y 3\r\n');
add(['eol trailing CR on value'], 'a 1\r\nb two words\r\n');
for (const [name, ch] of Object.entries(SPACES)) {
  add([`space ${name} after value`], `a value${ch}\nb 2\n`);
  add([`space ${name} before value`], `a${ch}value\nb 2\n`);
  add([`space ${name} as indent`], `g\n${ch}x 1\n${ch}${ch}y 2\nz 3\n`);
  add([`space ${name} inside key`], `a${ch}b 1\n`);
  add([`space ${name} line start`], `${ch}a 1\nb 2\n`);
  add([`space ${name} in (int)`], `n(int) ${ch}5${ch}\n`);
  add([`space ${name} in list item`], `l\n  -${ch}a\n  - ${ch}b${ch}\n`);
}
add(['tabs vs spaces'], 'g\n\tx 1\n  y 2\n\t  z 3\nw 4\n');
add(['tab then spaces deeper'], 'g\n\th\n\t\tx 1\n\t  y 2\n');
add(['tab in list indent'], 'l\n\t- a\n  - b\n');
add(['bom start'], `${SPACES.bom}a 1\nb 2\n`);
add(['bom start active'], `${SPACES.bom}!active\np:env:default:5 V\n`, 'active');
add(['bom mid line'], `a 1${SPACES.bom}2\n`);
add(['bom line start mid file'], `a 1\n${SPACES.bom}b 2\n`);
add(['bom before directive mid file'], `a 1\n${SPACES.bom}!active\np:env:default:5 V\n`, 'active');
add(['nfc nfd twin keys'], `${cp(0xe9)} 1\ne${cp(0x301)} 2\n`);
add(['combining mark key'], `a${cp(0x301)}${cp(0x302)} 1\n`);
add(['bidi in key and value'], `ab${cp(0x202e)}cd ef${cp(0x202e)}gh\n`);
add(['zwj in key'], `a${cp(0x200d)}b 1\n`);
add(['astral key and value'], `${cp(0x1f600)} ${cp(0x1f4a9)}\n${cp(0x10400)}${cp(0x10428)} x\n`);
add(['case twin keys'], 'Key 1\nkey 2\nKEY 3\n');
add(['digits outside ascii as number'], `a ${cp(0x663)}${cp(0x664)}\nb ${cp(0xff11)}\nc(int) ${cp(0x663)}\n`);
add(['superscript digits'], `a ${cp(0xb2)}\nb 1${cp(0xb2)}\n`);
add(['fullwidth minus and dot'], `a ${cp(0xff0d)}1\nb 1${cp(0xff0e)}5\nc ${cp(0x2212)}1\n`);
add(['bool and null look-alikes'], `a TRUE\nb True\nc nul${cp(0x200b)}l\nd ${cp(0xff54)}rue\n`);
add(['quote look-alikes'], `a ${cp(0x201c)}x${cp(0x201d)}\nb ${cp(0xff02)}y${cp(0xff02)}\n`);
add(['long combining run'], `a e${cp(0x301).repeat(2000)}\n`);
add(['comment markers after unicode space'], `a x${SPACES.nbsp}# c\nb y${SPACES.ideo}// d\n`);
add(['unicode space before marker colon'], `!active\nk${SPACES.nbsp}:env:default:5 V\n`, 'active');
add(['key ends with unicode space before constraint'], `!active\nk${SPACES.ideo}[min:1] 0\n`, 'active');

// ── corpus ───────────────────────────────────────────────────
if (argv.includes('--corpus')) {
  const root = path.join(localConfig().snapshotDir || '', 'corpus');
  const walk = (d) => fs.readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(path.join(d, e.name)) : e.name.endsWith('.synx') ? [path.join(d, e.name)] : []));
  for (const f of walk(root)) {
    const text = fs.readFileSync(f, 'utf-8');
    if (text.length > 4 << 20 || /:random\b|:once\b|:spam\b/.test(text)) continue;
    const rel = path.relative(root, f).replace(/\\/g, '/');
    inputs.push({ id: `c${inputs.length}`, tags: [`corpus ${rel.split('/')[0]}`], file: rel, op: 'parse', text });
    if (/^\s*(!active|#!mode:\s*active)/m.test(text)) inputs.push({ id: `c${inputs.length}`, tags: [`corpus ${rel.split('/')[0]} active`], file: rel, op: 'active', text });
  }
}

// ── the core ─────────────────────────────────────────────────
const base = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-portsdiff-'));
const core = worker('rust');
const want = new Map();
for (const x of inputs) {
  const r = await core.call({ op: x.op, text: x.text, env: {}, base }, 30000);
  want.set(x.id, r.ok ? normJson(r.out.json) : 'ERROR');
}
core.close();

const feed = inputs.map((x) => [x.id, x.op, b64(x.text), '', base].join('\t')).join('\n') + '\n';
let bad = 0;
for (const name of only) {
  const got = new Map();
  let r = { status: 0 };
  if (name === 'ts') {
    const w = worker('ts');
    for (const x of inputs) {
      const a = await w.call({ op: x.op, text: x.text, env: {}, base }, 30000);
      got.set(x.id, a.ok ? a.out.json : `ERROR: ${a.err || a.crash || 'timeout'}`);
    }
    w.close();
  } else {
    const [cmd, args] = PORTS[name];
    if (cmd !== 'java' && !fs.existsSync(cmd)) { console.log(`${name}: not built (run ports.mjs first)`); continue; }
    r = spawnSync(cmd, args, { input: feed, encoding: 'utf-8', maxBuffer: 1 << 30, timeout: 900000, windowsHide: true });
    for (const line of (r.stdout || '').split('\n')) {
      const f = line.replace(/\r$/, '').split('\t');
      if (f.length >= 2) got.set(f[0], Buffer.from(f[1], 'base64').toString('utf-8'));
    }
  }
  const byTag = new Map();
  let diffs = 0;
  for (const x of inputs) {
    const g = got.get(x.id);
    const a = g === undefined ? `NO ANSWER${r.status ? ` (exit ${r.status})` : ''}` : g.startsWith('ERROR') ? 'ERROR' : normJson(g);
    if (a === want.get(x.id)) continue;
    diffs++;
    const tag = x.tags[0];
    const e = byTag.get(tag) || { n: 0, x, a };
    e.n++;
    byTag.set(tag, e);
  }
  bad += diffs;
  console.log(`\n${name}: ${inputs.length - diffs} of ${inputs.length} as the core, ${diffs} differ in ${byTag.size} classes`);
  for (const [tag, e] of [...byTag.entries()].slice(0, maxShown)) {
    console.log(`  ${tag} ×${e.n}${e.x.file ? ` (${e.x.file})` : `  in ${JSON.stringify(e.x.text).slice(0, 90)}`}\n      ${name}: ${e.a.slice(0, 150)}\n      core: ${String(want.get(e.x.id)).slice(0, 150)}`);
  }
  if (byTag.size > maxShown) console.log(`  … ${[...byTag.keys()].slice(maxShown).join(' | ')}`);
}
fs.rmSync(base, { recursive: true, force: true });
process.exit(bad ? 1 : 0);
