/**
 * Places where this port read a document differently from synx-core. Each
 * case failed before its fix; the expected value is what synx-core returns.
 */
import Synx from '../src/index';
import { parseData, metaMapOf } from '../src/parser';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
// The module object itself, as the engine requires it: a namespace import is frozen.
// eslint-disable-next-line @typescript-eslint/no-var-requires
const nodeFs: typeof import('fs') = require('fs');

type Any = Record<string, any>;
const json = (text: string): string => Synx.toJSON(Synx.parse(text, { env: {} }), false);

describe('the block under a rejected key goes with it (B-076)', () => {
  test('nested lines do not land in the previous sibling', () => {
    expect(json('user\n  profile\n    name x\n  __proto__\n    role admin\n    name y\n  age 3\n'))
      .toBe('{"user":{"age":3,"profile":{"name":"x"}}}');
  });

  test('nested lines do not overwrite a key of the parent', () => {
    expect(json('!active\nlimits\n  max 10\n  constructor\n    max 99\n    deep\n      max 100\nuse:calc limits.max * 2\n'))
      .toBe('{"limits":{"max":10},"use":20}');
  });

  test('text under one is the text of the dropped key, not lines of the document', () => {
    const r = Synx.parse('a 1\nprototype |\n  !active\n  b:env HOME\n  ###\nc 2\n', { env: { HOME: '/h' } }) as Any;
    expect(r).toEqual({ a: 1, c: 2 });
  });

  test('a list under one goes too, and the list above it goes on', () => {
    expect(json('l\n  - a 1\n    __proto__\n      - x\n      - y\n    b 2\n  - c 3\n    d 4\n'))
      .toBe('{"l":[{"a":1,"b":2},{"c":3,"d":4}]}');
    expect(json('l\n  - __proto__ x\n    k 1\n')).toBe('{"l":[{"k":1}]}');
  });

  test('format keeps what the parser keeps', () => {
    const text = 'user\n  profile\n    name x\n  __proto__\n    role admin\n  age 3\n';
    expect(json(Synx.format(text))).toBe(json(text));
  });
});

describe('a float with a whole value is a float (B-050, B-071.3)', () => {
  const act = (text: string, env: Record<string, string> = {}): Any => Synx.parse(`!active\n${text}`, { env }) as Any;

  test('[type:int] refuses 8080.0 written in the document or given by the environment', () => {
    expect(act('port[type:int] 8080.0\n').port).toBe("CONSTRAINT_ERR: 'port' expected type 'int'");
    expect(act('port[type:int]:env P\n', { P: '8080.0' }).port).toBe("CONSTRAINT_ERR: 'port' expected type 'int'");
    expect(act('port[type:int]:env:default:8080.0 P\n').port).toBe("CONSTRAINT_ERR: 'port' expected type 'int'");
    expect(act('port[type:int] 8080\nq[type:int]:env P\n', { P: '7' })).toEqual({ port: 8080, q: 7 });
  });

  test('(float) then [type:int]: the type error is written over the constraint error', () => {
    expect(act('r(float)[type:int] 5\n').r).toBe("TYPE_ERR: 'r' expected float but got string");
  });

  test('the type registry and TYPE_ERR see the float', () => {
    expect(act('hp(int)\nhp 1.0\n').hp).toBe("TYPE_ERR: 'hp' expected int but got float");
    expect(act('name(string)\ns\n  name(float) a\n').s.name).toBe("TYPE_ERR: 'name' expected string but got float");
    expect(act('en(float) t\nd\n  en[type:int\n').en).toBe("TYPE_ERR: 'en' expected float but got string");
  });

  test('a later line or marker that makes it an int clears it', () => {
    expect(act('p[type:int] 2.0\np 2\n').p).toBe(2);
    expect(act('p[type:int]:round 2.0\n').p).toBe(2);
  });

  // Every expected value below is what synx-core answers for the same text.
  const INT_ERR = (key: string): string => `CONSTRAINT_ERR: '${key}' expected type 'int'`;
  const TYPE_ERR = (key: string): string => `TYPE_ERR: '${key}' expected int but got float`;

  test('a float written next to markers is still a float', () => {
    expect(act('port[type:int]:default:1 8080.0\n').port).toBe(INT_ERR('port'));
    expect(act('port[type:int]:fallback:1 8080.0\n').port).toBe(INT_ERR('port'));
    expect(act('a[type:int]:calc 5.0\n').a).toBe(INT_ERR('a'));
    expect(act('a[type:int]:version:>=:1 5.0\n').a).toBe(INT_ERR('a'));
    expect(act('hp(int) 5\nunit\n  hp:default:3 1.0\n').unit.hp).toBe(TYPE_ERR('hp'));
    // The markers that make an integer of it still do.
    expect(act('port[type:int]:clamp:1:65535 8080.0\n').port).toBe(8080);
    expect(act('a[type:int]:clamp:1:5 9.7\n').a).toBe(5);
  });

  test(':round with decimals and :sum of fractions give floats', () => {
    expect(act('a[type:int]:round:2 5\nb[type:int]:round:2 5.0\nc:round:1 2.25\n'))
      .toEqual({ a: INT_ERR('a'), b: INT_ERR('b'), c: 2.3 });
    expect(act('a[type:int]:round 8080.0\nb[type:int]:round:0 2.5\n')).toEqual({ a: 8080, b: 3 });
    expect(act('a[type:int]:sum\n  - 1.5\n  - 2.5\nb[type:int]:sum\n  - 1\n  - 2\nc[type:int]:sum\n  - 2.0\n  - 2.0\n'))
      .toEqual({ a: INT_ERR('a'), b: 3, c: 4 });
  });

  test('a copy of a float is a float: :alias, :ref, :spam, alone or inside a group', () => {
    expect(act('rate 2.0\na[type:int]:alias rate\n').a).toBe(INT_ERR('a'));
    expect(act('rate 2.0\na[type:int]:ref rate\n').a).toBe(INT_ERR('a'));
    expect(act('rate 2.0\na[type:int]:spam:100000:1 rate\n').a).toBe(INT_ERR('a'));
    expect(act('srv\n  rate 2.0\na[type:int]:alias srv.rate\n').a).toBe(INT_ERR('a'));
    expect(act('rate 2.0\na:alias rate\nb[type:int]:alias a\n').b).toBe(INT_ERR('b'));
    expect(act('rate(int) 1\nsrv\n  rate 2.0\ncopy:alias srv\n').copy.rate).toBe(TYPE_ERR('rate'));
    expect(act('rate(int) 1\nsrv\n  rate 2.0\ncopy:ref srv\n').copy.rate).toBe(TYPE_ERR('rate'));
    // An int, or a key declared again as one, copies as an int.
    expect(act('rate 2\na[type:int]:alias rate\n').a).toBe(2);
    expect(act('rate 2.0\nrate 2\na[type:int]:alias rate\n').a).toBe(2);
    // `:ref:calc` computes: a whole result is an integer.
    expect(act('rate 2.0\na[type:int]:ref:calc:*3 rate\nb[type:int]:ref:calc:*1.5 rate\n')).toEqual({ rate: 2, a: 6, b: 3 });
  });

  test('an inherited float is a float, and the child may replace it', () => {
    expect(act('rate(int) 1\n_base\n  rate 2.0\nchild:inherit:_base\n  x 1\n').child.rate).toBe(TYPE_ERR('rate'));
    expect(act('rate(int) 1\n_base\n  rate 2.0\nchild:inherit:_base\n  rate 3\n').child.rate).toBe(3);
    expect(act('rate(int) 1\n_base\n  rate 2\nchild:inherit:_base\n  rate 3.0\n').child.rate).toBe(TYPE_ERR('rate'));
    expect(act('rate(int) 1\n_base\n  inner\n    rate 2.0\nchild:inherit:_base\n  x 1\n').child.inner.rate).toBe(TYPE_ERR('rate'));
  });

  test(':map, :i18n and text from outside keep what the text said', () => {
    expect(act('k a\nv[type:int]:map:k\n  - a 5.0\n  - b 6\nw[type:int]:map:k\n  - a 5\n')).toEqual({ k: 'a', v: INT_ERR('v'), w: 5 });
    expect(act('k a\nv[type:int]:map:k\n  - a 1e3\n').v).toBe(INT_ERR('v'));
    expect(act('t[type:int]:i18n\n  en 5.0\n  ru 6\n').t).toBe(INT_ERR('t'));
    expect(act('n 1\nt[type:int]:i18n:n\n  en\n    one 5.0\n    other 6\n').t).toBe(INT_ERR('t'));
    expect(act('c[type:int]:env C\n', { C: '1e3' }).c).toBe(INT_ERR('c'));
  });

  test('a whole number past i64 is a float', () => {
    const r = act('a[type:int]:calc 1000000000000 * 1000000000000\nb:calc 4611686018427387904 * 2\nc[type:int]:calc 4611686018427387904 * 2\n');
    expect(r.a).toBe(INT_ERR('a'));
    expect(r.b).toBe(9223372036854775808);
    expect(r.c).toBe(INT_ERR('c'));
  });

  test('a document read for an active one keeps its floats, and JSON its integers', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-floats-'));
    try {
      fs.writeFileSync(path.join(dir, 'data.json'),
        '{"port":8080.0,"id":9007199254740993,"n":5,"big":18446744073709551615,"nested":{"rate":2.0,"count":3}}');
      fs.writeFileSync(path.join(dir, 'static.synx'), 'port 8080.0\ncount 3\ngroup\n  rate 2.0\n');
      const read = (text: string): Any => Synx.parse(`!active\n${text}`, { env: {}, basePath: dir }) as Any;
      expect(read('port[type:int]:watch:port data.json\nn[type:int]:watch:n data.json\n')).toEqual({ port: INT_ERR('port'), n: 5 });
      // An id past 2^53 came back as another id.
      expect(read('id:watch:id data.json\n').id).toBe(BigInt('9007199254740993'));
      expect(read('big:watch:big data.json\n').big).toBe(18446744073709551615);
      expect(read('rate(int) 1\nall:watch:nested data.json\n').all).toEqual({ count: 3, rate: TYPE_ERR('rate') });
      expect(read('port[type:int]:watch:port static.synx\ncount[type:int]:watch:count static.synx\n'))
        .toEqual({ port: INT_ERR('port'), count: 3 });
      expect(read('port(int) 1\nrate(int) 1\ncfg:include static.synx\n').cfg)
        .toEqual({ port: TYPE_ERR('port'), count: 3, group: { rate: TYPE_ERR('rate') } });
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe('a secret stays one through :inherit and :include (B-017.6…B-017.8)', () => {
  const act = (text: string, env: Record<string, string> = {}): Any => Synx.parse(`!active\n${text}`, { env }) as Any;
  const shown = (text: string, env: Record<string, string> = {}): string => Synx.toJSON(act(text, env), false);

  test('what is inherited from a group under :secret is secret in the child', () => {
    expect(shown('child:inherit base\nbase:secret\n  p hunter2\n')).toBe('{"base":{"p":"[SECRET]"},"child":{"p":"[SECRET]"}}');
    expect(act('child:inherit base\nbase:secret\n  p hunter2\n').child.p.reveal()).toBe('hunter2');
  });

  test('declared before the child and read through another key', () => {
    const r = act('creds:secret\n  pass hunter2\nc:inherit creds\nleak:alias c.pass\ntext {c.pass}\n');
    expect(JSON.stringify(r)).not.toContain('hunter2');
    expect(r.leak.reveal()).toBe('hunter2');
    expect(r.text).toBe('[SECRET]');
  });

  test("the field's own markers still run, a nested group is masked whole, the child's own fields are not", () => {
    const r = act('base:secret\n  port:env:default:5 PORT\n  inner\n    token abc\nchild:inherit:base\n  own 1\n', { PORT: '7101' });
    expect(r.child.port.reveal()).toBe('7101');
    expect(r.child.inner.token.reveal()).toBe('abc');
    expect(r.child.own).toBe(1);
  });

  test('a child of the child inherits the mark with the field; an open group hands its fields on as they are', () => {
    const r = act('base:secret\n  p hunter2\nc1:inherit base\nc2:inherit c1\n');
    expect(JSON.stringify(r)).not.toContain('hunter2');
    expect(r.c2.p.reveal()).toBe('hunter2');
    expect(act('base\n  p open\nchild:inherit base\n').child.p).toBe('open');
  });

  test('SPAM_ERR does not quote a value under :secret (B-088)', () => {
    // The limit is per process: the second resolve of the same key trips it.
    const text = 'sx2_spam_secret:spam:1:60:secret hunter2\nsx2_spam_open:spam:1:60 visible\n';
    act(text);
    const second = act(text);
    expect(second.sx2_spam_secret).toBe("SPAM_ERR: '[SECRET]' exceeded 1 calls per 60s");
    expect(second.sx2_spam_open).toBe("SPAM_ERR: 'visible' exceeded 1 calls per 60s");
  });

  test(':secret:include on a value that is not a path is still masked', () => {
    const text = shown('k:secret:include\n  - hunter2\nn:include:secret 7731\nm:secret:import\n  - hunter2\n');
    expect(text).not.toContain('hunter2');
    expect(text).not.toContain('7731');
    expect(text).toBe('{"k":["[SECRET]"],"m":["[SECRET]"],"n":"[SECRET]"}');
  });
});

describe('the resolution budget runs out where it does in synx-core', () => {
  const lines = (n: number, f: (i: number) => string): string => {
    let s = '';
    for (let i = 0; i < n; i++) s += f(i);
    return s;
  };
  /** The first root key left with LIMIT_ERR, and how many hold it. */
  const refused = (text: string): [string, number] => {
    const r = Synx.parse(`!active\n${text}`, { env: {} }) as Any;
    let first = 'none';
    let count = 0;
    for (const k of Object.keys(r)) {
      if (r[k] === 'LIMIT_ERR: resolution budget exceeded') {
        if (count === 0) first = k;
        count++;
      }
    }
    return [first, count];
  };
  const children = (n: number): string => lines(n, i => `c${i}:inherit:p\n  own ${i}\n`);

  // Each pair is what synx-core answers for the same document.
  test('copies of a group: :alias, :ref', () => {
    const group = `g\n${lines(1500, i => `  f${i} ${i}\n`)}`;
    expect(refused(group + lines(1500, i => `k${i}:alias g\n`))).toEqual(['k338', 1162]);
    expect(refused(group + lines(1500, i => `k${i}:ref g\n`))).toEqual(['k338', 1162]);
  });

  test(':inherit counts the declarations it copies: markers, constraints, nested ones, the :secret of a group', () => {
    expect(refused(`p\n${lines(600, i => `  f${i}:env:default:1 V\n`)}${children(600)}`)).toEqual(['c235', 365]);
    expect(refused(`p\n${lines(600, i => `  f${i}[type:int, min:0, enum:1|2|3](int) 1\n`)}${children(600)}`)).toEqual(['c223', 377]);
    expect(refused(`p\n${lines(600, i => `  g${i}\n    x:default:1\n    y[min:1] 2\n`)}${children(600)}`)).toEqual(['c102', 498]);
    expect(refused(`p:secret\n${lines(600, i => `  f${i} v${i}\n`)}${children(600)}`)).toEqual(['c282', 318]);
    expect(refused(`p\n${lines(600, i => `  f${i} ${i}\n`)}${children(600)}`)).toEqual(['none', 0]);
  });

  test('once it is spent, a copy is refused without being measured (B-030.7, B-031.4)', () => {
    const started = Date.now();
    const n = 12000;
    expect(refused(`g\n${lines(n, i => `  f${i} ${i}\n`)}${lines(n, i => `k${i}:alias g\n`)}`)[1]).toBeGreaterThan(11900);
    expect(refused(`p\n${lines(n, i => `  f${i} ${i}\n`)}${lines(n, i => `c${i}:inherit:p\n  own ${i}\n`)}`)[1]).toBeGreaterThan(11900);
    // 17 s and 25 s before; a loaded machine still does both in well under this.
    expect(Date.now() - started).toBeLessThan(10000);
  });
});

describe('SYNXL edges read as synx-core reads them (B-060…B-064)', () => {
  const P = '!synxl 1\n';
  const kinds = (text: string): string[][] =>
    Synx.parseSynxl(text, { validate: true }).diagnostics.map(d => [d.kind, String(d.record), String(d.line)]);
  const hardLine = (text: string): string => {
    try { Synx.parseSynxl(text); } catch (e: any) { return String(e.message); }
    return 'no error';
  };
  const hardKind = (text: string): string => {
    try { Synx.parseSynxl(text); } catch (e: any) { return String(e.condition); }
    return 'no error';
  };

  test('a missing prologue is reported at the last line read', () => {
    expect(hardLine('\n#')).toMatch(/\(line 2\)$/);
    expect(hardLine('###\n1')).toMatch(/\(line 2\)$/);
    expect(hardLine('')).toMatch(/\(line 1\)$/);
  });

  test('[min], [max] and [type:int] see an integer beyond 2^53', () => {
    expect(kinds(`${P}!fields e[max:2]\n9007199254740993\n`)).toEqual([['ConstraintViolation', '0', '3']]);
    expect(kinds(`${P}!fields o[min:1]\n-9372036854775808\n`)).toEqual([['ConstraintViolation', '0', '3']]);
    expect(kinds(`${P}!fields v[type:int]\n9223372036854775807\n`)).toEqual([]);
  });

  test('an indented ### closes a block comment', () => {
    expect(Synx.parseSynxl(`${P}###\n ###\n!fields a\nx\n`).records.map(r => r.values)).toEqual([{ a: 'x' }]);
  });

  test('an empty () is a hint', () => {
    expect(hardKind(`${P}!fields x()[block]\n`)).toBe('BlockCombinedWithType');
    const d = Synx.parseSynxl(`${P}!fields a()[type:float]\nabc\n`, { validate: true });
    expect(d.records[0].values).toEqual({ a: 'abc' });
    expect(d.diagnostics.map(x => x.kind)).toEqual(['ConstraintViolation']);
    const s = Synx.parseSynxl(`${P}!fields a()[type:string]\n0\n`, { validate: true });
    expect(s.records[0].values).toEqual({ a: 0 });
    expect(s.diagnostics.map(x => x.kind)).toEqual(['ConstraintViolation']);
  });

  test('a float with a whole value is not an int', () => {
    expect(kinds(`${P}!fields a(float)[type:int]\n3\n`)).toEqual([['ConstraintViolation', '0', '3']]);
    expect(kinds(`${P}!fields a()[type:int]\n3.0\n`)).toEqual([['ConstraintViolation', '0', '3']]);
    expect(kinds(`${P}!fields a[type:int]\n3\n`)).toEqual([]);
  });

  test('!fields with a lone colon is a malformed field list', () => {
    expect(hardKind(`${P}!fields :\n`)).toBe('MalformedFieldList');
  });

  test('a colon after a name is a marker chain, with or without a space before it (B-064.2)', () => {
    expect(hardKind(`${P}!fields a :\n`)).toBe('MarkerChainInFieldDecl');
    expect(hardKind(`${P}!fields a:x\n`)).toBe('MarkerChainInFieldDecl');
    expect(hardKind(`${P}!fields a(int) \t:x\n`)).toBe('MarkerChainInFieldDecl');
    expect(hardKind(`${P}!fields a b\n`)).toBe('MalformedFieldList');
  });
});

describe('[pattern] in the dialect of the regex crate, linear in the value (B-048, S3-06)', () => {
  const check = (pattern: string, value: string): unknown =>
    (Synx.parse(`!active\nv[pattern:${pattern}] ${value}\n`, { env: {} }) as Any).v;
  const refused = (pattern: string, value: string): boolean =>
    typeof check(pattern, value) === 'string' && String(check(pattern, value)).startsWith('CONSTRAINT_ERR');

  test('flag groups, Unicode classes and a lone brace read as synx-core reads them', () => {
    expect(refused('(?i)^abc$', 'ABC')).toBe(false);
    expect(refused('(?i)^abc$', 'abd')).toBe(true);
    expect(refused(String.raw`^\p{L}+$`, 'мир')).toBe(false);
    expect(refused(String.raw`^\p{L}+$`, 'm1')).toBe(true);
    expect(refused(String.raw`^\d+$`, '١٢٣')).toBe(false);
    expect(refused(String.raw`^\d+$`, '12a')).toBe(true);
    expect(check('{', 'x')).toBe('x'); // does not compile in synx-core: not checked
  });

  test('a flag set in one alternative holds for the next ones, and no further than its group (B-083.1)', () => {
    expect(refused('(?i)^yes$|^no$', 'NO')).toBe(false);
    expect(refused('(?i)^yes$|^no$', 'maybe')).toBe(true);
    expect(refused('^yes$|(?i)^no$', 'YES')).toBe(true);
    expect(refused('^((?i)a|b)c$', 'Bc')).toBe(false);
    expect(refused('^((?i)a|b)c$', 'bC')).toBe(true);
  });

  test('with case ignored a Unicode class is folded, then negated (B-083.3)', () => {
    expect(refused(String.raw`(?i)^\p{Lu}+$`, 'abc')).toBe(false);
    expect(refused(String.raw`(?i)^\p{Lu}+$`, 'a1')).toBe(true);
    expect(refused(String.raw`(?i)^\P{Lu}+$`, 'abc')).toBe(true);
    expect(refused(String.raw`(?i)^\P{Lu}+$`, '12-')).toBe(false);
    expect(refused(String.raw`^\P{Lu}+$`, 'abc')).toBe(false);
  });

  test('a flag group the crate refuses is not checked: nothing set, a flag twice, a dangling minus (B-083.6)', () => {
    for (const pattern of ['(?)^x$', '(?ii)^x$', '(?i-)^x$', '(?--i)^x$', '(?i-i)^x$']) {
      expect([pattern, check(pattern, 'abc')]).toEqual([pattern, 'abc']);
    }
    // The ones it takes are still checked.
    expect(refused('(?i)^x$', 'abc')).toBe(true);
    expect(refused('(?i-m)^x$', 'X')).toBe(false);
    expect(refused('(?:x)', 'abc')).toBe(true);
  });

  test('a pattern that only repeats a repetition is still not checked (conformance 105)', () => {
    expect(check('^(a+)+$', 'aaaa!')).toBe('aaaa!');
    expect(refused('^(ab){2}$', 'abab!')).toBe(true);
  });

  test('an alternation under a quantifier does not backtrack', () => {
    const started = Date.now();
    expect(refused('^(a|aa)+$', `${'a'.repeat(50000)}b`)).toBe(true);
    expect(refused('^(a|a)*$', `${'a'.repeat(50000)}`)).toBe(false);
    expect(Date.now() - started).toBeLessThan(2000);
  });
});

describe('.synxb with declarations comes back with the same value (B-082)', () => {
  // The declarations of every object, in a comparable form.
  const metaOf = (v: unknown, path = '$'): Record<string, unknown> => {
    const out: Record<string, unknown> = {};
    if (v === null || typeof v !== 'object') return out;
    if (Array.isArray(v)) {
      v.forEach((item, i) => Object.assign(out, metaOf(item, `${path}[${i}]`)));
      return out;
    }
    const m = metaMapOf(v);
    if (m !== undefined) out[path] = JSON.stringify([...m.entries()]);
    for (const [k, child] of Object.entries(v)) Object.assign(out, metaOf(child, `${path}.${k}`));
    return out;
  };

  test.each([
    'x(int)\nx h\n',
    'b:join\nb\n',
    'a:inherit\na x\n',
    'b(int)|\n',
    't:\n- 2\n',
    't\n- 2\n',
    'l\n  - x(int)\n    x h\n    y 1\n',
    'g\n  x(int)\n  x h\n  b:join\n  b\n    c 1\n',
    'port:env:default:8080 PORT\nn(int) 5\nr:random 90 10\n  - a\n  - b\n',
  ])('%j', body => {
    const doc = `!active\n${body}`;
    const text = Synx.decompile(Synx.compile(doc, false));
    const before = parseData(doc);
    const after = parseData(text);
    expect(Synx.toJSON(after.root, false)).toBe(Synx.toJSON(before.root, false));
    expect(metaOf(after.root)).toEqual(metaOf(before.root));
    expect(after.mode).toBe(before.mode);
  });

  test('lines that already read back stay one line each', () => {
    expect(Synx.decompile(Synx.compile('!active\nport:env:default:8080 PORT\nn(int) 5\n', false)))
      .toBe('!active\n\nport:env:default:8080 PORT\nn(int) 5\n');
  });
});

describe(':once shares .synx.lock with other processes (B-065)', () => {
  test('a line another process appends meanwhile is kept, and its value wins', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-once-'));
    const lockPath = path.join(dir, '.synx.lock');
    const realWrite = nodeFs.writeSync;
    let injected = false;
    // Another process appends between this one's read and its write.
    const spy = jest.spyOn(nodeFs, 'writeSync').mockImplementation(((fd: number, data: string) => {
      if (!injected) {
        injected = true;
        fs.appendFileSync(lockPath, 'other 42' + String.fromCharCode(10) + 'id theirs' + String.fromCharCode(10));
      }
      return (realWrite as (fd: number, data: string) => number)(fd, data);
    }) as typeof fs.writeSync);
    try {
      const r = Synx.parse('!active' + String.fromCharCode(10) + 'id:once' + String.fromCharCode(10) + 'mine:once' + String.fromCharCode(10), { basePath: dir }) as Any;
      expect(r.id).toBe('theirs');
      const lines = fs.readFileSync(lockPath, 'utf-8').trim().split(String.fromCharCode(10));
      expect(lines[0]).toBe('other 42');
      expect(lines[1]).toBe('id theirs');
      expect(lines.some(l => l.startsWith('mine '))).toBe(true);
      // The next read gives the same values.
      const again = Synx.parse('!active' + String.fromCharCode(10) + 'id:once' + String.fromCharCode(10) + 'mine:once' + String.fromCharCode(10), { basePath: dir }) as Any;
      expect(again).toEqual(r);
    } finally {
      spy.mockRestore();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  test('a .synx.lock that is a link or a directory is neither read nor written (B-089)', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-lock-link-'));
    try {
      const base = path.join(dir, 'base');
      const outside = path.join(dir, 'outside.txt');
      fs.mkdirSync(base);
      fs.writeFileSync(outside, 'id stolen-from-outside\n');
      let linked = true;
      try {
        fs.symlinkSync(outside, path.join(base, '.synx.lock'), 'file');
      } catch {
        linked = false; // no right to make links here: the directory form below still runs
      }
      if (linked) {
        const r = Synx.parse('!active\nid:once\nother:once\n', { env: {}, basePath: base }) as Any;
        expect(r.id).not.toBe('stolen-from-outside');
        expect(typeof r.other).toBe('string');
        expect(fs.readFileSync(outside, 'utf-8')).toBe('id stolen-from-outside\n');
      }
      const second = path.join(dir, 'second');
      fs.mkdirSync(path.join(second, '.synx.lock'), { recursive: true });
      const r2 = Synx.parse('!active\nid:once\n', { env: {}, basePath: second }) as Any;
      expect(typeof r2.id).toBe('string');
      expect(fs.readdirSync(path.join(second, '.synx.lock'))).toEqual([]);
      // A plain file, or none yet, works as before.
      const third = path.join(dir, 'third');
      fs.mkdirSync(third);
      const first = (Synx.parse('!active\nid:once\n', { env: {}, basePath: third }) as Any).id;
      expect(fs.readFileSync(path.join(third, '.synx.lock'), 'utf-8')).toBe(`id ${first}\n`);
      expect((Synx.parse('!active\nid:once\n', { env: {}, basePath: third }) as Any).id).toBe(first);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  test('readOnly reads the file and writes nothing', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-once-'));
    try {
      const r = Synx.parse('!active' + String.fromCharCode(10) + 'id:once' + String.fromCharCode(10), { basePath: dir, readOnly: true }) as Any;
      expect(typeof r.id).toBe('string');
      expect(fs.existsSync(path.join(dir, '.synx.lock'))).toBe(false);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});
