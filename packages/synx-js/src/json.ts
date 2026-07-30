/**
 * Canonical JSON writer — @aperturesyndicate/synx-format
 *
 * SYNX §10 / SYNXL §12: object keys sorted lexicographically at every nesting
 * level, arrays in order, internal `__synx*` metadata hidden, and — in compact
 * mode — no insignificant whitespace.
 *
 * This lives in its own module so that the SYNX facade (`index.ts`, Node-only)
 * and the SYNXL reader (`synxl.ts`, runtime-agnostic) share one implementation
 * without importing each other.
 */

import type { SynxObject } from './types';

/**
 * Compare by Unicode **scalar** order, which is what SYNX §10 / SYNXL §12.1
 * specify — not JavaScript's default UTF-16 code-unit order. The two disagree
 * only for astral characters, which a naive `<` sorts below U+E000..U+FFFF.
 * (Rust compares `String`s by UTF-8 bytes, i.e. scalar order.)
 */
export function compareUnicodeScalar(a: string, b: string): number {
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) {
    const ca = a.charCodeAt(i);
    const cb = b.charCodeAt(i);
    if (ca === cb) continue;
    const sa = ca >= 0xd800 && ca <= 0xdbff ? (a.codePointAt(i) as number) : ca;
    const sb = cb >= 0xd800 && cb <= 0xdbff ? (b.codePointAt(i) as number) : cb;
    return sa < sb ? -1 : 1;
  }
  return a.length - b.length;
}

/** Serialize a parsed SYNX/SYNXL value as canonical JSON. */
export function toCanonicalJSONString(obj: SynxObject, pretty = true): string {
  // A replacer returning a sorted shallow copy of any plain object gives
  // canonical key order at every level without building a second tree.
  const replacer = (_k: string, v: unknown): unknown => {
    if (v && typeof v === 'object' && !Array.isArray(v)) {
      const sorted: Record<string, unknown> = {};
      const keys = Object.keys(v as Record<string, unknown>).sort(compareUnicodeScalar);
      for (const k of keys) {
        if (k.startsWith('__synx')) continue; // hide internal metadata
        sorted[k] = (v as Record<string, unknown>)[k];
      }
      return sorted;
    }
    return v;
  };
  return pretty ? JSON.stringify(obj, replacer, 2) : JSON.stringify(obj, replacer);
}
