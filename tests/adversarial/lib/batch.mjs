/**
 * Send a list of requests to several implementations and report, per request,
 * what each said and whether they agree. The exploration scripts and the
 * regression runner are both built on this.
 */
import { b64, normJson, verdict, isFailure } from './harness.mjs';
import { worker, runOnce } from './impls.mjs';

const FIELD_OF = { format: 'text', stringify: 'text', decompile: 'text', compile: 'b64', calc: 'num' };

export function fieldFor(op, explicit) {
  return explicit || FIELD_OF[op] || 'json';
}

/** Turn a case's input into request fields. */
export function requestOf(c) {
  const req = { op: c.op, ...(c.req || {}) };
  const input = typeof c.input === 'function' ? c.input() : c.input;
  if (input !== undefined) req.b64 = b64(input);
  const second = typeof c.second === 'function' ? c.second() : c.second;
  if (second !== undefined) req.b64b = b64(second);
  if (c.units) req.units = c.units;
  return req;
}

export class Pool {
  constructor() {
    this.workers = new Map();
  }

  get(name) {
    if (!this.workers.has(name)) this.workers.set(name, worker(name));
    return this.workers.get(name);
  }

  /** Ask every named implementation; returns `{ name: answer }`. */
  async ask(c, impls = ['rust', 'ts']) {
    const req = requestOf(c);
    const answers = {};
    for (const name of impls) {
      answers[name] = c.once
        ? runOnce(name, req, { timeoutMs: c.timeout || 120000 })
        : await this.get(name).call(req, c.timeout || 20000);
    }
    return answers;
  }

  close() {
    for (const w of this.workers.values()) w.close();
    this.workers.clear();
  }
}

/** One comparable string per implementation, numbers normalised. */
export function shown(answers, op, field) {
  const f = fieldFor(op, field);
  const out = {};
  for (const [name, resp] of Object.entries(answers)) {
    const v = verdict(resp, f);
    out[name] = typeof v === 'string' ? normJson(v) : JSON.stringify(v);
  }
  return out;
}

export function failures(answers) {
  return Object.entries(answers).filter(([, r]) => isFailure(r)).map(([n, r]) => `${n}: ${verdict(r)}`);
}

export function disagree(answers, op, field) {
  return new Set(Object.values(shown(answers, op, field))).size > 1;
}
