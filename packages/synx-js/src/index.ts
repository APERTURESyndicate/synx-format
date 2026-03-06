/**
 * SYNX — @aperturesyndicate/synx
 *
 * The Active Data Format.
 * Faster than JSON. Cheaper for AI tokens. Built-in logic.
 *
 * Auto-engine: files < 5 KB use the pure-JS parser;
 * files >= 5 KB use the native Rust binding (if available).
 *
 * @packageDocumentation
 */

import * as fs from 'fs';
import * as path from 'path';
import { parseData } from './parser';
import { resolve } from './engine';
import type { SynxObject, SynxOptions, SynxValue } from './types';

export type { SynxObject, SynxOptions, SynxValue, SynxArray, SynxPrimitive } from './types';

// ─── Native binding auto-detection ───────────────────────

const NATIVE_THRESHOLD = 5120; // 5 KB

interface NativeBinding {
  parse(text: string): unknown;
  parseToJson(text: string): string;
  parseActive(text: string): unknown;
}

let nativeBinding: NativeBinding | null | false = null; // null = not tried, false = unavailable

function tryLoadNative(): NativeBinding | false {
  if (nativeBinding !== null) return nativeBinding;
  try {
    // Walk up from synx-js to find bindings/node
    const bindingDir = path.resolve(__dirname, '..', '..', '..', 'bindings', 'node');
    const mod = require(bindingDir) as NativeBinding;
    if (typeof mod.parse === 'function') {
      nativeBinding = mod;
      return mod;
    }
  } catch { /* native not available */ }
  nativeBinding = false;
  return false;
}

class Synx {
  /**
   * Parse a .synx text string into a native JS object.
   *
   * Automatically selects the engine:
   * - text < 5 KB → pure-JS parser (zero startup cost)
   * - text >= 5 KB → native Rust binding (faster on large files)
   * Falls back to JS if the native binding is not built.
   *
   * @param text    - The .synx file contents as a string.
   * @param options - Optional settings (basePath, env overrides, region).
   * @returns A plain JS object with all data resolved.
   */
  static parse<T extends SynxObject = SynxObject>(text: string, options: SynxOptions = {}): T {
    // Large files → try native Rust binding
    if (text.length >= NATIVE_THRESHOLD) {
      const native = tryLoadNative();
      if (native) {
        const isActive = /(?:^|\n)\s*!active\s*(?:\r?\n|$)/.test(text) ||
                         /(?:^|\n)\s*#!mode:active/.test(text);
        const result = isActive
          ? native.parseActive(text)
          : native.parse(text);
        return result as T;
      }
    }

    // Small files or no native binding → pure JS
    const { root, mode, locked } = parseData(text);
    if (mode === 'active') {
      resolve(root, options);
    }
    if (locked) {
      Object.defineProperty(root, '__synx_locked', {
        value: true,
        enumerable: false,
        writable: false,
        configurable: false,
      });
    }
    return root as T;
  }

  /**
   * Load and parse a .synx file synchronously.
   *
   * @param filePath - Path to the .synx file.
   * @param options  - Optional settings.
   * @returns A plain JS object.
   *
   * @example
   * ```ts
   * const config = Synx.loadSync('config.synx');
   * console.log(config.app_name); // "TotalWario"
   * ```
   */
  static loadSync<T extends SynxObject = SynxObject>(filePath: string, options: SynxOptions = {}): T {
    const absPath = path.resolve(filePath);
    const text = fs.readFileSync(absPath, 'utf-8');
    // Spread to avoid mutating the caller's options object
    const opts = options.basePath ? options : { ...options, basePath: path.dirname(absPath) };
    return Synx.parse<T>(text, opts);
  }

  /**
   * Load and parse a .synx file asynchronously.
   *
   * @param filePath - Path to the .synx file.
   * @param options  - Optional settings.
   * @returns A Promise resolving to a plain JS object.
   *
   * @example
   * ```ts
   * const config = await Synx.load('config.synx');
   * console.log(config.gameplay.boss_hp); // 500
   * ```
   */
  static async load<T extends SynxObject = SynxObject>(filePath: string, options: SynxOptions = {}): Promise<T> {
    const absPath = path.resolve(filePath);
    const text = await fs.promises.readFile(absPath, 'utf-8');
    // Spread to avoid mutating the caller's options object
    const opts = options.basePath ? options : { ...options, basePath: path.dirname(absPath) };
    return Synx.parse<T>(text, opts);
  }

  /**
   * Serialize a JS object back to .synx format string.
   *
   * @param obj    - The object to serialize.
   * @param active - If true, prepends `!active` header.
   * @returns A .synx formatted string.
   */
  static stringify(obj: SynxObject, active = false): string {
    let out = '';
    if (active) {
      out += '!active\n';
    }
    out += serializeObject(obj, 0);
    return out;
  }

  // ─── Runtime Manipulation API ─────────────────────────

  /**
   * Set a value on a parsed SYNX config object.
   * Supports dot-path notation for nested keys.
   * Throws if config has `!lock` directive.
   *
   * @example
   * ```ts
   * const config = Synx.loadSync('config.synx');
   * Synx.set(config, 'max_players', 100);
   * Synx.set(config, 'server.host', 'localhost');
   * ```
   */
  static set(obj: SynxObject, keyPath: string, value: SynxValue): void {
    if ((obj as any).__synx_locked) {
      throw new Error(`SYNX: Cannot set "${keyPath}" — config is locked (!lock)`);
    }
    const parts = keyPath.split('.');
    let current: any = obj;
    for (let i = 0; i < parts.length - 1; i++) {
      if (current[parts[i]] == null || typeof current[parts[i]] !== 'object') {
        current[parts[i]] = {};
      }
      current = current[parts[i]];
    }
    current[parts[parts.length - 1]] = value;
  }

  /**
   * Get a value from a parsed SYNX config using dot-path notation.
   *
   * @example
   * ```ts
   * const port = Synx.get(config, 'server.port'); // 8080
   * ```
   */
  static get(obj: SynxObject, keyPath: string): SynxValue | undefined {
    const parts = keyPath.split('.');
    let current: any = obj;
    for (const part of parts) {
      if (current == null || typeof current !== 'object') return undefined;
      current = current[part];
    }
    return current;
  }

  /**
   * Add an item to an array value in the config.
   * Creates the array if it doesn't exist.
   * Throws if config has `!lock` directive.
   *
   * @example
   * ```ts
   * Synx.add(config, 'your_random_name', 'Mark');
   * // your_random_name: ["Alice", "Caroline", "Mark"]
   * ```
   */
  static add(obj: SynxObject, keyPath: string, item: SynxValue): void {
    if ((obj as any).__synx_locked) {
      throw new Error(`SYNX: Cannot add to "${keyPath}" — config is locked (!lock)`);
    }
    const parts = keyPath.split('.');
    let current: any = obj;
    for (let i = 0; i < parts.length - 1; i++) {
      if (current[parts[i]] == null || typeof current[parts[i]] !== 'object') {
        current[parts[i]] = {};
      }
      current = current[parts[i]];
    }
    const finalKey = parts[parts.length - 1];
    if (!Array.isArray(current[finalKey])) {
      current[finalKey] = current[finalKey] != null ? [current[finalKey]] : [];
    }
    (current[finalKey] as SynxValue[]).push(item);
  }

  /**
   * Remove an item from an array value, or delete a key entirely.
   * - If value is an array and `item` is provided: removes first occurrence of `item`.
   * - If `item` is omitted: deletes the key entirely.
   * Throws if config has `!lock` directive.
   *
   * @example
   * ```ts
   * Synx.remove(config, 'your_random_name', 'Alice');
   * // or delete entirely:
   * Synx.remove(config, 'max_players');
   * ```
   */
  static remove(obj: SynxObject, keyPath: string, item?: SynxValue): void {
    if ((obj as any).__synx_locked) {
      throw new Error(`SYNX: Cannot remove "${keyPath}" — config is locked (!lock)`);
    }
    const parts = keyPath.split('.');
    let current: any = obj;
    for (let i = 0; i < parts.length - 1; i++) {
      if (current == null || typeof current !== 'object') return;
      current = current[parts[i]];
    }
    if (current == null || typeof current !== 'object') return;
    const finalKey = parts[parts.length - 1];

    if (item !== undefined && Array.isArray(current[finalKey])) {
      const arr = current[finalKey] as SynxValue[];
      const idx = arr.findIndex(v => v === item || String(v) === String(item));
      if (idx !== -1) arr.splice(idx, 1);
    } else {
      delete current[finalKey];
    }
  }

  /**
   * Check if the config is locked (`!lock` directive).
   */
  static isLocked(obj: SynxObject): boolean {
    return !!(obj as any).__synx_locked;
  }

  /**
   * Reformat a .synx string into canonical form:
   * - Keys sorted alphabetically at every nesting level
   * - Exactly 2 spaces per indentation level
   * - One blank line between top-level blocks (objects / lists)
   * - Comments stripped — canonical form is comment-free
   * - Directive lines (`!active`, `!lock`) preserved at the top
   *
   * The same data always produces byte-for-byte identical output,
   * making `.synx` files deterministic and noise-free in `git diff`.
   *
   * @param text - Raw .synx file contents.
   * @returns Canonical .synx string.
   *
   * @example
   * ```ts
   * const raw = fs.readFileSync('config.synx', 'utf-8');
   * fs.writeFileSync('config.synx', Synx.format(raw));
   * ```
   */
  static format(text: string): string {
    const lines = text.split(/\r?\n/);
    const directives: string[] = [];
    let bodyStart = 0;

    for (let i = 0; i < lines.length; i++) {
      const t = lines[i].trim();
      if (t === '!active' || t === '!lock' || t === '#!mode:active') {
        directives.push(t);
        bodyStart = i + 1;
      } else if (!t || t.startsWith('#') || t.startsWith('//')) {
        bodyStart = i + 1;
      } else {
        break;
      }
    }

    const [nodes] = fmtParse(lines, bodyStart, 0);
    fmtSort(nodes);

    let out = directives.join('\n');
    if (directives.length) out += '\n\n';
    out += fmtEmit(nodes, 0).trimEnd();
    return out + '\n';
  }
}

// ─── Serializer ───────────────────────────────────────────

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

// ─── Canonical Formatter ──────────────────────────────────

interface FmtNode {
  header: string;
  children: FmtNode[];
  listItems: string[];
  isMultiline: boolean;
}

function fmtParse(lines: string[], start: number, base: number): [FmtNode[], number] {
  const nodes: FmtNode[] = [];
  let i = start;
  while (i < lines.length) {
    const raw = lines[i];
    const t = raw.trim();
    if (!t) { i++; continue; }
    const ind = raw.search(/\S/);
    if (ind < base) break;
    if (ind > base) { i++; continue; }
    if (t.startsWith('- ') || t.startsWith('#') || t.startsWith('//')) { i++; continue; }
    const isMultiline = t.trimEnd().endsWith(' |') || t === '|';
    const node: FmtNode = { header: t, children: [], listItems: [], isMultiline };
    i++;
    while (i < lines.length) {
      const cr = lines[i];
      const ct = cr.trim();
      if (!ct) { i++; continue; }
      const ci = cr.search(/\S/);
      if (ci <= base) break;
      if (isMultiline || ct.startsWith('- ')) {
        node.listItems.push(ct);
        i++;
      } else if (ct.startsWith('#') || ct.startsWith('//')) {
        i++;
      } else {
        const [subs, ni] = fmtParse(lines, i, ci);
        node.children.push(...subs);
        i = ni;
      }
    }
    nodes.push(node);
  }
  return [nodes, i];
}

function fmtSort(nodes: FmtNode[]): void {
  nodes.sort((a, b) => {
    const ka = a.header.split(/[\s\[:(]/)[0].toLowerCase();
    const kb = b.header.split(/[\s\[:(]/)[0].toLowerCase();
    return ka.localeCompare(kb);
  });
  for (const n of nodes) fmtSort(n.children);
}

function fmtEmit(nodes: FmtNode[], indent: number): string {
  const sp = ' '.repeat(indent);
  let out = '';
  for (const n of nodes) {
    out += `${sp}${n.header}\n`;
    if (n.children.length > 0) out += fmtEmit(n.children, indent + 2);
    for (const li of n.listItems) out += `${sp}  ${li}\n`;
    if (indent === 0 && (n.children.length > 0 || n.listItems.length > 0)) out += '\n';
  }
  return out;
}

// ─── Exports ──────────────────────────────────────────────

export default Synx;
export { Synx };
module.exports = Synx;
module.exports.default = Synx;
module.exports.Synx = Synx;
