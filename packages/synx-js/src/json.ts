/**
 * Canonical JSON writer — @aperturesyndicate/synx-format
 *
 * SYNX §10 / SYNXL §12: object keys sorted lexicographically at every nesting
 * level, arrays in order, internal `__synx` metadata hidden, and — in compact
 * mode — no insignificant whitespace.
 *
 * This lives in its own module so that the SYNX facade (`index.ts`, Node-only)
 * and the SYNXL reader (`synxl.ts`, runtime-agnostic) share one implementation
 * without importing each other.
 *
 * The output is built by hand rather than through `JSON.stringify` with a
 * replacer. A replacer can sort keys, but the sorted object it returns is
 * re-ordered again on the way out — JavaScript emits integer-like keys first,
 * in numeric order — and assigning `__proto__` onto a plain object hits the
 * prototype setter instead of creating a property, so that field disappears
 * from the projection entirely. Both are silent data loss against §12.1.
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

/** RFC 8259 string escaping, matching the reference implementation's choices. */
function escapeString(s: string): string {
  let out = '"';
  for (const ch of s) {
    const c = ch.codePointAt(0) as number;
    if (ch === '"') out += '\\"';
    else if (ch === '\\') out += '\\\\';
    else if (ch === '\n') out += '\\n';
    else if (ch === '\r') out += '\\r';
    else if (ch === '\t') out += '\\t';
    // `\b` and `\f` have short forms in RFC 8259, but the reference
    // implementation writes them as `` / ``, and §1.4.3 measures
    // conformance on byte-identical output.
    else if (c < 0x20) out += '\\u' + c.toString(16).padStart(4, '0');
    else out += ch;
  }
  return out + '"';
}

/**
 * Number formatting matched to the reference implementation.
 *
 * Two differences from `String(n)` matter: Rust writes exponents without the
 * `+` sign, and it keeps the sign of negative zero. What JavaScript cannot
 * reproduce is the int/float distinction — `5.0` and `5` are the same value
 * here — which §14.3 already calls out as a permitted divergence.
 */
function formatNumber(n: number): string {
  if (!Number.isFinite(n)) return 'null';
  if (Object.is(n, -0)) return '-0.0';
  const s = String(n);
  return s.includes('e+') ? s.replace('e+', 'e') : s;
}

function writeValue(v: unknown, hideMeta: boolean, indent: number, depth: number): string {
  if (v === null || v === undefined) return 'null';
  const t = typeof v;
  if (t === 'boolean') return v ? 'true' : 'false';
  if (t === 'number') return formatNumber(v as number);
  if (t === 'bigint') return (v as bigint).toString();
  if (t === 'string') return escapeString(v as string);
  // A `:secret` value must serialise as the redaction mask, never as its
  // fields. Without this a SynxSecret fell through to the object branch and
  // came out as `{}`, diverging from the Rust writer's `"[SECRET]"` and losing
  // the marker. Detected via a global-symbol brand (see engine.ts SynxSecret).
  if (t === 'object' && (v as Record<symbol, unknown>)[Symbol.for('synx.secret')] === true) {
    return escapeString('[SECRET]');
  }

  const nl = indent > 0 ? '\n' : '';
  const pad = indent > 0 ? ' '.repeat(indent * (depth + 1)) : '';
  const padEnd = indent > 0 ? ' '.repeat(indent * depth) : '';
  const sep = indent > 0 ? ': ' : ':';

  if (Array.isArray(v)) {
    if (v.length === 0) return '[]';
    const items = v.map(item => pad + writeValue(item, hideMeta, indent, depth + 1));
    return '[' + nl + items.join(',' + nl) + nl + padEnd + ']';
  }

  const obj = v as Record<string, unknown>;
  // `Object.keys` plus a descriptor read, so an own `__proto__` property —
  // which `obj[k]` would resolve through the prototype getter — is read as the
  // data it is.
  const keys = Object.keys(obj)
    .filter(k => !(hideMeta && k === '__synx'))
    .sort(compareUnicodeScalar);
  if (keys.length === 0) return '{}';
  const entries = keys.map(k => {
    const desc = Object.getOwnPropertyDescriptor(obj, k);
    const value = desc && 'value' in desc ? desc.value : obj[k];
    return pad + escapeString(k) + sep + writeValue(value, hideMeta, indent, depth + 1);
  });
  return '{' + nl + entries.join(',' + nl) + nl + padEnd + '}';
}

/**
 * Serialize a parsed SYNX/SYNXL value as canonical JSON.
 *
 * `hideMeta` drops the engine's `__synx` metadata key, which is a SYNX-side
 * implementation detail. SYNXL disables metadata capture entirely (§9.5), so
 * a `__synx` key in a record is caller data and is kept.
 */
export function toCanonicalJSONString(obj: SynxObject, pretty = true, hideMeta = true): string {
  return writeValue(obj, hideMeta, pretty ? 2 : 0, 0);
}
