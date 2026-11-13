/**
 * Inputs that used to escape as an untyped exception, read a built-in off
 * `Object.prototype`, or build an unbounded string. Every case here failed
 * against 3.8.0.
 */
import Synx, { SynxError } from '../src/index';
import BrowserSynx from '../src/browser';

type Any = Record<string, any>;
const active = (text: string, options: Record<string, unknown> = {}): Any =>
  Synx.parse(`!active\n${text}`, { env: {}, ...options }) as Any;

describe('keys named like Object.prototype members', () => {
  test.each(['valueOf', 'toString', 'hasOwnProperty', 'isPrototypeOf', 'toLocaleString'])(
    'a plain key `%s` beside a marked key resolves',
    name => {
      const r = active(`${name} 1\nx:calc 2 + 3\nsecret:secret abc\n`);
      expect(r[name]).toBe(1);
      expect(r.x).toBe(5);
      expect(r.secret.reveal()).toBe('abc');
    },
  );

  test('a group whose child is `valueOf` inherits and validates', () => {
    const r = active('base\n  valueOf 1\n  port[min:1] 5\nchild:inherit:base\n  toString 2\n');
    expect(r.child).toEqual({ valueOf: 1, port: 5, toString: 2 });
  });

  test(':alias to a key with no declaration of its own is not taken for a marked one', () => {
    const r = active('toString hello\na:alias toString\nb:alias constructor\n');
    expect(r.a).toBe('hello');
    expect(r.b).toBeNull();
  });

  test(':env of a name that only exists on the prototype is unset', () => {
    const r = active('a:env toString\nb:env:default:fallback constructor\nc:env __proto__\n', { env: {} });
    expect(r.a).toBeNull();
    expect(r.b).toBe('fallback');
    expect(r.c).toBeNull();
  });

  test('an `env` option may hold numbers and booleans; anything else is unset', () => {
    const r = active('a:env A\nb:env B\nc:env C\nd:env D\n', {
      env: { A: 8080, B: true, C: { x: 1 }, D: () => 1 } as any,
    });
    expect(r).toEqual({ a: 8080, b: true, c: null, d: null });
  });

  test(':split and :join take a delimiter word, not a prototype member', () => {
    const r = active('a:split:constructor xconstructory\nb:join:toString\n  - 1\n  - 2\n');
    expect(r.a).toEqual(['x', 'y']);
    expect(r.b).toBe('1toString2');
  });

  test(':i18n with a language named like a prototype member falls back', () => {
    const r = active('t:i18n\n  en Hello\n  ru Привет\n', { lang: 'toString' });
    expect(r.t).toBe('Hello');
  });

  test('schema keeps an unknown type name as text', () => {
    const s = Synx.schema('!active\na[type:constructor] 1\nb[type:int] 2\n');
    expect(s.properties.a.type).toBe('constructor');
    expect(s.properties.b.type).toBe('integer');
  });
});

describe(':format stays within its limits', () => {
  test('a precision or width from the document cannot throw out of parse', () => {
    expect(() => active('x:format:%.999f 1.5\n')).not.toThrow();
    expect(() => active('x:format:%0999999999d 5\n')).not.toThrow();
    expect(() => active('x:format:%999999999d 5\n')).not.toThrow();
    expect(() => active('x:format:%.99999999999999999999f 1.5\n')).not.toThrow();
  });

  test('width is capped at 4096 and precision at 1024, as in synx-core', () => {
    expect(String(active('x:format:%0999999999d 5\n').x)).toHaveLength(4096);
    expect(String(active('x:format:%999999999d 5\n').x)).toHaveLength(4096);
    const wide = String(active('x:format:%.5000f 1.5\n').x);
    expect(wide.startsWith('1.5000')).toBe(true);
    expect(wide).toHaveLength(2 + 1024);
  });

  test('fixed decimals are the exact value, ties to even', () => {
    const r = active('a:format:%.1f 0.25\nb:format:%.0f 0.5\nc:format:%.0f 1.5\nd:format:%.0f 2.5\ne:format:%.2f 1.005\nf:format:%.1f -0.25\ng:format:%.3f 2\nh:format:%.0f -0.4\n');
    expect(r).toEqual({ a: '0.2', b: '0', c: '2', d: '2', e: '1.00', f: '-0.2', g: '2.000', h: '-0' });
  });

  test('a long precision prints the digits the number really has', () => {
    const r = active('x:format:%.120f 0.1\n');
    expect(String(r.x)).toBe('0.1000000000000000055511151231257827021181583404541015625' + '0'.repeat(120 - 55));
  });

  test('%i is the integer pattern and %.Ne the exponent pattern', () => {
    const r = active('a:format:%05i 42\nb:format:%i 7\nc:format:%.2e 1234.5\nd:format:%.0e 5\ne:format:%e 0\nf:format:%.3e 0\n');
    expect(r).toEqual({ a: '00042', b: '7', c: '1.23e3', d: '5e0', e: '0e+0', f: '0.000e0' });
  });
});

describe('converters accept every value the parser returns', () => {
  const doc = 'id 9007199254740993\nname x\nlist\n  - 9223372036854775807\n  - 1\n';
  test('toYAML writes a 64-bit integer', () => {
    expect(Synx.toYAML(Synx.parse(doc))).toBe('id: 9007199254740993\nname: x\nlist:\n  - 9223372036854775807\n  - 1\n');
  });
  test('toTOML writes a 64-bit integer', () => {
    expect(Synx.toTOML(Synx.parse(doc))).toBe('id = 9007199254740993\nname = "x"\nlist = [9223372036854775807, 1]\n');
  });
  test('toYAML, toTOML and toEnv mask a secret', () => {
    const r = active('token:secret abc123\nnested\n  key:secret def456\n');
    for (const text of [Synx.toYAML(r), Synx.toTOML(r), Synx.toEnv(r)]) {
      expect(text).toContain('[SECRET]');
      expect(text).not.toContain('abc123');
      expect(text).not.toContain('def456');
    }
    expect(Synx.toYAML(r)).toBe('token: "[SECRET]"\nnested:\n  key: "[SECRET]"\n');
    expect(Synx.toTOML(r)).toBe('token = "[SECRET]"\n\n[nested]\nkey = "[SECRET]"\n');
  });
});

describe('input that is not text', () => {
  test.each([
    ['a number', 12345],
    ['null', null],
    ['undefined', undefined],
    ['an object', { a: 1 }],
  ])('%s is refused with a SynxError', (_name, value) => {
    expect(() => Synx.parse(value as any)).toThrow(SynxError);
    expect(() => Synx.format(value as any)).toThrow(SynxError);
    expect(() => Synx.compile(value as any)).toThrow(SynxError);
    expect(() => Synx.schema(value as any)).toThrow(SynxError);
    expect(() => Synx.parseTool(value as any)).toThrow(SynxError);
    expect(() => Synx.parseSynxl(value as any)).toThrow(SynxError);
  });

  test('bytes are read as UTF-8', () => {
    const bytes = Buffer.from('﻿name Привет\nport 8080\n', 'utf-8');
    expect(Synx.parse(bytes as any)).toEqual({ name: 'Привет', port: 8080 });
    expect(Synx.parse(new Uint8Array(bytes) as any)).toEqual({ name: 'Привет', port: 8080 });
  });
});

describe('browser build agrees with the Node build', () => {
  const doc = '!active\n!lock\nname x\nzip "007"\nitems\n  - id 1\n    tag a\n  - id 2\n    tag b\ntext |+\n  line 1\n    line 2\n';

  test('stringify writes text that reads back as the same data', () => {
    const parsed = BrowserSynx.parse(doc);
    const back = BrowserSynx.parse(BrowserSynx.stringify(parsed));
    expect(back).toEqual(parsed);
    expect(BrowserSynx.stringify(parsed)).toBe(Synx.stringify(Synx.parse(doc)));
  });

  test('strict throws on a constraint error', () => {
    expect(() => BrowserSynx.parse('!active\nport[min:10] 5\n', { strict: true })).toThrow(SynxError);
  });

  test('!lock is honoured', () => {
    const parsed = BrowserSynx.parse(doc);
    expect(Synx.isLocked(parsed)).toBe(true);
  });
});

describe('path jail reads the same as synx-core', () => {
  const fs = require('fs');
  const os = require('os');
  const path = require('path');

  test('a base directory that does not exist is refused, not searched', () => {
    const missing = path.join(os.tmpdir(), 'synx-no-such-dir-' + process.pid);
    const r = active('x:include a.synx\n', { basePath: missing });
    expect(r.x).toBe(`INCLUDE_ERR: SECURITY: base directory not found: '${missing}'`);
  });

  // The same words on every platform: a `..` segment in a path that does not
  // exist is judged by the path as the document wrote it.
  test('a path through `..` is judged as synx-core judges it', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-jail2-'));
    const dir = path.join(root, 'base');
    fs.mkdirSync(path.join(dir, 'sub'), { recursive: true });
    fs.mkdirSync(path.join(root, 'outside'));
    fs.writeFileSync(path.join(root, 'outside', 'secret.synx'), 'token OUTSIDE\n');
    fs.writeFileSync(path.join(dir, 'a.synx'), 'name inner\n');
    try {
      const r = active(
        'inside:include sub/../a.synx\nmissing:include sub/../nope.synx\nplain:include nope.synx\n'
        + 'out:include ../outside/secret.synx\ngone:include ../outside/nope.synx\nw:watch ../outside/secret.synx\n',
        { basePath: dir },
      );
      expect(r.inside).toEqual({ name: 'inner' });
      expect(r.plain).toBe('INCLUDE_ERR: file not found: nope.synx');
      expect(r.out).toBe("INCLUDE_ERR: SECURITY: path escapes base directory: '../outside/secret.synx'");
      expect(r.w).toBe("WATCH_ERR: SECURITY: path escapes base directory: '../outside/secret.synx'");
      expect(r.missing).toBe("INCLUDE_ERR: SECURITY: path traversal detected: 'sub/../nope.synx'");
      expect(r.gone).toBe("INCLUDE_ERR: SECURITY: path traversal detected: '../outside/nope.synx'");
      expect(JSON.stringify(r)).not.toContain('OUTSIDE');
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  test('an error of the file system never carries the host path', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-jail3-'));
    fs.mkdirSync(path.join(dir, 'sub'));
    fs.writeFileSync(path.join(dir, 'bad.synx'), Buffer.from([0x61, 0x20, 0xff, 0xfe, 0x0a]));
    try {
      const r = active('nul:include a\u0000b.synx\ndir:include sub\nbad:include bad.synx\nw:watch bad.synx\n', { basePath: dir });
      const text = JSON.stringify(r);
      expect(text).not.toContain(path.basename(dir));
      // No wording of the operating system: the same text on every machine.
      expect(r.nul).toBe('INCLUDE_ERR: file not found: a\u0000b.synx');
      expect(r.dir).toBe('INCLUDE_ERR: not a file: sub');
      expect(r.bad).toBe('INCLUDE_ERR: file is not valid UTF-8: bad.synx');
      expect(r.w).toBe('WATCH_ERR: file is not valid UTF-8: bad.synx');
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  test('a file over the size cap is refused with the words of synx-core', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-big-'));
    const size = 10 * 1024 * 1024 + 1;
    fs.writeFileSync(path.join(dir, 'big.synx'), Buffer.alloc(size, 0x61));
    try {
      const r = active('x:include big.synx\ny:watch big.synx\n', { basePath: dir });
      expect(r.x).toBe(`INCLUDE_ERR: SECURITY: file too large (${size} bytes, max 10485760)`);
      expect(r.y).toBe(`WATCH_ERR: SECURITY: file too large (${size} bytes, max 10485760)`);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  test('the budget counts every include opened, found or not', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-budget-'));
    try {
      // In a group: a root key is also opened by the alias pre-pass.
      let doc = 'g\n';
      for (let i = 0; i < 1030; i++) doc += `  k${i}:include nope${i}.synx\n`;
      const r = active(doc, { basePath: dir }).g;
      expect(r.k0).toBe('INCLUDE_ERR: file not found: nope0.synx');
      expect(r.k1023).toBe('INCLUDE_ERR: file not found: nope1023.synx');
      expect(r.k1024).toBe('INCLUDE_ERR: include budget (1024 files) exceeded');
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  test(':watch of a JSON file reads own keys only', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-watch-'));
    fs.writeFileSync(path.join(dir, 'd.json'), '{"a":{"b":7}}');
    try {
      const r = active('x:watch:a.b d.json\ny:watch:constructor.name d.json\nz:watch:a.__proto__ d.json\n', { basePath: dir });
      expect(r).toEqual({ x: 7, y: null, z: null });
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe('an included !active document is resolved as a document', () => {
  const fs = require('fs');
  const os = require('os');
  const path = require('path');

  test('its own root, its private keys, its own !include lines', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-inc-'));
    fs.writeFileSync(path.join(dir, 'common.synx'), 'site Example\n');
    fs.writeFileSync(path.join(dir, 'child.synx'), [
      '!active',
      '!include common.synx',
      '_private 1',
      'base_hp 100',
      'title Welcome to {site:common}',
      'gameplay',
      '  boss_hp:calc base_hp * 5',
      '  label hp {base_hp}',
      '',
    ].join('\n'));
    try {
      const r = active('base_hp 7\nx:include child.synx\n', { basePath: dir });
      expect(r.x).toEqual({
        base_hp: 100,
        title: 'Welcome to Example',
        gameplay: { boss_hp: 500, label: 'hp 100' },
      });
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe('a secret keeps the text it was given', () => {
  test('a number-like or boolean-like variable is not rewritten', () => {
    const env = { A: '00123', B: '1e5', C: '+5', D: '5.0', E: 'true', F: '"quoted"', G: 'plain' };
    const r = active(
      'a:secret:env A\nb:secret:env B\nc:secret:env C\nd:env:secret D\ne:secret:env E\nf:secret:env F\ng:secret:env G\nh:secret:env:default:007 NOPE\ni:secret:default:1e3\n',
      { env },
    );
    expect(r.a.reveal()).toBe('00123');
    expect(r.b.reveal()).toBe('1e5');
    expect(r.c.reveal()).toBe('+5');
    expect(r.d.reveal()).toBe('5.0');
    expect(r.e.reveal()).toBe('true');
    expect(r.f.reveal()).toBe('quoted');
    expect(r.g.reveal()).toBe('plain');
    expect(r.h.reveal()).toBe('007');
  });

  test('without :secret the cast is unchanged', () => {
    const r = active('a:env A\nb:env B\n', { env: { A: '00123', B: '1e5' } });
    expect(r).toEqual({ a: 123, b: 100000 });
  });
});

describe(':round follows synx-core', () => {
  test('a half rounds away from zero', () => {
    const r = active('a:round -2.5\nb:round 2.5\nc:round:1 -0.25\nd:round:1 0.25\ne:round -0.5\nf:round 0.5\n');
    expect(r).toEqual({ a: -3, b: 3, c: -0.3, d: 0.3, e: -1, f: 1 });
  });

  test('the count is an unsigned integer or nothing', () => {
    const r = active('a:round:-1 15\nb:round:2abc 1.234\nc:round:+2 1.234\nd:round: 1.5\n');
    expect(r).toEqual({ a: 15, b: 1, c: 1.23, d: 2 });
  });
});

describe('strict sees every error the engine leaves', () => {
  test('a type error', () => {
    expect(() => Synx.parse('!active\nport(int) 8080\nserver\n  port abc\n', { strict: true })).toThrow(SynxError);
  });
});
