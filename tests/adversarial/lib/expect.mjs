/**
 * What a regression case may demand of the answers. Each helper returns a
 * function `(view) => null | "what went wrong"`; `all(...)` chains them.
 *
 * `view` holds, per implementation name:
 *   view.answers[name]  the raw answer of the worker
 *   view.shown[name]    its comparable text, numbers normalised
 *   view.out(name)      the `out` object of a successful answer, else {}
 */
import { isFailure, verdict } from './harness.mjs';

const clip = (s, n = 220) => (s.length > n ? s.slice(0, n) + '…' : s);

export const all = (...checks) => (view) => {
  for (const c of checks) {
    const r = c(view);
    if (r) return r;
  }
  return null;
};

/** No crash, hang, panic or untyped exception anywhere. */
export const survive = (view) => {
  for (const [name, r] of Object.entries(view.answers)) {
    if (isFailure(r)) return `${name}: ${clip(String(verdict(r)))}`;
  }
  return null;
};

/** Every implementation gives the same answer, and none of them fails. */
export const agree = (view) => {
  const failed = survive(view);
  if (failed) return failed;
  const entries = Object.entries(view.shown);
  const first = entries[0][1];
  const other = entries.find(([, v]) => v !== first);
  return other ? `${entries[0][0]}: ${clip(first)}\n      ${other[0]}: ${clip(other[1])}` : null;
};

/** Every implementation answers exactly `value` (numbers normalised). */
export const gives = (value) => (view) => {
  const failed = survive(view);
  if (failed) return failed;
  for (const [name, v] of Object.entries(view.shown)) {
    if (v !== value) return `${name}: ${clip(v)}\n      wanted: ${clip(value)}`;
  }
  return null;
};

/** No implementation's answer contains `text`. */
export const lacks = (text) => (view) => {
  for (const [name, r] of Object.entries(view.answers)) {
    if (JSON.stringify(r).includes(text)) return `${name}: the answer contains ${JSON.stringify(text)}`;
  }
  return null;
};

/** Every answer contains `text`. */
export const has = (text) => (view) => {
  for (const [name, v] of Object.entries(view.shown)) {
    if (!v.includes(text)) return `${name}: ${clip(v)}\n      wanted it to contain ${JSON.stringify(text)}`;
  }
  return null;
};

/** Each implementation answers within `ms` milliseconds (its own clock). */
export const within = (ms) => (view) => {
  for (const [name, r] of Object.entries(view.answers)) {
    if (r && r.timeout) return `${name}: no answer in ${r.ms} ms`;
    if (r && typeof r.ms === 'number' && r.ms > ms) return `${name}: ${Math.round(r.ms)} ms, limit ${ms} ms`;
  }
  return null;
};

/** Peak memory of the fresh process stays under `mb` MiB (cases with `once: true`). */
export const peakUnder = (mb) => (view) => {
  for (const [name, r] of Object.entries(view.answers)) {
    if (r && typeof r.peak_mb === 'number' && r.peak_mb > mb) return `${name}: peak ${r.peak_mb} MiB, limit ${mb} MiB`;
  }
  return null;
};

/** A field of the `out` object must satisfy `test` in every implementation. */
export const field = (name, test, wanted = 'the expected value') => (view) => {
  const failed = survive(view);
  if (failed) return failed;
  for (const impl of Object.keys(view.answers)) {
    const v = view.out(impl)[name];
    if (!test(v, view.out(impl))) return `${impl}: ${name} = ${clip(JSON.stringify(v) ?? 'undefined')}, wanted ${wanted}`;
  }
  return null;
};

/** `*_check` operations: the tree before and after must be the same. */
export const preserved = (before = 'p0', after = 'p1') => (view) => {
  const failed = survive(view);
  if (failed) return failed;
  for (const impl of Object.keys(view.answers)) {
    const o = view.out(impl);
    if (o[after] !== undefined && o[before] !== o[after]) {
      return `${impl}: ${clip(String(o[before]), 160)}\n      became: ${clip(String(o[after]), 160)}`;
    }
  }
  return null;
};
