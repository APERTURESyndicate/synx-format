/**
 * Regression set for the SYNXL hardening pass.
 *
 * Every case here is the minimised form of a defect that shipped: silent data
 * loss, a divergence from the reference implementation, or a denial of service.
 * The conformance suite covers the ones expressible as a fixture; these are the
 * rest — the ones about *how* the implementation behaves (time, memory, types)
 * rather than about what a document projects to.
 *
 * Section references are to `docs/spec/SYNXL-1-NORMATIVE.md`.
 */

import { parseSynxl, writeSynxl, synxlToJSON, synxlToNDJSON, streamSynxl, streamSynxlAsync } from '../src/synxl';
import { parseData } from '../src/parser';
import { toCanonicalJSONString } from '../src/json';

const P = '!synxl 1';

/** §14.1 — parse, write, parse, and compare the projections. */
function roundTrips(doc: string): boolean {
  const first = parseSynxl(doc);
  const written = writeSynxl(first);
  return synxlToJSON(parseSynxl(written)) === synxlToJSON(first);
}

describe('SYNXL hardening — multiline bodies are raw text (SYNX §8.4.1, §9.4)', () => {
  test('comment-shaped lines survive inside | and |+', () => {
    // Deleting these emptied every code comment and Markdown heading out of a
    // dataset, with no diagnostic and a successful exit.
    expect(parseData('body |+\n    A\n    # hash\n    // slash\n    B\n').root)
      .toEqual({ body: 'A\n# hash\n// slash\nB' });
    expect(parseData('body |\n    A\n    # hash\n    B\n').root)
      .toEqual({ body: 'A\n# hash\nB' });
  });

  test('a ### fence inside a body does not swallow the next field', () => {
    expect(parseData('a |+\n    x\n    ###\n    y\nb 42\n').root)
      .toEqual({ a: 'x\n###\ny', b: 42 });
  });

  test('a directive inside a body stays data and is not executed', () => {
    const { root, includes } = parseData('body |+\n    one\n    !include secret.synx\n    two\n');
    expect(root).toEqual({ body: 'one\n!include secret.synx\ntwo' });
    // `includes` is left unset when no directive was recognised at all.
    expect(includes ?? []).toHaveLength(0);
  });

  test('a directive inside a block comment is not executed', () => {
    const { includes, mode } = parseData('###\n!include secret.synx\n!active\n###\nk 1\n');
    // `includes` is left unset when no directive was recognised at all.
    expect(includes ?? []).toHaveLength(0);
    expect(mode).toBe('static');
  });
});

describe('SYNXL hardening — indentation is Unicode (§3.3, SYNX §4)', () => {
  test('a block line indented with U+00A0 keeps its key', () => {
    expect(synxlToNDJSON(`${P}\n!fields id ; b[block]\n1\n b x\n`))
      .toBe('{"b":"x","id":1}\n');
  });

  test('a |+ body with mixed-width Unicode indents does not throw', () => {
    // This shape used to slice inside a character and panic in Rust.
    expect(() => parseData('m |+\n    a\n    b\n')).not.toThrow();
    expect(() => parseData('m |+\n  a\n\t b\n')).not.toThrow();
    expect(() => parseData('m |+\n    a\n    　b\n')).not.toThrow();
  });
});

describe('SYNXL hardening — the writer preserves what it wrote (§14.1, §14.3)', () => {
  test('a value with leading Unicode whitespace keeps its record', () => {
    // Written bare, the line acquires an indent (§3.3) and the whole record is
    // read back as orphaned block content.
    expect(roundTrips(`${P}\n!fields a ; b\n" x" ; 2\n`)).toBe(true);
    expect(roundTrips(`${P}\n!fields a ; b\n"　x" ; 2\n`)).toBe(true);
  });

  test('a quoted string in a typed column stays a string', () => {
    const doc = `${P}\n!fields id[type:int] ; code[type:int] ; name[type:float]\n1 ; "N/A" ; "n/a"\n`;
    expect(roundTrips(doc)).toBe(true);
    expect(parseSynxl(writeSynxl(parseSynxl(doc))).records[0].values.code).toBe('N/A');
  });

  test('a block value containing " //" or " #" survives', () => {
    // Quoting cannot protect these: SYNX truncates the comment before it
    // recognises quotes, so the value has to move into a |+ body.
    expect(roundTrips(`${P}\n!fields id ; b[block]\n1\n  b |+\n      a // b\n`)).toBe(true);
    expect(roundTrips(`${P}\n!fields id ; b[block]\n1\n  b |+\n      price 5 # each\n`)).toBe(true);
  });

  test('a value containing CR is quoted rather than promoted', () => {
    // A |+ body cannot hold a bare CR, so promoting one loses it.
    expect(roundTrips(`${P}\n!fields a ; b\nx\r\r; 2\n`)).toBe(true);
  });

  test('a value holding both quote characters still round-trips', () => {
    expect(roundTrips(`${P}\n!fields a\n"a'b\n`)).toBe(true);
  });
});

describe('SYNXL hardening — numbers match the reference implementation (§8.1, §8.2)', () => {
  test('a 64-bit id keeps every digit', () => {
    const values = parseSynxl(`${P}\n!fields id\n1526947345124249601\n`).records[0].values;
    expect(String(values.id)).toBe('1526947345124249601');
    expect(synxlToNDJSON(`${P}\n!fields id\n9223372036854775807\n`))
      .toBe('{"id":9223372036854775807}\n');
  });

  test('an integer past i64 falls through to the string rule', () => {
    expect(synxlToNDJSON(`${P}\n!fields a\n9223372036854775808\n`))
      .toBe('{"a":"9223372036854775808"}\n');
  });

  test('a typed int that overflows fails with CastFailed', () => {
    const doc = parseSynxl(`${P}\n!fields v[type:int]\n9223372036854775808\n`);
    expect(doc.records[0].values.v).toBeNull();
    expect(doc.records[0].diagnostics.map(d => d.kind)).toEqual(['CastFailed']);
  });

  test('negative zero keeps its sign in the projection', () => {
    expect(synxlToNDJSON(`${P}\n!fields a(float)\n-0.0\n`)).toBe('{"a":-0.0}\n');
  });
});

describe('SYNXL hardening — the canonical writer drops nothing (§12.1)', () => {
  test('numeric-looking keys stay in Unicode scalar order', () => {
    // A plain object re-orders integer-like keys numerically, undoing the sort.
    expect(toCanonicalJSONString({ '10': 1, '2': 2, a: 3 }, false))
      .toBe('{"10":1,"2":2,"a":3}');
  });

  test('a field named __proto__ survives into the projection', () => {
    expect(synxlToNDJSON(`${P}\n!fields id ; __proto__\n1 ; boo\n`))
      .toBe('{"__proto__":"boo","id":1}\n');
  });

  test('a user field starting with __synx is not mistaken for metadata', () => {
    expect(synxlToNDJSON(`${P}\n!fields id ; __synx_id\n1 ; keep\n`))
      .toBe('{"__synx_id":"keep","id":1}\n');
  });

  test('parsing never pollutes Object.prototype', () => {
    parseSynxl(`${P}\n!fields id ; __proto__ ; b[block]\n1 ; boo\n  b\n    __proto__ x\n`);
    expect(({} as Record<string, unknown>).boo).toBeUndefined();
    expect(({} as Record<string, unknown>).x).toBeUndefined();
  });
});

describe('SYNXL hardening — hostile input costs bounded work (§16.3)', () => {
  test('a catastrophic pattern does not hang validation', () => {
    // `^(a+)+$` against 45 characters is exponential in a backtracking engine.
    const doc = `${P}\n!fields v[type:string, pattern:^(a+)+$]\n${'a'.repeat(45)}!\n`;
    const started = Date.now();
    parseSynxl(doc, { validate: true });
    expect(Date.now() - started).toBeLessThan(1000);
  });

  test('many undeclared block keys stay linear', () => {
    const build = (n: number) => {
      const lines = [P, '!fields id ; m[block]', '1'];
      for (let i = 0; i < n; i++) lines.push(`  k${i} v`);
      return lines.join('\n') + '\n';
    };
    const time = (doc: string) => { const t = Date.now(); parseSynxl(doc); return Date.now() - t; };
    const small = time(build(2000));
    const large = time(build(8000));
    // 4x the keys; anything past ~16x the time is superlinear, not noise.
    expect(large).toBeLessThanOrEqual((small + 20) * 16);
  });

  test('a writer given a long run of spaces stays linear', () => {
    const build = (n: number) =>
      writeSynxl(parseSynxl(`${P}\n!fields id ; v\n1 ; a${' '.repeat(n)}b\n`));
    const time = (n: number) => { const t = Date.now(); build(n); return Date.now() - t; };
    const small = time(25_000);
    const large = time(100_000);
    expect(large).toBeLessThanOrEqual((small + 20) * 16);
  });

  test('the async reader bounds its line buffer', async () => {
    // Input with no LF used to grow the buffer without limit and rescan it.
    async function* chunks() {
      yield `${P}\n!fields id ; payload\n1; `;
      for (let i = 0; i < 8; i++) yield 'x'.repeat(1024 * 1024);
      yield '\n2; ok\n';
    }
    const started = Date.now();
    const records = [];
    for await (const rec of streamSynxlAsync(chunks(), { maxRecordBytes: 256 * 1024 })) {
      records.push(rec);
    }
    expect(records).toHaveLength(2);
    expect(Date.now() - started).toBeLessThan(5000);
  });
});

describe('SYNXL hardening — readers agree with each other (§1.4, §15.1)', () => {
  const documents = [
    `${P}\n!fields a ; b\n1 ; 2\n3 ; 4\n`,
    `${P}\n!fields a ; b[block]\n1\n  b\n    k v\n`,
    `${P}\n!fields a\n;\n`,
    `${P}\n!fields a ; b\n1 ; 2\n!fields a ; c\n5 ; 6\n`,
  ];

  test.each(documents)('streaming and in-memory readers agree (%#)', doc => {
    const streamed = [...streamSynxl(doc)].map(r => r.values);
    const whole = parseSynxl(doc).records.map(r => r.values);
    expect(JSON.stringify(streamed)).toBe(JSON.stringify(whole));
  });
});
