/**
 * What a service pays to read its config: the modules a first `Synx.parse`
 * loads, and the pattern matcher's reuse of its own scratch.
 */
import { compileLinear, runLinear, matchesLinear } from '../src/pattern';

type Any = Record<string, any>;

describe('reading a document loads the parser and the engine, nothing else', () => {
  // Every module of the package but the three on the path of a config read.
  const others = ['synxl', 'pattern', 'calc', 'json', 'format', 'binary', 'types'];

  /** Load the package in a registry of its own; `loaded` receives each other module as it is required. */
  const fresh = (run: (Synx: Any, loaded: string[]) => void): void => {
    jest.isolateModules(() => {
      const loaded: string[] = [];
      for (const name of others) {
        jest.doMock(`../src/${name}`, () => {
          loaded.push(name);
          return jest.requireActual(`../src/${name}`);
        });
      }
      // eslint-disable-next-line @typescript-eslint/no-var-requires
      const mod = require('../src/index');
      run(mod.Synx, loaded);
      for (const name of others) jest.dontMock(`../src/${name}`);
    });
  };

  test('a static document and an active config with markers, constraints and a secret', () => {
    fresh((Synx, loaded) => {
      expect(loaded).toEqual([]);
      expect(Synx.parse('name app\nport 8080\nlist\n  - a\n  - b\ntext |\n  one\n  two\n')).toEqual({
        name: 'app', port: 8080, list: ['a', 'b'], text: 'one\ntwo',
      });
      const config = Synx.parse(
        '!active\n'
        + 'port[type:int, min:1, max:65535]:env:default:3001 PORT\n'
        + 'node_env[enum:development|production]:env:default:development NODE_ENV\n'
        + 'database\n  url[required]:secret:env DATABASE_URL\n  pool_max[type:int]:env:default:10 POOL\n'
        + 'site http://localhost:{port}\n',
        { env: { DATABASE_URL: 'postgres://x' }, strict: true },
      );
      expect(config.port).toBe(3001);
      expect(config.site).toBe('http://localhost:3001');
      expect(config.database.url.reveal()).toBe('postgres://x');
      expect(loaded).toEqual([]);
    });
  });

  test('each of the others arrives with the first call that needs it', () => {
    fresh((Synx, loaded) => {
      expect((Synx.parse('!active\nbase 4\ntotal:calc base * 2\n', { env: {} }) as Any).total).toBe(8);
      expect(loaded).toEqual(['calc']);
      expect((Synx.parse('!active\nslug[pattern:^[a-z-]+$] Not-A-Slug\n', { env: {} }) as Any).slug)
        .toBe("CONSTRAINT_ERR: 'slug' does not match pattern /^[a-z-]+$/");
      expect(loaded).toEqual(['calc', 'pattern']);
      expect(Synx.toJSON({ b: 1, a: 2 }, false)).toBe('{"a":2,"b":1}');
      expect(loaded).toEqual(['calc', 'pattern', 'json']);
      expect(Synx.stringify({ a: 1 })).toBe('a 1\n');
      expect(loaded).toEqual(['calc', 'pattern', 'json', 'synxl']);
      expect(Synx.parseSynxl('!synxl 1\n!fields id ; name\n1 ; a\n').records[0].values).toEqual({ id: 1, name: 'a' });
      expect(Synx.format('b 2\na 1\n')).toBe('a 1\nb 2\n');
      expect(Synx.decompile(Synx.compile('a 1\n'))).toBe('a 1\n');
      expect(loaded).toEqual(['calc', 'pattern', 'json', 'synxl', 'format', 'binary']);
    });
  });

  test('the functions and limits of SYNXL are exports of the package as before', () => {
    fresh((Synx, loaded) => {
      // eslint-disable-next-line @typescript-eslint/no-var-requires
      const mod = require('../src/index');
      expect(mod.SYNXL_VERSION).toBe(1);
      expect(Synx.SYNXL_VERSION).toBe(1);
      expect(mod.MAX_SYNXL_RECORD_BYTES).toBe(16 * 1024 * 1024);
      expect(mod.MAX_SYNXL_FIELDS).toBe(4096);
      expect(mod.MAX_SYNXL_FIELD_NAME_BYTES).toBe(255);
      expect(mod.MAX_SYNXL_FIELD_LISTS).toBe(65536);
      expect(mod.MAX_SYNXL_RECORDS).toBe(16777216);
      expect(mod.utf8Length('aé€😀')).toBe(10);
      expect(new mod.SynxError('CALC_ERR: x').code).toBe('CALC_ERR');
      expect(loaded).toEqual([]);
      expect(mod.splitRecordLine('1 ; "a ; b" ; c')).toEqual([
        { value: '1', quoted: false }, { value: 'a ; b', quoted: true }, { value: 'c', quoted: false },
      ]);
      expect([...mod.streamSynxl('!synxl 1\n!fields id\n7\n')].map((r: Any) => r.values)).toEqual([{ id: 7 }]);
      expect(mod.synxlToNDJSON('!synxl 1\n!fields id\n7\n')).toBe('{"id":7}\n');
      expect(mod.synxlToJSON('!synxl 1\n!fields id\n7\n')).toBe('[{"id":7}]');
      expect(mod.writeSynxl([{ id: 7 }])).toBe('!synxl 1\n!fields id\n7\n');
      expect(mod.parseSynxl('!synxl 1\n!fields id\n7\n').records).toHaveLength(1);
      expect(typeof mod.streamSynxlAsync).toBe('function');
      // A failure of the record reader is the package's own error type.
      expect(() => mod.parseSynxl('no prologue\n')).toThrow(mod.SynxError);
    });
  });
});

describe('a compiled pattern keeps nothing of one value for the next', () => {
  const patterns: [string, RegExp][] = [
    ['^[a-z0-9-]+$', /^[a-z0-9-]+$/u],
    ['^[^@\\s]+@[^@\\s]+\\.[a-z]{2,}$', /^[^@\s]+@[^@\s]+\.[a-z]{2,}$/u],
    ['ab|cd', /ab|cd/u],
    ['^a|b', /^a|b/u],
    ['(^a)+b', /(^a)+b/u],
    ['^(ab|a)(bc|c)?$', /^(ab|a)(bc|c)?$/u],
    ['b$', /b$/u],
    ['^$', /^$/u],
    ['^.{2,4}x', /^.{2,4}x/u],
    ['[0-9]{3}', /[0-9]{3}/u],
  ];
  const values = [
    '', 'a', 'b', 'ab', 'ba', 'xb', 'abc', 'aab', 'cd', 'xcdx', 'alpha-12', 'Alpha', 'a b', 'user@example.com',
    'user@example', '@example.com', 'aaaax', 'ax', '12', '123', 'x1234y', 'é', 'aé', '😀b', `${'a'.repeat(300)}b`,
  ];

  test('one program run over every value answers as a pattern compiled for that value alone', () => {
    for (const [pattern, re] of patterns) {
      const shared = compileLinear(pattern);
      expect(shared).not.toBeNull();
      for (let round = 0; round < 2; round++) {
        for (const value of values) {
          const own = compileLinear(pattern);
          const expected = runLinear(own!, value);
          expect([pattern, value, runLinear(shared!, value)]).toEqual([pattern, value, expected]);
          expect([pattern, value, expected]).toEqual([pattern, value, re.test(value)]);
          expect(matchesLinear(pattern, value)).toBe(expected);
        }
      }
    }
  });

  test('the table of states answers as the stepped threads do', () => {
    // A small generator of patterns and values over a tiny alphabet, the same on every run.
    let seed = 20261112;
    const rnd = (n: number): number => {
      seed = (Math.imul(seed, 1103515245) + 12345) & 0x7fffffff;
      return seed % n;
    };
    const atoms = ['a', 'b', 'c', '.', '[ab]', '[^a]', '\\d', '(?:ab)', '(a|b)', '(b|bc)', '[a-c]'];
    const repeats = ['', '', '', '*', '+', '?', '{2}', '{1,3}', '{2,}'];
    const pattern = (): string => {
      let out = rnd(3) === 0 ? '^' : '';
      const parts = 1 + rnd(4);
      for (let i = 0; i < parts; i++) {
        out += atoms[rnd(atoms.length)] + repeats[rnd(repeats.length)];
        if (rnd(6) === 0) out += '|';
        if (rnd(9) === 0) out += '$';
        if (rnd(11) === 0) out += '^';
      }
      return rnd(3) === 0 ? `${out}$` : out;
    };
    const letters = ['a', 'b', 'c', '1', ' ', 'é'];
    const value = (): string => {
      let out = '';
      for (let n = rnd(9); n > 0; n--) out += letters[rnd(rnd(8) === 0 ? letters.length : 4)];
      return out;
    };
    let compared = 0;
    for (let i = 0; i < 400; i++) {
      const text = pattern();
      const tabled = compileLinear(text);
      const stepped = compileLinear(text);
      if (tabled === null || stepped === null) {
        expect(tabled).toBe(stepped);
        continue;
      }
      stepped.tabled = false;
      for (let k = 0; k < 40; k++) {
        const v = value();
        expect([text, v, runLinear(tabled, v)]).toEqual([text, v, runLinear(stepped, v)]);
        compared++;
      }
    }
    expect(compared).toBeGreaterThan(10000);
  });

  test('a pattern with more states than the table holds goes back to stepping', () => {
    // The eighth character from the end is `a`: 2^8 states for a table.
    const program = compileLinear('^[ab]*a[ab]{7}$')!;
    const re = /^[ab]*a[ab]{7}$/;
    expect(program.tabled).toBe(true);
    for (let i = 0; i < 4096; i++) {
      const v = i.toString(2).padStart(12, '0').replace(/0/g, 'a').replace(/1/g, 'b');
      expect([v, runLinear(program, v)]).toEqual([v, re.test(v)]);
    }
    expect(program.tabled).toBe(false);
    expect(program.dfa).toBeNull();
  });

  test('a word boundary is read from both sides, without a table', () => {
    const program = compileLinear('\\bfoo\\b')!;
    expect(program.tabled).toBe(false);
    expect(runLinear(program, 'a foo b')).toBe(true);
    expect(runLinear(program, 'afoo b')).toBe(false);
  });

  test('the step counter starts over before it could wrap', () => {
    for (const tabled of [true, false]) {
      const program = compileLinear('^a+b$')!;
      program.tabled = tabled;
      program.gen = 0x7fffffff - 3;
      expect(runLinear(program, 'aaab')).toBe(true);
      expect(program.gen).toBeLessThan(16);
      program.gen = 0x7fffffff - 3;
      expect(runLinear(program, 'aaa')).toBe(false);
      expect(runLinear(program, 'aab')).toBe(true);
    }
  });

  test('a pattern too large for the table of ASCII answers still matches', () => {
    // 1 200 steps: past the size the table is kept for.
    const pattern = `^${'[a-c]'.repeat(1200)}$`;
    const program = compileLinear(pattern)!;
    program.tabled = false;
    expect(runLinear(program, 'abc'.repeat(400))).toBe(true);
    expect(program.ascii).toBeNull();
    expect(runLinear(program, `${'abc'.repeat(399)}abd`)).toBe(false);
  });
});
