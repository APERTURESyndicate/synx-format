import Synx from '../src/index';
import {
  parseSynxl,
  streamSynxl,
  streamSynxlAsync,
  synxlToJSON,
  synxlToNDJSON,
  writeSynxl,
  splitRecordLine,
  utf8Length,
  SynxlError,
  SYNXL_VERSION,
  MAX_SYNXL_FIELD_LISTS,
} from '../src/synxl';
import { SynxError } from '../src/types';
import type { SynxlDiagnosticKind, SynxlRecord, SynxObject } from '../src/types';

const PROLOGUE = '!synxl 1';

/** Kinds only, in order — the shape most assertions care about. */
function kinds(ds: { kind: SynxlDiagnosticKind }[]): SynxlDiagnosticKind[] {
  return ds.map(d => d.kind);
}

/** `kind record line` triples, the conformance-suite diagnostic format (§18). */
function triples(ds: { kind: string; record: number; line: number }[]): string[] {
  return ds.map(d => `${d.kind} ${d.record} ${d.line}`);
}

/** The §11.1 condition token a document fails with. */
function conditionOf(text: string): string {
  try {
    parseSynxl(text);
  } catch (e) {
    if (e instanceof SynxlError) return e.condition;
    throw e;
  }
  throw new Error('expected a hard error');
}

// ─── §4 Document structure ────────────────────────────────

describe('SYNXL — prologue and document structure', () => {
  test('parses a minimal document', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields id ; name\n1 ; Alice\n`);
    expect(doc.version).toBe(SYNXL_VERSION);
    expect(doc.records).toHaveLength(1);
    expect(doc.records[0].values).toEqual({ id: 1, name: 'Alice' });
    expect(doc.records[0].index).toBe(0);
    expect(doc.records[0].line).toBe(3);
    expect(doc.diagnostics).toHaveLength(0);
  });

  test('comments and blank lines may precede the prologue', () => {
    const doc = parseSynxl('# hi\n// there\n\n###\nignored\n###\n!synxl 1\n!fields a\n7\n');
    expect(doc.records[0].values).toEqual({ a: 7 });
  });

  test('missing prologue is a hard error', () => {
    expect(() => parseSynxl('!fields a\n1\n')).toThrow(SynxError);
    expect(() => parseSynxl('1 ; 2\n')).toThrow(/missing prologue/);
    expect(() => parseSynxl('')).toThrow(/missing prologue/);
  });

  test('malformed prologue is a hard error', () => {
    expect(() => parseSynxl('!synxl\n!fields a\n1\n')).toThrow(/malformed prologue/);
    expect(() => parseSynxl('!synxl one\n!fields a\n1\n')).toThrow(/malformed prologue/);
  });

  test('unsupported version is a hard error', () => {
    expect(() => parseSynxl('!synxl 2\n!fields a\n1\n')).toThrow(/unsupported SYNXL format version 2/);
  });

  test('record line with no field list in effect is a hard error', () => {
    expect(() => parseSynxl(`${PROLOGUE}\n1 ; 2\n`)).toThrow(/no field list in effect/);
  });

  test('unknown directive at indent 0 is a hard error', () => {
    expect(() => parseSynxl(`${PROLOGUE}\n!fields a\n!active\n`)).toThrow(/unknown directive/);
  });

  test('a repeated identical prologue (concatenated shards) is accepted', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a\n1\n${PROLOGUE}\n!fields a\n2\n`);
    expect(doc.records.map(r => r.values.a)).toEqual([1, 2]);
  });

  test('BOM and CRLF line endings are handled', () => {
    const doc = parseSynxl('﻿!synxl 1\r\n!fields a ; b\r\n1 ; x\r\n');
    expect(doc.records[0].values).toEqual({ a: 1, b: 'x' });
  });

  test('### toggles block-comment mode at indent 0', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a\n1\n###\n2\n3\n###\n4\n`);
    expect(doc.records.map(r => r.values.a)).toEqual([1, 4]);
  });

  test('a comment line terminates the preceding record block', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; b[block]\n1\n  b\n    k v\n# done\n2\n`);
    expect(doc.records[0].values.b).toEqual({ k: 'v' });
    expect(doc.records[1].values.b).toBeNull();
  });
});

// ─── §5 Field list ────────────────────────────────────────

describe('SYNXL — field list', () => {
  test('parses types, constraints and the block flag', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields id[type:int, required] ; score(float) ; messages[block]\n1 ; 0.5\n`);
    const fl = doc.fieldLists[0];
    expect(fl.arity).toBe(2);
    expect(fl.fields.map(f => f.name)).toEqual(['id', 'score', 'messages']);
    expect(fl.fields[0].type).toBe('int');
    expect(fl.fields[0].constraints).toEqual({ type: 'int', required: true });
    expect(fl.fields[1].type).toBe('float');
    expect(fl.fields[2].block).toBe(true);
  });

  test('unrecognized constraint parts are ignored (forward compatibility)', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a[shiny, futuristic:yes]\n1\n`);
    expect(doc.records[0].values).toEqual({ a: 1 });
  });

  test('duplicate field names are a hard error', () => {
    expect(() => parseSynxl(`${PROLOGUE}\n!fields a ; b ; a\n1 ; 2 ; 3\n`)).toThrow(/duplicate field name "a"/);
  });

  test('any marker in a declaration is a hard error — chain or single', () => {
    // §5.2 covers a chain (`:a:b`) and a lone marker (`:custom`) alike.
    for (const decl of ['a:env:default', 'a[type:int]:env', 'a:custom', ':custom']) {
      const src = `${PROLOGUE}\n!fields ${decl}\n1\n`;
      expect(() => parseSynxl(src)).toThrow(/marker/);
      expect(conditionOf(src)).toBe('MarkerChainInFieldDecl');
    }
  });

  test('a field list with arity 0 is a hard error', () => {
    expect(() => parseSynxl(`${PROLOGUE}\n!fields body[block]\n1\n`)).toThrow(/arity 0/);
    expect(conditionOf(`${PROLOGUE}\n!fields a[block] ; b[block]\n1\n`)).toBe('ZeroArityFieldList');
  });

  test('non-deterministic hints are a hard error', () => {
    expect(() => parseSynxl(`${PROLOGUE}\n!fields a(random)\n1\n`)).toThrow(/non-deterministic/);
    expect(() => parseSynxl(`${PROLOGUE}\n!fields a[type:random:bool]\n1\n`)).toThrow(/non-deterministic/);
  });

  test('block combined with a type is a hard error', () => {
    expect(() => parseSynxl(`${PROLOGUE}\n!fields a[block, type:int]\n1\n`)).toThrow(/combines \[block\] with a type/);
    expect(() => parseSynxl(`${PROLOGUE}\n!fields a(int)[block]\n1\n`)).toThrow(/combines \[block\] with a type/);
  });

  test('empty or unparsable field declarations are hard errors', () => {
    expect(() => parseSynxl(`${PROLOGUE}\n!fields\n1\n`)).toThrow(/malformed field list/);
    expect(() => parseSynxl(`${PROLOGUE}\n!fields   \n1\n`)).toThrow(/malformed field list/);
    expect(() => parseSynxl(`${PROLOGUE}\n!fields a ;; b\n1\n`)).toThrow(/empty field declaration/);
    expect(() => parseSynxl(`${PROLOGUE}\n!fields a b\n1\n`)).toThrow(/malformed field declaration/);
    expect(() => parseSynxl(`${PROLOGUE}\n!fields a[type:int\n1\n`)).toThrow(/unbalanced/);
  });

  test('field name over MAX_SYNXL_FIELD_NAME_BYTES is a hard error', () => {
    const long = 'x'.repeat(256);
    expect(() => parseSynxl(`${PROLOGUE}\n!fields ${long}\n1\n`)).toThrow(/MAX_SYNXL_FIELD_NAME_BYTES/);
    // 255 bytes is fine; the limit is in UTF-8 bytes, not UTF-16 units.
    const ok = 'y'.repeat(255);
    expect(() => parseSynxl(`${PROLOGUE}\n!fields ${ok}\n1\n`)).not.toThrow();
    const cyrillic = 'я'.repeat(128); // 256 UTF-8 bytes, 128 UTF-16 units
    expect(() => parseSynxl(`${PROLOGUE}\n!fields ${cyrillic}\n1\n`)).toThrow(/MAX_SYNXL_FIELD_NAME_BYTES/);
  });

  test('field count over MAX_SYNXL_FIELDS is a hard error', () => {
    const many = Array.from({ length: 4097 }, (_, i) => `f${i}`).join(' ; ');
    expect(() => parseSynxl(`${PROLOGUE}\n!fields ${many}\n1\n`)).toThrow(/MAX_SYNXL_FIELDS/);
  });

  test('a later field list replaces the previous one (schema evolution)', () => {
    const doc = parseSynxl(
      `${PROLOGUE}\n!fields id ; score\n1 ; 0.9\n# new column\n!fields id ; score ; lang\n3 ; 0.5 ; ru\n`,
    );
    expect(doc.fieldLists).toHaveLength(2);
    expect(doc.records[0].values).toEqual({ id: 1, score: 0.9 });
    expect(Object.keys(doc.records[0].values)).not.toContain('lang');
    expect(doc.records[1].values).toEqual({ id: 3, score: 0.5, lang: 'ru' });
  });
});

// ─── §6 Record lines ──────────────────────────────────────

describe('SYNXL — record lines', () => {
  test('the SYNX first-character filter is not applied', () => {
    const doc = parseSynxl(
      `${PROLOGUE}\n!fields a ; b ; c ; d ; e ; f\n-5 ; [unparsed] ; /var/log/app ; @kaiserberg ; :marker ; (2+3)*4\n`,
    );
    expect(doc.records[0].values).toEqual({
      a: -5,
      b: '[unparsed]',
      c: '/var/log/app',
      d: '@kaiserberg',
      e: ':marker',
      f: '(2+3)*4',
    });
  });

  test('`//` is a comment but a single `/` is data', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a\n/var/log/app\n//var\n`);
    expect(doc.records).toHaveLength(1);
    expect(doc.records[0].values.a).toBe('/var/log/app');
  });

  test('a leading reserved prefix can be carried by quoting it', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; b\n"#tag" ; 1\n"!active" ; 2\n"//note" ; 3\n`);
    expect(doc.records.map(r => r.values.a)).toEqual(['#tag', '!active', '//note']);
  });
});

// ─── §7 Inline fields ─────────────────────────────────────

describe('SYNXL — inline fields', () => {
  test('splitting recognizes quotes in the same pass', () => {
    expect(splitRecordLine(`"a;b" ; c`)).toEqual([
      { value: 'a;b', quoted: true },
      { value: 'c', quoted: false },
    ]);
    expect(splitRecordLine(`  spaced  ;  x  `)).toEqual([
      { value: 'spaced', quoted: false },
      { value: 'x', quoted: false },
    ]);
    // §7.1.4 — only ASCII spaces and tabs are trimmed; NBSP is content.
    expect(splitRecordLine(' hi  ; \tz\t')).toEqual([
      { value: ' hi ', quoted: false },
      { value: 'z', quoted: false },
    ]);
    expect(splitRecordLine(`a;`)).toEqual([
      { value: 'a', quoted: false },
      { value: '', quoted: false },
    ]);
    expect(splitRecordLine(`" padded " ; y`)[0]).toEqual({ value: ' padded ', quoted: true });
  });

  test('a quote with trailing garbage falls back to an unquoted part', () => {
    // The closing quote is followed by `s'`, so §7.1 step 3 applies and the
    // whole text is one unquoted part; the `;` after it still delimits.
    const parts = splitRecordLine(`'it's' ; 2`);
    expect(parts).toEqual([
      { value: `'it's'`, quoted: false },
      { value: '2', quoted: false },
    ]);
    // §8.1.2 — the SYNX quote-stripping step is NOT applied to an inline part:
    // a part the splitter classified as unquoted keeps its quotes as content.
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; b\n'it's' ; 2\n`);
    expect(doc.records[0].values.a).toBe(`'it's'`);
    // The §8.1 example: `"a"b"` stays whole rather than collapsing to `a"b`.
    expect(parseSynxl(`${PROLOGUE}\n!fields a\n"a"b"\n`).records[0].values.a).toBe(`"a"b"`);
  });

  test('a type hint inside a cell value is not interpreted (§8.3)', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; b ; c\n(int)007 ; (string)5 ; (random)\n`);
    expect(doc.records[0].values).toEqual({ a: '(int)007', b: '(string)5', c: '(random)' });
  });

  test('§3.3 indent is Unicode-aware even though §7.1.4 trimming is not', () => {
    // A line opening with U+00A0 is indented, so it is block content...
    const doc = parseSynxl(`${PROLOGUE}\n!fields id(int)\n1\n second value\n`);
    expect(doc.records).toHaveLength(1);
    expect(doc.records[0].values).toEqual({ id: 1 });
    expect(kinds(doc.diagnostics)).toEqual(['UnknownBlockKey']);
    // ...while U+00A0 inside a part is ordinary content (§7.1.4).
    const doc2 = parseSynxl(`${PROLOGUE}\n!fields id(int) ; a\n1 ;  hello \n`);
    expect(doc2.records[0].values.a).toBe(' hello ');
  });

  test('a record line must start at column 0 — indented lines are block content', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a\n1\n  2\n`);
    expect(doc.records).toHaveLength(1);
    expect(doc.records[0].values.a).toBe(1);
  });

  test('an unmatched quote is ordinary content', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; b\nhe said "hi ; 2\n`);
    expect(doc.records[0].values.a).toBe('he said "hi');
  });

  test('empty part is null, `""` is the empty string', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; b ; c\n; "" ; ''\n`);
    expect(doc.records[0].values).toEqual({ a: null, b: '', c: '' });
  });

  test('quoted values bypass casting entirely', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a(int) ; b ; c\n"42" ; "true" ; "null"\n`);
    expect(doc.records[0].values).toEqual({ a: '42', b: 'true', c: 'null' });
    expect(doc.diagnostics).toHaveLength(0);
  });

  test('inline comments are NOT stripped from values', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; b\nhello # world ; a // b\n`);
    expect(doc.records[0].values).toEqual({ a: 'hello # world', b: 'a // b' });
  });

  test('P < N sets trailing fields to null and reports MissingFields', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; b ; c\n1 ; 2\n`);
    expect(doc.records[0].values).toEqual({ a: 1, b: 2, c: null });
    expect(kinds(doc.records[0].diagnostics)).toEqual(['MissingFields']);
    expect(doc.diagnostics[0].record).toBe(0);
    expect(doc.diagnostics[0].line).toBe(3);
  });

  test('P > N discards the surplus and reports ExtraFields', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; b\n1 ; 2 ; 3 ; 4\n`);
    expect(doc.records[0].values).toEqual({ a: 1, b: 2 });
    expect(kinds(doc.records[0].diagnostics)).toEqual(['ExtraFields']);
  });

  test('a record line of exactly `;` sets every field null without diagnostics', () => {
    // §7.2 — recognized before the §7.1 split, so the arity check never runs.
    for (const decl of ['a', 'a ; b', 'a ; b ; c', 'a ; body[block]']) {
      const doc = parseSynxl(`${PROLOGUE}\n!fields ${decl}\n;\n`);
      expect(doc.records).toHaveLength(1);
      expect(Object.values(doc.records[0].values).every(v => v === null)).toBe(true);
      expect(doc.diagnostics).toEqual([]);
    }
    // Trailing spaces or tabs do not disturb the form; anything else does.
    expect(parseSynxl(`${PROLOGUE}\n!fields a ; b\n;  \n`).diagnostics).toEqual([]);
    expect(kinds(parseSynxl(`${PROLOGUE}\n!fields a ; b ; c ; d\n; ;\n`).diagnostics)).toEqual(['MissingFields']);
  });

  test('an all-null record still carries its block', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; body[block]\n;\n  body\n    k v\n`);
    expect(doc.records[0].values).toEqual({ a: null, body: { k: 'v' } });
    expect(doc.diagnostics).toEqual([]);
  });

  test('block fields never occupy an inline position', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; body[block] ; b\n1 ; 2\n`);
    expect(doc.records[0].values).toEqual({ a: 1, body: null, b: 2 });
    expect(doc.diagnostics).toHaveLength(0);
  });
});

// ─── §8 Casting ───────────────────────────────────────────

describe('SYNXL — casting', () => {
  test('automatic casting matches SYNX', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; b ; c ; d ; e ; f\ntrue ; false ; null ; 42 ; 3.5 ; text\n`);
    expect(doc.records[0].values).toEqual({ a: true, b: false, c: null, d: 42, e: 3.5, f: 'text' });
  });

  test('large integers follow the same i64→number treatment as the SYNX parser', () => {
    const synx = Synx.parse('n 9007199254740993');
    const doc = parseSynxl(`${PROLOGUE}\n!fields n[type:int]\n9007199254740993\n`);
    expect(doc.records[0].values.n).toBe(synx.n);
  });

  test('typed casting applies when a type is declared', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a[type:int] ; b(float) ; c[type:bool] ; d(string)\n7 ; 7 ; true ; 7\n`);
    expect(doc.records[0].values).toEqual({ a: 7, b: 7, c: true, d: '7' });
  });

  test('failed typed casting yields null plus CastFailed, keeping the row', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a[type:int] ; b\nabc ; keep\n`);
    expect(doc.records[0].values).toEqual({ a: null, b: 'keep' });
    expect(kinds(doc.records[0].diagnostics)).toEqual(['CastFailed']);
    expect(doc.records[0].diagnostics[0].field).toBe('a');
  });

  test('float-shaped text does not satisfy type:int', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a[type:int]\n3.0\n`);
    expect(doc.records[0].values.a).toBeNull();
    expect(kinds(doc.records[0].diagnostics)).toEqual(['CastFailed']);
  });

  test('non-deterministic hints inside a cell stay literal', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; b\n(random) ; (random:int)\n`);
    expect(doc.records[0].values).toEqual({ a: '(random)', b: '(random:int)' });
  });

  test('validation is opt-in and reports ConstraintViolation', () => {
    const src = `${PROLOGUE}\n!fields a[required] ; b[type:int, min:10] ; c[enum:x|y]\n; 3 ; z\n`;
    expect(parseSynxl(src).diagnostics).toHaveLength(0);

    const doc = parseSynxl(src, { validate: true });
    expect(kinds(doc.diagnostics)).toEqual([
      'ConstraintViolation',
      'ConstraintViolation',
      'ConstraintViolation',
    ]);
    expect(doc.diagnostics.map(d => d.field)).toEqual(['a', 'b', 'c']);
  });
});

// ─── §9 Block fields ──────────────────────────────────────

describe('SYNXL — block fields', () => {
  const WORKED_EXAMPLE = [
    '!synxl 1',
    '!fields id[type:int, required] ; score[type:float] ; messages[block]',
    '',
    '1 ; 0.91',
    '  messages',
    '    - role system',
    '      content You are a helpful assistant.',
    '    - role user',
    '      content |+',
    '          def f(x):',
    '              return x + 1',
    '',
    '2 ; 0.74',
    '  messages',
    '    - role user',
    '      content Привет',
    '',
    '# schema evolution: a new column appears mid-file',
    '!fields id[type:int, required] ; score[type:float] ; lang ; messages[block]',
    '',
    '3 ; 0.55 ; ru',
    '  messages',
    '    - role user',
    '      content Как дела?',
    '',
  ].join('\n');

  test('the §10 worked example projects exactly as specified', () => {
    const doc = parseSynxl(WORKED_EXAMPLE);
    expect(doc.records).toHaveLength(3);
    expect(doc.diagnostics).toHaveLength(0);

    const json = synxlToJSON(doc);
    const arr = JSON.parse(json) as Array<Record<string, unknown>>;
    expect(arr[0]).toEqual({
      id: 1,
      score: 0.91,
      messages: [
        { role: 'system', content: 'You are a helpful assistant.' },
        { role: 'user', content: 'def f(x):\n    return x + 1' },
      ],
    });
    expect(json.startsWith('[{"id":1,"messages":[{"content":"You are a helpful assistant.","role":"system"}')).toBe(true);
    expect(arr[1]).toEqual({ id: 2, score: 0.74, messages: [{ role: 'user', content: 'Привет' }] });
    expect(arr[2]).toEqual({ id: 3, score: 0.55, lang: 'ru', messages: [{ role: 'user', content: 'Как дела?' }] });
    expect(arr[0].lang).toBeUndefined();
  });

  test('records with no block get null for every block field', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; body[block]\n1\n`);
    expect(doc.records[0].values).toEqual({ a: 1, body: null });
  });

  test('empty lines inside a block do not terminate it', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; body[block]\n1\n  body\n    x 1\n\n    y 2\n`);
    expect(doc.records[0].values.body).toEqual({ x: 1, y: 2 });
  });

  test('a block key matching a non-block field is discarded with a diagnostic', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; body[block]\ninline\n  a overridden\n  body\n    k v\n`);
    expect(doc.records[0].values.a).toBe('inline');
    expect(doc.records[0].values.body).toEqual({ k: 'v' });
    expect(kinds(doc.records[0].diagnostics)).toEqual(['BlockFieldNotDeclared']);
  });

  test('a block key matching no field is discarded with a diagnostic', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; body[block]\n1\n  nope 2\n  body\n    k v\n`);
    expect(doc.records[0].values).toEqual({ a: 1, body: { k: 'v' } });
    expect(kinds(doc.records[0].diagnostics)).toEqual(['UnknownBlockKey']);
    expect(doc.records[0].diagnostics[0].field).toBe('nope');
  });

  test('§9.4 — a directive outside a multiline body is discarded', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; body[block]\n1\n  !include /etc/passwd\n  body\n    k v\n`);
    expect(doc.records[0].values).toEqual({ a: 1, body: { k: 'v' } });
    expect(doc.records[0].diagnostics).toHaveLength(0);
  });

  test('§9.4 — a directive inside a |+ body is preserved verbatim', () => {
    const doc = parseSynxl(
      `${PROLOGUE}\n!fields a ; body[block]\n1\n  body |+\n    !include /etc/passwd\n    !active\n    line three\n`,
    );
    expect(doc.records[0].values.body).toBe('!include /etc/passwd\n!active\nline three');
  });

  test('§9.5 — no !active metadata leaks out of a block', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; body[block]\n1\n  !active\n  body\n    k[type:int] 5\n`);
    const body = doc.records[0].values.body as Record<string, unknown>;
    expect(body).toEqual({ k: 5 });
    expect(Object.prototype.hasOwnProperty.call(body, '__synx')).toBe(false);
    expect(JSON.stringify(doc.records[0].values)).not.toContain('__synx');
  });
});

// ─── §12 JSON projections ─────────────────────────────────

describe('SYNXL — JSON projections', () => {
  const SRC = `${PROLOGUE}\n!fields b ; a\n1 ; x\n2 ; y\n`;

  test('array projection is canonical (sorted keys, no whitespace)', () => {
    expect(synxlToJSON(SRC)).toBe('[{"a":"x","b":1},{"a":"y","b":2}]');
  });

  test('NDJSON projection is one canonical object per line', () => {
    expect(synxlToNDJSON(SRC)).toBe('{"a":"x","b":1}\n{"a":"y","b":2}');
  });

  test('unset fields project as JSON null', () => {
    expect(synxlToJSON(`${PROLOGUE}\n!fields a ; b[block]\n\n`)).toBe('[]');
    expect(synxlToJSON(`${PROLOGUE}\n!fields a ; b[block]\n;\n`)).toBe('[{"a":null,"b":null}]');
  });
});

// ─── §13 Resource limits ──────────────────────────────────

describe('SYNXL — resource limits', () => {
  test('an oversized record is truncated, not rejected', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; b\n1 ; abcdefghijklmno\n2 ; ok\n`, { maxRecordBytes: 10 });
    expect(doc.records).toHaveLength(2);
    expect(doc.records[0].values).toEqual({ a: 1, b: 'abcdef' });
    expect(kinds(doc.records[0].diagnostics)).toEqual(['RecordTruncated']);
    expect(doc.records[1].values).toEqual({ a: 2, b: 'ok' });
    expect(doc.records[1].diagnostics).toHaveLength(0);
  });

  test('truncation cuts at a UTF-8 boundary and counts bytes, not UTF-16 units', () => {
    expect(utf8Length('Привет')).toBe(12);
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; b\n1 ; Привет\n`, { maxRecordBytes: 12 });
    expect(doc.records[0].values.b).toBe('Прив');
    expect(kinds(doc.records[0].diagnostics)).toEqual(['RecordTruncated']);
  });

  test('the budget spans the block as well as the record line', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a ; body[block]\n1\n  body\n    k value-is-long\n`, { maxRecordBytes: 14 });
    expect(kinds(doc.records[0].diagnostics)).toContain('RecordTruncated');
  });

  test('too many field lists is a hard error', () => {
    const atLimit = `${PROLOGUE}\n${'!fields a\n'.repeat(MAX_SYNXL_FIELD_LISTS)}1\n`;
    expect(parseSynxl(atLimit).fieldLists).toHaveLength(MAX_SYNXL_FIELD_LISTS);
    const overLimit = `${PROLOGUE}\n${'!fields a\n'.repeat(MAX_SYNXL_FIELD_LISTS + 1)}1\n`;
    expect(() => parseSynxl(overLimit)).toThrow(/MAX_SYNXL_FIELD_LISTS/);
  });
});

// ─── §14 Writer / round-trip ──────────────────────────────

describe('SYNXL — writer', () => {
  test('emits the prologue, a field list, and one record per group', () => {
    const out = writeSynxl([{ id: 1, name: 'Alice' }, { id: 2, name: 'Bob' }]);
    expect(out).toBe('!synxl 1\n!fields id; name\n1; Alice\n2; Bob\n');
  });

  test('quotes values that would otherwise be misread', () => {
    const records = [{
      semi: 'x;y',
      numeric: '42',
      empty: '',
      absent: null,
      hash: '#tag',
      slashes: '//note',
      bang: '!active',
      spaced: '  padded  ',
    }];
    const text = writeSynxl(records);
    const doc = parseSynxl(text);
    expect(doc.records[0].values).toEqual(records[0]);
    expect(doc.diagnostics).toHaveLength(0);
  });

  test('promotes multi-line and structural values to block fields', () => {
    const records = [{
      id: 1,
      note: 'line one\nline two',
      messages: [{ role: 'user', content: 'hi' }],
      meta: { nested: { deep: true } },
    }];
    const text = writeSynxl(records);
    expect(text).toContain('note[block]');
    expect(text).toContain('|+');
    const doc = parseSynxl(text);
    expect(doc.records[0].values).toEqual(records[0]);
  });

  test('promotes a value that quoting cannot express', () => {
    const records = [{ id: 1, a: `mix "double" and 'single' ; semicolon` }];
    const doc = parseSynxl(writeSynxl(records));
    expect(doc.records[0].values).toEqual(records[0]);
  });

  test('a value starting with a quote is quoted with the other quote character', () => {
    const records = [{ id: 1, a: `"quoted"`, b: `'single'` }];
    const text = writeSynxl(records);
    expect(text).toContain(`'"quoted"'`);
    expect(text).toContain(`"'single'"`);
    expect(parseSynxl(text).records[0].values).toEqual(records[0]);
  });

  test('refuses to write records whose every field must be promoted', () => {
    expect(() => writeSynxl([{ only: `line one\nline two` }])).toThrow(/arity 0/);
  });

  test('floats are written in a form SYNX reads back (§14.3)', () => {
    const records = [{ id: 1, big: 1e21, tiny: 1e-7, inf: Infinity, nan: NaN }];
    const text = writeSynxl(records);
    expect(text).toContain('1000000000000000000000');
    expect(text).toContain('0.0000001');
    const values = parseSynxl(text).records[0].values;
    expect(values.big).toBe(1e21);
    expect(values.tiny).toBe(1e-7);
    // A non-finite float has no SYNX representation → empty part → null, which
    // is what the JSON projection carries anyway.
    expect(values.inf).toBeNull();
    expect(values.nan).toBeNull();
  });

  test('an all-null record round-trips at every arity, with no diagnostics', () => {
    const shapes: SynxObject[] = [
      { a: null },
      { a: null, b: null },
      { a: null, b: null, c: null },
    ];
    for (const shape of shapes) {
      const text = writeSynxl([shape]);
      // §7.2 — the all-null form is exactly `;`.
      expect(text.split('\n')[2]).toBe(';');
      const doc = parseSynxl(text);
      expect(doc.records).toHaveLength(1);
      expect(doc.records[0].values).toEqual(shape);
      expect(doc.diagnostics).toEqual([]);
    }
  });

  test('§14.1 — parse(write(parse(D))) projects identically', () => {
    const source = [
      '!synxl 1',
      '!fields id[type:int, required] ; score[type:float] ; messages[block]',
      '1 ; 0.91',
      '  messages',
      '    - role system',
      '      content You are a helpful assistant.',
      '    - role user',
      '      content |+',
      '          def f(x):',
      '              return x + 1',
      '2 ; 0.74',
      '  messages',
      '    - role user',
      '      content Привет',
      '!fields id[type:int] ; lang ; note[block]',
      '3 ; ru',
      '  note |+',
      '    hashes # stay',
      '    and // stay',
      '4 ; en',
      '',
    ].join('\n');

    const first = parseSynxl(source);
    const written = writeSynxl(first);
    const second = parseSynxl(written);
    expect(synxlToJSON(second)).toBe(synxlToJSON(first));
    // Schema evolution survives: the second group keeps its own field list.
    expect(second.fieldLists).toHaveLength(2);
    expect(second.records[3].values).toEqual({ id: 4, lang: 'en', note: null });
  });

  test('a written document keeps its declarations verbatim', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields id[type:int, required] ; body[block]\n1\n`);
    expect(writeSynxl(doc)).toContain('!fields id[type:int, required]; body[block]');
  });
});

// ─── §15 Streaming ────────────────────────────────────────

describe('SYNXL — streaming', () => {
  const SRC = [
    '!synxl 1',
    '!fields id[type:int] ; body[block]',
    '1',
    '  body',
    '    k v',
    '2',
    '!fields id[type:int] ; lang',
    '3 ; ru',
    '',
  ].join('\n');

  test('the sync generator yields the same records as the batch parser', () => {
    const streamed = [...streamSynxl(SRC)];
    const batch = parseSynxl(SRC).records;
    expect(streamed.map(r => r.values)).toEqual(batch.map(r => r.values));
    expect(streamed.map(r => r.index)).toEqual([0, 1, 2]);
    expect(streamed.map(r => r.line)).toEqual(batch.map(r => r.line));
  });

  test('records are yielded lazily, before the document is exhausted', () => {
    const it = streamSynxl(SRC);
    const first = it.next().value as SynxlRecord;
    expect(first.values).toEqual({ id: 1, body: { k: 'v' } });
  });

  test('diagnostics are enumerable per record while streaming', () => {
    const recs = [...streamSynxl(`${PROLOGUE}\n!fields a ; b\n1\n1 ; 2 ; 3\n`)];
    expect(kinds(recs[0].diagnostics)).toEqual(['MissingFields']);
    expect(kinds(recs[1].diagnostics)).toEqual(['ExtraFields']);
  });

  test('the async reader handles records split across chunks', async () => {
    async function* chunks(): AsyncGenerator<string> {
      const size = 7;
      for (let i = 0; i < SRC.length; i += size) yield SRC.slice(i, i + size);
    }
    const out: SynxlRecord[] = [];
    for await (const rec of streamSynxlAsync(chunks())) out.push(rec);
    expect(out.map(r => r.values)).toEqual(parseSynxl(SRC).records.map(r => r.values));
  });

  test('the async reader decodes byte chunks split mid-character', async () => {
    const bytes = Buffer.from(`${PROLOGUE}\n!fields a\nПривет\n`, 'utf-8');
    async function* chunks(): AsyncGenerator<Uint8Array> {
      for (let i = 0; i < bytes.length; i += 3) yield new Uint8Array(bytes.subarray(i, i + 3));
    }
    const out: SynxlRecord[] = [];
    for await (const rec of streamSynxlAsync(chunks())) out.push(rec);
    expect(out.map(r => r.values)).toEqual([{ a: 'Привет' }]);
  });

  test('a document truncated mid-record still yields every complete record', () => {
    const recs = [...streamSynxl(`${PROLOGUE}\n!fields a ; b\n1 ; x\n2 ; par`)];
    expect(recs.map(r => r.values)).toEqual([{ a: 1, b: 'x' }, { a: 2, b: 'par' }]);
    const flagged = parseSynxl(`${PROLOGUE}\n!fields a ; b\n1 ; x\n2 ; par`, { reportTruncatedTail: true });
    expect(kinds(flagged.records[1].diagnostics)).toEqual(['RecordTruncated']);
    expect(flagged.records[0].diagnostics).toHaveLength(0);
  });
});

// ─── §11.2 Diagnostic contract ────────────────────────────

describe('SYNXL — diagnostic line attribution and order', () => {
  test('block diagnostics report the offending line inside the block', () => {
    const doc = parseSynxl([
      '!synxl 1',
      '!fields id(int) ; label',
      '',
      '1 ; hello',
      '  extra_stuff xyz',
      '',
    ].join('\n'));
    expect(triples(doc.diagnostics)).toEqual(['UnknownBlockKey 0 5']);

    const doc2 = parseSynxl([
      '!synxl 1',
      '!fields id(int) ; label',
      '',
      '1 ; hello',
      '  label',
      '    x extra',
      '',
    ].join('\n'));
    expect(triples(doc2.diagnostics)).toEqual(['BlockFieldNotDeclared 0 5']);
  });

  test('block keys are visited in Unicode scalar order', () => {
    const doc = parseSynxl([
      '!synxl 1',
      '!fields id',
      '1',
      '  zeta 1',
      '  alpha 2',
      '  Beta 3',
      '',
    ].join('\n'));
    // Sorted: "Beta" (line 6) < "alpha" (line 5) < "zeta" (line 4).
    expect(triples(doc.diagnostics)).toEqual([
      'UnknownBlockKey 0 6',
      'UnknownBlockKey 0 5',
      'UnknownBlockKey 0 4',
    ]);
    expect(doc.diagnostics.map(d => d.field)).toEqual(['Beta', 'alpha', 'zeta']);
  });

  test('diagnostics within a record follow the normative order', () => {
    const src = [
      '!synxl 1',
      '!fields a[type:int] ; b[type:int, min:100] ; body[block]',
      '  orphan 1',
      'oops ; 5 ; surplus ; more',
      '  body',
      '    k v',
      '  a 9',
      '  unknown 1',
      '',
    ].join('\n');
    // 59 bytes is exactly the record line plus its four block lines; the
    // trailing empty line then exhausts the budget, so truncation is reported
    // without any block content being lost.
    const doc = parseSynxl(src, { validate: true, maxRecordBytes: 59 });
    expect(kinds(doc.diagnostics)).toEqual([
      'OrphanBlockLine',
      'RecordTruncated',
      'CastFailed',
      'ExtraFields',
      'BlockFieldNotDeclared', // "a" sorts before "unknown"
      'UnknownBlockKey',
      'ConstraintViolation',
    ]);
    expect(doc.diagnostics[0].line).toBe(3);
    expect(doc.diagnostics[0].record).toBe(0);
  });

  test('an indented line with no record open is an OrphanBlockLine', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a\n  stray 1\n1\n`);
    expect(triples(doc.diagnostics)).toEqual(['OrphanBlockLine 0 3']);
    expect(doc.records[0].values).toEqual({ a: 1 });
  });

  test('orphan lines after the last record still reach the result', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields a\n1\n# end\n  stray\n`);
    expect(triples(doc.diagnostics)).toEqual(['OrphanBlockLine 1 5']);
  });

  test('an indented first significant line is a missing prologue', () => {
    expect(conditionOf('  indented\n!synxl 1\n')).toBe('MissingOrMalformedPrologue');
  });
});

// ─── Public surface ───────────────────────────────────────

describe('SYNXL — public API surface', () => {
  test('the Synx class exposes the SYNXL entry points', () => {
    const src = `${PROLOGUE}\n!fields id[type:int] ; name\n1 ; Alice\n`;
    expect(Synx.SYNXL_VERSION).toBe(1);
    expect(Synx.parseSynxl(src).records[0].values).toEqual({ id: 1, name: 'Alice' });
    expect(Synx.synxlToJSON(src)).toBe('[{"id":1,"name":"Alice"}]');
    expect(Synx.synxlToNDJSON(src)).toBe('{"id":1,"name":"Alice"}');
    expect([...Synx.streamSynxl(src)]).toHaveLength(1);
    expect(Synx.writeSynxl(Synx.parseSynxl(src))).toContain('!synxl 1');
  });

  test('hard errors are SynxError with a SYNXL_ERR code and a condition token', () => {
    try {
      parseSynxl('nope\n');
      throw new Error('expected a hard error');
    } catch (e) {
      expect(e).toBeInstanceOf(SynxError);
      expect(e).toBeInstanceOf(SynxlError);
      expect((e as SynxError).code).toBe('SYNXL_ERR');
      expect((e as SynxlError).condition).toBe('MissingOrMalformedPrologue');
      expect((e as SynxlError).line).toBe(1);
    }
  });

  test('every §11.1 condition maps to its conformance token', () => {
    expect(conditionOf('nope\n')).toBe('MissingOrMalformedPrologue');
    expect(conditionOf('!synxl 2\n!fields a\n1\n')).toBe('UnsupportedFormatVersion');
    // A repeated prologue reports the more specific mismatch condition.
    expect(conditionOf(`${PROLOGUE}\n!fields a\n1\n!synxl 3\n`)).toBe('RepeatedPrologueVersionMismatch');
    expect(conditionOf(`${PROLOGUE}\n!fields a\n!active\n`)).toBe('UnknownDirective');
    expect(conditionOf(`${PROLOGUE}\n1\n`)).toBe('RecordWithoutFieldList');
    expect(conditionOf(`${PROLOGUE}\n!fields\n1\n`)).toBe('MalformedFieldList');
    expect(conditionOf(`${PROLOGUE}\n!fields a ; a\n1\n`)).toBe('DuplicateFieldName');
    expect(conditionOf(`${PROLOGUE}\n!fields a:x:y\n1\n`)).toBe('MarkerChainInFieldDecl');
    expect(conditionOf(`${PROLOGUE}\n!fields a(random)\n1\n`)).toBe('NonDeterministicTypeHint');
    expect(conditionOf(`${PROLOGUE}\n!fields a[block, type:int]\n1\n`)).toBe('BlockCombinedWithType');
    expect(conditionOf(`${PROLOGUE}\n!fields a[block]\n1\n`)).toBe('ZeroArityFieldList');
    expect(conditionOf(`${PROLOGUE}\n!fields ${'x'.repeat(256)}\n1\n`)).toBe('LimitExceeded');
  });

  test('a repeated prologue with a different in-range version is rejected', () => {
    // `!synxl 2` is caught as an unsupported version before the mismatch rule;
    // the mismatch branch guards a version this build would otherwise accept.
    expect(() => parseSynxl(`${PROLOGUE}\n!fields a\n1\n!synxl 1\n2\n`)).not.toThrow();
  });

  test('a field named __proto__ cannot pollute the prototype', () => {
    const doc = parseSynxl(`${PROLOGUE}\n!fields __proto__ ; a\npolluted ; 1\n`);
    expect(doc.records[0].values.__proto__).toBe('polluted');
    expect(({} as Record<string, unknown>).polluted).toBeUndefined();
    expect(Object.getPrototypeOf(doc.records[0].values)).toBe(Object.prototype);
  });
});
