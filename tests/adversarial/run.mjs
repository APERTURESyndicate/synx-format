#!/usr/bin/env node
/**
 * Regression suite of inputs that broke an implementation: panics, hangs,
 * super-linear cost, Rust/TypeScript disagreement, leaks, escapes.
 *
 *   node tests/adversarial/run.mjs                 every case
 *   node tests/adversarial/run.mjs --only B-014    cases whose id starts with B-014
 *   node tests/adversarial/run.mjs --zone core     cases of one area (core, ts, tools)
 *   node tests/adversarial/run.mjs --list          ids and titles, nothing is run
 *   node tests/adversarial/run.mjs --verbose       show every answer, not only failures
 *   node tests/adversarial/run.mjs --ts ts-bun     the TypeScript side on Bun instead of Node
 *
 * Both workers are built from the working tree first (cargo for fuzz/, tsc for
 * packages/synx-js/src). Cases live in cases/*.mjs; each names the operation,
 * the input and what the answers have to satisfy (lib/expect.mjs).
 *
 * Exit code: 0 all pass, 1 some fail, 2 setup error.
 */
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { HERE } from './lib/harness.mjs';
import { Pool, shown } from './lib/batch.mjs';

const argv = process.argv.slice(2);
const opt = (name) => {
  const i = argv.indexOf(name);
  return i === -1 ? null : argv[i + 1];
};
if (argv.includes('--help')) {
  process.stdout.write(fs.readFileSync(new URL(import.meta.url), 'utf-8').split('*/')[0].replace(/^#!.*\n\/\*\*\n?/, '').replace(/^ \* ?/gm, ''));
  process.exit(0);
}

const cases = [];
const dir = path.join(HERE, 'cases');
for (const file of fs.readdirSync(dir).filter((f) => f.endsWith('.mjs')).sort()) {
  const mod = await import(pathToFileURL(path.join(dir, file)).href);
  for (const c of mod.default) cases.push({ ...c, file });
}

const only = opt('--only');
const zone = opt('--zone');
const selected = cases.filter((c) => (!only || only.split(',').some((p) => c.id.startsWith(p))) && (!zone || c.zone === zone));
if (argv.includes('--list')) {
  for (const c of selected) console.log(`${c.id.padEnd(8)} ${c.zone.padEnd(6)} ${c.title}`);
  process.exit(0);
}
if (selected.length === 0) {
  console.error('no case selected');
  process.exit(2);
}

// Workers inherit this variable; no case passes it as `env`, so a document
// must never be able to read it (see B-056).
process.env.SYNX_ADVERSARIAL_CANARY = 'canary-from-the-host';

const verbose = argv.includes('--verbose');
// `--ts ts-bun` runs the TypeScript side on Bun instead of Node.
const tsImpl = opt('--ts') || 'ts';
const pool = new Pool();
let failed = 0;
const started = Date.now();
for (const c of selected) {
  const impls = (c.impls || ['rust', 'ts']).map((n) => (n === 'ts' ? tsImpl : n));
  // A case that needs more than one request (several processes at once, say)
  // brings its own `run(impls)` and returns the problem it found, or null.
  if (c.run) {
    const problem = await c.run(impls);
    if (problem) failed++;
    if (problem || verbose) console.log(`${problem ? 'FAIL' : 'pass'}  ${c.id}  [${c.zone}]  ${c.title}${problem ? `\n      ${problem}` : ''}`);
    continue;
  }
  let answers;
  try {
    if (c.setup) c.ctx = c.setup();
    answers = await pool.ask({ ...c, req: typeof c.req === 'function' ? c.req(c.ctx) : c.req, input: typeof c.input === 'function' ? () => c.input(c.ctx) : c.input }, impls);
  } catch (e) {
    console.error(`setup error in ${c.id}: ${e.message}`);
    pool.close();
    process.exit(2);
  } finally {
    if (c.cleanup) try { c.cleanup(c.ctx); } catch { /* scratch only */ }
  }
  const view = {
    answers,
    shown: shown(answers, c.op, c.field),
    out: (name) => (answers[name] && answers[name].ok && answers[name].out) || {},
  };
  const problem = c.expect(view);
  if (problem) failed++;
  if (problem || verbose) {
    console.log(`${problem ? 'FAIL' : 'pass'}  ${c.id}  [${c.zone}]  ${c.title}`);
    if (problem) console.log(`      ${problem}`);
    if (verbose) for (const [name, v] of Object.entries(view.shown)) console.log(`      ${name}: ${v.length > 300 ? v.slice(0, 300) + '…' : v}  [${answers[name] && answers[name].ms} ms]`);
  }
}
pool.close();
console.log(`\n${selected.length - failed} of ${selected.length} cases pass, ${failed} fail  (${((Date.now() - started) / 1000).toFixed(1)} s)`);
process.exit(failed ? 1 : 0);
