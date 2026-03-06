/**
 * SYNX Engine — @aperturesyndicate/synx
 *
 * Resolves active markers (:random, :calc, :env, :alias, :secret, etc.)
 * in a parsed SYNX object tree. Only runs in !active mode.
 */

import type { SynxObject, SynxValue, SynxMetaMap, SynxOptions } from './types';
import { safeCalc } from './calc';
import { parseData } from './parser';

// Lazy-loaded Node.js modules (not available in browser)
let fs: typeof import('fs') | undefined;
let pathModule: typeof import('path') | undefined;
try {
  fs = require('fs');
  pathModule = require('path');
} catch {
  // Browser environment — :include will not work
}

// ─── Secret wrapper ───────────────────────────────────────

const SECRET_TAG = Symbol('synx:secret');

class SynxSecret {
  private _value: string;

  constructor(value: string) {
    this._value = value;
    Object.defineProperty(this, SECRET_TAG, { value: true, enumerable: false });
  }

  /** Returns the real value (for code usage) */
  valueOf(): string {
    return this._value;
  }

  toString(): string {
    return '[SECRET]';
  }

  toJSON(): string {
    return '[SECRET]';
  }

  /** Call this explicitly to get the real value */
  reveal(): string {
    return this._value;
  }

  [Symbol.toPrimitive](hint: string): string | number {
    if (hint === 'number') return NaN;
    return '[SECRET]';
  }
}

// ─── Engine ───────────────────────────────────────────────

export function resolve(
  obj: SynxObject,
  options: SynxOptions = {},
  root?: SynxObject,
): SynxObject {
  if (!root) root = obj;

  const metaMap: SynxMetaMap | undefined = (obj as any).__synx;

  for (const key of Object.keys(obj)) {
    if (key === '__synx') continue;

    let value = obj[key];

    // Recurse into nested objects
    if (value && typeof value === 'object' && !Array.isArray(value)) {
      resolve(value as SynxObject, options, root);
    }

    // Recurse into arrays of objects
    if (Array.isArray(value)) {
      for (const item of value) {
        if (item && typeof item === 'object' && !Array.isArray(item)) {
          resolve(item as SynxObject, options, root);
        }
      }
    }

    // Apply markers
    if (!metaMap || !metaMap[key]) continue;
    const { markers, args } = metaMap[key];

    // ── :include ──
    if (markers.includes('include') && typeof obj[key] === 'string') {
      if (!fs || !pathModule) {
        obj[key] = 'INCLUDE_ERR: :include is not supported in browser';
        continue;
      }
      const includePath = String(obj[key]);
      const basePath = options.basePath || (typeof process !== 'undefined' ? process.cwd() : '.');
      const fullPath = pathModule.resolve(basePath, includePath);
      try {
        const text = fs.readFileSync(fullPath, 'utf-8');
        const { root: included, mode: includedMode } = parseData(text);
        if (includedMode === 'active') {
          resolve(included, { ...options, basePath: pathModule.dirname(fullPath) }, root);
        }
        obj[key] = included;
      } catch (e: any) {
        obj[key] = `INCLUDE_ERR: ${e.message}`;
      }
      continue;
    }

    // ── :env ──
    if (markers.includes('env')) {
      const varName = String(value);
      const envSource = options.env || (typeof process !== 'undefined' ? process.env : {});
      const envVal = envSource[varName];

      // Check for :default in the marker chain
      const defaultIdx = markers.indexOf('default');
      if (envVal !== undefined && envVal !== '') {
        obj[key] = isNaN(Number(envVal)) ? envVal : Number(envVal);
      } else if (defaultIdx !== -1 && markers.length > defaultIdx + 1) {
        // :env:default:VALUE — the value after default is the fallback
        const fallback = markers[defaultIdx + 1];
        obj[key] = isNaN(Number(fallback)) ? fallback : Number(fallback);
      } else {
        obj[key] = null;
      }
    }

    // ── :random ──
    if (markers.includes('random') && Array.isArray(obj[key])) {
      const arr = obj[key] as SynxValue[];
      if (arr.length === 0) {
        obj[key] = null;
        continue;
      }

      if (args && args.length > 0) {
        // Weighted random
        const weights = args.map(Number);
        obj[key] = weightedRandom(arr, weights);
      } else {
        // Equal probability
        obj[key] = arr[Math.floor(Math.random() * arr.length)];
      }
    }

    // ── :calc ──
    if (markers.includes('calc') && typeof obj[key] === 'string') {
      let expr = obj[key] as string;
      // Collect numeric variables from root + local scope
      const vars = new Map<string, string>();
      for (const rKey of Object.keys(root)) {
        if (typeof root[rKey] === 'number') vars.set(rKey, String(root[rKey]));
      }
      for (const rKey of Object.keys(obj)) {
        if (rKey !== key && typeof obj[rKey] === 'number') vars.set(rKey, String(obj[rKey]));
      }
      // Substitute whole-word occurrences without building RegExp objects
      if (vars.size > 0) expr = replaceVars(expr, vars);
      try {
        obj[key] = safeCalc(expr);
      } catch (e: any) {
        obj[key] = `CALC_ERR: ${e.message}`;
      }
    }

    // ── :alias ──
    if (markers.includes('alias') && typeof obj[key] === 'string') {
      const target = obj[key] as string;
      obj[key] = deepGet(root, target) ?? null;
    }

    // ── :secret ──
    if (markers.includes('secret')) {
      obj[key] = new SynxSecret(String(obj[key])) as any;
    }

    // ── :unique ──
    if (markers.includes('unique') && Array.isArray(obj[key])) {
      const seen = new Set<string>();
      obj[key] = (obj[key] as SynxValue[]).filter((item) => {
        const s = String(item);
        if (seen.has(s)) return false;
        seen.add(s);
        return true;
      });
    }

    // ── :geo ──
    if (markers.includes('geo') && Array.isArray(obj[key])) {
      const region = options.region || 'US';
      const arr = obj[key] as string[];
      const found = arr.find((item) => String(item).startsWith(region + ' '));
      if (found) {
        obj[key] = found.substring(region.length + 1).trim();
      } else {
        // Fallback to first entry
        const first = arr[0];
        if (typeof first === 'string' && first.includes(' ')) {
          obj[key] = first.substring(first.indexOf(' ') + 1).trim();
        } else {
          obj[key] = first ?? null;
        }
      }
    }

    // ── :template ──
    if (markers.includes('template') && typeof obj[key] === 'string') {
      let tpl = obj[key] as string;
      tpl = tpl.replace(/\{(\w+(?:\.\w+)*)\}/g, (_match, ref: string) => {
        const val = deepGet(root, ref) ?? deepGet(obj, ref);
        return val != null ? String(val) : `{${ref}}`;
      });
      obj[key] = tpl;
    }

    // ── :split ──
    if (markers.includes('split') && typeof obj[key] === 'string') {
      const splitIdx = markers.indexOf('split');
      const delimArg = (splitIdx + 1 < markers.length) ? markers[splitIdx + 1] : ',';
      const sep = delimiterFromKeyword(delimArg);
      obj[key] = (obj[key] as string).split(sep).map(s => s.trim()).filter(s => s !== '').map(s => castPrimitive(s));
    }

    // ── :join ──
    if (markers.includes('join') && Array.isArray(obj[key])) {
      const joinIdx = markers.indexOf('join');
      const delimArg = (joinIdx + 1 < markers.length) ? markers[joinIdx + 1] : ',';
      const sep = delimiterFromKeyword(delimArg);
      obj[key] = (obj[key] as SynxValue[]).map(v => String(v)).join(sep);
    }

    // ── :default (standalone, not combined with :env) ──
    if (markers.includes('default') && !markers.includes('env')) {
      if (obj[key] === null || obj[key] === undefined || obj[key] === '') {
        const defaultIdx = markers.indexOf('default');
        if (defaultIdx !== -1 && markers.length > defaultIdx + 1) {
          const fallback = markers[defaultIdx + 1];
          obj[key] = isNaN(Number(fallback)) ? fallback : Number(fallback);
        }
      }
    }

    // ── :clamp ──
    // Syntax: key:clamp:MIN:MAX value
    if (markers.includes('clamp')) {
      const idx = markers.indexOf('clamp');
      const lo = parseFloat(markers[idx + 1] ?? '');
      const hi = parseFloat(markers[idx + 2] ?? '');
      if (!isNaN(lo) && !isNaN(hi)) {
        if (lo > hi) {
          obj[key] = `CONSTRAINT_ERR: clamp min (${lo}) > max (${hi})`;
        } else if (typeof obj[key] === 'number') {
          obj[key] = Math.min(hi, Math.max(lo, obj[key] as number));
        }
      }
    }

    // ── :round ──
    // Syntax: key:round:N value  (N = decimal places, default 0)
    if (markers.includes('round')) {
      const idx = markers.indexOf('round');
      const decimals = parseInt(markers[idx + 1] ?? '0', 10) || 0;
      if (typeof obj[key] === 'number') {
        const factor = Math.pow(10, decimals);
        obj[key] = Math.round((obj[key] as number) * factor) / factor;
      }
    }

    // ── :map ──
    // Syntax: key:map:source_key\n  - lookup_val result_text
    if (markers.includes('map') && Array.isArray(obj[key])) {
      const idx = markers.indexOf('map');
      const sourceKey = markers[idx + 1] ?? '';
      const lookupVal = String(sourceKey ? (deepGet(root, sourceKey) ?? deepGet(obj, sourceKey) ?? '') : '');
      const arr = obj[key] as string[];
      const found = arr.find((item) => {
        const s = String(item);
        const sep = s.indexOf(' ');
        return sep !== -1 && s.substring(0, sep).trim() === lookupVal;
      });
      obj[key] = found
        ? castPrimitive(found.substring(found.indexOf(' ') + 1).trim())
        : null;
    }

    // ── :format ──
    // Syntax: key:format:PATTERN value  (e.g. %.2f, %05d)
    if (markers.includes('format')) {
      const idx = markers.indexOf('format');
      const pattern = markers[idx + 1] ?? '%s';
      obj[key] = applyFormatPattern(pattern, obj[key]);
    }

    // ── :fallback ──
    // Syntax: key:fallback:DEFAULT_PATH value
    // If value is empty OR file does not exist, use the fallback.
    if (markers.includes('fallback')) {
      const idx = markers.indexOf('fallback');
      const defaultVal = markers[idx + 1] ?? '';
      const current = obj[key];
      let useFallback = current === null || current === undefined || current === '';
      if (!useFallback && typeof current === 'string' && fs && pathModule && options.basePath) {
        const fullPath = pathModule.resolve(options.basePath, current);
        useFallback = !fs.existsSync(fullPath);
      }
      if (useFallback && defaultVal) {
        obj[key] = defaultVal;
      }
    }

    // ── :once ──
    // Syntax: key:once  or  key:once:uuid  or  key:once:random  or  key:once:timestamp
    // Generates a value once and persists it in a .synx.lock file.
    if (markers.includes('once')) {
      const idx = markers.indexOf('once');
      const genType = markers[idx + 1] ?? 'uuid';
      const lockPath = pathModule && options.basePath
        ? pathModule.resolve(options.basePath, '.synx.lock')
        : '.synx.lock';

      const existing = readLockValue(lockPath, key);
      if (existing !== null) {
        obj[key] = existing;
      } else {
        let generated: string;
        if (genType === 'uuid') {
          generated = generateUuid();
        } else if (genType === 'timestamp') {
          generated = String(Date.now());
        } else if (genType === 'random') {
          generated = String(Math.floor(Math.random() * 2147483647));
        } else {
          generated = generateUuid();
        }
        writeLockValue(lockPath, key, generated);
        obj[key] = generated;
      }
    }

    // ── :version ──
    // Syntax: key:version:OP:REQUIRED value
    // Compares current version string against required, returns bool.
    if (markers.includes('version') && typeof obj[key] === 'string') {
      const idx = markers.indexOf('version');
      const op = markers[idx + 1] ?? '>=';
      const required = markers[idx + 2] ?? '0';
      obj[key] = compareVersions(obj[key] as string, op, required) as any;
    }

    // ── :watch ──
    // Syntax: key:watch:KEY_PATH ./file  (reads at parse time)
    if (markers.includes('watch') && typeof obj[key] === 'string') {
      if (!fs || !pathModule) {
        obj[key] = 'WATCH_ERR: not supported in browser';
      } else {
        const filePath = obj[key] as string;
        const basePath = options.basePath || (typeof process !== 'undefined' ? process.cwd() : '.');
        const fullPath = pathModule.resolve(basePath, filePath);
        const idx = markers.indexOf('watch');
        const keyPath = markers[idx + 1];
        try {
          const content = fs.readFileSync(fullPath, 'utf-8');
          const ext = pathModule.extname(fullPath).slice(1);
          if (keyPath) {
            obj[key] = extractFromFileContent(content, keyPath, ext) as any;
          } else {
            obj[key] = content.trim();
          }
        } catch (e: any) {
          obj[key] = `WATCH_ERR: ${e.message}`;
        }
      }
    }

    // ── Constraint validation (always last, after all markers resolved) ──
    if (metaMap && metaMap[key]?.constraints) {
      validateConstraints(obj, key, metaMap[key].constraints!);
    }
  }

  return obj;
}

// ─── Constraint enforcement ───────────────────────────────────────────────

function validateConstraints(obj: SynxObject, key: string, c: import('./types').SynxConstraints): void {
  const val = obj[key];

  // required
  if (c.required && (val === null || val === undefined || val === '')) {
    obj[key] = `CONSTRAINT_ERR: '${key}' is required`;
    return;
  }
  if (val === null || val === undefined) return;

  // type check
  if (c.type) {
    const ok = (() => {
      switch (c.type) {
        case 'int':    return typeof val === 'number' && Number.isInteger(val);
        case 'float':  return typeof val === 'number';
        case 'bool':   return typeof val === 'boolean';
        case 'string': return typeof val === 'string';
        default:       return true;
      }
    })();
    if (!ok) {
      obj[key] = `CONSTRAINT_ERR: '${key}' expected type '${c.type}'`;
      return;
    }
  }

  // enum check
  if (c.enum) {
    const strVal = String(val);
    if (!c.enum.includes(strVal)) {
      obj[key] = `CONSTRAINT_ERR: '${key}' must be one of [${c.enum.join('|')}]`;
      return;
    }
  }

  // min / max  (numbers: value range; strings: length range)
  const n = typeof val === 'number' ? val
          : typeof val === 'string' && (c.min !== undefined || c.max !== undefined)
            ? val.length : null;
  if (n !== null) {
    if (c.min !== undefined && n < c.min) {
      obj[key] = `CONSTRAINT_ERR: '${key}' value ${n} is below min ${c.min}`;
      return;
    }
    if (c.max !== undefined && n > c.max) {
      obj[key] = `CONSTRAINT_ERR: '${key}' value ${n} exceeds max ${c.max}`;
      return;
    }
  }

  // pattern (regex match)
  if (c.pattern && typeof val === 'string') {
    try {
      if (!new RegExp(c.pattern).test(val)) {
        obj[key] = `CONSTRAINT_ERR: '${key}' does not match pattern /${c.pattern}/`;
        return;
      }
    } catch { /* invalid regex — skip silently */ }
  }
}

// ─── New-marker helpers ───────────────────────────────────

function applyFormatPattern(pattern: string, value: unknown): string {
  const n = typeof value === 'number' ? value : parseFloat(String(value));
  if (isNaN(n)) return String(value);

  // %.2f → fixed decimals
  const floatMatch = pattern.match(/^%\.(\d+)f$/);
  if (floatMatch) return n.toFixed(parseInt(floatMatch[1], 10));

  // %05d → zero-padded integer
  const intMatch = pattern.match(/^%0(\d+)d$/);
  if (intMatch) return String(Math.round(n)).padStart(parseInt(intMatch[1], 10), '0');

  // %5d → right-padded integer
  const widthMatch = pattern.match(/^%(\d+)d$/);
  if (widthMatch) return String(Math.round(n)).padStart(parseInt(widthMatch[1], 10));

  // %e → exponential
  if (pattern === '%e') return n.toExponential();

  return String(value);
}

function readLockValue(lockPath: string, key: string): string | null {
  if (!fs) return null;
  try {
    const content = fs.readFileSync(lockPath, 'utf-8');
    for (const line of content.split(/\r?\n/)) {
      if (line.startsWith(key + ' ')) {
        return line.substring(key.length + 1);
      }
    }
  } catch { /* file doesn't exist yet */ }
  return null;
}

function writeLockValue(lockPath: string, key: string, value: string): void {
  if (!fs) return;
  let lines: string[] = [];
  try { lines = fs.readFileSync(lockPath, 'utf-8').split(/\r?\n/); } catch { /* ok */ }
  const newLine = `${key} ${value}`;
  const idx = lines.findIndex((l) => l.startsWith(key + ' '));
  if (idx !== -1) {
    lines[idx] = newLine;
  } else {
    lines.push(newLine);
  }
  try { fs.writeFileSync(lockPath, lines.filter(Boolean).join('\n') + '\n', 'utf-8'); } catch { /* ok */ }
}

function generateUuid(): string {
  if (typeof crypto !== 'undefined' && typeof (crypto as any).randomUUID === 'function') {
    return (crypto as any).randomUUID();
  }
  // Fallback manual generation
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = Math.random() * 16 | 0;
    return (c === 'x' ? r : (r & 0x3 | 0x8)).toString(16);
  });
}

function compareVersions(current: string, op: string, required: string): boolean {
  const parseVer = (s: string) => s.split('.').map((p) => parseInt(p, 10) || 0);
  const cv = parseVer(current);
  const rv = parseVer(required);
  const len = Math.max(cv.length, rv.length);
  let cmp = 0;
  for (let i = 0; i < len; i++) {
    const a = cv[i] ?? 0;
    const b = rv[i] ?? 0;
    if (a !== b) { cmp = a > b ? 1 : -1; break; }
  }
  switch (op) {
    case '>=': return cmp >= 0;
    case '<=': return cmp <= 0;
    case '>':  return cmp > 0;
    case '<':  return cmp < 0;
    case '==': case '=': return cmp === 0;
    case '!=': return cmp !== 0;
    default:   return false;
  }
}

function extractFromFileContent(content: string, keyPath: string, ext: string): unknown {
  if (ext === 'json') {
    try {
      const obj = JSON.parse(content);
      return keyPath.split('.').reduce((o: any, k) => o?.[k], obj) ?? null;
    } catch { return null; }
  }
  // SYNX key lookup
  for (const line of content.split(/\r?\n/)) {
    const trimmed = line.trimStart();
    if (trimmed.startsWith(keyPath + ' ')) {
      return castPrimitive(trimmed.substring(keyPath.length + 1).trimStart());
    }
  }
  return null;
}

// ─── Helpers ──────────────────────────────────────────────

function castPrimitive(val: string): SynxValue {
  if (val === 'true') return true;
  if (val === 'false') return false;
  if (val === 'null') return null;
  if (/^-?\d+$/.test(val)) return parseInt(val, 10);
  if (/^-?\d+\.\d+$/.test(val)) return parseFloat(val);
  return val;
}

const DELIM_MAP: Record<string, string> = {
  space: ' ', pipe: '|', dash: '-', dot: '.', semi: ';', tab: '\t', slash: '/',
};

function delimiterFromKeyword(keyword: string): string {
  return DELIM_MAP[keyword] || keyword;
}

function weightedRandom(items: SynxValue[], weights: number[]): SynxValue {
  const w: number[] = [...weights];
  if (w.length < items.length) {
    const assigned = w.reduce((a, b) => a + b, 0);
    // When assigned < 100: distribute remaining budget equally.
    // When assigned >= 100: give each unassigned item the same average weight
    // as the assigned ones so they remain reachable.
    const perItem = assigned < 100
      ? (100 - assigned) / (items.length - w.length)
      : assigned / w.length;
    while (w.length < items.length) w.push(perItem);
  }

  const total = w.reduce((a, b) => a + b, 0);
  if (total <= 0) return items[Math.floor(Math.random() * items.length)];

  const rand = Math.random();
  let cumulative = 0;
  for (let i = 0; i < items.length; i++) {
    cumulative += w[i] / total;
    if (rand <= cumulative) return items[i];
  }
  return items[items.length - 1];
}

// ─── Word-boundary variable substitution (replaces per-key RegExp creation) ─

function isWordChar(code: number): boolean {
  return (code >= 48 && code <= 57)   // 0-9
      || (code >= 65 && code <= 90)   // A-Z
      || (code >= 97 && code <= 122)  // a-z
      || code === 95;                 // _
}

function replaceWord(haystack: string, word: string, replacement: string): string {
  const wLen = word.length;
  const hLen = haystack.length;
  let result = '';
  let i = 0;
  while (i <= hLen - wLen) {
    if (haystack.slice(i, i + wLen) === word) {
      const before = i === 0 || !isWordChar(haystack.charCodeAt(i - 1));
      const after  = i + wLen >= hLen || !isWordChar(haystack.charCodeAt(i + wLen));
      if (before && after) {
        result += replacement;
        i += wLen;
        continue;
      }
    }
    result += haystack[i++];
  }
  while (i < hLen) result += haystack[i++];
  return result;
}

/** Substitute all variable references in `expr` without creating RegExp objects. */
function replaceVars(expr: string, vars: Map<string, string>): string {
  // Process longer keys first to avoid partial-match issues (e.g. "hp" vs "base_hp")
  const sorted = [...vars.entries()].sort((a, b) => b[0].length - a[0].length);
  let result = expr;
  for (const [k, v] of sorted) result = replaceWord(result, k, v);
  return result;
}

function deepGet(obj: SynxObject, path: string): SynxValue | undefined {
  // Try direct key first
  if (path in obj) return obj[path];
  // Try dot-path
  const parts = path.split('.');
  let current: any = obj;
  for (const part of parts) {
    if (current == null || typeof current !== 'object') return undefined;
    current = current[part];
  }
  return current;
}
