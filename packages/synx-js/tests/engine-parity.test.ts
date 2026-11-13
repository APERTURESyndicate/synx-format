/**
 * Active mode, marker by marker, against what synx-core gives for the same
 * document. Every case here failed against 3.8.0 — a different value, a
 * different refusal, an exception of the runtime, or seconds of work — except
 * those named `kept:`, which hold what the rewrite of the engine and of the
 * calculator had to leave as it was.
 */
import Synx, { SynxError } from '../src/index';

const fs = require('fs');
const os = require('os');
const path = require('path');

type Any = Record<string, any>;
const active = (text: string, options: Record<string, unknown> = {}): Any =>
  Synx.parse(`!active\n${text}`, { env: {}, ...options }) as Any;
/** The canonical JSON of an active document — secrets masked, floats as the reference writes them. */
const json = (text: string, options: Record<string, unknown> = {}): string =>
  Synx.toJSON(active(text, options), false);

function tempDir(files: Record<string, string | Buffer> = {}): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-engine-'));
  for (const name of Object.keys(files)) {
    fs.mkdirSync(path.dirname(path.join(dir, name)), { recursive: true });
    fs.writeFileSync(path.join(dir, name), files[name]);
  }
  return dir;
}

function withDir<T>(files: Record<string, string | Buffer>, run: (dir: string) => T): T {
  const dir = tempDir(files);
  try {
    return run(dir);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

describe(':secret hides the value from every other key', () => {
  test('a key above the secret reads only the mask', () => {
    const r = active('leak:alias password\nline {password}\npassword:secret:default:x hunter2\nafter:alias password\n');
    expect(r.password.reveal()).toBe('hunter2');
    expect(r.line).toBe('[SECRET]');
    expect(r.leak.reveal()).toBe('hunter2');
    expect(JSON.stringify(r)).not.toContain('hunter2');
    expect(Synx.toJSON(r, false)).toBe('{"after":"[SECRET]","leak":"[SECRET]","line":"[SECRET]","password":"[SECRET]"}');
  });

  test.each([
    ['pw:secret:split a,b', 'm:map:pw\n  - a,b LEAKED'],
    ['pw:secret:fallback:zz hunter2', 'p:prompt\n  v {pw}'],
  ])('%s is masked before the document resolves', (secret, reader) => {
    const text = Synx.toJSON(active(`${reader}\n${secret}\n`), false);
    expect(text).not.toContain('hunter2');
    expect(text).not.toContain('LEAKED');
  });

  test('a group and a list under :secret keep their shape, every scalar masked', () => {
    const r = active('g:secret\n  a 1\n  b\n    c two\nl:secret\n  - x\n  - 2\ncopy:alias g\n');
    expect(r.g.a.reveal()).toBe('1');
    expect(r.g.b.c.reveal()).toBe('two');
    expect(r.l.map((s: any) => s.reveal())).toEqual(['x', '2']);
    expect(Synx.toJSON(r, false)).toBe(
      '{"copy":{"a":"[SECRET]","b":{"c":"[SECRET]"}},"g":{"a":"[SECRET]","b":{"c":"[SECRET]"}},"l":["[SECRET]","[SECRET]"]}',
    );
  });

  test(':secret is applied to what the rest of the chain produced', () => {
    const r = active('f:secret:format:%05d 5\ns:secret:split a,b\nd:secret:default:abc null\nt:secret:once\n', { basePath: os.tmpdir(), readOnly: true });
    expect(r.f.reveal()).toBe('00005');
    expect(r.s.map((s: any) => s.reveal())).toEqual(['a', 'b']);
    expect(r.d.reveal()).toBe('abc');
    expect(typeof r.t.reveal()).toBe('string');
    expect(String(r.t)).toBe('[SECRET]');
  });

  test('an absent value stays null and an engine error stays readable', () => {
    withDir({}, dir => {
      const r = active('n:secret null\nw:secret:watch missing.txt\ni:secret:include missing.synx\nc:secret:calc 1 / 0\n', { basePath: dir });
      expect(r.n).toBeNull();
      expect(r.w).toBe('WATCH_ERR: file not found: missing.txt');
      expect(r.i).toBe('INCLUDE_ERR: file not found: missing.synx');
      expect(r.c).toBe('CALC_ERR: SYNX :calc — division by zero');
    });
  });

  test('constraints read a secret by its text and quote none of it', () => {
    const r = active('a[enum:x|y]:secret x\nb[enum:x|y]:secret zzz\nc[min:3]:secret ab\nd[pattern:^a+$]:secret aab\n');
    expect(r.a.reveal()).toBe('x');
    expect(r.b).toBe("CONSTRAINT_ERR: 'b' must be one of [x|y]");
    expect(r.c).toBe("CONSTRAINT_ERR: 'c' value 2 is below min 3");
    expect(r.d).toBe("CONSTRAINT_ERR: 'd' does not match pattern /^a+$/");
  });

  test(':secret:include masks the leaves of the included document', () => {
    withDir({ 'c.synx': 'a 1\nb\n  c hi\nl\n  - q\n' }, dir => {
      expect(json('x:secret:include c.synx\n', { basePath: dir }))
        .toBe('{"x":{"a":"[SECRET]","b":{"c":"[SECRET]"},"l":["[SECRET]"]}}');
    });
  });

  test('a secret is not an own property of its wrapper', () => {
    const r = active('k:secret abc\n');
    expect(Object.getOwnPropertyNames(r.k)).toEqual([]);
    expect(Object.getOwnPropertySymbols(r.k)).toEqual([]);
    expect(r.k[Symbol.for('synx.secret')]).toBe(true);
    expect(`${r.k}`).toBe('[SECRET]');
  });
});

describe('a marker places a copy, never the value itself', () => {
  test(':alias, :ref and :spam of an ancestor do not make the tree a cycle', () => {
    const r = active('a\n  b\n    up:alias a\n    r:ref a\n    s:spam:5 a\n  c 1\n');
    expect(() => JSON.stringify(r)).not.toThrow();
    expect(r.a.b.up).toEqual({ b: { up: 'a', r: 'a', s: 'a' }, c: 1 });
    expect(r.a.b.up).not.toBe(r.a);
  });

  test('two aliases of one group do not share it', () => {
    const r = active('g\n  x 1\nl:alias g\nr:alias g\n');
    r.l.x = 99;
    expect(r.g.x).toBe(1);
    expect(r.r.x).toBe(1);
  });

  test('kept: an inherited subtree belongs to the child', () => {
    const r = active('_base\n  sub\n    x 1\na:inherit:_base\nb:inherit:_base\n');
    r.a.sub.x = 99;
    expect(r.b.sub.x).toBe(1);
  });
});

describe('the expansion budget', () => {
  const LIMIT = 'LIMIT_ERR: resolution budget exceeded';

  test('{a}{a} doubled forty times ends in an error of the engine, not a RangeError', () => {
    let doc = 'a0 xxxxxxxxxxxxxxxx\n';
    for (let i = 1; i <= 40; i++) doc += `a${i} {a${i - 1}}{a${i - 1}}\n`;
    const r = active(doc);
    expect(r.a10.length).toBe(16 * 1024);
    // One template builds 4 MiB at most, as in synx-core …
    expect(r.a28.length).toBe(4 * 1024 * 1024);
    // … and the document as a whole 64 MiB.
    expect(r.a40).toBe(LIMIT);
  });

  test('aliases of a growing group stop at the budget', () => {
    let doc = 'g0\n  v xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n';
    for (let i = 1; i <= 30; i++) doc += `g${i}\n  l:alias g${i - 1}\n  r:alias g${i - 1}\n`;
    const started = Date.now();
    const r = active(doc);
    expect(Date.now() - started).toBeLessThan(20000);
    expect(JSON.stringify(r.g30)).toContain(LIMIT);
    expect(r.g5.l.l.l.l.l.v.length).toBe(64);
  });

  test(':replace and :join charge what they add before writing it', () => {
    const long = 'y'.repeat(4000);
    let doc = `s ${'a'.repeat(4000)}\n`;
    for (let i = 0; i < 6; i++) doc += `r${i}:replace:a:${long} ${'a'.repeat(4000)}\n`;
    const r = active(doc);
    expect(r.r0.length).toBe(16_000_000);
    expect(r.r5).toBe(LIMIT);
    const items = '  - x\n'.repeat(5000);
    let joins = '';
    for (let i = 0; i < 6; i++) joins += `j${i}:join:${long}\n${items}`;
    expect(active(joins).j5).toBe(LIMIT);
  });

  test('3 000 children of a 3 000-field parent do not become nine million fields', () => {
    let doc = '_p\n';
    for (let i = 0; i < 3000; i++) doc += `  f${i} ${i}\n`;
    for (let i = 0; i < 3000; i++) doc += `c${i}:inherit:_p\n`;
    const started = Date.now();
    const r = active(doc);
    expect(Date.now() - started).toBeLessThan(20000);
    expect(r.c0.f2999).toBe(2999);
    expect(r.c2999).toBe(LIMIT);
  });

  test('a value a marker places cannot nest past the depth limit', () => {
    let doc = 'g0\n  v 1\n';
    for (let i = 1; i <= 600; i++) doc += `g${i}\n  n:alias g${i - 1}\n`;
    const r = active(doc);
    expect(JSON.stringify(r.g600)).toContain('NESTING_ERR: maximum object nesting depth exceeded');
    expect(() => Synx.toJSON(r, false)).not.toThrow();
  });
});

describe(':calc', () => {
  test('a result of minus zero is zero', () => {
    const r = active('limit 0\ne:calc limit * -1\n');
    expect(Object.is(r.e, 0)).toBe(true);
  });

  test('keys resolve in document order even when they look like numbers', () => {
    const r = active('x:calc 2 + 3\n1:calc x * 2\ny:env:default:5 V\n2:alias y\n');
    expect(r['1']).toBe(10);
    expect(r['2']).toBe(5);
  });

  test('parentheses of any depth are an expression, not a stack overflow', () => {
    const r = active(`a:calc ${'('.repeat(1500)}7${')'.repeat(1500)}\nb:ref:calc:+${'('.repeat(60000)}1${')'.repeat(60000)} one\none 1\n`);
    expect(r.a).toBe(7);
    expect(r.b).toBe(2);
  });

  test.each([
    ['(', 'unexpected end of expression'],
    ['(1', 'missing closing parenthesis'],
    ['1 2', 'unexpected token at position 1'],
    ['()', "unexpected token: ')'"],
    ['1 + * 2', "unexpected token: '*'"],
    ['1.2.3', "invalid number: '1.2.3'"],
    ['-3 * -(2)', "invalid number: '-'"],
    ['1 / (2 - 2) )', 'division by zero'],
    ['1 + 2', "unexpected character: ' ' in expression"],
  ])('kept: %s is refused with the words of synx-core', (expr, message) => {
    expect(active(`x:calc ${expr}\n`).x).toBe(`CALC_ERR: SYNX :calc — ${message}`);
  });

  test('a long run of digits ahead of a second dot is read once', () => {
    const digits = '1'.repeat(4000);
    let doc = '';
    for (let i = 0; i < 400; i++) doc += `k${i}:calc ${digits}.5.\n`;
    const started = Date.now();
    const r = active(doc);
    expect(Date.now() - started).toBeLessThan(1000);
    expect(r.k0).toBe(`CALC_ERR: SYNX :calc — invalid number: '${digits}.5.'`);
  });

  test('one key that is not a word does not make every :calc scan the document', () => {
    let doc = 'max-hp 5\n';
    for (let i = 0; i < 4000; i++) doc += `v${i} ${i}\n`;
    for (let i = 0; i < 4000; i++) doc += `c${i}:calc v${i} + max-hp\n`;
    const started = Date.now();
    const r = active(doc);
    expect(Date.now() - started).toBeLessThan(5000);
    expect(r.c3999).toBe(4004);
  });

  test('a :ref:calc that fails is an error, not the number it started from', () => {
    expect(active('base 10\nb:ref:calc:%0 base\n').b).toMatch(/^CALC_ERR: /);
  });

  test('a whole result past 2^53 is the exact integer', () => {
    const r = active('a:calc 9007199254740992 * 4\nb:calc 0.5 + 0.25\n');
    expect(r.a).toBe(36028797018963968n);
    expect(r.b).toBe(0.75);
  });
});

describe('number markers', () => {
  test(':round keeps a number it cannot hold as an integer', () => {
    expect(json('x(float):round nan\ny(float):round inf\nz:round 1000000000000000000000000000000.5\nh:round:400 1.5\n'))
      .toBe('{"h":1.5,"x":null,"y":null,"z":1e30}');
  });

  test(':sum reads text by the number grammar of the format', () => {
    const r = active('t:sum\n  - 1\n  - "0x10"\n  - " 5 "\n  - "0b11"\n  - "2"\n  - "1e2"\n  - "inf"\n');
    expect(r.t).toBe(103);
  });

  test(':sort is a total order: numbers by value, then text', () => {
    const r = active('t:sort\n  - 10\n  - 1a\n  - 9\n  - b\n  - 2.5\n  - true\n');
    expect(r.t).toEqual([2.5, 9, 10, '1a', 'b', true]);
    const mixed = Array.from({ length: 60 }, (_, i) => `  - ${i % 3 === 0 ? i : `${i}a`}`).join('\n');
    expect(() => active(`m:sort\n${mixed}\n`)).not.toThrow();
  });

  test('kept: :clamp takes only a finite bound', () => {
    const r = active('a:clamp:1:5abc 9\nb:clamp:1:Infinity 9\nc:clamp:1:5 9\n');
    expect(r).toEqual({ a: 9, b: 9, c: 5 });
  });

  test(':i18n counts by this object first, and only by a number', () => {
    const r = active('n 3\ng\n  n 1\n  t:i18n:n\n    en\n      one {count} item\n      other {count} items\nu:i18n:s\n  en\n    one {count} item\n    other {count} items\ns "1"\n');
    expect(r.g.t).toBe('1 item');
    expect(r.u).toBe('0 items');
  });
});

describe(':watch', () => {
  test('a list has no keys: neither an index nor `length`', () => {
    withDir({ 'd.json': '{"list":[{"name":"item0"},{"name":"item1"}]}' }, dir => {
      const r = active('a:watch:list.0.name d.json\nb:watch:list.length d.json\nc:watch:list d.json\n', { basePath: dir });
      expect(r.a).toBeNull();
      expect(r.b).toBeNull();
      expect(r.c).toEqual([{ name: 'item0' }, { name: 'item1' }]);
    });
  });

  test('a JSON document the reference reader refuses gives null', () => {
    const deep = (n: number) => `{"k":1,"z":${'['.repeat(n)}${']'.repeat(n)}}`;
    withDir({
      'range.json': '{"k":1,"z":1e999}',
      'deep.json': deep(127),
      'fits.json': deep(126),
      'lone.json': '{"k":1,"z":"\\ud800"}',
    }, dir => {
      const r = active('a:watch:k range.json\nb:watch:k deep.json\nc:watch:k fits.json\nd:watch:k lone.json\n', { basePath: dir });
      expect(r).toEqual({ a: null, b: null, c: 1, d: null });
    });
  });

  test('a byte-order mark is content, not something the reader drops', () => {
    withDir({ 'b.txt': '﻿  hi  \n' }, dir => {
      expect(active('a:watch b.txt\n', { basePath: dir }).a).toBe('﻿  hi');
    });
  });
});

describe(':once', () => {
  test('the lock file is written once for the whole document', () => {
    withDir({}, dir => {
      let doc = '';
      for (let i = 0; i < 2000; i++) doc += `k${i}:once\n`;
      const started = Date.now();
      const first = active(doc, { basePath: dir });
      expect(Date.now() - started).toBeLessThan(1500);
      const lock = fs.readFileSync(path.join(dir, '.synx.lock'), 'utf8');
      expect(lock.split('\n').length).toBe(2001);
      expect(lock.startsWith(`k0 ${first.k0}\n`)).toBe(true);
      const second = active(doc, { basePath: dir });
      expect(second.k1999).toBe(first.k1999);
    });
  });

  test('readOnly resolves without writing', () => {
    withDir({}, dir => {
      const r = active('a:once\nb:once:random\n', { basePath: dir, readOnly: true });
      expect(r.a).toMatch(/^[0-9a-f-]{36}$/);
      expect(fs.existsSync(path.join(dir, '.synx.lock'))).toBe(false);
      fs.writeFileSync(path.join(dir, '.synx.lock'), 'a kept\n');
      expect(active('a:once\n', { basePath: dir, readOnly: true }).a).toBe('kept');
      expect(fs.readFileSync(path.join(dir, '.synx.lock'), 'utf8')).toBe('a kept\n');
    });
  });
});

describe('files', () => {
  test('a refusal reads the same on every platform', () => {
    withDir({ 'sub/x.txt': 'x', 'bad.synx': Buffer.from([0x61, 0x20, 0xff, 0xfe, 0x0a]) }, dir => {
      const r = active(
        'a:include sub/../nope.synx\nb:watch sub\\..\\nope.txt\nc:include sub\nd:watch bad.synx\ne:include nope.synx\n',
        { basePath: dir },
      );
      expect(r.a).toBe("INCLUDE_ERR: SECURITY: path traversal detected: 'sub/../nope.synx'");
      expect(r.b).toBe("WATCH_ERR: SECURITY: path traversal detected: 'sub\\..\\nope.txt'");
      expect(r.c).toBe('INCLUDE_ERR: not a file: sub');
      expect(r.d).toBe('WATCH_ERR: file is not valid UTF-8: bad.synx');
      expect(r.e).toBe('INCLUDE_ERR: file not found: nope.synx');
    });
  });

  test('kept: an unset base directory is the current one by name, not by path', () => {
    const r = Synx.parse('!active\nx:include no-such-file-here.synx\n') as Any;
    expect(r.x).toBe('INCLUDE_ERR: file not found: no-such-file-here.synx');
  });

  test('a file that includes itself is cut at the cycle', () => {
    withDir({ 'self.synx': '!active\na:include self.synx\nb:include self.synx\nc:include self.synx\n' }, dir => {
      const started = Date.now();
      const r = active('x:include self.synx\n', { basePath: dir });
      expect(Date.now() - started).toBeLessThan(5000);
      expect(r.x.a).toBe("INCLUDE_ERR: circular include: 'self.synx'");
    });
  });

  test('kept: a chain of root includes resolves each file once', () => {
    const files: Record<string, string> = { 'f0.synx': '!active\nv:once\n' };
    for (let i = 1; i <= 14; i++) files[`f${i}.synx`] = `!active\nn:include f${i - 1}.synx\n`;
    withDir(files, dir => {
      const started = Date.now();
      const r = active('top:include f14.synx\n', { basePath: dir });
      expect(Date.now() - started).toBeLessThan(5000);
      let cur = r.top;
      for (let i = 0; i < 14; i++) cur = cur.n;
      expect(cur.v).toMatch(/^[0-9a-f-]{36}$/);
    });
  });
});

describe('canonical JSON', () => {
  test('a float is written as the reference writes it', () => {
    const r = Synx.parse('a 0.000001\nb 0.00001\nc 12345678901234567890.5\nd 0.1\ne -0.0000015\n');
    expect(Synx.toJSON(r, false)).toBe('{"a":1e-6,"b":0.00001,"c":1.2345678901234567e19,"d":0.1,"e":-1.5e-6}');
  });

  test('nesting past the reference depth is null, and a loop ends', () => {
    let v: any = 1;
    for (let i = 0; i < 200; i++) v = { k: v };
    const text = Synx.toJSON(v, false);
    expect(text.startsWith('{"k":{"k":')).toBe(true);
    expect(text).toContain('null');
    const loop: any = { name: 'x' };
    loop.self = loop;
    expect(() => Synx.toJSON(loop, false)).not.toThrow();
  });
});

describe('strict', () => {
  test('a budget or nesting error is an error', () => {
    let doc = '!active\na0 xxxxxxxxxxxxxxxx\n';
    for (let i = 1; i <= 40; i++) doc += `a${i} {a${i - 1}}{a${i - 1}}\n`;
    expect(() => Synx.parse(doc, { strict: true })).toThrow(SynxError);
  });
});
