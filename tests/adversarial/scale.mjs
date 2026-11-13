#!/usr/bin/env node
/**
 * Cost curves: how time and memory grow when the input doubles.
 *
 *   node tests/adversarial/scale.mjs                       every shape, rust and ts
 *   node tests/adversarial/scale.mjs --only inherit,alias  shapes whose name contains one of these
 *   node tests/adversarial/scale.mjs --impl rust           one implementation
 *   node tests/adversarial/scale.mjs --steps 4 --budget 8000
 *
 * Every shape is a function of a size `n`. It is run at n, 2n, 4n… in a fresh
 * process each time (so peak memory is that run's own), until a run takes
 * longer than `--budget` ms (default 4000) or `--steps` sizes are done
 * (default 4). Reported per shape and implementation:
 *
 *   bytes      input size of the largest run
 *   ms         time of the largest run, inside the worker
 *   ×time      time ratio of the last doubling (2 is linear, 4 quadratic)
 *   MiB        peak memory of the largest run
 *   ×mem       memory ratio of the last doubling, baseline process size removed
 *   amp        peak memory over input size
 *
 * A shape is flagged when the last doubling costs more than ×2.6 in time or
 * memory, when memory exceeds 100× the input, or when the run dies.
 *
 * Exit code 1 if anything is flagged.
 */
import fs from 'node:fs';
import { b64, verdict, isFailure } from './lib/harness.mjs';
import { runOnce } from './lib/impls.mjs';
import { SHAPES } from './lib/shapes.mjs';

const argv = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = argv.indexOf(name);
  return i === -1 ? fallback : argv[i + 1];
};
if (argv.includes('--help')) {
  process.stdout.write(fs.readFileSync(new URL(import.meta.url), 'utf-8').split('*/')[0].replace(/^#!.*\n\/\*\*\n?/, '').replace(/^ \* ?/gm, ''));
  process.exit(0);
}
const impls = opt('--impl', 'rust,ts').split(',');
const only = opt('--only', '');
const steps = Number(opt('--steps', '4'));
const budget = Number(opt('--budget', '4000'));

const BASELINE = {};
function baseline(impl) {
  if (BASELINE[impl] === undefined) {
    const r = runOnce(impl, { op: 'ping' }, { timeoutMs: 60000 });
    BASELINE[impl] = r.peak_mb || 0;
  }
  return BASELINE[impl];
}

let flagged = 0;
const rows = [];
for (const [name, [start, make]] of Object.entries(SHAPES)) {
  if (only && !only.split(',').some((p) => name.includes(p))) continue;
  for (const impl of impls) {
    const runs = [];
    let n = start;
    let died = null;
    for (let s = 0; s < steps; s++) {
      const c = make(n);
      const input = Buffer.isBuffer(c.input) ? c.input : Buffer.from(c.input);
      const r = runOnce(impl, { op: c.op, b64: b64(input), brief: true, ...(c.req || {}) }, { timeoutMs: budget * 6 });
      if (isFailure(r)) { died = `n=${n} (${input.length} bytes): ${String(verdict(r)).slice(0, 140)}`; break; }
      runs.push({ n, bytes: input.length, ms: r.ms, mb: Math.max(0, (r.peak_mb || 0) - baseline(impl)), note: r.ok ? '' : String(r.err || '').slice(0, 60) });
      if (r.ms > budget) break;
      n *= 2;
    }
    const last = runs[runs.length - 1];
    const prev = runs[runs.length - 2];
    const xt = last && prev && prev.ms > 2 ? last.ms / prev.ms : null;
    const xm = last && prev && prev.mb > 8 ? last.mb / prev.mb : null;
    const amp = last ? (last.mb * 1048576) / last.bytes : null;
    const why = [];
    if (died) why.push('DIED');
    if (xt !== null && xt > 2.6) why.push('time');
    if (xm !== null && xm > 2.6) why.push('memory');
    if (amp !== null && amp > 100 && last.mb > 64) why.push('amplification');
    if (why.length) flagged++;
    rows.push({ name, impl, last, xt, xm, amp, why, died });
    const f = (v, d = 1) => (v === null || v === undefined ? '   -' : v.toFixed(d).padStart(6));
    console.log(`${why.length ? '!!' : '  '} ${name.padEnd(58)} ${impl.padEnd(5)} ${last ? String(last.bytes).padStart(9) : '        -'} B ${last ? f(last.ms, 0) : '     -'} ms ×${f(xt, 2)} ${last ? f(last.mb, 0) : '     -'} MiB ×${f(xm, 2)} amp ${f(amp, 0)}  ${why.join(',')}${died ? '  ' + died : ''}${last && last.note ? '  (' + last.note + ')' : ''}`);
  }
}
console.log(`\n${flagged} flagged of ${rows.length}`);
process.exit(flagged ? 1 : 0);
