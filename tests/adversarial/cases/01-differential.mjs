/**
 * One document, two implementations, two different answers — or an exception
 * where the other answers. `agree` wants the same result from both and no
 * crash from either.
 */
import { agree, all, lacks, survive } from '../lib/expect.mjs';

const A = '!active\n';

export default [
  {
    id: 'B-001', zone: 'ts', title: 'format keeps the first key of a file that starts with a BOM',
    op: 'format', input: '﻿name synx\nport 8080\n', expect: agree,
  },

  // Keys that are also members of Object.prototype.
  ...['toString', 'valueOf', 'hasOwnProperty', 'isPrototypeOf', 'propertyIsEnumerable', 'toLocaleString',
    '__defineGetter__', '__defineSetter__', '__lookupGetter__', '__lookupSetter__'].map((name, i) => ({
    id: `B-002.${i + 1}`, zone: 'ts', title: `a key named ${name} beside a marked key`,
    op: 'active', input: `${A}${name} 1\nport:env PORT\n`, expect: agree,
  })),
  {
    id: 'B-002.11', zone: 'ts', title: 'a marked key named toString',
    op: 'active', input: `${A}toString:env HOME\n`, req: { env: { HOME: '/h' } }, expect: agree,
  },
  {
    id: 'B-002.12', zone: 'ts', title: 'inherit from a parent that has a key named valueOf',
    op: 'active', input: `${A}base\n  valueOf 1\n  port:env:default:5 P\nchild:inherit:base\n  x 2\n`, expect: agree,
  },

  // A document key named like the engine's own metadata slot.
  { id: 'B-003.1', zone: 'ts', title: 'static: a key named __synx stays in the tree', op: 'parse', input: '__synx 1\nb 2\n', expect: agree },
  { id: 'B-003.2', zone: 'ts', title: 'static: a nested key named __synx stays in the tree', op: 'parse', input: 'a\n  __synx 1\n  b 2\n', expect: agree },
  { id: 'B-003.3', zone: 'ts', title: 'active: scalar key __synx beside a marked key', op: 'active', input: `${A}a\n  __synx abc\n  port:env PORT\n`, expect: agree },
  { id: 'B-003.4', zone: 'ts', title: 'active: group __synx posing as metadata', op: 'active', input: `${A}a\n  __synx\n    x\n      markers 5\n  x 1\n  y:env Z\n`, expect: agree },
  { id: 'B-003.5', zone: 'ts', title: 'active: group __synx forging a marker list', op: 'active', input: `${A}a\n  __synx\n    x\n      markers\n        - env\n  x HOME\n`, req: { env: { HOME: '/h' } }, expect: agree },

  { id: 'B-004', zone: 'ts', title: 'list item whose key is constructor / prototype / __proto__', op: 'parse', input: 'list\n  - __proto__\n    a 1\n  - constructor x\n    b 2\n  - prototype y\n    c 3\n', expect: agree },

  // [min:…] / [max:…]: what counts as a number.
  { id: 'B-005.1', zone: 'ts', title: '[min:0x10] is not a bound', op: 'active', input: `${A}p[min:0x10] 5\n`, expect: agree },
  { id: 'B-005.2', zone: 'ts', title: '[min:] is not a bound', op: 'active', input: `${A}p[min:] -5\n`, expect: agree },
  { id: 'B-005.3', zone: 'ts', title: '[min:0b11] and [max:0o7] are not bounds', op: 'active', input: `${A}p[min:0b11] 1\nq[max:0o7] 9\n`, expect: agree },
  { id: 'B-005.4', zone: 'core', title: '[max:-inf] / [min:inf] / [min:nan]', op: 'active', input: `${A}p[max:-inf] 5\nq[min:inf] 5\nr[min:nan] 5\ns[max:Infinity] 5\n`, expect: agree },

  { id: 'B-006', zone: 'ts', title: '(int) reads decimal digits only', op: 'parse', input: 'x(int) 0x10\ny(int) 0b11\nz(int) 0o17\nw(int) +5\nv(int) 1e3\n', expect: agree },

  // U+FEFF is not SYNX whitespace; JavaScript's trim() and \s think it is.
  { id: 'B-007.1', zone: 'ts', title: '(bool) value ending in U+FEFF', op: 'parse', input: 'x(bool) true﻿\n', expect: agree },
  { id: 'B-007.2', zone: 'ts', title: '### followed by U+FEFF is not a block-comment fence', op: 'parse', input: '###﻿\nkey value\n###\nother 1\n', expect: agree },
  { id: 'B-007.3', zone: 'ts', title: 'value ending in U+FEFF before an inline comment', op: 'parse', input: 'k v﻿ # c\n', expect: agree },
  { id: 'B-007.4', zone: 'ts', title: 'list item wrapped in U+FEFF', op: 'parse', input: 'l\n  - ﻿a﻿\n  - b\n', expect: agree },
  { id: 'B-007.5', zone: 'ts', title: 'constraint part padded with U+FEFF', op: 'active', input: `${A}p[﻿required﻿]:env NOPE\n`, expect: agree },
  { id: 'B-007.6', zone: 'ts', title: '!include alias separated by U+FEFF / U+0085', op: 'parse', field: 'includes', input: '!include a.synx﻿b\n!include c.synx\u0085d\nk v\n', expect: agree },
  { id: 'B-007.7', zone: 'ts', title: ':inherit parent name padded with U+FEFF', op: 'active', input: `${A}base\n  a 1\nchild:inherit ﻿base\n  b 2\n`, expect: agree },

  // :round
  { id: 'B-008', zone: 'both', title: ':round of a negative half', op: 'active', input: `${A}x:round -2.5\ny:round 2.5\nz:round -0.5\nw:round:1 -0.25\n`, expect: agree },
  { id: 'B-009', zone: 'both', title: ':round:N with N that is not a plain count', op: 'active', input: `${A}x:round:-1 1234\ny:round:2.7 3.14159\nz:round:1e2 3.14159\nw:round:+2 3.14159\nv:round:2abc 3.14159\n`, expect: agree },
  { id: 'B-010', zone: 'core', title: ':round of NaN, infinity and a float beyond i64', op: 'active', input: `${A}x(float):round nan\ny(float):round inf\nz:round 1${'0'.repeat(30)}.5\n`, expect: agree },

  { id: 'B-011', zone: 'both', title: ':random — what counts as a weight', op: 'active', input: `${A}a:random inf\nb:random -5\nc:random 1e2\nd:random .5\ne:random +3\nf:random nan\n`, expect: agree },
  { id: 'B-012', zone: 'both', title: '#!mode: with nothing after the colon', op: 'active', input: `${A}#!mode:\nx:env Y\n`, expect: agree },
  { id: 'B-013', zone: 'both', title: ':format patterns %5i, %.2e, %i', op: 'active', input: `${A}x:format:%5i 42\ny:format:%.2e 123456\nz:format:%i 7\nw:format:%05i 7\n`, expect: agree },
  { id: 'B-014.1', zone: 'ts', title: ':format:%.101f', op: 'active', input: `${A}x:format:%.101f 1.5\n`, expect: agree },
  { id: 'B-014.2', zone: 'ts', title: ':format:%.999f', op: 'active', input: `${A}x:format:%.999f 1.5\n`, expect: agree },
  { id: 'B-015', zone: 'both', title: ':sum over strings that look numeric to one side only', op: 'active', input: `${A}a:sum\n  - 1\n  - "0x10"\nb:sum\n  - 1\n  - inf\nc:sum\n  - nan\n  - 2\nd:sum\n  - " 5 "\n  - 1\ne:sum\n  - 0b11\n`, expect: agree },

  // A reference to an ancestor must not come back as a cyclic object.
  { id: 'B-016.1', zone: 'ts', title: ':alias to the parent group', op: 'active', input: `${A}parent\n  child:alias parent\n`, expect: agree },
  { id: 'B-016.2', zone: 'ts', title: ':ref to the parent group', op: 'active', input: `${A}parent\n  child:ref parent\n`, expect: agree },
  { id: 'B-016.3', zone: 'ts', title: ':alias to a grandparent', op: 'active', input: `${A}a\n  b\n    c:alias a\n`, expect: agree },
  { id: 'B-016.4', zone: 'ts', title: ':spam target that is the parent group', op: 'active', input: `${A}parent\n  child:spam:100000 parent\n`, expect: agree },

  // :secret must hold before anything else can read the value.
  { id: 'B-017.1', zone: 'both', title: 'literal :secret:default read through :alias declared earlier', op: 'active', input: `${A}leak:alias password\npassword:secret:default:x hunter2\n`, expect: all(survive, lacks('hunter2')) },
  { id: 'B-017.2', zone: 'both', title: 'literal :secret:replace read through {…} in an earlier group', op: 'active', input: `${A}a\n  x {password}\npassword:secret:replace:q:z hunter2\n`, expect: all(survive, lacks('hunter2')) },
  { id: 'B-017.3', zone: 'both', title: 'literal :secret:format read through :ref declared earlier', op: 'active', input: `${A}leak:ref password\npassword:secret:format:%s hunter2\n`, expect: all(survive, lacks('hunter2')) },
  { id: 'B-017.4', zone: 'both', title: 'a :secret group is readable field by field from an earlier group', op: 'active', input: `${A}e\n  x {creds.pass}\n  y:alias creds.pass\ncreds:secret\n  pass hunter2\n`, expect: all(survive, lacks('hunter2')) },
  { id: 'B-017.5', zone: 'both', title: 'literal :secret:fallback read through :calc-free :map lookup', op: 'active', input: `${A}pick:map:password\n  - hunter2 leaked\n  - other no\npassword:secret:fallback:zz hunter2\n`, expect: all(survive, lacks('leaked')) },
];
