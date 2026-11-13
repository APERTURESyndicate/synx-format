/**
 * Hostile `.synxb` containers and signature strings. A refusal is fine — a
 * panic, an untyped exception or two different readings are not.
 */
import { agree, all, field, has, lacks, survive } from '../lib/expect.mjs';
import { T, U64_MAX, container, stringTable, varint } from '../lib/synxb.mjs';

const obj1 = (strings, ...rest) => container(Buffer.concat([stringTable(strings), Buffer.from([T.obj]), varint(1), ...rest]));

/** Both implementations refuse the container, each with its own typed error. */
const bothRefuse = (view) => {
  const failed = survive(view);
  if (failed) return failed;
  for (const [name, r] of Object.entries(view.answers)) {
    if (r.ok) return `${name}: accepted — ${JSON.stringify(r.out).slice(0, 120)}`;
  }
  return null;
};

export default [
  { id: 'B-018.1', zone: 'core', title: 'string-table length 2^64-1', op: 'decompile', input: container(Buffer.concat([varint(1), varint(U64_MAX)])), expect: bothRefuse },
  { id: 'B-018.2', zone: 'core', title: 'string-table length 2^64-6 with bytes after it', op: 'decompile', input: container(Buffer.concat([varint(1), varint(U64_MAX - 5n), Buffer.from('abcdefgh')])), expect: bothRefuse },
  { id: 'B-019.1', zone: 'ts', title: 'varint longer than ten bytes', op: 'decompile', input: container(Buffer.from([0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01])), expect: bothRefuse },
  { id: 'B-019.2', zone: 'ts', title: 'payload ends inside a varint', op: 'decompile', input: container(Buffer.concat([varint(0), Buffer.from([T.int, 0x80])])), expect: bothRefuse },
  { id: 'B-019.3', zone: 'ts', title: 'string table that is not UTF-8', op: 'decompile', input: container(Buffer.concat([varint(1), varint(2), Buffer.from([0xff, 0xfe]), Buffer.from([T.obj]), varint(1), varint(0), Buffer.from([T.nil])])), expect: bothRefuse },
  { id: 'B-019.4', zone: 'ts', title: 'deflate stream cut short', op: 'decompile', input: Buffer.from('SYNXB\x01\x00\x10\x00\x00\x00', 'latin1'), expect: bothRefuse },
  { id: 'B-020', zone: 'core', title: 'string index 2^32 is out of range, not index 0', op: 'decompile', input: obj1(['zero', 'k'], varint(1), Buffer.from([T.str]), varint(1n << 32n)), expect: bothRefuse },

  // decompile must not write text that parses to something else than the container holds.
  {
    id: 'B-021.1', zone: 'both', title: 'a key with a line break does not become directives and keys',
    op: 'decompile', input: obj1(['a\n!active\nleak:env PATH', 'v'], varint(0), Buffer.from([T.str]), varint(1)),
    expect: all(survive, lacks('\\n!active\\n')),
  },
  {
    id: 'B-021.2', zone: 'both', title: 'a list item with a line break does not become directives and keys',
    op: 'decompile', input: obj1(['k', 'x\n!active\nleak:env PATH'], varint(0), Buffer.from([T.arr]), varint(1), Buffer.from([T.str]), varint(1)),
    expect: all(survive, lacks('\\n!active\\n')),
  },
  {
    id: 'B-021.3', zone: 'both', title: 'JSON with a line break in a key converts to SYNX that reads back the same',
    op: 'json2synx', input: JSON.stringify({ 'a\n!active\nleak:env PATH': 1, list: ['x\n!include ../../secret.synx s', 'y'] }),
    expect: all(survive, field('active1', (v) => v === false, 'false: the text must stay static'), field('includes1', (v) => v === 0, '0 includes')),
  },
  {
    id: 'B-021.4', zone: 'both', title: 'JSON keys SYNX cannot write are renamed: none lost, none merged, no marker or directive made',
    op: 'json2synx', input: JSON.stringify({ 'my key': 1, 'port:env': 'HOME', 'x[min:1]': 2, '#c': 3, '- d': 4, 'e(int)': '5', '': 6 }),
    expect: all(survive, field('p1', (v, o) => {
      const before = JSON.parse(o.p0);
      const after = JSON.parse(v);
      const values = (t) => Object.values(t).map((x) => JSON.stringify(x)).sort().join();
      return Object.keys(after).length === Object.keys(before).length && values(after) === values(before);
    }, 'as many keys, with the same values'), field('active1', (v) => v === false, 'false: the text must stay static')),
  },

  // Signatures.
  { id: 'B-022.1', zone: 'core', title: 'hex with a multi-byte character', impls: ['rust'], op: 'pubkey', input: 'aéb', expect: survive },
  { id: 'B-022.2', zone: 'core', title: 'signature hex with a multi-byte character', impls: ['rust'], op: 'sig', input: 'aéb:00', expect: survive },
  { id: 'B-022.3', zone: 'core', title: 'hex digits only: "+1" is not a byte', impls: ['rust'], op: 'pubkey', input: '+1'.repeat(32), expect: (v) => (v.answers.rust.ok ? 'accepted "+1+1…" as a 32-byte key' : null) },
  {
    id: 'B-023', zone: 'core', title: 'one signature string must not verify for arbitrary data',
    impls: ['rust'], op: 'sig', second: 'any data at all',
    input: '01' + '00'.repeat(31) + '00'.repeat(32) + ':' + '01' + '00'.repeat(31),
    expect: (v) => (v.answers.rust.ok && v.answers.rust.out.verified ? 'verified: identity-point key with R = identity, S = 0 signs everything' : null),
  },
  // Keys of order 2 and 8: the same fixed signature holds for a half and an eighth of all inputs.
  ...[['order 2', 'ec' + 'ff'.repeat(30) + '7f'], ['order 8', '26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05']].flatMap(([label, key], k) =>
    Array.from({ length: 12 }, (_, i) => ({
      id: `B-023.${k + 2}.${i + 1}`, zone: 'core', title: `a key of ${label} verifies nothing (archive #${i})`,
      impls: ['rust'], op: 'sig', second: `archive bytes #${i}`,
      input: '01' + '00'.repeat(31) + '00'.repeat(32) + ':' + key,
      expect: (v) => (v.answers.rust.ok && v.answers.rust.out.verified ? 'verified with a small-order key' : null),
    }))),
];
