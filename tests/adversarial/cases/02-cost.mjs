/**
 * Small documents that cost far more than their size: quadratic time,
 * memory in the hundreds of megabytes, or an exception when a string outgrows
 * the runtime. Each runs in a fresh process, so the peak is its own.
 *
 * The limits are an order of magnitude above what a linear, bounded
 * implementation needs for the same input on a desktop machine.
 * Curves for every shape: `node tests/adversarial/scale.mjs --only "<name>"`.
 */
import { all, peakUnder, survive, within } from '../lib/expect.mjs';
import { SHAPES } from '../lib/shapes.mjs';

/** A cost case over a named shape at size `n`. */
function cost(id, zone, shape, n, ms, mb, extra = {}) {
  const make = SHAPES[shape][1];
  return {
    id, zone, title: `${shape}, n=${n}: under ${ms} ms and ${mb} MiB`,
    once: true, timeout: Math.max(20000, ms * 8),
    op: make(1).op,
    input: () => make(n).input,
    req: () => ({ brief: true, ...(make(n).req || {}) }),
    expect: all(survive, within(ms), peakUnder(mb)),
    ...extra,
  };
}

export default [
  // References and templates that multiply a value.
  cost('B-030.1', 'both', 'active: doubling chain of {…}', 28, 2000, 300),
  cost('B-030.2', 'both', 'active: {big} in many values', 3000, 2000, 300),
  cost('B-030.3', 'both', 'active: :alias of one big group', 1500, 1500, 300),
  cost('B-030.4', 'both', 'active: :ref of one big group', 1500, 1500, 300),
  cost('B-030.5', 'both', 'active: :prompt of a group, via :ref, many times', 600, 1500, 300),
  cost('B-031.1', 'both', 'active: :inherit, many children of one wide parent', 2000, 2000, 300),
  cost('B-031.2', 'both', 'active: :inherit with markers everywhere', 1200, 2000, 300),
  cost('B-031.3', 'both', 'active: :inherit chain', 2000, 2000, 300),
  cost('B-032.1', 'both', 'active: :replace, long replacement', 8000, 1500, 300),
  cost('B-032.2', 'both', 'active: :join, long separator', 8000, 1500, 300),

  // Quadratic time with modest memory.
  cost('B-033', 'ts', 'active: :calc with a key containing a dash', 4000, 1500, 400),
  cost('B-034', 'core', 'active: key redeclared, metadata elsewhere', 6000, 1500, 400),
  cost('B-035', 'core', 'parse: ancestor key and its groups both growing', 128000, 1500, 400),
  cost('B-036', 'core', 'active: ancestor key and its marked keys both growing', 128000, 1500, 600),
  cost('B-037', 'ts', 'active: :once, many keys', 1200, 1500, 300),

  // .synxb containers a few kilobytes long.
  cost('B-038.1', 'both', 'decompile: array of nulls behind a zero bomb', 8 << 20, 3000, 256),
  cost('B-038.2', 'both', 'decompile: object count far above its content', 8 << 20, 3000, 256),
  cost('B-038.3', 'both', 'decompile: one long string referenced many times', 6000, 3000, 256),
  cost('B-038.4', 'both', 'decompile: declared size only', 256 << 20, 3000, 64),
];
