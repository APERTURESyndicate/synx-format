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
import {
  parseData, inputText, decodeUtf8, metaMapOf, markLocked, isLockedRoot, utf8Length, SynxError,
  SYNXL_VERSION, MAX_SYNXL_RECORD_BYTES, MAX_SYNXL_FIELDS, MAX_SYNXL_FIELD_NAME_BYTES,
  MAX_SYNXL_FIELD_LISTS, MAX_SYNXL_RECORDS,
} from './parser';
import { resolve, SynxSecret, assertNoRuntimeErrors } from './engine';
import type { SynxObject, SynxOptions, SynxValue, SynxDiff, SynxMeta } from './types';
import type {
  SynxlDocument,
  SynxlOptions,
  SynxlRecord,
  SynxlWriteOptions,
} from './types';
import type { SynxlPart } from './synxl';

// perf: a service reads its config once, at start, and what it pays for is
// mostly the loading of this package. Reading a document takes the parser and
// the engine; the record reader and writer, the JSON writer, the formatter and
// the `.synxb` codec are loaded by the first call that needs them.
let synxlModule: typeof import('./synxl') | undefined;
let jsonModule: typeof import('./json') | undefined;
let formatModule: typeof import('./format') | undefined;
let binaryModule: typeof import('./binary') | undefined;
function synxl(): typeof import('./synxl') {
  return synxlModule ?? (synxlModule = require('./synxl') as typeof import('./synxl'));
}
function json(): typeof import('./json') {
  return jsonModule ?? (jsonModule = require('./json') as typeof import('./json'));
}
function format(): typeof import('./format') {
  return formatModule ?? (formatModule = require('./format') as typeof import('./format'));
}
function binary(): typeof import('./binary') {
  return binaryModule ?? (binaryModule = require('./binary') as typeof import('./binary'));
}

export type { SynxObject, SynxOptions, SynxValue, SynxArray, SynxPrimitive, SynxDiff } from './types';
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
export {
  SynxError,
  utf8Length,
  SYNXL_VERSION,
  MAX_SYNXL_RECORD_BYTES,
  MAX_SYNXL_FIELDS,
  MAX_SYNXL_FIELD_NAME_BYTES,
  MAX_SYNXL_FIELD_LISTS,
  MAX_SYNXL_RECORDS,
};

// ─── SYNXL, as functions of this module ───────────────────
// The same functions as the statics of `Synx` below; each loads the record
// module on its first call.

/**
 * Parse a `.synxl` record-stream document (SYNXL §4–§11).
 * @throws SynxError on a hard error (§11.1) — no partial result is returned.
 */
export function parseSynxl(text: string, options: SynxlOptions = {}): SynxlDocument {
  return synxl().parseSynxl(text, options);
}

/** Streaming SYNXL reader over text or an iterable of lines (§15.1). */
export function streamSynxl(
  source: string | Iterable<string>,
  options: SynxlOptions = {},
): Generator<SynxlRecord, void, undefined> {
  return synxl().streamSynxl(source, options);
}

/** Async streaming SYNXL reader over chunks (§15.1). */
export function streamSynxlAsync(
  source: AsyncIterable<string | Uint8Array>,
  options: SynxlOptions = {},
): AsyncGenerator<SynxlRecord, void, undefined> {
  return synxl().streamSynxlAsync(source, options);
}

/** Canonical JSON array projection of a SYNXL document (§12.1). */
export function synxlToJSON(source: string | SynxlDocument, options: SynxlOptions = {}): string {
  return synxl().synxlToJSON(source, options);
}

/** Canonical NDJSON projection of a SYNXL document (§12.2). */
export function synxlToNDJSON(source: string | SynxlDocument, options: SynxlOptions = {}): string {
  return synxl().synxlToNDJSON(source, options);
}

/** Serialize records back to SYNXL text, with automatic quoting (§14). */
export function writeSynxl(
  input: SynxlDocument | readonly SynxObject[],
  options: SynxlWriteOptions = {},
): string {
  return synxl().writeSynxl(input, options);
}

/** Split a record line into parts on `;` (SYNXL §7.1). */
export function splitRecordLine(line: string): SynxlPart[] {
  return synxl().splitRecordLine(line);
}

// ─── Native binding auto-detection ───────────────────────

// NOTE (3.6.x stability fix): The previous "auto-engine" that switched between
// pure-TS and the native Rust binding at 5 KB caused observably *different*
// trees for the same input depending on file size — a critical surprise.
// We now always use the pure-TS engine in this package; the native binding
// remains available via `bindings/node` for callers who explicitly want it.
// Setting this to Infinity disables the auto-route without removing the code path.
const NATIVE_THRESHOLD = Number.POSITIVE_INFINITY;

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
    text = inputText(text, 'Synx.parse');
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
    if (locked) markLocked(root);
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
    const text = decodeUtf8(fs.readFileSync(absPath), filePath);
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
    const text = decodeUtf8(await fs.promises.readFile(absPath), filePath);
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
    // spec: the round-trip block writer (SYNXL §14.1) — strings that would
    // cast or be cut as a comment are quoted or carried as `|+`, multi-line
    // text keeps its relative indent. The previous writer quoted nothing
    // (`"007"` came back as 7) and printed objects in list items as [object Object].
    out += synxl().writeSynxBody(obj);
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
    if (isLockedRoot(obj)) {
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
    if (isLockedRoot(obj)) {
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
    if (isLockedRoot(obj)) {
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
    return isLockedRoot(obj);
  }

  /**
   * Reformat a .synx string into canonical form:
   * - Exactly 2 spaces per indentation level
   * - Keys sorted alphabetically at every nesting level of a static
   *   document; list items, and the fields inside one, keep their order
   * - An `!active` document keeps its key order — markers resolve in it
   * - One blank line between top-level blocks (objects / lists)
   * - Comments stripped — canonical form is comment-free
   * - Directive lines (`!active`, `!lock`, `!include …`) moved to the top
   *
   * The result always reads back as the same data: it is parsed and compared
   * with the input before it is returned, and a document the formatter cannot
   * reproduce — or one cut by a parser limit — is returned unchanged.
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
    return format().formatSynx(inputText(text, 'Synx.format'));
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
      const text = decodeUtf8(fs.readFileSync(absPath), filePath);
      const config = Synx.parse(text, opts);
      callback(config);
    } catch (e: any) {
      callback({}, e);
    }

    const watcher = fs.watch(absPath, { persistent: true }, (_event) => {
      try {
        const text = decodeUtf8(fs.readFileSync(absPath), filePath);
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
    const { root } = parseData(inputText(text, 'Synx.schema'));
    const metaMap = metaMapOf(root);
    const properties: Record<string, SynxSchemaProperty> = {};
    const required: string[] = [];

    if (metaMap) {
      // In the order an object lists its keys, as this always reported them.
      const byKey: Record<string, SynxMeta> = Object.create(null);
      for (const [key, meta] of metaMap) byKey[key] = meta;
      for (const key of Object.keys(byKey)) {
        const meta = byKey[key];
        if (!meta.constraints) continue;
        const c = meta.constraints;
        const prop: SynxSchemaProperty = {};
        if (c.type) prop.type = schemaTypeName(c.type);
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
  // Not read here any more; 3.8.0 had it on the class, private by type but
  // reachable at run time, so it stays where a caller may have found it.
  private static readonly SYNXB_MAGIC = new Uint8Array([0x53, 0x59, 0x4e, 0x58, 0x42]); // "SYNXB"

  static isSynxb(data: Buffer | Uint8Array): boolean {
    return data instanceof Uint8Array && data.length >= 6
      && data[0] === 0x53 && data[1] === 0x59 && data[2] === 0x4e && data[3] === 0x58 && data[4] === 0x42; // SYNXB
  }

  /**
   * Compile a .synx string into compact binary `.synxb` format.
   *
   * Wire format (v1) — matches `synx-core::binary` for cross-language interop:
   *   header   : "SYNXB" (5) + version (1) + flags (1) + uncompressed_size u32 LE (4) = 11 bytes
   *   payload  : raw-deflate(level=9) of (string_table || value_tree [|| declarations || includes])
   *
   * Without `resolved` the document is written as parsed: an `!active`
   * document keeps its markers, constraints and `!include` lines, and nothing
   * of it is resolved — no variable of the environment is read and no file is
   * touched. With `resolved`, an `!active` document is resolved first (against
   * the environment and the current directory of the process) and its values
   * are written as they came out.
   *
   * @param text     - The .synx source text.
   * @param resolved - If true and text is `!active`, resolve markers first.
   * @returns A Uint8Array containing the `.synxb` binary.
   */
  static compile(text: string, resolved: boolean = false): Uint8Array {
    const parsed = parseData(inputText(text, 'Synx.compile'));
    // safety: only on request. Every compile of an `!active` document used to
    // resolve it — the values of the host's environment went into the file,
    // `:once` wrote `.synx.lock` into the current directory — and the
    // declarations were dropped, so the file could not be resolved again.
    if (resolved && parsed.mode === 'active') {
      resolve(parsed.root, { _includes: parsed.includes } as any);
    }
    return binary().compileSynxb(parsed, resolved === true);
  }

  /**
   * Decompile a `.synxb` binary back into a .synx text string.
   *
   * A file compiled without `resolved` carries its declarations, and they are
   * written back: `port:env:default:8080 PORT`, `name[required] x`,
   * `!include` lines, keys in the order the file holds them. A resolved file
   * is written as the static document it now is — its values are final, and
   * `!active` would have `{braces}` in them interpolated a second time.
   *
   * @param data - Raw `.synxb` bytes.
   * @returns The reconstructed .synx text.
   * @throws SynxError if the data is not valid `.synxb`.
   *
   * @example
   * ```ts
   * const buf = fs.readFileSync('config.synxb');
   * const text = Synx.decompile(buf);
   * console.log(text);
   * ```
   */
  static decompile(data: Buffer | Uint8Array): string {
    const doc = binary().decodeSynxb(data);
    const root = doc.root;
    const isObject = root !== null && typeof root === 'object' && !Array.isArray(root) && !(root instanceof SynxSecret);
    // safety: a document is an object, and the compiler writes no other root.
    // A file whose root was a string came back as that string — text of its
    // author's choosing, `!active` and `!include` lines included, out of a
    // file flagged static; a list or a scalar came back as another document
    // (B-086). As in synx-core, such a root has no text form.
    if (!isObject) throw new SynxError('the root of this .synxb file is not an object; it has no text form');
    // Directives in the order synx-core writes them.
    let out = '';
    if (doc.tool) out += '!tool\n';
    if (doc.schema) out += '!schema\n';
    if (doc.llm) out += '!llm\n';
    if (doc.mode === 'active' && !doc.resolved) out += '!active\n';
    if (doc.locked) out += '!lock\n';
    for (const inc of doc.includes) out += `!include ${inc.path} ${inc.alias}\n`;
    if (out !== '') out += '\n';
    // An unresolved active document goes through the declared writer even
    // without declarations, as in synx-core: the plain one writes an empty
    // list as `key:join`, a marker the next `resolve` would apply (B-082.5).
    if (doc.hasMeta || (doc.mode === 'active' && !doc.resolved)) {
      return out + synxl().writeSynxDeclared(root as SynxObject);
    }
    // Sorted at every level: the order a file without declarations is in.
    return out + synxl().writeSynxBody(root as SynxObject, 2, true);
  }

  /**
   * Parse a `!tool` mode SYNX text, reshaping into `{ tool, params }` format
   * per SYNX 3.6 §9.
   *
   * Call mode (`!tool` without `!schema`):
   *   First top-level key (lexicographically) is the tool name; its children become params.
   *   Output: `{ tool: "name", params: { ... } }`. Empty input → `{ tool: null, params: {} }`.
   *
   * Schema mode (`!tool` + `!schema`):
   *   Each top-level key becomes `{ name, params }` in a sorted `tools` array.
   *   Output: `{ tools: [{ name, params }, ...] }`.
   *
   * @example
   * ```ts
   * Synx.parseTool('!tool\nweb_search\n  query latest Rust');
   * // → { tool: 'web_search', params: { query: 'latest Rust' } }
   * ```
   */
  static parseTool(
    text: string,
    options: SynxOptions = {}
  ): { tool: string | null; params: SynxObject } | { tools: Array<{ name: string; params: SynxObject }> } {
    // Run the parse without auto-engine routing — the reshape semantics are tied
    // to the directive flags, which the pure-TS parser sets.
    const parsed = parseData(inputText(text, 'Synx.parseTool'));
    const root = parsed.root;
    if (parsed.mode === 'active') {
      resolve(root, { ...options, _includes: parsed.includes } as any);
    }

    const keys = Object.keys(root).sort();

    if (parsed.schema) {
      const tools = keys.map(k => {
        const child = root[k];
        const params: SynxObject =
          child && typeof child === 'object' && !Array.isArray(child)
            ? (child as SynxObject)
            : {};
        return { name: k, params };
      });
      return { tools };
    }

    // Call mode
    if (keys.length === 0) {
      return { tool: null, params: {} };
    }
    const toolName = keys[0];
    const child = root[toolName];
    const params: SynxObject =
      child && typeof child === 'object' && !Array.isArray(child)
        ? (child as SynxObject)
        : {};
    return { tool: toolName, params };
  }

  // ─── SYNXL — "SYNX Lines" record stream (SYNXL 1) ───────

  /** SYNXL format version implemented by this package. */
  static readonly SYNXL_VERSION = SYNXL_VERSION;

  /**
   * Parse a `.synxl` record-stream document (SYNXL §4–§11).
   *
   * @throws SynxError on a hard error (§11.1) — no partial result is returned.
   *
   * @example
   * ```ts
   * const doc = Synx.parseSynxl('!synxl 1\n!fields id[type:int] ; name\n1 ; Alice\n');
   * doc.records[0].values; // { id: 1, name: 'Alice' }
   * ```
   */
  static parseSynxl(text: string, options: SynxlOptions = {}): SynxlDocument {
    return synxl().parseSynxl(text, options);
  }

  /** Load and parse a `.synxl` file synchronously. */
  static loadSynxlSync(filePath: string, options: SynxlOptions = {}): SynxlDocument {
    const text = decodeUtf8(fs.readFileSync(path.resolve(filePath)), filePath);
    return synxl().parseSynxl(text, options);
  }

  /** Load and parse a `.synxl` file asynchronously. */
  static async loadSynxl(filePath: string, options: SynxlOptions = {}): Promise<SynxlDocument> {
    const text = decodeUtf8(await fs.promises.readFile(path.resolve(filePath)), filePath);
    return synxl().parseSynxl(text, options);
  }

  /** Streaming SYNXL reader over text or an iterable of lines (§15.1). */
  static streamSynxl(
    source: string | Iterable<string>,
    options: SynxlOptions = {},
  ): Generator<SynxlRecord, void, undefined> {
    return synxl().streamSynxl(source, options);
  }

  /** Async streaming SYNXL reader over chunks (§15.1). */
  static streamSynxlAsync(
    source: AsyncIterable<string | Uint8Array>,
    options: SynxlOptions = {},
  ): AsyncGenerator<SynxlRecord, void, undefined> {
    return synxl().streamSynxlAsync(source, options);
  }

  /** Stream records from a `.synxl` file without materialising it (§15.1). */
  static streamSynxlFile(
    filePath: string,
    options: SynxlOptions = {},
  ): AsyncGenerator<SynxlRecord, void, undefined> {
    // Bytes, not text: the reader decodes them itself and refuses what is not UTF-8.
    const stream = fs.createReadStream(path.resolve(filePath));
    return synxl().streamSynxlAsync(stream as AsyncIterable<Uint8Array>, options);
  }

  /** Canonical JSON array projection of a SYNXL document (§12.1). */
  static synxlToJSON(source: string | SynxlDocument, options: SynxlOptions = {}): string {
    return synxl().synxlToJSON(source, options);
  }

  /** Canonical NDJSON projection of a SYNXL document (§12.2). */
  static synxlToNDJSON(source: string | SynxlDocument, options: SynxlOptions = {}): string {
    return synxl().synxlToNDJSON(source, options);
  }

  /** Serialize records back to SYNXL text, with automatic quoting (§14). */
  static writeSynxl(
    input: SynxlDocument | readonly SynxObject[],
    options: SynxlWriteOptions = {},
  ): string {
    return synxl().writeSynxl(input, options);
  }

  /** Save records to a `.synxl` file synchronously. */
  static saveSynxlSync(
    filePath: string,
    input: SynxlDocument | readonly SynxObject[],
    options: SynxlWriteOptions = {},
  ): void {
    fs.writeFileSync(path.resolve(filePath), synxl().writeSynxl(input, options), 'utf-8');
  }

  /** Save records to a `.synxl` file asynchronously. */
  static async saveSynxl(
    filePath: string,
    input: SynxlDocument | readonly SynxObject[],
    options: SynxlWriteOptions = {},
  ): Promise<void> {
    await fs.promises.writeFile(path.resolve(filePath), synxl().writeSynxl(input, options), 'utf-8');
  }
}

/** JSON Schema's name for a SYNX type; one it does not know is kept as written.
 *  A switch, not a table: `[type:constructor]` read a built-in off the table. */
function schemaTypeName(type: string): string {
  switch (type) {
    case 'int': return 'integer';
    case 'float': return 'number';
    case 'bool': return 'boolean';
    default: return type;
  }
}

// ─── Binary encoding helpers ──────────────────────────────

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

// ─── Export Converters ────────────────────────────────────

function toJSONString(obj: SynxObject, pretty = true): string {
  // Per SYNX 3.6 §10: canonical JSON has object keys sorted lexicographically.
  // The implementation lives in ./json so that the SYNXL projection (§12) uses
  // the very same writer.
  return json().toCanonicalJSONString(obj, pretty);
}

function toYAMLString(value: unknown, indent = 0): string {
  const sp = ' '.repeat(indent);
  if (value === null || value === undefined) return `${sp}null\n`;
  if (typeof value === 'boolean' || typeof value === 'number' || typeof value === 'bigint') return `${sp}${value}\n`;
  if (value instanceof SynxSecret) return `${sp}"[SECRET]"\n`;
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
      if (item && typeof item === 'object' && !Array.isArray(item) && !(item instanceof SynxSecret)) {
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
      if (v && typeof v === 'object' && !(v instanceof SynxSecret)) {
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
  // A 64-bit integer is a YAML integer; `JSON.stringify` throws on a bigint.
  if (typeof v === 'boolean' || typeof v === 'number' || typeof v === 'bigint') return String(v);
  // safety: a secret is written as its mask, never as an empty mapping.
  if (v instanceof SynxSecret) return '"[SECRET]"';
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
    if (Array.isArray(v)) {
      const allObjects = v.length > 0 && v.every(i => i && typeof i === 'object' && !Array.isArray(i));
      if (allObjects) {
        arrays.push([k, v]);
      } else {
        simple.push([k, v]);
      }
    } else if (v && typeof v === 'object' && !(v instanceof SynxSecret)) {
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
  if (typeof v === 'boolean' || typeof v === 'bigint') return String(v);
  if (v instanceof SynxSecret) return '"[SECRET]"';
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
    const envKey = prefix ? `${prefix}_${k}`.toUpperCase() : k.toUpperCase();
    if (v && typeof v === 'object' && !Array.isArray(v) && !(v instanceof SynxSecret)) {
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
// CommonJS consumers get the class itself as `module.exports`. When the source is
// loaded as an ES module (Bun or a bundler reading src/) `module` does not exist,
// and an unguarded assignment threw `ReferenceError` on import.
if (typeof module !== 'undefined' && module.exports) {
  module.exports = Synx;
  module.exports.default = Synx;
  module.exports.Synx = Synx;
  module.exports.SynxError = SynxError;
  // SYNXL: `module.exports` is the Synx class, so every SYNXL static above is
  // already reachable as a named CommonJS export. Only the helpers and limits
  // that have no static counterpart need re-attaching here.
  module.exports.splitRecordLine = splitRecordLine;
  module.exports.utf8Length = utf8Length;
  module.exports.MAX_SYNXL_RECORD_BYTES = MAX_SYNXL_RECORD_BYTES;
  module.exports.MAX_SYNXL_FIELDS = MAX_SYNXL_FIELDS;
  module.exports.MAX_SYNXL_FIELD_NAME_BYTES = MAX_SYNXL_FIELD_NAME_BYTES;
  module.exports.MAX_SYNXL_FIELD_LISTS = MAX_SYNXL_FIELD_LISTS;
  module.exports.MAX_SYNXL_RECORDS = MAX_SYNXL_RECORDS;
}
