/**
 * SYNX Browser Bundle — @aperturesyndicate/synx-format
 *
 * Lightweight browser-compatible build.
 * No Node.js dependencies (fs, path).
 * Provides: parse, stringify.
 */

import { parseData, inputText, markLocked } from './parser';
import { resolve, assertNoRuntimeErrors } from './engine';
import {
  writeSynxBody,
  parseSynxl,
  streamSynxl,
  streamSynxlAsync,
  synxlToJSON,
  synxlToNDJSON,
  writeSynxl,
  SYNXL_VERSION,
} from './synxl';
import type { SynxObject, SynxOptions } from './types';
import type { SynxlDocument, SynxlOptions, SynxlRecord, SynxlWriteOptions } from './types';

export type { SynxObject, SynxOptions, SynxValue, SynxArray, SynxPrimitive } from './types';
export type {
  SynxlDocument,
  SynxlRecord,
  SynxlField,
  SynxlFieldList,
  SynxlDiagnostic,
  SynxlDiagnosticKind,
  SynxlOptions,
  SynxlWriteOptions,
} from './types';
export { SynxError } from './types';
export {
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
} from './synxl';

class Synx {
  static parse<T extends SynxObject = SynxObject>(text: string, options: SynxOptions = {}): T {
    const { root, mode, locked, includes } = parseData(inputText(text, 'Synx.parse'));
    if (mode === 'active') {
      resolve(root, { ...options, _includes: includes } as any);
    }
    // spec: the same result as the Node build — `!lock` and `strict` were
    // ignored here.
    if (locked) markLocked(root);
    if (options.strict) {
      assertNoRuntimeErrors(root);
    }
    return root as T;
  }

  static stringify(obj: SynxObject, active = false): string {
    // spec: the round-trip writer of the Node build. The writer that lived
    // here quoted nothing (`"007"` came back as 7), wrote `|` for text that
    // needs `|+`, and printed an object in a list item as [object Object].
    return (active ? '!active\n' : '') + writeSynxBody(obj);
  }

  // ─── SYNXL — "SYNX Lines" record stream (SYNXL 1) ───────
  // File I/O is Node-only and therefore absent here; everything else works
  // unchanged in the browser, including the streaming reader over a `fetch`
  // body's chunks.

  static readonly SYNXL_VERSION = SYNXL_VERSION;

  static parseSynxl(text: string, options: SynxlOptions = {}): SynxlDocument {
    return parseSynxl(text, options);
  }

  static streamSynxl(
    source: string | Iterable<string>,
    options: SynxlOptions = {},
  ): Generator<SynxlRecord, void, undefined> {
    return streamSynxl(source, options);
  }

  static streamSynxlAsync(
    source: AsyncIterable<string | Uint8Array>,
    options: SynxlOptions = {},
  ): AsyncGenerator<SynxlRecord, void, undefined> {
    return streamSynxlAsync(source, options);
  }

  static synxlToJSON(source: string | SynxlDocument, options: SynxlOptions = {}): string {
    return synxlToJSON(source, options);
  }

  static synxlToNDJSON(source: string | SynxlDocument, options: SynxlOptions = {}): string {
    return synxlToNDJSON(source, options);
  }

  static writeSynxl(
    input: SynxlDocument | readonly SynxObject[],
    options: SynxlWriteOptions = {},
  ): string {
    return writeSynxl(input, options);
  }
}

export default Synx;
export { Synx };
