/**
 * SYNX conformance runner (tests/conformance/README.md).
 *
 * For each `tests/conformance/cases/NNN-name.synx` + `.expected.json` pair the
 * parse projection must match byte for byte, trailing newline stripped. Cases
 * whose input opens with `!tool` go through `Synx.parseTool`, as in the Rust
 * runner (`crates/synx-core/tests/conformance.rs`); an `!active` document (the
 * `100`–`199` cases) is resolved before the projection is taken.
 *
 * Until 3.7.4 this package never ran the static suite, which is how parser
 * fixes landed in Rust alone without anything noticing.
 */

import * as fs from 'fs';
import * as path from 'path';

import Synx from '../src/index';
import { parseData } from '../src/parser';
import { toCanonicalJSONString } from '../src/json';
import type { SynxObject } from '../src/types';

const CASES_DIR = path.resolve(__dirname, '..', '..', '..', 'tests', 'conformance', 'cases');

const cases = fs
  .readdirSync(CASES_DIR)
  .filter(f => f.endsWith('.synx'))
  .filter(f => fs.existsSync(path.join(CASES_DIR, f.replace(/\.synx$/, '.expected.json'))))
  .sort();

describe('SYNX conformance', () => {
  it('finds the corpus', () => {
    expect(cases.length).toBeGreaterThan(0);
  });

  it.each(cases)('%s', file => {
    const input = fs.readFileSync(path.join(CASES_DIR, file), 'utf-8');
    const expected = fs
      .readFileSync(path.join(CASES_DIR, file.replace(/\.synx$/, '.expected.json')), 'utf-8')
      .trim();

    const root = input.trimStart().startsWith('!tool')
      ? (Synx.parseTool(input) as unknown as SynxObject)
      : parseData(input).mode === 'active'
        ? Synx.parse(input, { env: {} })
        : parseData(input).root;

    expect(toCanonicalJSONString(root, false)).toBe(expected);
  });
});
