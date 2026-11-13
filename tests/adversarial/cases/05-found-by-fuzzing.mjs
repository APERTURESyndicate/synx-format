/**
 * Classes the differential fuzzer (fuzz.mjs) turned up, each reduced to its
 * smallest input. All of them want the two implementations to agree.
 */
import { agree } from '../lib/expect.mjs';

const A = '!active\n';
const both = (id, zone, title, op, input, extra = {}) => ({ id, zone, title, op, input, expect: agree, ...extra });

export default [
  both('B-042.1', 'ts', 'a dash with only a space after it is not a list item', 'parse', 's\n- '),
  both('B-042.2', 'ts', 'the same under a list-marker key', 'parse', 'l:join\n   z\n - '),
  both('B-043.1', 'both', 'comment line between a key and its first item', 'parse', 'k\n# note\n  - a\n  - b\n'),
  both('B-043.2', 'both', 'comment line, items at the key\'s own indent', 'parse', '0\n#\n- 0'),
  both('B-044.1', 'ts', 'list item whose first key is :inherit with a value', 'parse', 'e\n  - s:inherit n\n    a 1\n'),
  both('B-044.2', 'ts', 'list item whose first key is :random with weights', 'parse', 'e\n  - s:random 70 30\n    a 1\n'),
  both('B-045.1', 'both', 'list item whose first key has an empty hint and no value', 'parse', 't\n  - m()\n    x 1\n'),
  both('B-045.2', 'both', 'list item whose first key is (int) with no value', 'parse', 't\n  - m(int)\n    x 1\n'),
  both('B-046', 'both', '!include alias that contains spaces', 'parse', '!include a.synx my alias\nk v\n', { field: 'includes' }),
  both('B-006.2', 'ts', '(int) value wrapped in Unicode whitespace', 'parse', 'd(int) 5\nt(int)\r2\n'),

  // JavaScript iterates integer-like keys first; the engine resolves in that order.
  both('B-047.1', 'ts', 'document order of keys survives integer-like names', 'parse', 'n 1\n10 a\n2 b\n', { field: 'ordered' }),
  both('B-047.2', 'ts', ':calc reading a key declared before a numeric key', 'active', `${A}x:calc 2 + 3\n1:calc x * 2\n`),
  both('B-047.3', 'ts', ':alias reading an :env key declared before a numeric key', 'active', `${A}x:env:default:5 V\n1:alias x\n`),
  both('B-047.4', 'ts', '{…} inside a numeric-key group declared after its source', 'active', `${A}x:env:default:5 V\ng\n  7:ref x\n`),

  // [pattern]: one dialect.
  both('B-048.1', 'core', '[pattern:^\\d+$] is enforced', 'active', `${A}t[pattern:^\\d+$] abc\n`),
  both('B-048.2', 'core', '[pattern:^\\w+$] is enforced', 'active', `${A}t[pattern:^\\w+$] a-b\n`),
  both('B-048.3', 'core', '[pattern:\\s] and [pattern:\\bfoo\\b]', 'active', `${A}t[pattern:^\\S+$] a b\nu[pattern:\\bfoo\\b] food\n`),
  both('B-048.4', 'both', '[pattern:{] — a lone brace', 'active', `${A}r[pattern:{] y\n`),
  both('B-048.5', 'both', '[pattern:(?i)^abc$]', 'active', `${A}r[pattern:(?i)^abc$] ABC\ns[pattern:(?i)^abc$] x\n`),
  both('B-048.6', 'both', '[pattern:^\\p{L}+$]', 'active', `${A}r[pattern:^\\p{L}+$] мир\ns[pattern:^\\p{L}+$] 123\n`),

  both('B-049', 'both', ':calc that comes out as negative zero', 'active', `${A}limit 0\ne:calc limit * -1\nf:calc 0 * -5\n`),
  both('B-050.1', 'both', '(float) value against [type:int]', 'active', `${A}r(float)[type:int] 5\n`),
  both('B-050.2', 'both', 'a float with a zero fraction against [type:int]', 'active', `${A}port[type:int] 8080.0\n`),
  both('B-050.3', 'both', ':calc result against (int) and [type:int]', 'active', `${A}a(int):calc 10 / 2\nb[type:int]:calc 2.5 * 2\n`),
  both('B-050.4', 'ts', 'a float with a zero fraction against the (int) declared for its name', 'active', `${A}hp(int)\nhp 1.0\n`),
  both('B-050.5', 'ts', 'TYPE_ERR names the type of a float with a zero fraction', 'active', `${A}name(string)\ns\n  name(float) a\n`),
  both('B-051.1', 'both', ':i18n over an empty group', 'active', `${A}t:i18n\n`),
  both('B-051.2', 'both', ':prompt, :format, :join, :clamp on a key with no value', 'active', `${A}a:prompt\nb:format\nc:format:%d\nd:join\ne:clamp:10:1\nf:version:>=:1\n`),
  both('B-051.3', 'both', ':secret on a key with no value', 'active', `${A}s:secret\n`),

  // The first key of a list item carries markers like any other key.
  both('B-024.2', 'ts', ':env on the first key of a list item', 'active', `${A}t\n  - u:env SET\n    v 1\n`, { req: { env: { SET: 'from-env' } } }),
  both('B-024.3', 'ts', ':calc, :split, :version, :clamp on the first key of a list item', 'active', `${A}t\n  - c:calc 1 + 2\n    v 1\n  - d:split a,b\n    v 2\n  - x:version:>=:1 1.2\n    v 3\n  - y:clamp:1:5 9\n    v 4\n`),
  both('B-024.4', 'ts', 'a constraint on the first key of a list item', 'active', `${A}t\n  - port[type:int] abc\n    v 1\n`),

  // An empty `()` or `[]` on a key line: no type, no constraint — and the key is still a new declaration.
  both('B-071.1', 'both', 'parent with an empty () hint, child of the same name with (int)', 'active', `${A}x1()\n  x1(int)\n`),
  both('B-071.2', 'both', 'empty () first, (float) for the same name deeper', 'active', `${A}max()\n  max(float)\n`),
  both('B-071.3', 'both', '(float) on a value that is not a number, the name redeclared deeper', 'active', `${A}en(float) t\nd\n  en[type:int\n`),
  both('B-072.1', 'ts', 'a key redeclared with () drops the markers of its first declaration', 'active', `${A}min:ref base\nbase 5\nmin() .\n`),
  both('B-072.2', 'ts', 'a key redeclared with [] drops the markers of its first declaration', 'active', `${A}c:clamp:1:0 5\nc[] 7\n`),
  both('B-072.3', 'ts', 'a key redeclared with an unclosed [ drops the first constraint', 'active', `${A}list[type:string] c\nlist[\n`),
  both('B-072.4', 'ts', 'a key redeclared with [] after :alias', 'active', `${A}k:alias base\nbase 5\nk[] plus\n`),

  // A key without a value followed by a dash line opens a list (§8.5.2) — an empty one when the
  // dash is not indented deeper — and the items of the outer list all survive.
  {
    id: 'B-058.1', zone: 'both', title: 'a key without a value as the last key of a list item keeps the following items',
    op: 'parse', input: 'servers\n  - name a\n    tags\n  - name b\n    tags\n  - name c\n    port 1\n',
    expect: (view) => {
      for (const [name, v] of Object.entries(view.shown)) {
        if (v !== '{"servers":[{"name":"a","tags":[]},{"name":"b","tags":[]},{"name":"c","port":1}]}') return `${name}: ${v}`;
      }
      return null;
    },
  },
  both('B-058.2', 'both', 'the same, both implementations alike', 'parse', 'p\n  - u 1\n    e\n  - h\n  - z\n'),
  {
    id: 'B-056', zone: 'ts', title: 'compile without `resolved` does not read the environment',
    op: 'compile_check', input: `${A}x:env SYNX_ADVERSARIAL_CANARY\n`, field: 'text',
    expect: (view) => {
      for (const [name, r] of Object.entries(view.answers)) {
        if (JSON.stringify(r).includes('canary-from-the-host')) return `${name}: the host's variable is inside the compiled document`;
      }
      return null;
    },
  },
  both('B-057', 'ts', 'a float beyond 2^53 is still a float after compile and decompile', 'compile_check', 'h 89012345678901234.5\n', {
    expect: (view) => {
      for (const [name, r] of Object.entries(view.answers)) {
        if (!r.ok) return `${name}: ${r.err || r.panic}`;
        if (r.out.p0 !== r.out.p2) return `${name}: ${r.out.p0} became ${r.out.p2}`;
      }
      return null;
    },
  }),

  // format: the result parses to the same data, keeps the mode, and formatting it again changes nothing.
  ...[
    ['B-052.1', 'a document whose first line is indented', ' t 1\n u 2\n'],
    ['B-052.2', 'a document indented with a tab', '\tname x\n\tport 1\n'],
    ['B-052.3', 'a comment, then an indented key', '# note\n  e 1\n'],
    ['B-052.4', 'a list item followed by a sibling key', 't\n - e\n P 1\n'],
    ['B-052.5', 'keys inside a ### block comment stay commented', '###\nold 1\n###\nnew 2\n'],
    ['B-053.1', '#!mode:active after the first key', 't 1\n#!mode:active\np:env:default:5 V\n'],
    ['B-053.2', 'an indented !active', ' x 1\n !active\n p:env:default:5 V\n'],
    ['B-053.3', '!active inside a ### block comment', '###\n!active\n###\np:env:default:5 V\n'],
    ['B-053.4', 'a directive after the first key (idempotence)', 't 1\n!llm\n'],
    ['B-054.1', 'active: sorting must not change what :calc sees', `${A}price 1\nv:calc price + tax\ntax:calc price * 2\n`],
    ['B-054.2', 'active: sorting must not change what {…} in a group sees', `${A}total:calc 1 + 2\nc\n  t {total}\n`],
    ['B-055.1', 'a | block opened with a tab before the bar', 'text\t|\n  zeta\n  alpha\nname x\n'],
    ['B-055.2', 'a | block opened with an inline comment after the bar', 'text | # body\n  zeta\n  alpha\nname x\n'],
    ['B-055.3', 'a value that merely ends in " |"', 'row a | b |\n  child\n    grand 1\n'],
  ].map(([id, title, input]) => ({
    id, zone: 'both', title: `format: ${title}`, op: 'format_check', input, field: 'f1',
    expect: (view) => {
      for (const [name, r] of Object.entries(view.answers)) {
        if (!r.ok) return `${name}: ${r.panic || r.err || r.crash || 'no answer'}`;
        const o = r.out;
        if (o.m0 !== o.m1) return `${name}: mode ${o.m0 ? 'active' : 'static'} became ${o.m1 ? 'active' : 'static'}`;
        if (o.p0 !== o.p1) return `${name}: ${String(o.p0).slice(0, 110)}\n      became: ${String(o.p1).slice(0, 110)}`;
        if (!o.idempotent) return `${name}: a second format changes the text again`;
      }
      return null;
    },
  })),
];
