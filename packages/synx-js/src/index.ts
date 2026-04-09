/**
 * SYNX — @aperturesyndicate/synx-format
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
import type { SynxObject, SynxOptions, SynxValue, SynxMetaMap, SynxDiff } from './types';
import { SynxError } from './types';

export type { SynxObject, SynxOptions, SynxValue, SynxArray, SynxPrimitive, SynxDiff } from './types';
export { SynxError } from './types';

// ─── Native binding auto-detection ───────────────────────

const NATIVE_THRESHOLD = 5120; // 5 KB

const UNSAFE_KEYS = new Set(['__proto__', 'constructor', 'prototype']);

interface NativeBinding {
  parse(text: string): unknown;
  parseToJson(text: string): string;
  parseActive(text: string, options?: SynxOptions): unknown;
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

const RUNTIME_ERROR_PREFIXES = [
  'INCLUDE_ERR:',
  'WATCH_ERR:',
  'CALC_ERR:',
  'SPAM_ERR:',
  'CONSTRAINT_ERR:',
  'ALIAS_ERR:',
  'NESTING_ERR:',
] as const;

function assertNoRuntimeErrors(value: unknown, path = 'root'): void {
  if (typeof value === 'string') {
    for (const prefix of RUNTIME_ERROR_PREFIXES) {
      if (value.startsWith(prefix)) {
        throw new SynxError(`${value}`);
      }
    }
    return;
  }

  if (Array.isArray(value)) {
    for (let i = 0; i < value.length; i++) {
      assertNoRuntimeErrors(value[i], `${path}[${i}]`);
    }
    return;
  }

  if (value && typeof value === 'object') {
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      assertNoRuntimeErrors(v, `${path}.${k}`);
    }
  }
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
          ? native.parseActive(text, options)
          : native.parse(text);
        if (options.strict) {
          assertNoRuntimeErrors(result);
        }
        return result as T;
      }
    }

    // Small files or no native binding → pure JS
    const { root, mode, locked, includes } = parseData(text);
    if (mode === 'active') {
      resolve(root, { ...options, _includes: includes } as any);
    }
    if (locked) {
      Object.defineProperty(root, '__synx_locked', {
        value: true,
        enumerable: false,
        writable: false,
        configurable: false,
      });
    }
    if (options.strict) {
      assertNoRuntimeErrors(root);
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
   * Save a JS object to a .synx file synchronously.
   *
   * @param filePath - Path to the .synx file.
   * @param obj      - The object to serialize and save.
   * @param active   - If true, include `!active` directive.
   *
   * @example
   * ```ts
   * Synx.saveSync('config.synx', { app_name: 'TotalWario', port: 8080 });
   * ```
   */
  static saveSync(filePath: string, obj: SynxObject, active: boolean = false): void {
    const absPath = path.resolve(filePath);
    const text = Synx.stringify(obj, active);
    fs.writeFileSync(absPath, text, 'utf-8');
  }

  /**
   * Save a JS object to a .synx file asynchronously.
   *
   * @param filePath - Path to the .synx file.
   * @param obj      - The object to serialize and save.
   * @param active   - If true, include `!active` directive.
   *
   * @example
   * ```ts
   * await Synx.save('config.synx', { app_name: 'TotalWario', port: 8080 });
   * ```
   */
  static async save(filePath: string, obj: SynxObject, active: boolean = false): Promise<void> {
    const absPath = path.resolve(filePath);
    const text = Synx.stringify(obj, active);
    await fs.promises.writeFile(absPath, text, 'utf-8');
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
    for (const p of parts) if (UNSAFE_KEYS.has(p)) throw new Error(`SYNX: unsafe key "${p}"`);
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
      if (!Object.prototype.hasOwnProperty.call(current, part)) return undefined;
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
    for (const p of parts) if (UNSAFE_KEYS.has(p)) throw new Error(`SYNX: unsafe key "${p}"`);
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
    for (const p of parts) if (UNSAFE_KEYS.has(p)) throw new Error(`SYNX: unsafe key "${p}"`);
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

  // ─── Export Converters ──────────────────────────────────

  /** Convert a parsed SYNX object to JSON string. @since 3.1.3 */
  static toJSON(obj: SynxObject, pretty = true): string {
    return toJSONString(obj, pretty);
  }

  /** Convert a parsed SYNX object to YAML string. @since 3.1.3 */
  static toYAML(obj: SynxObject): string {
    return toYAMLString(obj);
  }

  /** Convert a parsed SYNX object to TOML string. @since 3.1.3 */
  static toTOML(obj: SynxObject): string {
    return toTOMLString(obj as Record<string, unknown>);
  }

  /** Convert a parsed SYNX object to .env format (KEY=VALUE lines). @since 3.1.3 */
  static toEnv(obj: SynxObject, prefix = ''): string {
    return toEnvString(obj as Record<string, unknown>, prefix);
  }

  /** Watch a .synx file for changes. Re-parses and calls callback on change. @since 3.1.3 */
  static watch(filePath: string, callback: WatchCallback, options: SynxOptions = {}): WatchHandle {
    if (!fs) throw new Error('Synx.watch() is not supported in browser');
    const absPath = path.resolve(filePath);
    const opts = options.basePath ? options : { ...options, basePath: path.dirname(absPath) };

    try {
      const text = fs.readFileSync(absPath, 'utf-8');
      const config = Synx.parse(text, opts);
      callback(config);
    } catch (e: any) {
      callback({}, e);
    }

    const watcher = fs.watch(absPath, { persistent: true }, (_event) => {
      try {
        const text = fs.readFileSync(absPath, 'utf-8');
        const config = Synx.parse(text, opts);
        callback(config);
      } catch (e: any) {
        callback({}, e);
      }
    });

    return { close: () => watcher.close() };
  }

  /** Extract a JSON Schema from SYNX constraints. @since 3.1.3 */
  static schema(text: string): SynxSchema {
    const { root } = parseData(text);
    const metaMap: SynxMetaMap | undefined = (root as any).__synx;
    const properties: Record<string, SynxSchemaProperty> = {};
    const required: string[] = [];

    if (metaMap) {
      for (const [key, meta] of Object.entries(metaMap)) {
        if (!meta.constraints) continue;
        const c = meta.constraints;
        const prop: SynxSchemaProperty = {};
        if (c.type) {
          const typeMap: Record<string, string> = {
            int: 'integer', float: 'number', bool: 'boolean', string: 'string',
          };
          prop.type = typeMap[c.type] || c.type;
        }
        if (c.min !== undefined) prop.minimum = c.min;
        if (c.max !== undefined) prop.maximum = c.max;
        if (c.pattern) prop.pattern = c.pattern;
        if (c.enum) prop.enum = c.enum;
        if (c.required) {
          required.push(key);
        }
        properties[key] = prop;
      }
    }

    return {
      $schema: 'https://json-schema.org/draft/2020-12/schema',
      type: 'object',
      properties,
      required,
    };
  }

  /**
   * Structural diff between two parsed SYNX objects.
   * Compares top-level keys and returns added, removed, changed, and unchanged.
   *
   * @param a - First object (before).
   * @param b - Second object (after).
   * @returns A SynxDiff describing the structural differences.
   *
   * @since 3.6.0
   *
   * @example
   * ```ts
   * const before = Synx.parse('name Alice\nage 30');
   * const after  = Synx.parse('name Bob\nage 30\nrole admin');
   * const diff   = Synx.diff(before, after);
   * // diff.added   → { role: 'admin' }
   * // diff.removed → {}
   * // diff.changed → { name: { from: 'Alice', to: 'Bob' } }
   * // diff.unchanged → ['age']
   * ```
   */
  static diff(a: SynxObject, b: SynxObject): SynxDiff {
    const added: Record<string, SynxValue> = {};
    const removed: Record<string, SynxValue> = {};
    const changed: Record<string, { from: SynxValue; to: SynxValue }> = {};
    const unchanged: string[] = [];

    const aKeys = new Set(Object.keys(a));
    const bKeys = new Set(Object.keys(b));

    for (const key of aKeys) {
      if (!bKeys.has(key)) {
        removed[key] = a[key];
      } else if (deepEqual(a[key], b[key])) {
        unchanged.push(key);
      } else {
        changed[key] = { from: a[key], to: b[key] };
      }
    }

    for (const key of bKeys) {
      if (!aKeys.has(key)) {
        added[key] = b[key];
      }
    }

    return { added, removed, changed, unchanged };
  }

  // ─── Binary Format (.synxb) ─────────────────────────────

  /** Magic header for .synxb files */
  private static readonly SYNXB_MAGIC = new Uint8Array([0x53, 0x59, 0x4e, 0x58, 0x42]); // "SYNXB"

  /**
   * Check if data is a `.synxb` binary file.
   *
   * @param data - Raw bytes to check.
   * @returns True if the data starts with the SYNXB magic header.
   *
   * @example
   * ```ts
   * const buf = fs.readFileSync('config.synxb');
   * if (Synx.isSynxb(buf)) { ... }
   * ```
   */
  static isSynxb(data: Buffer | Uint8Array): boolean {
    if (data.length < 6) return false;
    for (let i = 0; i < 5; i++) {
      if (data[i] !== Synx.SYNXB_MAGIC[i]) return false;
    }
    return true;
  }

  /**
   * Compile a .synx string into compact binary `.synxb` format.
   *
   * The binary format stores the parsed value tree with an interned string table.
   * It is deterministic and much faster to load than re-parsing text.
   *
   * @param text     - The .synx source text.
   * @param resolved - If true and text is `!active`, resolve markers first.
   * @returns A Uint8Array containing the `.synxb` binary.
   *
   * @example
   * ```ts
   * const binary = Synx.compile('name Alice\nage 30');
   * fs.writeFileSync('config.synxb', binary);
   * ```
   */
  static compile(text: string, resolved: boolean = false): Uint8Array {
    const parsed = Synx.parse(text, { active: resolved });
    const strings: string[] = [];
    const stringIndex = new Map<string, number>();

    function internString(s: string): number {
      if (stringIndex.has(s)) return stringIndex.get(s)!;
      const idx = strings.length;
      strings.push(s);
      stringIndex.set(s, idx);
      return idx;
    }

    // Collect all strings first
    function collectStrings(val: unknown): void {
      if (val === null || val === undefined) return;
      if (typeof val === 'string') { internString(val); return; }
      if (Array.isArray(val)) { val.forEach(collectStrings); return; }
      if (typeof val === 'object') {
        for (const [k, v] of Object.entries(val as Record<string, unknown>)) {
          internString(k);
          collectStrings(v);
        }
      }
    }
    collectStrings(parsed);

    // Encode
    const buf: number[] = [];

    // Magic + version
    buf.push(0x53, 0x59, 0x4e, 0x58, 0x42); // SYNXB
    buf.push(0x01); // version 1

    // Flags: bit0=active, bit3=resolved
    const isActive = text.trimStart().startsWith('!active');
    let flags = 0;
    if (isActive) flags |= 0x01;
    if (resolved) flags |= 0x08;
    buf.push(flags);

    // String table
    writeVarint(buf, strings.length);
    for (const s of strings) {
      const encoded = new TextEncoder().encode(s);
      writeVarint(buf, encoded.length);
      for (const b of encoded) buf.push(b);
    }

    // Value tree
    function writeValue(val: unknown): void {
      if (val === null || val === undefined) { buf.push(0x00); return; }
      if (typeof val === 'boolean') { buf.push(val ? 0x02 : 0x01); return; }
      if (typeof val === 'number') {
        if (Number.isInteger(val)) {
          buf.push(0x03);
          writeZigzag(buf, val);
        } else {
          buf.push(0x04);
          const view = new DataView(new ArrayBuffer(8));
          view.setFloat64(0, val, true); // little-endian
          for (let i = 0; i < 8; i++) buf.push(view.getUint8(i));
        }
        return;
      }
      if (typeof val === 'string') {
        buf.push(0x05);
        writeVarint(buf, internString(val));
        return;
      }
      if (Array.isArray(val)) {
        buf.push(0x06);
        writeVarint(buf, val.length);
        val.forEach(writeValue);
        return;
      }
      if (typeof val === 'object') {
        const entries = Object.entries(val as Record<string, unknown>);
        buf.push(0x07);
        writeVarint(buf, entries.length);
        for (const [k, v] of entries) {
          writeVarint(buf, internString(k));
          writeValue(v);
        }
      }
    }
    writeValue(parsed);

    return new Uint8Array(buf);
  }

  /**
   * Decompile a `.synxb` binary back into a .synx text string.
   *
   * @param data - Raw `.synxb` bytes.
   * @returns The reconstructed .synx text.
   * @throws Error if the data is not valid `.synxb`.
   *
   * @example
   * ```ts
   * const buf = fs.readFileSync('config.synxb');
   * const text = Synx.decompile(buf);
   * console.log(text);
   * ```
   */
  static decompile(data: Buffer | Uint8Array): string {
    if (!Synx.isSynxb(data)) {
      throw new SynxError('Not a valid .synxb file');
    }

    let offset = 5; // skip magic
    const version = data[offset++];
    if (version !== 1) throw new SynxError(`Unsupported .synxb version: ${version}`);

    const flags = data[offset++];
    const isActive = (flags & 0x01) !== 0;
    const isLocked = (flags & 0x02) !== 0;

    // String table
    const [strCount, o1] = readVarint(data, offset); offset = o1;
    const strings: string[] = [];
    const decoder = new TextDecoder();
    for (let i = 0; i < strCount; i++) {
      const [len, o2] = readVarint(data, offset); offset = o2;
      strings.push(decoder.decode(data.slice(offset, offset + len)));
      offset += len;
    }

    // Value tree
    function readValue(): unknown {
      const tag = data[offset++];
      switch (tag) {
        case 0x00: return null;
        case 0x01: return false;
        case 0x02: return true;
        case 0x03: { const [v, o] = readZigzag(data, offset); offset = o; return v; }
        case 0x04: {
          const view = new DataView(data.buffer, data.byteOffset + offset, 8);
          offset += 8;
          return view.getFloat64(0, true);
        }
        case 0x05: { const [idx, o] = readVarint(data, offset); offset = o; return strings[idx]; }
        case 0x06: {
          const [len, o] = readVarint(data, offset); offset = o;
          const arr: unknown[] = [];
          for (let i = 0; i < len; i++) arr.push(readValue());
          return arr;
        }
        case 0x07: {
          const [len, o] = readVarint(data, offset); offset = o;
          const obj: Record<string, unknown> = {};
          for (let i = 0; i < len; i++) {
            const [ki, o2] = readVarint(data, offset); offset = o2;
            obj[strings[ki]] = readValue();
          }
          return obj;
        }
        case 0x08: { const [idx, o] = readVarint(data, offset); offset = o; return '[SECRET]'; }
        default: throw new SynxError(`Unknown value tag: 0x${tag.toString(16)}`);
      }
    }
    const value = readValue() as SynxObject;

    let header = '';
    if (isActive) header += '!active\n';
    if (isLocked) header += '!lock\n';
    if (header) header += '\n';
    return header + serializeObject(value, 0);
  }

  /**
   * Parse a `!tool` mode SYNX text, reshaping into `{ tool, params }` format.
   *
   * @param text    - The .synx text (should contain `!tool` directive).
   * @param options - Optional settings.
   * @returns `{ tool: string, params: SynxObject }`.
   *
   * @example
   * ```ts
   * const toolCall = Synx.parseTool('!tool\ntool search\nparams\n  query AI');
   * // { tool: 'search', params: { query: 'AI' } }
   * ```
   */
  static parseTool(text: string, options: SynxOptions = {}): { tool: string; params: SynxObject } {
    const parsed = Synx.parse(text, options);
    const tool = typeof parsed.tool === 'string' ? parsed.tool : '';
    const params = (typeof parsed.params === 'object' && parsed.params !== null && !Array.isArray(parsed.params))
      ? parsed.params as SynxObject
      : {};
    return { tool, params };
  }
}

// ─── Binary encoding helpers ──────────────────────────────

function writeVarint(buf: number[], value: number): void {
  let v = value >>> 0;
  while (v > 0x7f) {
    buf.push((v & 0x7f) | 0x80);
    v >>>= 7;
  }
  buf.push(v);
}

function readVarint(data: Uint8Array | Buffer, offset: number): [number, number] {
  let result = 0;
  let shift = 0;
  while (offset < data.length) {
    const byte = data[offset++];
    result |= (byte & 0x7f) << shift;
    if ((byte & 0x80) === 0) break;
    shift += 7;
  }
  return [result >>> 0, offset];
}

function writeZigzag(buf: number[], value: number): void {
  const zigzag = (value << 1) ^ (value >> 31);
  writeVarint(buf, zigzag >>> 0);
}

function readZigzag(data: Uint8Array | Buffer, offset: number): [number, number] {
  const [raw, newOffset] = readVarint(data, offset);
  const value = (raw >>> 1) ^ -(raw & 1);
  return [value, newOffset];
}

function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (a === null || b === null) return false;
  if (typeof a !== typeof b) return false;

  if (Array.isArray(a) && Array.isArray(b)) {
    if (a.length !== b.length) return false;
    return a.every((item, i) => deepEqual(item, b[i]));
  }

  if (typeof a === 'object' && typeof b === 'object') {
    const aObj = a as Record<string, unknown>;
    const bObj = b as Record<string, unknown>;
    const aKeys = Object.keys(aObj);
    const bKeys = Object.keys(bObj);
    if (aKeys.length !== bKeys.length) return false;
    return aKeys.every(k => deepEqual(aObj[k], bObj[k]));
  }

  return false;
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

// ─── Export Converters ────────────────────────────────────

function toJSONString(obj: SynxObject, pretty = true): string {
  return pretty ? JSON.stringify(obj, null, 2) : JSON.stringify(obj);
}

function toYAMLString(value: unknown, indent = 0): string {
  const sp = ' '.repeat(indent);
  if (value === null || value === undefined) return `${sp}null\n`;
  if (typeof value === 'boolean' || typeof value === 'number') return `${sp}${value}\n`;
  if (typeof value === 'string') {
    if (value.includes('\n') || value.includes(':') || value.includes('#') ||
        value.startsWith('{') || value.startsWith('[') || value.startsWith('"') ||
        value.startsWith("'") || /^(true|false|null|yes|no|on|off)$/i.test(value) ||
        value === '') {
      return `${sp}${JSON.stringify(value)}\n`;
    }
    return `${sp}${value}\n`;
  }
  if (Array.isArray(value)) {
    if (value.length === 0) return `${sp}[]\n`;
    let out = '';
    for (const item of value) {
      if (item && typeof item === 'object' && !Array.isArray(item)) {
        out += `${sp}- `;
        const entries = Object.entries(item as Record<string, unknown>);
        if (entries.length > 0) {
          const [fk, fv] = entries[0];
          out += `${fk}: ${toYAMLValue(fv)}\n`;
          for (let i = 1; i < entries.length; i++) {
            out += `${sp}  ${entries[i][0]}: ${toYAMLValue(entries[i][1])}\n`;
          }
        }
      } else {
        out += `${sp}- ${toYAMLValue(item)}\n`;
      }
    }
    return out;
  }
  if (typeof value === 'object') {
    let out = '';
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      if (k.startsWith('__synx')) continue;
      if (v && typeof v === 'object') {
        out += `${sp}${k}:\n`;
        out += Array.isArray(v)
          ? toYAMLString(v, indent + 2)
          : toYAMLString(v, indent + 2);
      } else {
        out += `${sp}${k}: ${toYAMLValue(v)}\n`;
      }
    }
    return out;
  }
  return `${sp}${String(value)}\n`;
}

function toYAMLValue(v: unknown): string {
  if (v === null || v === undefined) return 'null';
  if (typeof v === 'boolean' || typeof v === 'number') return String(v);
  if (typeof v === 'string') {
    if (v.includes('\n') || v.includes(':') || v.includes('#') ||
        v.startsWith('{') || v.startsWith('[') || v.startsWith('"') ||
        v.startsWith("'") || /^(true|false|null|yes|no|on|off)$/i.test(v) ||
        v === '') {
      return JSON.stringify(v);
    }
    return v;
  }
  return JSON.stringify(v);
}

function toTOMLString(obj: Record<string, unknown>, prefix = ''): string {
  let out = '';
  const simple: [string, unknown][] = [];
  const tables: [string, Record<string, unknown>][] = [];
  const arrays: [string, unknown[]][] = [];

  for (const [k, v] of Object.entries(obj)) {
    if (k.startsWith('__synx')) continue;
    if (Array.isArray(v)) {
      const allObjects = v.length > 0 && v.every(i => i && typeof i === 'object' && !Array.isArray(i));
      if (allObjects) {
        arrays.push([k, v]);
      } else {
        simple.push([k, v]);
      }
    } else if (v && typeof v === 'object') {
      tables.push([k, v as Record<string, unknown>]);
    } else {
      simple.push([k, v]);
    }
  }

  for (const [k, v] of simple) {
    out += `${k} = ${toTOMLValue(v)}\n`;
  }

  for (const [k, v] of tables) {
    const path = prefix ? `${prefix}.${k}` : k;
    out += `\n[${path}]\n`;
    out += toTOMLString(v, path);
  }

  for (const [k, arr] of arrays) {
    const path = prefix ? `${prefix}.${k}` : k;
    for (const item of arr) {
      out += `\n[[${path}]]\n`;
      out += toTOMLString(item as Record<string, unknown>, path);
    }
  }

  return out;
}

function toTOMLValue(v: unknown): string {
  if (v === null || v === undefined) return '""';
  if (typeof v === 'boolean') return String(v);
  if (typeof v === 'number') {
    if (Number.isInteger(v)) return String(v);
    const s = String(v);
    return s.includes('.') ? s : `${s}.0`;
  }
  if (typeof v === 'string') return JSON.stringify(v);
  if (Array.isArray(v)) return `[${v.map(toTOMLValue).join(', ')}]`;
  return JSON.stringify(v);
}

function toEnvString(obj: Record<string, unknown>, prefix = ''): string {
  let out = '';
  for (const [k, v] of Object.entries(obj)) {
    if (k.startsWith('__synx')) continue;
    const envKey = prefix ? `${prefix}_${k}`.toUpperCase() : k.toUpperCase();
    if (v && typeof v === 'object' && !Array.isArray(v)) {
      out += toEnvString(v as Record<string, unknown>, envKey);
    } else if (Array.isArray(v)) {
      out += `${envKey}=${v.map(String).join(',')}\n`;
    } else if (v === null) {
      out += `${envKey}=\n`;
    } else {
      const s = String(v);
      out += s.includes(' ') || s.includes('"') ? `${envKey}="${s}"\n` : `${envKey}=${s}\n`;
    }
  }
  return out;
}

// ─── Watch ────────────────────────────────────────────────

type WatchCallback = (config: SynxObject, error?: Error) => void;

interface WatchHandle {
  close(): void;
}

// ─── Schema ───────────────────────────────────────────────

interface SynxSchemaProperty {
  type?: string;
  minimum?: number;
  maximum?: number;
  minLength?: number;
  maxLength?: number;
  pattern?: string;
  enum?: string[];
}

interface SynxSchema {
  $schema: string;
  type: 'object';
  properties: Record<string, SynxSchemaProperty>;
  required: string[];
}

// ─── Exports ──────────────────────────────────────────────

export default Synx;
export { Synx };
module.exports = Synx;
module.exports.default = Synx;
module.exports.Synx = Synx;
module.exports.SynxError = SynxError;
