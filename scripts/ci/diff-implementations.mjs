#!/usr/bin/env node
/**
 * Cross-implementation differential check for SYNXL.
 *
 * SYNXL §1.4.3 makes conformance a byte-for-byte comparison against the
 * reference implementation, so "both suites are green" is not the same as "both
 * agree": each suite compares one implementation against fixtures, and a
 * divergence on any input the fixtures do not cover goes unnoticed. The audit
 * that prompted this script found 21 such divergences while all 67 conformance
 * cases passed on both sides.
 *
 * Every input goes through the Rust CLI and the TypeScript package, and both
 * projections (§12.1 array, §12.2 NDJSON) are compared byte for byte, along
 * with whether the document was accepted at all (§11.1).
 *
 * Usage:  node scripts/ci/diff-implementations.mjs
 *
 * Expects `cargo build -p synx-cli` and `npm --prefix packages/synx-js run
 * build` to have run first. Exits non-zero on the first divergence, listing all
 * of them.
 */
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath, pathToFileURL } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const CASES = join(ROOT, 'tests', 'conformance-synxl', 'cases');
const EXE = process.platform === 'win32' ? '.exe' : '';
const SYNX = [
  join(ROOT, 'target', 'debug', `synx${EXE}`),
  join(ROOT, 'target', 'release', `synx${EXE}`),
].find(existsSync);

if (!SYNX) {
  console.error('error: no synx binary found — run `cargo build -p synx-cli` first');
  process.exit(2);
}

const distEntry = join(ROOT, 'packages', 'synx-js', 'dist', 'synxl.js');
if (!existsSync(distEntry)) {
  console.error('error: packages/synx-js/dist is missing — run `npm --prefix packages/synx-js run build` first');
  process.exit(2);
}
const { synxlToJSON, synxlToNDJSON } = await import(pathToFileURL(distEntry).href);

/** Run the Rust CLI over a file, capturing acceptance and both projections. */
function rust(path) {
  const run = format => {
    try {
      return {
        ok: true,
        out: execFileSync(SYNX, ['synxl', 'parse', path, '-f', format], {
          encoding: 'utf-8',
          stdio: ['ignore', 'pipe', 'ignore'],
          maxBuffer: 64 * 1024 * 1024,
        }),
      };
    } catch {
      return { ok: false, out: '' };
    }
  };
  const json = run('json');
  const ndjson = run('ndjson');
  // §12.1 output is newline-terminated by the CLI; the library projections are
  // compared on their own terms below.
  return { ok: json.ok, json: json.out.trimEnd(), ndjson: ndjson.out };
}

/** Run the TypeScript package over the same text. */
function typescript(text) {
  try {
    return { ok: true, json: synxlToJSON(text), ndjson: synxlToNDJSON(text) };
  } catch {
    return { ok: false, json: '', ndjson: '' };
  }
}

/**
 * Inputs beyond the corpus.
 *
 * Each one is a minimised divergence the audit found, kept so that a
 * regression in either implementation shows up here rather than in a dataset.
 */
const EXTRA = [
  ['unicode-indent-in-pipe-body', '!synxl 1\n!fields id ; m[block]\n1\n  m |+\n    a\n    b\n'],
  ['comments-in-pipe-body', '!synxl 1\n!fields id ; b[block]\n1\n  b |+\n      A\n      # hash\n      // slash\n      B\n'],
  ['hash-fence-in-body', '!synxl 1\n!fields a[block] ; id\n1\n  a |+\n      x\n      ###\n      y\n'],
  ['directive-in-body', '!synxl 1\n!fields id ; b[block]\n1\n  b |+\n      one\n      !include secret\n      two\n'],
  ['random-hint-in-block', '!synxl 1\n!fields id ; b[block]\n1\n  b\n    r1(random) x\n'],
  ['all-null-with-nbsp', '!synxl 1\n!fields a ; b\n; \n'],
  ['indented-line-in-block-comment', '!synxl 1\n###\n  x\n###\n!fields a\n1\n'],
  ['block-key-line-attribution', '!synxl 1\n!fields id\n1\n    deep 1\n  shallow 2\n'],
  ['bom-mid-document', '!synxl 1﻿\n!fields a\n1\n'],
  ['nel-after-prologue', '!synxl 1\n!fields a\n1\n'],
  ['indented-prologue', ' !synxl 1\n!fields a\n1\n'],
  ['integers-around-i64', '!synxl 1\n!fields a ; b ; c\n1526947345124249601 ; 9223372036854775807 ; 9223372036854775808\n'],
  ['typed-int-overflow', '!synxl 1\n!fields v[type:int]\n9223372036854775808\n'],
  ['reserved-js-field-names', '!synxl 1\n!fields id ; __proto__ ; __synx_meta\n1 ; boo ; m\n'],
  ['numeric-looking-keys', '!synxl 1\n!fields 2 ; 10 ; a\nx ; y ; z\n'],
  ['block-root-is-a-list', '!synxl 1\n!fields id\n1\n  -\n'],
  ['negative-zero-float', '!synxl 1\n!fields a(float) ; b(float)\n-0.0 ; 1.5\n'],
  ['control-character-escapes', '!synxl 1\n!fields a\n"xy"\n'],
  ['nbsp-indent-block-key', '!synxl 1\n!fields id ; b[block]\n1\n b x\n'],
  ['quote-fallback', '!synxl 1\n!fields a ; b\n"a"b" ; 2\n'],
  ['crlf-document', '!synxl 1\r\n!fields a ; b\r\n1 ; 2\r\n'],
  ['leading-bom', '﻿!synxl 1\n!fields a\n1\n'],
];

const dir = mkdtempSync(join(tmpdir(), 'synxl-diff-'));
const problems = [];
let checked = 0;

function compare(label, path, text) {
  checked++;
  const r = rust(path);
  const t = typescript(text);

  if (r.ok !== t.ok) {
    problems.push(
      `${label}: one implementation accepted the document and the other rejected it\n` +
      `    rust: ${r.ok ? 'accepted' : 'rejected'}\n` +
      `    ts:   ${t.ok ? 'accepted' : 'rejected'}`,
    );
    return;
  }
  if (!r.ok) return; // both rejected — §11.1 agreement is all that is required

  if (r.json !== t.json) {
    problems.push(
      `${label}: §12.1 array projections differ\n    rust: ${r.json}\n    ts:   ${t.json}`,
    );
  }
  if (r.ndjson !== t.ndjson) {
    problems.push(
      `${label}: §12.2 NDJSON projections differ\n    rust: ${JSON.stringify(r.ndjson)}\n    ts:   ${JSON.stringify(t.ndjson)}`,
    );
  }
}

try {
  for (const file of readdirSync(CASES).filter(n => n.endsWith('.synxl')).sort()) {
    compare(file, join(CASES, file), readFileSync(join(CASES, file), 'utf-8'));
  }
  for (const [name, text] of EXTRA) {
    const path = join(dir, `${name}.synxl`);
    writeFileSync(path, text, 'utf-8');
    compare(name, path, text);
  }
} finally {
  rmSync(dir, { recursive: true, force: true });
}

if (problems.length > 0) {
  console.error(`\n${problems.length} divergence(s) across ${checked} documents:\n`);
  for (const p of problems) console.error(`  ${p}\n`);
  console.error('SYNXL §1.4.3 requires the projections to be byte-identical.\n');
  process.exit(1);
}

console.log(`ok: Rust and TypeScript agree on all ${checked} documents`);
