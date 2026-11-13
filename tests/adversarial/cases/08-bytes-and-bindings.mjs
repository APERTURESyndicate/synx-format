/**
 * Input that arrives as bytes rather than as text: a file on disk, a buffer.
 * Bytes that are not UTF-8 have no text to parse — the reader has to refuse
 * them, as the command-line tool and the file markers (:include, :watch) do.
 * Replacing them with U+FFFD loads two different files as one document.
 */
const bytes = (...parts) => Buffer.concat(parts.map((p) => (typeof p === 'string' ? Buffer.from(p, 'utf-8') : Buffer.from(p))));

/** Every implementation asked must answer with an error. */
const refused = (view) => {
  for (const [name, r] of Object.entries(view.answers)) {
    if (r.crash) return `${name}: ${r.crash}`;
    if (r.ok) return `${name}: loaded as ${String(r.out.json).slice(0, 120)}`;
  }
  return null;
};

const file = (id, title, name, input) => ({ id, zone: 'ts', title, op: 'load_file', input, req: { name }, impls: ['ts'], expect: refused });

/** Every implementation gives exactly this tree. */
const gives = (want) => (view) => {
  for (const [name, v] of Object.entries(view.shown)) if (v !== want) return `${name}: ${v}`;
  return null;
};

export default [
  // A key the parser refuses (`__proto__`, `constructor`, `prototype`) takes its block with it:
  // the lines under it are not keys of the object that happened to be open above.
  {
    id: 'B-076.1', zone: 'both', title: 'the block under a refused key does not land in the previous sibling',
    op: 'parse', input: 'user\n  profile\n    name x\n  __proto__\n    role admin\n    name y\n  age 3\n',
    expect: gives('{"user":{"age":3,"profile":{"name":"x"}}}'),
  },
  {
    id: 'B-076.2', zone: 'both', title: 'the block under a refused key does not overwrite a key of its parent',
    op: 'active', input: '!active\nlimits\n  max 10\n  constructor\n    max 99\nuse:calc limits.max * 2\n',
    expect: gives('{"limits":{"max":10},"use":20}'),
  },
  // JSON → SYNX → the same tree: empty containers inside a list.
  ...[
    ['B-081.1', 'an empty object as the last key of a list item that has a successor', { servers: [{ name: 'a', tags: {} }, { name: 'b' }] }],
    ['B-081.2', 'a list that holds an empty list', { c: [[]], after: 1 }],
    ['B-081.3', 'a list that holds an empty object', { d: [{}], after: 1 }],
  ].map(([id, title, value]) => ({
    id, zone: 'both', title: `stringify: ${title}`, op: 'json2synx', input: JSON.stringify(value), field: 'p1',
    expect: (view) => {
      for (const [name, r] of Object.entries(view.answers)) {
        if (!r.ok) return `${name}: ${r.err || r.panic || r.crash}`;
        if (r.out.p1 !== r.out.p0) return `${name}: ${r.out.p0} became ${r.out.p1} via ${JSON.stringify(r.out.text)}`;
      }
      return null;
    },
  })),

  // text → .synxb (declarations kept) → text: the tree a reader gets stays the same.
  ...[
    ['B-082.1', 'a key declared (int) and declared again with a value', '!active\nx(int)\nx h\n'],
    ['B-082.2', 'a key declared :join and declared again as a group', '!active\nb:join\nb\n'],
    ['B-082.3', 'a key declared :inherit and declared again with a value', '!active\na:inherit\na x\n'],
    ['B-082.4', '(int) on an empty | block', '!active\nb(int)|\n'],
    ['B-082.5', 'an empty list', '!active\nt:\n- 2\n'],
  ].map(([id, title, input]) => ({
    id, zone: 'both', title: `compile and decompile: ${title}`, op: 'compile_check', input, field: 'p2',
    expect: (view) => {
      for (const [name, r] of Object.entries(view.answers)) {
        if (!r.ok) return `${name}: ${r.err || r.panic || r.crash}`;
        if (r.out.p0 !== r.out.p2) return `${name}: ${r.out.p0} became ${r.out.p2} via ${JSON.stringify(r.out.text)}`;
      }
      return null;
    },
  })),

  file('B-073.1', 'a .synx file in Latin-1 is refused, not loaded with U+FFFD', 'in.synx', bytes('name caf', [0xe9], '\nport 1\n')),
  file('B-073.2', 'a .synx file in UTF-16 is refused', 'in.synx', bytes([0xff, 0xfe], Buffer.from('name x\nport 1\n', 'utf16le'))),
  file('B-073.3', 'a .synx file that ends inside a character is refused', 'in.synx', bytes('name ok\ntail ', [0xd0])),
  file('B-073.4', 'a .synxl file with a byte that is not UTF-8 is refused (§3.1)', 'in.synxl', bytes('!synxl 1\n!fields user ; note\nann ; caf', [0xe9], '\nbob ; ok\n')),
  {
    id: 'B-073.5', zone: 'ts', title: 'bytes handed to parse() that are not UTF-8 are refused',
    op: 'parse_bytes', input: bytes('name caf', [0xe9], '\nport 1\n'), impls: ['ts'], expect: refused,
  },
];
