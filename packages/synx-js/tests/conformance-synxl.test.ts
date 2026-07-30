/**
 * SYNXL conformance runner (SYNXL §18).
 *
 * For each `tests/conformance-synxl/cases/NNN-name.synxl` the suite carries
 * **exactly one** of:
 *
 * - `.expected.json`  — the canonical array projection (§12.1), compared
 *   byte-for-byte with the trailing newline stripped;
 * - `.expected.error` — a hard-error condition token (§11.1) on the first line,
 *   with an informative explanation on the second.
 *
 * A case MAY additionally carry `.expected.diagnostics`: one `Kind index line`
 * triple per line, in the order §11.2 records them.
 *
 * The suite is derived from the normative text rather than from this
 * implementation, so a mismatch is a question to raise against the fixture or
 * the spec — never a reason to bend the parser to the fixture. Every case is
 * run and all mismatches are reported together.
 */

import * as fs from 'fs';
import * as path from 'path';

import { parseSynxl, synxlToJSON, SynxlError } from '../src/synxl';
import type { SynxlErrorCondition } from '../src/synxl';

const CASES_DIR = path.resolve(__dirname, '..', '..', '..', 'tests', 'conformance-synxl', 'cases');

/**
 * Condition tokens a `.expected.error` file may use for a given condition.
 *
 * The suite README fixes the spelling for the conditions it exercises; the
 * remaining aliases cover plausible spellings for conditions added to §11.1
 * after the suite was first written, so that a naming difference is not
 * reported as a semantic failure. (Same alias policy as the Rust runner.)
 */
const ERROR_TOKENS: Record<SynxlErrorCondition, string[]> = {
  MissingOrMalformedPrologue: ['MissingOrMalformedPrologue', 'MissingPrologue', 'MalformedPrologue'],
  UnsupportedFormatVersion: ['UnsupportedFormatVersion', 'UnsupportedVersion'],
  RepeatedPrologueVersionMismatch: [
    'RepeatedPrologueVersionMismatch',
    'RepeatedPrologueDifferentVersion',
    'UnsupportedFormatVersion',
    'UnsupportedVersion',
  ],
  UnknownDirective: [
    'UnknownDirectiveLine',
    'UnknownDirective',
    'UnknownBangLine',
    'UnknownBangLineAtIndentZero',
    'UnknownExclamationLine',
  ],
  RecordWithoutFieldList: ['RecordWithoutFieldList', 'NoFieldList'],
  MalformedFieldList: ['MalformedFieldList'],
  DuplicateFieldName: ['DuplicateFieldName', 'DuplicateField'],
  MarkerChainInFieldDecl: ['MarkerChainInFieldDecl', 'MarkerInFieldDecl', 'MarkerChain'],
  NonDeterministicTypeHint: ['NonDeterministicTypeHint', 'NonDeterministicHint'],
  BlockCombinedWithType: ['BlockCombinedWithType', 'BlockWithType'],
  ZeroArityFieldList: ['ZeroArityFieldList', 'FieldListArityZero', 'ZeroArity', 'ArityZero'],
  LimitExceeded: ['LimitExceeded'],
};

interface CaseResult {
  name: string;
  failure: string | null;
}

function listCases(): string[] {
  if (!fs.existsSync(CASES_DIR)) return [];
  return fs.readdirSync(CASES_DIR)
    .filter(f => f.endsWith('.synxl'))
    .sort();
}

function runCase(file: string): CaseResult {
  const name = file.slice(0, -'.synxl'.length);
  const base = path.join(CASES_DIR, name);
  const jsonPath = `${base}.expected.json`;
  const errorPath = `${base}.expected.error`;
  const diagPath = `${base}.expected.diagnostics`;
  const input = fs.readFileSync(path.join(CASES_DIR, file), 'utf-8');

  const hasJson = fs.existsSync(jsonPath);
  const hasError = fs.existsSync(errorPath);
  if (!hasJson && !hasError) {
    return { name, failure: 'no .expected.json and no .expected.error — the case states no expectation' };
  }
  if (hasJson && hasError) {
    return { name, failure: 'has BOTH .expected.json and .expected.error; the suite allows exactly one' };
  }

  // ── Hard-error case (§11.1) ──
  if (hasError) {
    const expected = fs.readFileSync(errorPath, 'utf-8').split('\n')[0].trim();
    try {
      const doc = parseSynxl(input);
      return {
        name,
        failure: `expected hard error \`${expected}\`, but the parse succeeded with ${doc.records.length} record(s): ${synxlToJSON(doc)}`,
      };
    } catch (e) {
      if (!(e instanceof SynxlError)) throw e;
      const accepted = ERROR_TOKENS[e.condition] ?? [e.condition];
      if (!accepted.includes(expected)) {
        return { name, failure: `expected hard error \`${expected}\`, got \`${e.condition}\` — ${e.message}` };
      }
      return { name, failure: null };
    }
  }

  // ── Accepted case (§12.1) ──
  let doc;
  try {
    doc = parseSynxl(input);
  } catch (e) {
    const err = e as Error;
    return { name, failure: `expected a successful parse, got hard error — ${err.message}` };
  }

  const expectedJson = fs.readFileSync(jsonPath, 'utf-8').trim();
  const gotJson = synxlToJSON(doc);
  if (gotJson !== expectedJson) {
    return { name, failure: `JSON projection mismatch\n    expected: ${expectedJson}\n    actual:   ${gotJson}` };
  }

  if (fs.existsSync(diagPath)) {
    const expectedDiags = fs.readFileSync(diagPath, 'utf-8')
      .split('\n').map(l => l.trim()).filter(l => l.length > 0);
    const gotDiags = doc.diagnostics.map(d => `${d.kind} ${d.record} ${d.line}`);
    if (expectedDiags.join('|') !== gotDiags.join('|')) {
      return {
        name,
        failure: `diagnostics mismatch\n    expected: [${expectedDiags.join(', ')}]\n    actual:   [${gotDiags.join(', ')}]`,
      };
    }
  }

  return { name, failure: null };
}

describe('SYNXL conformance suite (tests/conformance-synxl)', () => {
  const cases = listCases();

  test('the suite directory is present and non-empty', () => {
    expect(CASES_DIR.endsWith(path.join('tests', 'conformance-synxl', 'cases'))).toBe(true);
    expect(cases.length).toBeGreaterThan(0);
  });

  test('every case conforms', () => {
    const results = cases.map(runCase);
    const failures = results.filter(r => r.failure !== null);
    if (failures.length > 0) {
      const report = failures.map(f => `  ${f.name}: ${f.failure}`).join('\n');
      throw new Error(`${failures.length}/${results.length} conformance cases failed:\n${report}`);
    }
    expect(failures).toHaveLength(0);
  });
});
