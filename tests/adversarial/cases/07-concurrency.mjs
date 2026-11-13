/**
 * Several processes resolving documents in one directory at the same time.
 * `:once` promises a value that is generated once and then stays: every value
 * a process was given must still be the value on the next read.
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { b64 } from '../lib/harness.mjs';
import { worker } from '../lib/impls.mjs';

const PROCESSES = 6;
const ROUNDS = 12;
const KEYS = 3;
const docOf = (w, r) => '!active\n' + Array.from({ length: KEYS }, (_, k) => `id_${w}_${r}_${k}:once\n`).join('');

async function onceSurvives(impl) {
  const base = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-once-race-'));
  try {
    const first = new Map();
    const workers = Array.from({ length: PROCESSES }, () => worker(impl));
    await Promise.all(workers.map(async (wk, w) => {
      for (let r = 0; r < ROUNDS; r++) {
        const resp = await wk.call({ op: 'active', b64: b64(docOf(w, r)), base }, 20000);
        if (resp.ok) for (const [k, v] of Object.entries(JSON.parse(resp.out.json))) first.set(k, v);
      }
    }));
    workers.forEach((w) => w.close());
    const single = worker(impl);
    let changed = 0;
    for (let w = 0; w < PROCESSES; w++) {
      for (let r = 0; r < ROUNDS; r++) {
        const resp = await single.call({ op: 'active', b64: b64(docOf(w, r)), base }, 20000);
        if (resp.ok) for (const [k, v] of Object.entries(JSON.parse(resp.out.json))) if (first.get(k) !== v) changed++;
      }
    }
    single.close();
    return changed ? `${impl}: ${changed} of ${first.size} :once values differ on the next read` : null;
  } finally {
    fs.rmSync(base, { recursive: true, force: true });
  }
}

export default [
  {
    id: 'B-065', zone: 'both', title: `:once values survive ${PROCESSES} processes writing one .synx.lock`,
    run: async (impls) => {
      for (const impl of impls) {
        const problem = await onceSurvives(impl);
        if (problem) return problem;
      }
      return null;
    },
  },
];
