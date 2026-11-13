// One probe of the native Node addon per process: a probe that kills the
// process must not take the others with it.
//   node probe.cjs <synx_node.node> <probe>
// Prints one JSON line: { ok: true, value } or { ok: false, thrown }.
const addon = require(process.argv[2]);

const nested = (open, close, n, leaf = '1') => JSON.parse(open.repeat(n) + leaf + close.repeat(n));
const probes = {
  // 24 KB and 36 KB of JSON text.
  'deep-array': () => addon.stringify(nested('[', ']', 12000)).length,
  'deep-object': () => addon.stringify(nested('{"a":', '}', 6000)).length,
  'deep-diff': () => Object.keys(addon.diff(nested('{"a":', '}', 6000), {})).length,
  cycle: () => { const v = { a: 1 }; v.self = v; return addon.stringify(v).length; },
  'cycle-diff': () => { const v = { a: 1 }; v.self = v; return Object.keys(addon.diff(v, { a: 2 })).length; },
  sparse: () => { const a = []; a.length = 4294967295; return addon.stringify({ a }).length; },
  // What the text says, digit for digit.
  int53: () => String(addon.parse('id 9007199254740993\n').id),
  int64: () => String(addon.parseActive('!active\nmax 9223372036854775807\n', { env: {} }).max),
  'bigint-out': () => addon.stringify({ id: 9007199254740993n }),
  'two-to-63': () => addon.stringify({ n: 9223372036854775808 }),
  'nul-in-key': () => JSON.stringify(addon.parse('a\u0000b 1\nc 2\n')),
  'surrogate-key': () => addon.stringify({ '\udc00': 1 }),
};

let answer;
try {
  answer = { ok: true, value: probes[process.argv[3]]() };
} catch (e) {
  answer = { ok: false, thrown: `${e.name}: ${String(e.message).slice(0, 200)}` };
}
process.stdout.write(JSON.stringify(answer) + '\n');
