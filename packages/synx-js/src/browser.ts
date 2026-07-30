/**
 * SYNX Browser Bundle — @aperturesyndicate/synx-format
 *
 * Lightweight browser-compatible build.
 * No Node.js dependencies (fs, path).
 * Provides: parse, stringify.
 */

import { parseData } from './parser';
import { resolve } from './engine';
import {
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
    const { root, mode } = parseData(text);
    if (mode === 'active') {
      resolve(root, options);
    }
    return root as T;
  }

  static stringify(obj: SynxObject, active = false): string {
    let out = '';
    if (active) {
      out += '!active\n';
    }
    out += serializeObject(obj, 0);
    return out;
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

function serializeObject(obj: SynxObject, indent: number): string {
  let out = '';
  const spaces = ' '.repeat(indent);

  for (const [key, val] of Object.entries(obj)) {
    if (Array.isArray(val)) {
      out += `${spaces}${key}\n`;
      for (const item of val) {
        if (item && typeof item === 'object' && !Array.isArray(item)) {
          const entries = Object.entries(item as SynxObject);
          if (entries.length > 0) {
            const [firstKey, firstVal] = entries[0];
            out += `${spaces}  - ${firstKey} ${firstVal}\n`;
            for (let i = 1; i < entries.length; i++) {
              out += `${spaces}    ${entries[i][0]} ${entries[i][1]}\n`;
            }
          }
        } else {
          out += `${spaces}  - ${item}\n`;
        }
      }
    } else if (val && typeof val === 'object') {
      out += `${spaces}${key}\n`;
      out += serializeObject(val as SynxObject, indent + 2);
    } else if (typeof val === 'string' && val.includes('\n')) {
      out += `${spaces}${key} |\n`;
      for (const line of val.split('\n')) {
        out += `${spaces}  ${line}\n`;
      }
    } else {
      out += `${spaces}${key} ${val}\n`;
    }
  }

  return out;
}

export default Synx;
export { Synx };
