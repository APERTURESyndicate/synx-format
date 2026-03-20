/**
 * SYNX Browser Bundle — @aperturesyndicate/synx
 *
 * Lightweight browser-compatible build.
 * No Node.js dependencies (fs, path).
 * Provides: parse, stringify.
 */

import { parseData } from './parser';
import { resolve } from './engine';
import type { SynxObject, SynxOptions } from './types';

export type { SynxObject, SynxOptions, SynxValue, SynxArray, SynxPrimitive } from './types';
export { SynxError } from './types';

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
