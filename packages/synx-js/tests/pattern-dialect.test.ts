/**
 * `[pattern:…]` against the `regex` crate synx-core checks patterns with.
 *
 * Every expectation in this file is what the crate answered — `Regex::new`
 * and `is_match` of regex 1.12 (regex-syntax 0.8, regex-automata 0.4) with
 * the features of synx-core — asked pattern by pattern and written down.
 */
import { compileLinear, matchesLinear, runLinear } from '../src/pattern';
import { parsePattern } from '../src/pattern-syntax';
import { SIZE_LIMIT, automatonSizes, exceedsSizeLimit, sizeCeiling, sizeEstimate } from '../src/pattern-size';
import { Synx } from '../src/index';

type Any = Record<string, any>;

/** A pattern, values, and for each value whether the crate finds a match; `null`: the crate does not compile the pattern. */
const CRATE: [string, string[], boolean[] | null][] = [
  ['(?i)^s+$', ['\u017f', 'S', 's', 'x'], [true, true, true, false]],
  ['(?i)^\u00b5$', ['\u039c', '\u03bc', '\u00b5', 'm'], [true, true, true, false]],
  ['(?i)^\u01c5$', ['\u01c6', '\u01c4', '\u01c5', 'd'], [true, true, true, false]],
  ['(?i)^\u03a9$', ['\u2126', '\u03c9', '\u03a9', 'o'], [true, true, true, false]],
  ['(?i)\u1f88', ['\u1f80', '\u1f88', '\u03b1'], [true, true, false]],
  ['(?i)\u{10428}', ['\u{10400}', '\u{10428}', 'x'], [true, true, false]],
  ['(?i)^k$', ['\u212a', 'K', 'k', 'x'], [true, true, true, false]],
  ['(?i)^[^k]$', ['\u212a', 'K', 'x'], [false, false, true]],
  ['(?i)^\u0131$', ['I', 'i', '\u0131', '\u0130'], [false, false, true, false]],
  ['(?i)^\u00df$', ['\u1e9e', '\u00df', 'ss'], [true, true, false]],
  ['(?i)^[\u03c3]$', ['\u03c2', '\u03a3', '\u03c3'], [true, true, true]],
  ['(?-u)^\\w+$', ['\u00e9t\u00e9', 'ete', 'e_1'], [false, true, true]],
  ['(?-u)\\bx', ['\u00e9x', ' x', 'ax'], [true, true, false]],
  ['(?i-u)^k$', ['\u212a', 'K', 'k'], [false, true, true]],
  ['(?i-u)^\u00e9$', ['\u00e9', '\u00c9'], [true, false]],
  ['(?-u)^[a-z]+$', ['abc', 'ab\u00e9'], [true, false]],
  ['(?-u:\\d)\u0663', ['1\u0663', '\u0663\u0663'], [true, false]],
  ['(?-u)^\\x41$', ['A', 'B'], [true, false]],
  ['^[[:digit:]]+$', ['abc', '123'], [false, true]],
  ['^[[:^digit:]]+$', ['abc', '123'], [true, false]],
  ['(?x)^ \\d+ $', ['abc', '42'], [false, true]],
  ['(?x)^ a # one\n b $', ['ab', 'a b'], [true, false]],
  ['^\\d{3 }$', ['abc', '123'], [false, true]],
  ['^\\d{ 2 , 3 }$', ['1', '12', '1234'], [false, true, false]],
  ['^\\b{start}x', ['ax', 'x'], [false, true]],
  ['x\\b{end}', ['xy', 'x '], [false, true]],
  ['x\\b{end-half}', ['xy', 'x '], [false, true]],
  ['\\b{start-half}x', ['ax', ' x'], [false, true]],
  ['^[a-z&&[^aeiou]]+$', ['aei', 'xyz'], [false, true]],
  ['^[\\d--[5-9]]+$', ['1234', '15'], [true, false]],
  ['^[a-c~~b-d]+$', ['ad', 'b'], [true, false]],
  ['^[a-z&&]$', ['a'], [false]],
  ['^\\p{greek}+$', ['abc', '\u03b1\u03b2'], [false, true]],
  ['^\\pl+$', ['a1', 'ab'], [false, true]],
  ['^\\p{sc=Grek}$', ['\u03b1', 'a'], [true, false]],
  ['^\\p{scx=Latin}$', ['a', '\u03b1'], [true, false]],
  ['^\\p{gc!=Lu}$', ['a', 'A'], [true, false]],
  ['^\\P{gc!=Lu}$', ['a', 'A'], [false, true]],
  ['^\\p{isLu}$', ['a', 'A'], [false, true]],
  ['^\\p{L u}$', ['a', 'A'], [false, true]],
  ['^\\p{Any}$', ['a', '\u{1f600}'], [true, true]],
  ['^\\p{ascii}$', ['a', '\u00e9'], [true, false]],
  ['^\\p{assigned}$', ['a', '\u0378'], [true, false]],
  ['^a\\ b$', ['ab', 'a b'], [false, true]],
  ['^\\b?a$', ['ba', 'a'], [false, true]],
  ['^$*a', ['a', 'b'], [true, false]],
  ['(?R)^a$', ['b', 'a'], [false, true]],
  ['(?mR)^b$', ['a\r\nb\r\nc', 'ab'], [true, false]],
  ['(?m)^b$', ['a\nb', 'ab'], [true, false]],
  ['(?R)^.$', ['\r', 'a'], [false, true]],
  ['^a**$', ['aaa', 'b'], [true, false]],
  ['^a+?+$', ['aaa', 'b'], [true, false]],
  ['^\\x{000000041}$', ['A', 'B'], [true, false]],
  ['^\\x41\\u0042\\U00000043$', ['ABC', 'abc'], [true, false]],
  ['^[\\x{1F600}-\\x{1F64F}]$', ['\u{1f600}', 'a'], [true, false]],
  ['^[]a]+$', [']a', 'b'], [true, false]],
  ['^[-a]+$', ['-a', 'b'], [true, false]],
  ['^[a-]+$', ['a-', 'b'], [true, false]],
  ['^[\\r--]$', ['-', '\r'], [false, true]],
  ['^(?P<year>\\d{4})-(?<m>\\d{2})$', ['2026-11', '26-11'], [true, false]],
  ['^(?i:a)b$', ['Ab', 'AB'], [true, false]],
  ['^((?i)a|b)c$', ['Bc', 'bC', 'Ac'], [true, false, true]],
  ['(?s)^.$', ['\n', 'ab'], [true, false]],
  ['^.$', ['\n', '\u{1f600}'], [false, true]],
  ['^\\w+$', ['\u0436\u0301', 'a-b'], [true, false]],
  ['^\\d+$', ['\u0663', '\u00b2'], [true, false]],
  ['^\\s$', ['\u0085', '\u200b'], [true, false]],
  ['^\\<a\\>$', ['a', 'ab'], [true, false]],
  ['a\\z', ['ba', 'ab'], [true, false]],
  ['\\Aa', ['ab', 'ba'], [true, false]],
  ['^(?U)a+$', ['aaa', 'b'], [true, false]],
  ['^(?:)$', ['', 'a'], [true, false]],
  ['^(|a)$', ['', 'a', 'b'], [true, true, false]],
  ['a|', ['zzz'], [true]],
  ['(?)^\\d+$', ['abc'], null],
  ['(?ii)a', ['a'], null],
  ['(?i-)a', ['a'], null],
  ['(?--i)a', ['a'], null],
  ['(?z)a', ['a'], null],
  ['(?<n>a)(?<n>b)', ['ab'], null],
  ['(?<1n>a)', ['a'], null],
  ['a{2,1}', ['aa'], null],
  ['a{,2}', ['aa'], null],
  ['a{', ['a{'], null],
  ['{2}', ['a'], null],
  ['*a', ['a'], null],
  ['(?i)*', ['a'], null],
  ['\\8', ['8'], null],
  ['\\0', ['0'], null],
  ['(?=a)', ['a'], null],
  ['(?<!a)b', ['b'], null],
  ['[z-a]', ['b'], null],
  ['[a', ['a'], null],
  ['a)', ['a'], null],
  ['(a', ['a'], null],
  ['\\e', ['e'], null],
  ['\\Q', ['Q'], null],
  ['\\p{NoSuchProperty}', ['a'], null],
  ['\\p{sc=NoSuchScript}', ['a'], null],
  ['\\p{script}', ['a'], null],
  ['\\pZ{', ['a'], null],
  ['\\x{110000}', ['a'], null],
  ['\\x{D800}', ['a'], null],
  ['\\x4', ['a'], null],
  ['\\u004', ['a'], null],
  ['(?-u:\\xFF)', ['a'], null],
  ['(?-u:.)', ['a'], null],
  ['(?-u:\\W)', ['a'], null],
  ['(?-u:[^a])', ['b'], null],
  ['(?-u:[\\xFF--\\xFF])', ['a'], null],
  ['(?-u:[\u00e9])', ['a'], null],
  ['(?-u)\\pL', ['a'], null],
  ['(?-u:[[:^alpha:]])', ['1'], null],
  ['[\\d-z]', ['1'], null],
  ['[a-\\d]', ['1'], null],
  ['[\\b]', ['b'], null],
  ['\\b{nope}', ['a'], null],
  ['\\b{start', ['a'], null],
  ['a{4294967296}', ['a'], null],
  ['a' + '*'.repeat(250), ['a'], [true]],
  ['a' + '*'.repeat(251), ['a'], null],
  ['('.repeat(125) + 'a' + ')'.repeat(125), ['a'], [true]],
  ['['.repeat(120) + 'a' + ']'.repeat(120), ['a'], [true]],
];

describe('the dialect of [pattern] is the one the regex crate reads (B-083)', () => {
  test('each pattern compiles or is refused as in the crate, and matches the same values', () => {
    for (const [pattern, values, crate] of CRATE) {
      const program = compileLinear(pattern);
      const answers = program === null ? null : values.map(v => runLinear(program, v));
      expect([pattern, answers]).toEqual([pattern, crate]);
    }
  });

  test('through a document: the whole orbit of a letter, the ASCII world of (?-u), a POSIX class', () => {
    const check = (pattern: string, value: string): unknown =>
      (Synx.parse(`!active\nv[pattern:${pattern}] ${value}\n`, { env: {} }) as Any).v;
    // B-083.2: `ſ` is an `s` when case is ignored.
    expect(check('(?i)^s+$', '\u017f')).toBe('\u017f');
    // B-083.4: without Unicode `\w` is ASCII — this reader let `été` through.
    expect(check('(?-u)^\\w+$', '\u00e9t\u00e9')).toBe("CONSTRAINT_ERR: 'v' does not match pattern /(?-u)^\\w+$/");
    // B-083.5: `[[:digit:]]` was unknown syntax here, and the constraint went unchecked.
    expect(check('^[[:digit:]]+$', 'abc')).toBe("CONSTRAINT_ERR: 'v' does not match pattern /^[[:digit:]]+$/");
    // B-083.6: the crate does not compile `(?)`; this reader checked the rest of the pattern.
    expect(check('(?)^\\d+$', 'abc')).toBe('abc');
  });
});

describe('a pattern the regex crate refuses for its size is not checked (B-084)', () => {
  const fits = (pattern: string): boolean => !exceedsSizeLimit(parsePattern(pattern)!);

  // The largest count each of these compiles with in the crate; one more is refused.
  const LARGEST: [string, number][] = [
    ['a{N}', 327673],
    ['^a{N}$', 327674],
    ['(?:a{N})|x', 327670],
    ['a{N}a{N}', 163836],
    ['a{N,}', 327672],
    ['(a){N}', 109224],
    ['(?:a|q){N}', 131069],
    ['(?:aa|ab){N}', 41609],
    ['a?{N}', 100822],
    ['a*{N}', 145632],
    ['a{2,3}{N}', 62414],
    ['(?:ab){2,}{N}', 62414],
    ['(?:^|a){N}', 77099],
    ['[a-z]{N}', 145632],
    ['[0-9a-f]{N}', 131069],
    ['([0-9a-f]){N}', 72816],
    ['[[:alpha:]]{N}', 131069],
    ['(?-u:\\w){N}', 109224],
    ['(?i:a){N}', 131069],
    ['(?i:k){N}', 44430],
    ['(?i:[a-z]){N}', 34492],
    ['\u00e9{N}', 163836],
    ['\\x{10FFFF}{N}', 81918],
    ['(?:ab|cd){N}', 41609],
    ['(?:foo|bar|baz){N}', 19709],
    ['(?:a|bc){N}', 56986],
    ['(?:abc|abd){N}', 27024],
    ['(?:\u00e9|\u00e8|\u00ea){N}', 79436],
    ['.{N}', 10485],
    ['^.{N}$', 10485],
    ['.{N}.{N}', 5242],
    ['(?:.|q){N}', 9497],
    ['(?:..|.b){N}', 4983],
    ['(?s:.){N}', 10877],
    ['[^a]{N}', 10485],
    ['[\u0430-\u044f]{N}', 52428],
    ['[\u03b1-\u03c9]{N}', 52428],
    ['[a-z\u0430-\u044f\u0451]{N}', 38550],
    ['[\u{1f600}-\u{1f64f}]{N}', 39718],
  ];

  test('the limit falls where the crate has it', () => {
    for (const [shape, largest] of LARGEST) {
      const at = (n: number): string => shape.split('N').join(String(n));
      expect([at(largest), fits(at(largest))]).toEqual([at(largest), true]);
      expect([at(largest + 1), fits(at(largest + 1))]).toEqual([at(largest + 1), false]);
    }
  });

  test('classes named by a Unicode property: the limit is the crate\'s, give or take the tables of the runtime', () => {
    // The crate compiles `\w{209}` and `\d{2046}` and refuses one more of either.
    expect(fits('^\\w{190}$')).toBe(true);
    expect(fits('^\\w{230}$')).toBe(false);
    expect(fits('^\\d{1900}$')).toBe(true);
    expect(fits('^\\d{2200}$')).toBe(false);
    expect(fits('^[\\w.-]{1,255}$')).toBe(false);
    expect(fits('^[A-Za-z0-9_.-]{1,255}$')).toBe(true);
  });

  test('a refused pattern is not checked, in a document or on its own', () => {
    // 36 × `.{1000}` in 253 bytes: 36 000 copies of a class the crate spends 1 KB on.
    const pattern = `${'.{1000}'.repeat(36)}\\d`;
    expect(compileLinear(pattern)).toBeNull();
    expect(matchesLinear(pattern, 'a'.repeat(64000))).toBeNull();
    const value = 'a'.repeat(2000);
    expect((Synx.parse(`!active\nk[pattern:${pattern}] ${value}\n`, { env: {} }) as Any).k).toBe(value);
    // `^\w{300}$` is one of those: synx-core does not check it either.
    expect((Synx.parse('!active\nk[pattern:^\\w{300}$] no\n', { env: {} }) as Any).k).toBe('no');
    expect((Synx.parse('!active\nk[pattern:^\\w{3}$] no\n', { env: {} }) as Any).k)
      .toBe("CONSTRAINT_ERR: 'k' does not match pattern /^\\w{3}$/");
  });

  test('a count the crate would not even start on is refused without being expanded', () => {
    for (const pattern of ['a{4294967295}', '(?:a{60000}){60000}', '.{4000000000,}', '(?:\\b|a){4000000000}', 'a{0,4294967295}']) {
      const started = Date.now();
      expect([pattern, compileLinear(pattern)]).toEqual([pattern, null]);
      expect(Date.now() - started).toBeLessThan(2000);
    }
    // Only the empty string, however often: the crate keeps one copy, and so does the program.
    const program = compileLinear('^\\b{4000000000}a$')!;
    expect(program.op.length).toBeLessThan(10);
    expect(runLinear(program, 'a')).toBe(true);
  });

  test('the first look never undercounts what the automata take', () => {
    const shapes = ['X', 'X{3}', 'X{2,5}', 'X+', 'X*', 'X?', '(X)', '(?:X|q)', '(?:X|X)', 'XX', '(?:Xa|Xb)', '(?:aX|bX){2}', '(?i:X)'];
    const units = ['a', '\u00e9', '\u{1f600}', '.', '[a-z]', '[^a]', '\\d', '\\D', '\\s', '\\S', '\\w', '\\W', '\\pL', '\\p{Lu}',
      '[\\w.-]', '[\u0430-\u044f]', '\\b', '^', '(?:ab|cd)', '(?:a|bc|d)', '[\\pL--\\p{Lo}]', '[\\w&&[^\\d]]', '\\p{Greek}'];
    for (const shape of shapes) {
      for (const unit of units) {
        const pattern = shape.split('X').join(unit);
        const tree = parsePattern(pattern);
        if (tree === null) continue;
        const [forward, reversed] = automatonSizes(tree);
        expect([pattern, sizeCeiling(tree) >= Math.max(forward, reversed)]).toEqual([pattern, true]);
        expect(Math.max(forward, reversed)).toBeLessThan(SIZE_LIMIT);
      }
    }
  });

  test('the second look is within a tenth of the count: a pattern is counted state by state only near the limit', () => {
    const bodies = ['a', '\u00e9?', 'a*', 'a+', 'a{1,3}', '(a){0,2}', '.', '\\d?\\s', '[\u0430-\u044f]+', '(?:ab|cd)', '(?:a|\\d)', '\\w',
      '(?:\\b){3}a', '(?i:k){2,}', '[^a]{2,}(?-u:\\w)*', '(?:a?){2,}', '\\pL(?:\u00e9|\u00e8)?'];
    const shapes = ['(?:X){60}', '(?:X){40,}', '(?:X){2,40}', '(?:X){30}(?:X){30}', '((?:X){40})|q', '(?:X|z){40}'];
    for (const body of bodies) {
      for (const shape of shapes) {
        const pattern = shape.split('X').join(body);
        const tree = parsePattern(pattern)!;
        const [forward, reversed] = automatonSizes(tree);
        const ratio = sizeEstimate(tree) / Math.max(forward, reversed);
        expect([pattern, ratio > 0.9 && ratio < 1.1]).toEqual([pattern, true]);
      }
    }
  });
});
