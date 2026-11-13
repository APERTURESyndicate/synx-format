/**
 * SYNX Engine — @aperturesyndicate/synx-format
 *
 * Resolves active markers (:random, :calc, :env, :alias, :secret, etc.)
 * in a parsed SYNX object tree. Only runs in !active mode.
 *
 * The marker pipeline is a port of `apply_markers` in synx-core, marker for
 * marker and in the same order, so both give the same value and the same
 * refusal for the same document.
 */

import type { SynxObject, SynxValue, SynxConstraints, SynxMeta, SynxMetaMap, SynxOptions, SynxInclude } from './types';
import {
  parseData, parseI64, metaMapOf, setMetaMap, newMetaMap, keyOrderOf, setKeyOrder, keysOf, isIndexKey,
  trimSpace, isUnicodeSpaceCode, utf8Length, decodeUtf8, finiteDecimal, floatKind, FLOAT_DECIMAL,
  noteIntegralFloat, isIntegralFloat, copyIntegralFloats, scalarLength, expandExponent, compareUnicodeScalar, SynxError,
} from './parser';

// Lazy-loaded Node.js modules (not available in browser)
let fs: typeof import('fs') | undefined;
let pathModule: typeof import('path') | undefined;
try {
  fs = require('fs');
  pathModule = require('path');
} catch {
  // Browser environment — :include will not work
}

// perf: the arithmetic of `:calc` and the matcher of `[pattern]` are loaded by
// the first document that uses them. A service reads its config once, at
// start, and what it pays for is mostly the loading of this package: with
// every module loaded up front that took longer than 3.8.0 did.
let calcModule: typeof import('./calc') | undefined;
let patternModule: typeof import('./pattern') | undefined;

function safeCalc(expr: string): number {
  return (calcModule ?? (calcModule = require('./calc') as typeof import('./calc'))).safeCalc(expr);
}

function matchesLinear(pattern: string, value: string): boolean | null {
  return (patternModule ?? (patternModule = require('./pattern') as typeof import('./pattern'))).matchesLinear(pattern, value);
}

// ─── Limits ───────────────────────────────────────────────

/** Longest `:calc` expression, in bytes, before substitution. */
const MAX_CALC_EXPR_LEN = 4096;
/** Longest `:calc` expression once names are replaced by numbers. */
const MAX_CALC_RESOLVED_LEN = 64 * 1024;
const MAX_FILE_SIZE = 10 * 1024 * 1024; // 10 MB
const DEFAULT_MAX_INCLUDE_DEPTH = 16;
/** Maximum object nesting depth for active-mode resolution (prevents stack overflow). */
const MAX_RESOLVE_DEPTH = 512;
/** Longest text one `{…}` template or one `:calc` word replacement builds. */
const MAX_ENGINE_SCRATCH_STRING = 4 * 1024 * 1024;
/** Most included documents one top-level resolve may read and resolve. */
const MAX_INCLUDE_READS = 1024;
/** Most bytes one top-level resolve may read through `:include`, `!include` and `:watch`. */
const MAX_RESOLVE_READ_BYTES = 64 * 1024 * 1024;
/**
 * Most bytes the markers of one top-level resolve may add to the document:
 * values copied by `:alias`, `:ref`, `:spam` and `:inherit`, and text built by
 * `{interpolation}`, `:replace`, `:join`, `:format` and `:prompt`.
 *
 * safety: each of them can multiply what it reads. 28 lines of
 * `a1 {a0}{a0}` are 413 bytes and asked for a string of 2^28 characters — a
 * `RangeError` out of `Synx.parse`; 1 500 aliases of a 1 500-field group
 * were two million fields. The numbers are those of synx-core.
 */
const MAX_RESOLVE_EXPANSION_BYTES = 64 * 1024 * 1024;
/** What one copied value costs against the budget, text aside. */
const VALUE_COST = 128;
/** The value a marker is left with once the document's budget is spent. */
const LIMIT_ERR = 'LIMIT_ERR: resolution budget exceeded';
/** The value left where a marker would nest objects past MAX_RESOLVE_DEPTH. */
const NESTING_ERR = 'NESTING_ERR: maximum object nesting depth exceeded';

// ─── Path jail ────────────────────────────────────────────

const IS_WINDOWS = typeof process !== 'undefined' && process.platform === 'win32';

/**
 * Ensure a file path stays inside the base directory (path jail).
 *
 * spec: a port of `jail_path` in synx-core, step for step, so both refuse the
 * same path with the same words on every platform.
 */
function jailPath(base: string, filePath: string): string {
  if (!pathModule) throw new Error('path module not available');
  // Always check leading "/" or "\" first so the message is portable: POSIX
  // absolute paths and Windows rooted paths both produce the same string.
  const first = filePath.charCodeAt(0);
  if (first === 0x2f /* slash */ || first === 0x5c /* backslash */) {
    throw new Error(`SECURITY: rooted paths are not allowed: '${filePath}'`);
  }
  if (pathModule.isAbsolute(filePath)) {
    throw new Error(`SECURITY: absolute paths are not allowed: '${filePath}'`);
  }
  const baseDir = base === '' ? '.' : base;
  if (!fs) {
    // No file system to ask (a browser bundle): the lexical rule alone.
    const lexicalBase = pathModule.resolve(baseDir);
    const target = pathModule.resolve(lexicalBase, filePath);
    if (!isInside(target, lexicalBase)) {
      throw new Error(`SECURITY: path escapes base directory: '${filePath}'`);
    }
    return target;
  }
  // safety: a base that cannot be resolved refuses instead of jailing nothing,
  // and both ends are compared as real paths — a symlink or junction inside
  // the base that points out of it passed the old text comparison.
  let realBase: string;
  try {
    realBase = realPath(pathModule.resolve(baseDir));
  } catch {
    throw new Error(`SECURITY: base directory not found: '${baseDir}'`);
  }
  // A drive-relative path (`C:name`) does not join the base: it is read from
  // that drive's current directory, in Rust's `PathBuf::join` as in Windows.
  const driveRelative = IS_WINDOWS && filePath.charCodeAt(1) === 0x3a /* colon */ && isAsciiLetter(first);
  const joined = driveRelative ? pathModule.resolve(filePath) : pathModule.resolve(realBase, filePath);
  let real: string;
  try {
    real = realPath(joined);
  } catch {
    // The file does not exist (yet), so nothing proves where a link in the
    // path would lead: a `..` segment is refused, judged by the path as the
    // document wrote it.
    if (hasParentSegment(filePath)) {
      throw new Error(`SECURITY: path traversal detected: '${filePath}'`);
    }
    if (driveRelative || !isInside(joined, realBase)) {
      throw new Error(`SECURITY: path escapes base directory: '${filePath}'`);
    }
    return joined;
  }
  if (!isInside(real, realBase)) {
    throw new Error(`SECURITY: path escapes base directory: '${filePath}'`);
  }
  return real;
}

function isAsciiLetter(c: number): boolean {
  return (c >= 65 && c <= 90) || (c >= 97 && c <= 122);
}

/** Whether a path has a `..` segment between `/` or `\` separators. */
function hasParentSegment(filePath: string): boolean {
  let start = 0;
  for (let i = 0; i <= filePath.length; i++) {
    const c = i < filePath.length ? filePath.charCodeAt(i) : 0x2f;
    if (c !== 0x2f && c !== 0x5c) continue;
    if (i - start === 2 && filePath.charCodeAt(start) === 0x2e && filePath.charCodeAt(start + 1) === 0x2e) return true;
    start = i + 1;
  }
  return false;
}

/** Whether `target` is `base` or lies under it; both absolute and normalized. */
function isInside(target: string, base: string): boolean {
  if (target === base) return true;
  const sep = pathModule!.sep;
  // A root (`C:\`, `/`) already ends in a separator.
  const prefix = base.charAt(base.length - 1) === sep ? base : base + sep;
  return target.startsWith(prefix);
}

/** The path with every link and junction resolved; throws when it does not exist. */
function realPath(p: string): string {
  const native = fs!.realpathSync.native;
  return typeof native === 'function' ? native(p) : fs!.realpathSync(p);
}

/** Refuse a file larger than MAX_FILE_SIZE. One that cannot be measured is
 *  left for the read to report, as in synx-core. */
function checkFileSize(filePath: string): void {
  if (!fs) return;
  let size: number;
  try {
    size = fs.statSync(filePath).size;
  } catch {
    return;
  }
  if (size > MAX_FILE_SIZE) {
    throw new Error(`SECURITY: file too large (${size} bytes, max ${MAX_FILE_SIZE})`);
  }
}

// ─── State of one top-level resolve ───────────────────────

/** A `.synx.lock` sidecar, as `LockFile` in synx-core. */
interface LockFile {
  path: string;
  /** First value recorded for each key, in file order. */
  values: Map<string, string>;
  /** How many bytes of the file `values` covers. */
  seen: number;
  /** What was read ends inside a line (a file edited by hand). */
  openLine: boolean;
  /** Descriptor open for appending once this resolve records a value; -1 until then. */
  fd: number;
  /** The path is something other than a plain file: neither read nor written. */
  foreign: boolean;
}

/** Most of a `.synx.lock` that is read, as in synx-core. */
const MAX_LOCK_FILE_BYTES = 16 * 1024 * 1024;

// One top-level `Synx.parse` and the resolves it starts for included
// documents share these; the outermost one starts them afresh.
let resolvesInProgress = 0;
/** Files whose resolution is in progress, outermost first. */
const INCLUDE_CHAIN: string[] = [];
let includeReads = 0;
let readBytes = 0;
let expansion = 0;
let writesAllowed = true;
/** Lock files `:once` has touched, by path. */
const LOCKS = new Map<string, LockFile>();

/** Take `bytes` from the expansion budget; `false` once it is spent. */
function chargeExpansion(bytes: number): boolean {
  const total = expansion + bytes;
  if (total <= MAX_RESOLVE_EXPANSION_BYTES) {
    expansion = total;
    return true;
  }
  expansion = MAX_RESOLVE_EXPANSION_BYTES;
  return false;
}

/** Opens `fullPath` for resolution; the caller pops `INCLUDE_CHAIN` when done.
 *  safety: the depth cap alone let a self-including three-line file resolve
 *  2^16 times; a cycle now stops at its first repeat and the budget bounds an
 *  acyclic fan-out (same rule as the Rust engine's `include_enter`). */
function includeEnter(fullPath: string, shown: string): void {
  if (INCLUDE_CHAIN.includes(fullPath)) {
    throw new Error(`circular include: '${shown}'`);
  }
  if (includeReads >= MAX_INCLUDE_READS) {
    throw new Error(`include budget (${MAX_INCLUDE_READS} files) exceeded`);
  }
  includeReads++;
  INCLUDE_CHAIN.push(fullPath);
}

/**
 * A file's text, counted against MAX_RESOLVE_READ_BYTES — `read_budgeted` in
 * synx-core, down to the error for bytes that are not UTF-8.
 *
 * safety: every file is capped at MAX_FILE_SIZE, but a document may name the
 * same file from a thousand keys, and each read is parsed and kept.
 */
function readBudgeted(fullPath: string, shown: string): string {
  let size = 0;
  let isDirectory = false;
  try {
    const stat = fs!.statSync(fullPath);
    size = stat.size;
    isDirectory = stat.isDirectory();
  } catch {
    // The read below reports what is wrong with the file.
  }
  if (readBytes + size > MAX_RESOLVE_READ_BYTES) {
    throw new Error(`read budget (${MAX_RESOLVE_READ_BYTES} bytes) exceeded`);
  }
  // A directory reads as "permission denied" on Windows and "is a directory"
  // elsewhere; it is neither.
  if (isDirectory) throw new Error(`not a file: ${shown}`);
  const bytes = fs!.readFileSync(fullPath);
  let text: string;
  try {
    // A leading BOM stays in the text, as in synx-core.
    text = decodeUtf8(bytes, shown);
  } catch {
    throw new Error(`file is not valid UTF-8: ${shown}`);
  }
  readBytes += bytes.length;
  return text;
}

// ─── Secret wrapper ───────────────────────────────────────

/** The text of each secret. Kept beside the object, so `util.inspect`,
 *  `Object.entries` and a spread have nothing of it to print. */
const SECRET_TEXT = new WeakMap<object, string>();

export class SynxSecret {
  constructor(value: string) {
    SECRET_TEXT.set(this, value);
  }

  toString(): string {
    return '[SECRET]';
  }

  toJSON(): string {
    return '[SECRET]';
  }

  /** Call this explicitly to get the real value */
  reveal(): string {
    return SECRET_TEXT.get(this) as string;
  }

  [Symbol.for('nodejs.util.inspect.custom')](): string {
    return '[SECRET]';
  }

  [Symbol.toPrimitive](hint: string): string | number {
    if (hint === 'number') return NaN;
    return '[SECRET]';
  }
}

// Brand under a global symbol so the runtime-agnostic canonical JSON writer
// (json.ts) can recognise a secret and emit `"[SECRET]"` without importing
// this module (which would pull the whole engine, and fs/path, into it).
// perf: on the prototype — it was defined on every instance.
Object.defineProperty(SynxSecret.prototype, Symbol.for('synx.secret'), { value: true, enumerable: false });

/**
 * The text of a failed read, as synx-core's `fmt_io_err` writes it.
 *
 * spec: never the operating system's own wording, so the same document
 * resolves to the same string on every machine. A name the platform cannot
 * hold at all (a NUL, or `|` on Windows) is a file that is not there.
 *
 * safety: Node's message was passed through for anything unexpected, and it
 * carries the absolute path of the host — `:include` of a name with a NUL in
 * it put the server's directory into the parsed document.
 */
function fmtIoErr(err: any, ctx: string): string {
  const code = err && err.code;
  // The engine's own refusals (jail, size, cycle, budget) carry no code.
  if (code === undefined || code === null) return err && err.message ? err.message : String(err);
  switch (code) {
    case 'ENOENT':
    case 'EINVAL':
    case 'ERR_INVALID_ARG_VALUE':
    case 'ERR_INVALID_ARG_TYPE':
      return `file not found: ${ctx}`;
    case 'EACCES':
    case 'EPERM':
      return `permission denied: ${ctx}`;
    default:
      return `cannot read file: ${ctx}`;
  }
}

// ─── Marker classification ────────────────────────────────

const MK_ALIAS = 1 << 0;
const MK_CALC = 1 << 1;
const MK_CLAMP = 1 << 2;
const MK_DEFAULT = 1 << 3;
const MK_ENV = 1 << 4;
const MK_FALLBACK = 1 << 5;
const MK_FORMAT = 1 << 6;
const MK_GEO = 1 << 7;
const MK_I18N = 1 << 8;
const MK_IMPORT = 1 << 9;
const MK_INCLUDE = 1 << 10;
const MK_JOIN = 1 << 11;
const MK_MAP = 1 << 12;
const MK_ONCE = 1 << 13;
const MK_PROMPT = 1 << 14;
const MK_RANDOM = 1 << 15;
const MK_REF = 1 << 16;
const MK_REPLACE = 1 << 17;
const MK_ROUND = 1 << 18;
const MK_SECRET = 1 << 19;
const MK_SORT = 1 << 20;
const MK_SPAM = 1 << 21;
const MK_SPLIT = 1 << 22;
const MK_SUM = 1 << 23;
const MK_UNIQUE = 1 << 24;
const MK_VERSION = 1 << 25;
const MK_WATCH = 1 << 26;
/** Set once a chain has been classified, so 0 can mean "not yet". */
const MK_KNOWN = 1 << 30;

/** Classify a marker chain into `MK_*` bits; arguments and unknown names add nothing. */
function markerBits(markers: string[]): number {
  let bits = MK_KNOWN;
  for (let i = 0; i < markers.length; i++) {
    switch (markers[i]) {
      case 'alias': bits |= MK_ALIAS; break;
      case 'calc': bits |= MK_CALC; break;
      case 'clamp': bits |= MK_CLAMP; break;
      case 'default': bits |= MK_DEFAULT; break;
      case 'env': bits |= MK_ENV; break;
      case 'fallback': bits |= MK_FALLBACK; break;
      case 'format': bits |= MK_FORMAT; break;
      case 'geo': bits |= MK_GEO; break;
      case 'i18n': bits |= MK_I18N; break;
      case 'import': bits |= MK_IMPORT; break;
      case 'include': bits |= MK_INCLUDE; break;
      case 'join': bits |= MK_JOIN; break;
      case 'map': bits |= MK_MAP; break;
      case 'once': bits |= MK_ONCE; break;
      case 'prompt': bits |= MK_PROMPT; break;
      case 'random': bits |= MK_RANDOM; break;
      case 'ref': bits |= MK_REF; break;
      case 'replace': bits |= MK_REPLACE; break;
      case 'round': bits |= MK_ROUND; break;
      case 'secret': bits |= MK_SECRET; break;
      case 'sort': bits |= MK_SORT; break;
      case 'spam': bits |= MK_SPAM; break;
      case 'split': bits |= MK_SPLIT; break;
      case 'sum': bits |= MK_SUM; break;
      case 'unique': bits |= MK_UNIQUE; break;
      case 'version': bits |= MK_VERSION; break;
      case 'watch': bits |= MK_WATCH; break;
    }
  }
  return bits;
}

/** The bits of a declaration's chain, classified once. */
function bitsOf(meta: SynxMeta): number {
  return meta.bits || (meta.bits = markerBits(meta.markers));
}

// ─── Resolution ───────────────────────────────────────────

/** What the markers of one document resolve against. */
interface Scope {
  root: SynxObject;
  options: SynxOptions;
  basePath: string;
  includeDepth: number;
  maxIncludeDepth: number;
  /** Documents of `!include` lines and root `:include` keys, by alias. */
  includes: Map<string, SynxObject>;
  /** Root `:include` keys the pre-pass already resolved. */
  preresolved: Map<string, { path: string; root: SynxObject }> | null;
  /** What the keys under `:secret` held before the pre-pass masked them. */
  originals: Map<SynxObject, Map<string, SynxValue>>;
  /** Root keys whole-word `:calc` substitution has to try (see `isCalcSpecialKey`). */
  rootSpecial: string[] | null;
  /** The same for the object being resolved at each depth, found on its first `:calc`. */
  specialOf: (SynxObject | undefined)[];
  special: string[][];
  /** Position of every key of an object in document order — built on first need. */
  positions: Map<object, Map<string, number>> | null;
}

/**
 * Resolve the markers of an `!active` document in place.
 *
 * Options beyond the public ones, used by the package itself: `_includes`
 * (the document's `!include` lines) and `_includeDepth`.
 */
export function resolve(obj: SynxObject, options: SynxOptions = {}): SynxObject {
  // The outermost resolve starts the budgets afresh and, on the way out,
  // writes the lock files `:once` changed — once, not once per key.
  if (resolvesInProgress === 0) {
    INCLUDE_CHAIN.length = 0;
    includeReads = 0;
    readBytes = 0;
    expansion = 0;
    LOCKS.clear();
    writesAllowed = options.readOnly !== true;
  }
  resolvesInProgress++;
  try {
    resolveDocument(obj, options);
  } finally {
    resolvesInProgress--;
    if (resolvesInProgress === 0) flushLocks();
  }
  return obj;
}

function resolveDocument(root: SynxObject, options: SynxOptions): void {
  const includeDepth: number = (options as any)._includeDepth ?? 0;
  const scope: Scope = {
    root,
    options,
    // The current directory by name, not by path: a refusal that quotes the
    // base must not carry the host's directory into the document.
    basePath: options.basePath || '.',
    includeDepth,
    maxIncludeDepth: options.maxIncludeDepth ?? DEFAULT_MAX_INCLUDE_DEPTH,
    includes: new Map<string, SynxObject>(),
    preresolved: null,
    originals: new Map<SynxObject, Map<string, SynxValue>>(),
    rootSpecial: null,
    specialOf: [],
    special: [],
    positions: null,
  };

  // ── Load !include directives ──
  const directives = (options as any)._includes as SynxInclude[] | undefined;
  if (directives) loadIncludes(directives, scope);

  // ── Pre-pass: root `:include` / `:import` keys are aliases too ──
  // So `{site_name:db}` finds the document of `db:include ./common.synx`
  // wherever in the file it stands; the marker then takes this result.
  const rootMeta = metaMapOf(root);
  if (rootMeta && fs && pathModule && includeDepth < scope.maxIncludeDepth && hasIncludeKey(rootMeta)) {
    for (const key of keysOf(root)) {
      const meta = rootMeta.get(key);
      if (meta === undefined || (bitsOf(meta) & (MK_INCLUDE | MK_IMPORT)) === 0) continue;
      const file = root[key];
      if (typeof file !== 'string') continue;
      let doc: SynxObject;
      try {
        const fullPath = jailPath(scope.basePath, file);
        checkFileSize(fullPath);
        includeEnter(fullPath, file);
        try {
          doc = resolveIncluded(readBudgeted(fullPath, file), fullPath, scope);
        } finally {
          INCLUDE_CHAIN.pop();
        }
      } catch {
        continue; // The marker reports it on the key's own turn.
      }
      (scope.preresolved ??= new Map()).set(key, { path: file, root: doc });
      if (!scope.includes.has(key)) scope.includes.set(key, doc);
    }
  }

  // ── Type and constraint registries ──
  // spec: built from the document as it stands, before inheritance, each
  // object's own declarations and then its children.
  // perf: one walk fills both registries (it was a walk each).
  const typeRegistry = new Map<string, string>();
  const constraintRegistry = new Map<string, SynxConstraints>();
  collectRegistries(root, typeRegistry, constraintRegistry, 0);

  // ── :inherit pre-pass (root level) ──
  applyInheritance(root);
  // Remove private blocks (keys starting with _)
  let removed = false;
  for (const k of Object.keys(root)) {
    if (k.charCodeAt(0) === 95) {
      delete root[k];
      removed = true;
    }
  }
  if (removed) {
    const order = keyOrderOf(root);
    if (order) setKeyOrder(root, order.filter(k => k.charCodeAt(0) !== 95));
  }

  // ── :secret pre-pass ──
  // safety: every value under `:secret` is masked *before* any resolution,
  // so no field that names it — `{that_key}`, `:alias`, `:map` — can read
  // the plain text, wherever in the document it stands. The value comes
  // back for the key's own turn (its subtree and its marker chain), and
  // what that produces is masked again.
  maskSecretLiterals(root, scope.originals, 0);

  resolveObject(scope, root, 0, '');

  // spec: the reference engine then checks every field against the merged
  // `[...]` constraints declared for its name anywhere in the document
  // (`validate_field_constraints`), and finally `(type)` hints against the
  // resolved value.
  if (constraintRegistry.size > 0 || typeRegistry.size > 0) {
    validateFields(root, constraintRegistry, typeRegistry, 0);
  }
}

/** Whether any declaration of an object carries `:include` or `:import`. */
function hasIncludeKey(metaMap: SynxMetaMap): boolean {
  for (const meta of metaMap.values()) {
    if ((bitsOf(meta) & (MK_INCLUDE | MK_IMPORT)) !== 0) return true;
  }
  return false;
}

/**
 * One pass in document order: each key's subtree, then the key's own
 * markers, then the next key — so a marker sees every key before it fully
 * resolved, at any depth. Then `{…}` in every string of the object.
 */
function resolveObject(scope: Scope, obj: SynxObject, depth: number, path: string): void {
  // Guard: prevent stack overflow from deeply nested objects
  if (depth >= MAX_RESOLVE_DEPTH) {
    for (const k of Object.keys(obj)) obj[k] = NESTING_ERR;
    return;
  }

  const metaMap = metaMapOf(obj);
  const originals = scope.originals.size > 0 ? scope.originals.get(obj) : undefined;
  if (originals) scope.originals.delete(obj);
  // spec: document order. An object lists its integer-like keys first, so
  // `x:calc 2 + 3` / `1:calc x * 2` resolved `1` before `x` existed.
  const keys = keysOf(obj);

  for (let i = 0; i < keys.length; i++) {
    const key = keys[i];
    let value = obj[key];
    let mask: SynxValue = null;
    if (originals !== undefined && originals.has(key)) {
      mask = value;
      value = originals.get(key) as SynxValue;
      obj[key] = value;
      // Secrets nested in it stay masked until their own turn.
      if (value !== null && typeof value === 'object') maskSecretSubtree(value, scope.originals, depth + 1);
    }

    if (value !== null && typeof value === 'object') {
      if (Array.isArray(value)) {
        let childPath = '';
        for (let j = 0; j < value.length; j++) {
          const item = value[j];
          if (item !== null && typeof item === 'object' && !Array.isArray(item) && !(item instanceof SynxSecret)) {
            if (childPath === '') childPath = path ? `${path}.${key}` : key;
            resolveObject(scope, item as SynxObject, depth + 1, childPath);
          }
        }
      } else if (!(value instanceof SynxSecret)) {
        resolveObject(scope, value as SynxObject, depth + 1, path ? `${path}.${key}` : key);
      }
    }

    if (metaMap === undefined) continue;
    const meta = metaMap.get(key);
    if (meta !== undefined) applyMarkers(scope, obj, key, meta, depth, path, mask);
  }

  // ── Auto-{} interpolation (separate pass, runs on ALL string values) ──
  for (let i = 0; i < keys.length; i++) {
    const text = obj[keys[i]];
    if (typeof text !== 'string' || text.indexOf('{') === -1) continue;
    // safety: the text may grow by what is left of the budget and no further.
    const before = utf8Length(text);
    const result = resolveInterpolation(text, scope.root, obj, scope.includes, before + (MAX_RESOLVE_EXPANSION_BYTES - expansion));
    if (result === null) {
      expansion = MAX_RESOLVE_EXPANSION_BYTES;
      obj[keys[i]] = LIMIT_ERR;
    } else if (result !== text) {
      const grown = utf8Length(result) - before;
      if (grown > 0) chargeExpansion(grown);
      obj[keys[i]] = result;
    }
  }
}

/**
 * Apply the markers of one key, in the order of synx-core's `apply_markers`.
 * A marker that leaves an error of its own (`SPAM_ERR`, a refused include, a
 * `:calc` too long) ends the key there: nothing after it runs, not even the
 * key's constraints.
 */
function applyMarkers(
  scope: Scope, obj: SynxObject, key: string, meta: SynxMeta, depth: number, path: string, mask: SynxValue,
): void {
  const markers = meta.markers;
  const mb = bitsOf(meta);

  if ((mb & MK_SPAM) !== 0 && markSpam(scope, obj, key, markers, mb, depth)) return;
  if ((mb & (MK_INCLUDE | MK_IMPORT)) !== 0) {
    markInclude(scope, obj, key, mb, depth);
    // safety: the key ends here, so `:secret` is applied here too. A value
    // that was not a path — a list, a number — came back from the pre-pass
    // and left in the clear (B-017.8).
    if ((mb & MK_SECRET) !== 0) obj[key] = maskSecretValue(obj[key]);
    return;
  }
  if ((mb & MK_ENV) !== 0) markEnv(scope, obj, key, meta, mb);
  if ((mb & MK_RANDOM) !== 0 && markRandom(obj, key, meta)) return;
  if ((mb & MK_REF) !== 0) markRef(scope, obj, key, markers, mb, depth);
  if ((mb & MK_I18N) !== 0) markI18n(scope, obj, key, markers);
  if ((mb & MK_CALC) !== 0 && markCalc(scope, obj, key, depth)) return;
  if ((mb & MK_ALIAS) !== 0) markAlias(scope, obj, key, depth, path);
  if ((mb & MK_UNIQUE) !== 0) markUnique(obj, key);
  if ((mb & MK_GEO) !== 0) markGeo(scope, obj, key);
  if ((mb & MK_SPLIT) !== 0) markSplit(obj, key, markers);
  if ((mb & MK_JOIN) !== 0) markJoin(obj, key, markers);
  if ((mb & MK_DEFAULT) !== 0 && (mb & MK_ENV) === 0) markDefault(obj, key, meta, mb);
  if ((mb & MK_CLAMP) !== 0) markClamp(obj, key, markers);
  if ((mb & MK_ROUND) !== 0) markRound(obj, key, markers);
  if ((mb & MK_MAP) !== 0) markMap(scope, obj, key, markers);
  if ((mb & MK_FORMAT) !== 0) markFormat(obj, key, markers);
  if ((mb & MK_REPLACE) !== 0) markReplace(obj, key, markers);
  if ((mb & MK_SORT) !== 0) markSort(obj, key, markers);
  if ((mb & MK_SUM) !== 0) markSum(obj, key);
  if ((mb & MK_FALLBACK) !== 0) markFallback(scope, obj, key, markers);
  if ((mb & MK_ONCE) !== 0) markOnce(scope, obj, key, markers);
  if ((mb & MK_VERSION) !== 0) markVersion(obj, key, markers);
  if ((mb & MK_WATCH) !== 0 && markWatch(scope, obj, key, markers, depth)) return;

  // ── :secret ──
  // Last of the markers that produce the value, so whatever they resolved is
  // what gets masked. It used to sit after `:alias`: `t:secret:once` came out
  // as plain text, `:secret:default:x` masked the argument instead of the
  // result, and `:secret:format` formatted the mask.
  if ((mb & MK_SECRET) !== 0) {
    const produced = obj[key];
    // perf: a secret nothing changed is the one the pre-pass made.
    obj[key] = typeof produced === 'string' && mask instanceof SynxSecret && mask.reveal() === produced && !isRuntimeError(produced)
      ? mask
      : maskSecretValue(produced);
  }

  if ((mb & MK_PROMPT) !== 0) markPrompt(obj, key, markers);

  // ── :vision / :audio ──
  // Metadata-only markers. Recognized (no error), value passes through.

  // ── Constraint validation (always last, after all markers resolved) ──
  if (meta.constraints) validateConstraints(obj, key, meta.constraints);
}

// ─── Markers ──────────────────────────────────────────────

/** `:spam:MAX_CALLS[:WINDOW_SEC] target`. `true` when the key ends in an error. */
function markSpam(scope: Scope, obj: SynxObject, key: string, markers: string[], mb: number, depth: number): boolean {
  const idx = markers.indexOf('spam');
  const maxCalls = parseUnsigned(markers[idx + 1]) ?? 0;
  const windowSec = parseUnsigned(markers[idx + 2]) ?? 1;
  if (maxCalls === 0) {
    obj[key] = 'SPAM_ERR: invalid limit, use :spam:MAX[:WINDOW_SEC]';
    return true;
  }
  const target = valueToString(obj[key]);
  if (!allowSpamAccess(`${key}::${target}`, maxCalls, windowSec)) {
    // The numbers as written: one past 2^53 is not the number it was.
    const calls = unsignedDigits(markers[idx + 1]);
    const seconds = parseUnsigned(markers[idx + 2]) === null ? '1' : unsignedDigits(markers[idx + 2]);
    // safety: the error names the target, and under `:secret` the target is
    // the secret — the key's own turn has it in the clear (B-088).
    obj[key] = `SPAM_ERR: '${(mb & MK_SECRET) !== 0 ? '[SECRET]' : target}' exceeded ${calls} calls per ${seconds}s`;
    return true;
  }
  const found = lookup(scope.root, obj, target);
  if (found !== undefined) placeCopy(obj, key, found, depth);
  return false;
}

/** `:include` / `:import` — the document of another file in place of its path. */
function markInclude(scope: Scope, obj: SynxObject, key: string, mb: number, depth: number): void {
  const file = obj[key];
  if (typeof file !== 'string') return;
  if (!fs || !pathModule) {
    obj[key] = 'INCLUDE_ERR: :include/:import is not supported in browser';
    return;
  }
  if (scope.includeDepth >= scope.maxIncludeDepth) {
    obj[key] = `INCLUDE_ERR: max include depth (${scope.maxIncludeDepth}) exceeded`;
    return;
  }
  const secret = (mb & MK_SECRET) !== 0;
  if (obj === scope.root && scope.preresolved !== null) {
    // perf: the pre-pass has resolved this file already. Each level used to
    // resolve it twice, so a chain of includes cost 2^depth resolutions.
    const pre = scope.preresolved.get(key);
    if (pre !== undefined && pre.path === file) {
      obj[key] = placedDocument(cloneValue(pre.root), depth, secret);
      return;
    }
  }
  try {
    const fullPath = jailPath(scope.basePath, file);
    checkFileSize(fullPath);
    includeEnter(fullPath, file);
    let included: SynxObject;
    try {
      included = resolveIncluded(readBudgeted(fullPath, file), fullPath, scope);
    } finally {
      INCLUDE_CHAIN.pop();
    }
    obj[key] = placedDocument(included, depth, secret);
  } catch (e: any) {
    obj[key] = `INCLUDE_ERR: ${fmtIoErr(e, file)}`;
  }
}

/**
 * `:env` — only when the value is the variable's name. A key without a value
 * is a group and keeps its `{}`, as in synx-core.
 */
function markEnv(scope: Scope, obj: SynxObject, key: string, meta: SynxMeta, mb: number): void {
  const name = obj[key];
  if (typeof name !== 'string') return;
  const source = scope.options.env || (typeof process !== 'undefined' ? process.env : undefined);
  const text = source ? envText((source as Record<string, unknown>)[name]) : undefined;
  const markers = meta.markers;
  const asString = wantsString(meta);
  const secret = (mb & MK_SECRET) !== 0;
  if (text !== undefined && text !== '') {
    placeExternal(obj, key, text, asString, secret, markers);
    return;
  }
  const di = markers.indexOf('default');
  // :env:default:VALUE — join all parts after 'default' back with ':'
  // to preserve IPs (0.0.0.0) and compound values
  if (di !== -1 && markers.length > di + 1) placeExternal(obj, key, markers.slice(di + 1).join(':'), asString, secret, markers);
  else obj[key] = null;
}

/** `:random` — one item of the list. `true` when the list is empty. */
function markRandom(obj: SynxObject, key: string, meta: SynxMeta): boolean {
  const arr = obj[key];
  if (!Array.isArray(arr)) return false;
  if (arr.length === 0) {
    obj[key] = null;
    return true;
  }
  obj[key] = meta.args && meta.args.length > 0
    ? weightedRandom(arr, meta.args.map(Number))
    : arr[Math.floor(Math.random() * arr.length)];
  // An item of a list carries no note of its own.
  noteIntegralFloat(obj, key, false);
  return false;
}

/**
 * `:ref` — like `:alias`, but the value feeds the markers after it.
 * `key:ref:calc:*2 base_rate` resolves `base_rate`, then applies `VALUE * 2`.
 */
function markRef(scope: Scope, obj: SynxObject, key: string, markers: string[], mb: number, depth: number): void {
  const target = obj[key];
  if (typeof target !== 'string') return;
  const found = lookup(scope.root, obj, target);
  const resolved: SynxValue = found === undefined ? null : placeCopy(obj, key, found, depth);
  obj[key] = resolved;
  if ((mb & MK_CALC) === 0 || !isNum(resolved)) return;
  const tail = markers[markers.indexOf('calc') + 1];
  if (tail === undefined || tail === '') return;
  const first = tail.charCodeAt(0);
  // Shorthand: :ref:calc:*2 → VALUE * 2, :ref:calc:+10 → VALUE + 10
  if (first !== 43 && first !== 45 && first !== 42 && first !== 47 && first !== 37) return;
  try {
    obj[key] = calcValue(safeCalc(`${numberToText(Number(resolved))} ${tail}`));
    noteIntegralFloat(obj, key, false);
  } catch (e: any) {
    // spec: an error, as in synx-core — the value used to stay as resolved,
    // so `:ref:calc:/0` passed for a number.
    obj[key] = `CALC_ERR: ${e.message}`;
  }
}

/**
 * `:i18n` — the entry for `options.lang`, then `en`, then the first one.
 * With a count field (`title:i18n:item_count`) the entry holds plural forms.
 */
function markI18n(scope: Scope, obj: SynxObject, key: string, markers: string[]): void {
  const translations = obj[key];
  if (!isPlainObject(translations)) return;
  const table = translations as SynxObject;
  const lang = scope.options.lang || 'en';
  const picked = pick(table, lang, 'en');
  const countField = markers[markers.indexOf('i18n') + 1];
  if (countField === undefined || !isPlainObject(picked)) {
    obj[key] = picked;
    noteIntegralFloat(obj, key, typeof picked === 'number' && isIntegralFloat(table, pickedKey));
    return;
  }
  // spec: the count is a number of this object, else of the root, else 0 —
  // the order and the rule of synx-core. The root was asked first here, and a
  // numeric string counted.
  let count = 0;
  const local = ownValue(obj, countField);
  if (isNum(local)) count = Number(local);
  else {
    const global = deepGet(scope.root, countField);
    if (isNum(global)) count = Number(global);
  }
  count = Number.isNaN(count) ? 0 : Math.trunc(count);
  const chosen = pick(picked as SynxObject, pluralCategory(lang, count), 'other');
  obj[key] = typeof chosen === 'string' ? chosen.split('{count}').join(integerText(count)) : chosen;
  noteIntegralFloat(obj, key, typeof chosen === 'number' && isIntegralFloat(picked as SynxObject, pickedKey));
}

/** The key of the value the last `pick` returned. */
let pickedKey = '';

/** The value of `first`, else of `second`, else the first value of the object, else null. */
function pick(obj: SynxObject, first: string, second: string): SynxValue {
  if (hasOwn.call(obj, first)) return obj[pickedKey = first];
  if (hasOwn.call(obj, second)) return obj[pickedKey = second];
  const keys = keysOf(obj);
  if (keys.length === 0) return null;
  return obj[pickedKey = keys[0]];
}

/** `:calc` — arithmetic over the numbers of this object and of the root. `true` on an error that ends the key. */
function markCalc(scope: Scope, obj: SynxObject, key: string, depth: number): boolean {
  const expr = obj[key];
  if (typeof expr !== 'string') return false;
  const bytes = expr.length * 3 <= MAX_CALC_EXPR_LEN ? 0 : utf8Length(expr);
  if (bytes > MAX_CALC_EXPR_LEN) {
    obj[key] = `CALC_ERR: expression too long (${bytes} chars, max ${MAX_CALC_EXPR_LEN})`;
    return true;
  }
  // Already-resolved numeric variables: a key of this object shadows a
  // root key. Keys later in document order that still hold a marker value
  // (an unresolved :env string) are not numbers yet and stay as written.
  let resolved = substituteCalcVars(expr, obj, key, scope, depth);
  // Substitute dot-path references (e.g. base.hp, server.port) before evaluating.
  if (resolved !== null) resolved = replaceDotPaths(resolved, scope.root);
  if (resolved === null) {
    obj[key] = `CALC_ERR: resolved expression too long (max ${MAX_CALC_RESOLVED_LEN} bytes)`;
    return true;
  }
  try {
    obj[key] = calcValue(safeCalc(resolved));
    noteIntegralFloat(obj, key, false);
  } catch (e: any) {
    obj[key] = `CALC_ERR: ${e.message}`;
  }
  return false;
}

/**
 * The result of arithmetic as a value, the way synx-core types it: a whole
 * number within i64 is an integer — so `-0` is `0`, and one past 2^53 is the
 * exact `bigint` the parser gives such an integer — and anything else a float.
 */
function calcValue(n: number): number | bigint {
  if (n === 0) return 0;
  if (Number.isSafeInteger(n) || !Number.isInteger(n)) return n;
  return Math.abs(n) < 9223372036854775808 ? BigInt(n) : n;
}

/** `:alias` — a copy of another key's value. */
function markAlias(scope: Scope, obj: SynxObject, key: string, depth: number, path: string): void {
  const target = obj[key];
  if (typeof target !== 'string') return;
  // The full dot-path of this key; a list item adds nothing to it.
  const currentPath = path ? `${path}.${key}` : key;
  // Detect direct self-reference (bare key or full dot-path)
  if (target === key || target === currentPath) {
    obj[key] = `ALIAS_ERR: self-referential alias: ${currentPath} → ${target}`;
    return;
  }
  // Detect one-hop cycle: a → b → a. Only when the target key ALSO has an
  // :alias marker: a plain string that happens to spell this key is a value.
  const targetVal = deepGet(scope.root, target);
  const targetIsFloat = typeof targetVal === 'number' && foundIsFloat();
  if (typeof targetVal === 'string' && (targetVal === key || targetVal === currentPath)) {
    const lastDot = target.lastIndexOf('.');
    const parent = lastDot >= 0 ? deepGet(scope.root, target.substring(0, lastDot)) : scope.root;
    const parentMeta = parent !== null && typeof parent === 'object' ? metaMapOf(parent as object) : undefined;
    const targetMeta = parentMeta?.get(lastDot >= 0 ? target.substring(lastDot + 1) : target);
    if (targetMeta !== undefined && (bitsOf(targetMeta) & MK_ALIAS) !== 0) {
      // Stable, order-independent message: the participants in sorted order,
      // so both keys produce the same string.
      const inOrder = compareUnicodeScalar(currentPath, target) <= 0;
      obj[key] = `ALIAS_ERR: circular alias detected: ${inOrder ? currentPath : target} → ${inOrder ? target : currentPath}`;
      return;
    }
  }
  // safety: a copy. The value itself was placed here, so an alias of an
  // ancestor made the tree a cycle (`Synx.toJSON` overflowed the stack) and
  // two aliases of one group shared it.
  obj[key] = targetVal === undefined ? null : budgetedCopy(targetVal, depth);
  noteIntegralFloat(obj, key, targetIsFloat);
}

function markUnique(obj: SynxObject, key: string): void {
  const arr = obj[key];
  if (!Array.isArray(arr)) return;
  const seen = new Set<string>();
  const unique: SynxValue[] = [];
  for (const item of arr) {
    const text = valueToString(item);
    if (!seen.has(text)) {
      seen.add(text);
      unique.push(item);
    }
  }
  obj[key] = unique;
}

function markGeo(scope: Scope, obj: SynxObject, key: string): void {
  const arr = obj[key];
  if (!Array.isArray(arr)) return;
  const prefix = (scope.options.region || 'US') + ' ';
  for (const item of arr) {
    if (typeof item === 'string' && item.startsWith(prefix)) {
      obj[key] = trimSpace(item.substring(prefix.length));
      return;
    }
  }
  // Fallback to first entry
  const first = arr.length > 0 ? arr[0] : null;
  const space = typeof first === 'string' ? first.indexOf(' ') : -1;
  obj[key] = space === -1 ? first : trimSpace((first as string).substring(space + 1));
}

function markSplit(obj: SynxObject, key: string, markers: string[]): void {
  const text = obj[key];
  if (typeof text !== 'string') return;
  const idx = markers.indexOf('split');
  const sep = idx + 1 < markers.length ? delimiterFromKeyword(markers[idx + 1]) : ',';
  const out: SynxValue[] = [];
  // An empty delimiter splits into characters, not into halves of them.
  for (const part of sep === '' ? Array.from(text) : text.split(sep)) {
    const piece = trimSpace(part);
    if (piece !== '') out.push(castPrimitive(piece));
  }
  obj[key] = out;
}

function markJoin(obj: SynxObject, key: string, markers: string[]): void {
  const arr = obj[key];
  if (!Array.isArray(arr)) return;
  const idx = markers.indexOf('join');
  const sep = idx + 1 < markers.length ? delimiterFromKeyword(markers[idx + 1]) : ',';
  // safety: the separator is as long as the document likes and is written
  // once per item.
  const separators = utf8Length(sep) * Math.max(0, arr.length - 1);
  if (separators > arr.length && !chargeExpansion(separators)) {
    obj[key] = LIMIT_ERR;
    return;
  }
  const parts = new Array<string>(arr.length);
  for (let i = 0; i < arr.length; i++) parts[i] = valueToString(arr[i]);
  obj[key] = parts.join(sep);
}

/** `:default` on its own (with `:env` it is that marker's fallback). */
function markDefault(obj: SynxObject, key: string, meta: SynxMeta, mb: number): void {
  const current = obj[key];
  if (current !== null && current !== undefined && current !== '') return;
  const markers = meta.markers;
  const di = markers.indexOf('default');
  if (markers.length > di + 1) {
    placeExternal(obj, key, markers.slice(di + 1).join(':'), wantsString(meta), (mb & MK_SECRET) !== 0, markers);
  }
}

/** `key:clamp:MIN:MAX value` */
function markClamp(obj: SynxObject, key: string, markers: string[]): void {
  const idx = markers.indexOf('clamp');
  // A bound is a finite number in the synx-core grammar: `parseFloat`
  // read `5abc` as 5 and `Infinity` as a bound, where Rust skips both.
  const lo = finiteDecimal(markers[idx + 1] ?? '');
  const hi = finiteDecimal(markers[idx + 2] ?? '');
  if (lo === null || hi === null) return;
  if (lo > hi) {
    obj[key] = `CONSTRAINT_ERR: clamp min (${numberToText(lo)}) > max (${numberToText(hi)})`;
  } else if (isNum(obj[key])) {
    // A whole number within i64 comes out an integer, whatever went in.
    obj[key] = calcValue(Math.min(hi, Math.max(lo, Number(obj[key]))));
    noteIntegralFloat(obj, key, false);
  }
}

/** `key:round:N value` (N = decimal places, default 0) */
function markRound(obj: SynxObject, key: string, markers: string[]): void {
  // spec: the count is an unsigned integer or 0, and a half rounds away
  // from zero, as synx-core's `f64::round`. `parseInt` read `2abc` as 2 and
  // `-1` as a negative count; `Math.round` took -2.5 to -2.
  const decimals = parseUnsigned(markers[markers.indexOf('round') + 1], U32_MAX) ?? 0;
  const current = obj[key];
  if (!isNum(current)) return;
  const n = Number(current);
  // A scale past the range of a double rounds nothing: the number stays.
  const factor = Math.pow(10, Math.min(decimals, 400));
  const scaled = n * factor;
  const rounded = Number.isFinite(scaled) ? (scaled < 0 ? -Math.round(-scaled) : Math.round(scaled)) / factor : n;
  // Only a count of 0 gives an integer; with decimals the result is a float,
  // `:round:2` of 5 included.
  obj[key] = decimals === 0 ? calcValue(rounded) : rounded;
  noteIntegralFloat(obj, key, decimals !== 0 && Number.isInteger(rounded));
}

/** `key:map:source_key` over a list of `lookup result` lines. */
function markMap(scope: Scope, obj: SynxObject, key: string, markers: string[]): void {
  const arr = obj[key];
  if (!Array.isArray(arr)) return;
  const source = markers[markers.indexOf('map') + 1] ?? '';
  let wanted = '';
  if (source !== '') {
    const found = lookup(scope.root, obj, source);
    if (found !== undefined) wanted = valueToString(found);
  }
  for (const item of arr) {
    if (typeof item !== 'string') continue;
    const space = item.indexOf(' ');
    if (space !== -1 && trimSpace(item.substring(0, space)) === wanted) {
      const text = trimSpace(item.substring(space + 1));
      const value = castPrimitive(text);
      obj[key] = value;
      noteIntegralFloat(obj, key, isWholeFloat(value, text));
      return;
    }
  }
  obj[key] = null;
}

/** `key:format:PATTERN value` (e.g. %.2f, %05d) */
function markFormat(obj: SynxObject, key: string, markers: string[]): void {
  const pattern = markers[markers.indexOf('format') + 1] ?? '%s';
  const current = obj[key];
  const formatted = applyFormatPattern(pattern, current);
  // safety: a width of 4096 on a twenty-byte line is a 200-fold growth.
  obj[key] = isNum(current) && !chargeExpansion(utf8Length(formatted)) ? LIMIT_ERR : formatted;
}

/**
 * `:replace:FROM:TO` — literal substring replacement; `TO` defaults to "".
 * Neither can hold a `:`, the chain is colon-delimited.
 */
function markReplace(obj: SynxObject, key: string, markers: string[]): void {
  const text = obj[key];
  if (typeof text !== 'string') return;
  const idx = markers.indexOf('replace');
  const from = markers[idx + 1];
  const to = markers[idx + 2] ?? '';
  if (from === undefined || from === '') return;
  // safety: `:replace:a:<long>` on a run of `a` multiplies the two lengths;
  // the growth is known before a character is written.
  const fromBytes = utf8Length(from);
  const toBytes = utf8Length(to);
  if (toBytes > fromBytes) {
    let count = 0;
    for (let at = text.indexOf(from); at !== -1; at = text.indexOf(from, at + from.length)) count++;
    if (count > 0 && !chargeExpansion(count * (toBytes - fromBytes))) {
      obj[key] = LIMIT_ERR;
      return;
    }
  }
  obj[key] = text.split(from).join(to);
}

/**
 * `:sort` / `:sort:desc` — a total order: numbers first, by value, then
 * everything else by its text, by code point.
 */
function markSort(obj: SynxObject, key: string, markers: string[]): void {
  const arr = obj[key];
  if (!Array.isArray(arr)) return;
  const sorted = arr.slice().sort((a, b) => {
    const an = isNum(a);
    const bn = isNum(b);
    if (an && bn) {
      const x = Number(a);
      const y = Number(b);
      if (x < y) return -1;
      if (x > y) return 1;
      if (x === y) return 0;
      return (Number.isNaN(x) ? 1 : 0) - (Number.isNaN(y) ? 1 : 0);
    }
    if (an) return -1;
    if (bn) return 1;
    return compareUnicodeScalar(valueToString(a), valueToString(b));
  });
  if (markers[markers.indexOf('sort') + 1] === 'desc') sorted.reverse();
  obj[key] = sorted;
}

/** `:sum` — the numbers of a list; text that is a finite decimal number counts. */
function markSum(obj: SynxObject, key: string): void {
  const arr = obj[key];
  if (!Array.isArray(arr)) return;
  let total = 0;
  // An integer only when no summand has a fraction, as in synx-core.
  let allWhole = true;
  for (const v of arr) {
    let n: number;
    if (isNum(v)) n = Number(v);
    else if (typeof v === 'string') {
      // spec: the number grammar of the format. `Number()` also read `0x10`,
      // `0b11` and text padded with spaces.
      const parsed = finiteDecimal(v);
      if (parsed === null) continue;
      n = parsed;
    } else continue;
    total += n;
    // NaN and the infinities have no fraction to speak of.
    if (Number.isFinite(n) && !Number.isInteger(n)) allWhole = false;
  }
  obj[key] = allWhole ? calcValue(total) : total;
  // 1.5 + 2.5 is the float 4.
  noteIntegralFloat(obj, key, !allWhole && Number.isInteger(total));
}

/** `key:fallback:DEFAULT value` — the default when the value is empty or names a file that is not there. */
function markFallback(scope: Scope, obj: SynxObject, key: string, markers: string[]): void {
  const fallback = markers[markers.indexOf('fallback') + 1] ?? '';
  const current = obj[key];
  let missing = current === null || current === undefined || current === '';
  if (!missing && typeof current === 'string' && fs && pathModule) {
    try {
      missing = !fs.existsSync(jailPath(scope.basePath, current));
    } catch {
      missing = true; // path escapes jail → treat as missing → use fallback
    }
  }
  if (missing && fallback !== '') obj[key] = fallback;
}

/**
 * `key:once[:uuid|:random|:timestamp]` — a value generated once and kept in
 * `.synx.lock` beside the document.
 */
function markOnce(scope: Scope, obj: SynxObject, key: string, markers: string[]): void {
  const kind = markers[markers.indexOf('once') + 1] ?? 'uuid';
  const lockPath = pathModule && scope.options.basePath
    ? pathModule.resolve(scope.options.basePath, '.synx.lock')
    : '.synx.lock';
  obj[key] = lockedValue(lockPath, key, kind);
}

/** `key:version:OP:REQUIRED value` — compares versions, gives a boolean. */
function markVersion(obj: SynxObject, key: string, markers: string[]): void {
  const current = obj[key];
  if (typeof current !== 'string') return;
  const idx = markers.indexOf('version');
  obj[key] = compareVersions(current, markers[idx + 1] ?? '>=', markers[idx + 2] ?? '');
}

/** `key:watch[:KEY_PATH] ./file` — the file, or one value of it, read at parse time. `true` on a refusal. */
function markWatch(scope: Scope, obj: SynxObject, key: string, markers: string[], depth: number): boolean {
  const file = obj[key];
  if (typeof file !== 'string') return false;
  if (!fs || !pathModule) {
    obj[key] = 'WATCH_ERR: not supported in browser';
    return false;
  }
  if (scope.includeDepth >= scope.maxIncludeDepth) {
    obj[key] = `WATCH_ERR: max include depth (${scope.maxIncludeDepth}) exceeded`;
    return true;
  }
  let fullPath: string;
  try {
    fullPath = jailPath(scope.basePath, file);
    checkFileSize(fullPath);
  } catch (e: any) {
    obj[key] = `WATCH_ERR: ${fmtIoErr(e, file)}`;
    return true;
  }
  try {
    const content = readBudgeted(fullPath, file);
    const keyPath = markers[markers.indexOf('watch') + 1];
    const value = keyPath !== undefined
      ? extractFromFileContent(content, keyPath, pathModule.extname(fullPath).slice(1))
      : trimSpace(content);
    obj[key] = placedDocument(value, depth, false);
    noteIntegralFloat(obj, key, keyPath !== undefined && typeof value === 'number' && foundIsFloat());
  } catch (e: any) {
    obj[key] = `WATCH_ERR: ${fmtIoErr(e, file)}`;
  }
  return false;
}

/** `:prompt[:LABEL]` — the value as SYNX text in a labelled code fence. */
function markPrompt(obj: SynxObject, key: string, markers: string[]): void {
  const label = markers[markers.indexOf('prompt') + 1] ?? key;
  const block = `${label} (SYNX):\n\`\`\`synx\n${stringifyValue(obj[key], 0)}\`\`\``;
  obj[key] = chargeExpansion(utf8Length(block)) ? block : LIMIT_ERR;
}

// ─── Copies and budgets ───────────────────────────────────

/** A plain copy of a value: no declarations, no sharing with the original. */
function cloneValue<T extends SynxValue>(v: T): T {
  if (v === null || typeof v !== 'object' || v instanceof SynxSecret) return v;
  if (Array.isArray(v)) {
    const out = new Array<SynxValue>(v.length);
    for (let i = 0; i < v.length; i++) out[i] = cloneValue(v[i]);
    return out as T;
  }
  const src = v as SynxObject;
  const out: SynxObject = {};
  const keys = Object.keys(src);
  for (let i = 0; i < keys.length; i++) setOwn(out, keys[i], cloneValue(src[keys[i]]));
  const order = keyOrderOf(src);
  if (order) setKeyOrder(out, order.slice());
  copyIntegralFloats(src, out);
  return out as T;
}

/** Store under any key as own data — `__proto__` is a key like the rest here. */
function setOwn(obj: SynxObject, key: string, value: SynxValue): void {
  if (key === '__proto__') {
    Object.defineProperty(obj, key, { value, writable: true, enumerable: true, configurable: true });
  } else {
    obj[key] = value;
  }
}

// What copying the last measured value costs, and how deep it nests.
let costBytes = 0;
let costDepth = 0;
/** Budget left to the measure in progress. */
let costRoom = 0;

/** Measure a value into `costBytes` / `costDepth` — `value_cost` in synx-core. */
function measure(v: SynxValue): void {
  measureWithin(v, Number.MAX_SAFE_INTEGER);
}

/**
 * `measure` for a copy that has `room` bytes of budget left: `false` as soon
 * as the cost passes `room`, without measuring the rest — `value_cost_within`
 * in synx-core.
 *
 * perf: the measure was taken in full before the budget was asked. Once the
 * budget was spent every further `:alias` of a large group still walked all
 * of it to learn it would not fit: 12 000 aliases of a 12 000-field group took
 * 17 s to answer LIMIT_ERR 11 960 times (B-030.7).
 */
function measureWithin(v: SynxValue, room: number): boolean {
  costRoom = room;
  const depth = measureInto(v);
  if (depth < 0) return false;
  costBytes = room - costRoom;
  costDepth = depth;
  return true;
}

/** UTF-8 bytes of `s` taken from `costRoom`; `false` when they do not fit. */
function takeText(s: string): boolean {
  // A UTF-16 unit is at least one byte: a longer text cannot fit, unmeasured.
  if (s.length > costRoom) return false;
  const bytes = utf8Length(s);
  if (bytes > costRoom) return false;
  costRoom -= bytes;
  return true;
}

/** The nesting of `v`, its cost taken from `costRoom`; -1 when the room ran out. */
function measureInto(v: SynxValue): number {
  if (costRoom < VALUE_COST) return -1;
  costRoom -= VALUE_COST;
  if (typeof v === 'string') return takeText(v) ? 0 : -1;
  if (v === null || typeof v !== 'object') return 0;
  if (v instanceof SynxSecret) return takeText(v.reveal()) ? 0 : -1;
  let depth = 0;
  if (Array.isArray(v)) {
    for (let i = 0; i < v.length; i++) {
      const d = measureInto(v[i]);
      if (d < 0) return -1;
      if (d > depth) depth = d;
    }
  } else {
    const src = v as SynxObject;
    const keys = Object.keys(src);
    for (let i = 0; i < keys.length; i++) {
      if (!takeText(keys[i])) return -1;
      const d = measureInto(src[keys[i]]);
      if (d < 0) return -1;
      if (d > depth) depth = d;
    }
  }
  return depth + 1;
}

/**
 * A copy of `value` for a marker to place in an object `depth` levels below
 * the root, or the error it leaves instead: the copy is charged to the
 * document's budget and may not nest past MAX_RESOLVE_DEPTH.
 */
function budgetedCopy(value: SynxValue, depth: number): SynxValue {
  if (value === null || typeof value !== 'object') {
    // A scalar costs its text and nests nothing.
    return chargeExpansion(VALUE_COST + (typeof value === 'string' ? utf8Length(value) : 0)) ? value : LIMIT_ERR;
  }
  // spec: the budget is asked first, as in synx-core. A value that fits
  // neither the budget nor the depth is a LIMIT_ERR, and after it every copy
  // is.
  if (!measureWithin(value, MAX_RESOLVE_EXPANSION_BYTES - expansion)) {
    expansion = MAX_RESOLVE_EXPANSION_BYTES;
    return LIMIT_ERR;
  }
  if (depth + costDepth >= MAX_RESOLVE_DEPTH) return NESTING_ERR;
  if (!chargeExpansion(costBytes)) return LIMIT_ERR;
  return cloneValue(value);
}

/**
 * `obj[key]` becomes the copy of a value `lookup` just found, and the copy is
 * returned. A whole number that was read as a float stays one (E19).
 */
function placeCopy(obj: SynxObject, key: string, found: SynxValue, depth: number): SynxValue {
  const isFloat = typeof found === 'number' && foundIsFloat();
  const copy = budgetedCopy(found, depth);
  obj[key] = copy;
  noteIntegralFloat(obj, key, isFloat && typeof copy === 'number');
  return copy;
}

/**
 * A value read from a file, ready for an object `depth` levels below the
 * root: refused when it would nest past MAX_RESOLVE_DEPTH, and under
 * `:secret` masked leaf by leaf.
 */
function placedDocument(value: SynxValue, depth: number, secret: boolean): SynxValue {
  if (value !== null && typeof value === 'object') {
    measure(value);
    if (depth + costDepth >= MAX_RESOLVE_DEPTH) return NESTING_ERR;
  }
  return secret ? maskSecretValue(value) : value;
}

// ─── Secrets ──────────────────────────────────────────────

/**
 * Make a resolved value a secret. A scalar becomes a `SynxSecret` of its
 * text; a list or a group keeps its shape and every scalar in it is masked.
 * An absent value (null) stays null — wrapping it made the secret the text
 * "null", which `[required]` read as a present value — an existing secret is
 * kept, and an engine error stays readable: `:secret:watch missing.txt` is a
 * failure to report, not a value to hide.
 */
function maskSecretValue(v: SynxValue | undefined): SynxValue {
  if (v === null || v === undefined) return null;
  if (typeof v === 'object') {
    if (v instanceof SynxSecret) return v;
    if (Array.isArray(v)) {
      for (let i = 0; i < v.length; i++) v[i] = maskSecretValue(v[i]);
    } else {
      const o = v as SynxObject;
      for (const k of Object.keys(o)) o[k] = maskSecretValue(o[k]);
    }
    return v;
  }
  if (typeof v === 'string' && isRuntimeError(v)) return v;
  return new SynxSecret(valueToString(v)) as unknown as SynxValue;
}

/**
 * Mask every value under `:secret` before resolution, keeping what it held
 * in `originals` for the key's own turn (see `resolveObject`).
 *
 * safety: a value was pre-masked only when its chain had none of sixteen
 * "resolving" markers, so `password:secret:default:x hunter2` stayed plain
 * text until its turn — and `leak:alias password` on the line above read it.
 * A group under `:secret` was not masked at all. Now nothing waits in the
 * clear.
 */
function maskSecretLiterals(obj: SynxObject, originals: Map<SynxObject, Map<string, SynxValue>>, depth: number): void {
  if (depth > MAX_RESOLVE_DEPTH) return;
  const metaMap = metaMapOf(obj);
  if (metaMap !== undefined) {
    for (const [key, meta] of metaMap) {
      if ((bitsOf(meta) & MK_SECRET) === 0 || !hasOwn.call(obj, key)) continue;
      const v = obj[key];
      if (v === null || v === undefined || v instanceof SynxSecret) continue;
      let kept = originals.get(obj);
      if (kept === undefined) {
        kept = new Map<string, SynxValue>();
        originals.set(obj, kept);
      }
      kept.set(key, v);
      obj[key] = typeof v === 'string' && (bitsOf(meta) & MK_ENV) !== 0 ? variableNameMask(v) : maskSecretValue(cloneValue(v));
    }
  }
  for (const k of Object.keys(obj)) {
    const child = obj[k];
    if (child !== null && typeof child === 'object') maskSecretSubtree(child, originals, depth + 1);
  }
}

/** Masks of variable names, by name; emptied when it has grown to `MAX_NAME_MASKS`. */
const NAME_MASKS = new Map<string, SynxSecret>();
const MAX_NAME_MASKS = 512;

/**
 * The mask of a `:secret:env` value before its turn: the name of the variable.
 *
 * perf: such a mask is seen only by a key that names the secret ahead of it,
 * yet it was built for every secret of every parse. A name is not a secret,
 * so one mask per name is kept — a literal secret never comes here.
 */
function variableNameMask(name: string): SynxValue {
  let mask = NAME_MASKS.get(name);
  if (mask === undefined) {
    if (NAME_MASKS.size >= MAX_NAME_MASKS) NAME_MASKS.clear();
    mask = new SynxSecret(name);
    NAME_MASKS.set(name, mask);
  }
  return mask as unknown as SynxValue;
}

/** `maskSecretLiterals` for a group or for the groups of a list. */
function maskSecretSubtree(child: SynxValue, originals: Map<SynxObject, Map<string, SynxValue>>, depth: number): void {
  if (Array.isArray(child)) {
    for (const item of child) {
      if (isPlainObject(item)) maskSecretLiterals(item as SynxObject, originals, depth);
    }
  } else if (isPlainObject(child)) {
    maskSecretLiterals(child as SynxObject, originals, depth);
  }
}

// ─── Strict mode ──────────────────────────────────────────

/**
 * Throw a `SynxError` for the first value that is an error the engine left in
 * place of a result (`strict` option).
 *
 * spec: `TYPE_ERR:` and `LIMIT_ERR:` were missing from the list, so a field
 * declared `(int)` holding text passed a strict parse; the CLI's
 * `validate --strict` counted it.
 */
export function assertNoRuntimeErrors(value: unknown): void {
  if (typeof value === 'string') {
    if (isRuntimeError(value)) throw new SynxError(value);
    return;
  }
  if (Array.isArray(value)) {
    for (let i = 0; i < value.length; i++) assertNoRuntimeErrors(value[i]);
    return;
  }
  if (value !== null && typeof value === 'object') {
    const obj = value as Record<string, unknown>;
    for (const k of Object.keys(obj)) assertNoRuntimeErrors(obj[k]);
  }
}

/** Whether `s` is an error the engine left in place of a value. */
function isRuntimeError(s: string): boolean {
  // Every prefix is capitals, `_ERR:`: one cheap test before the list.
  const c = s.charCodeAt(0);
  if (c < 65 || c > 90) return false;
  return s.startsWith('CONSTRAINT_ERR:') || s.startsWith('TYPE_ERR:') || s.startsWith('INCLUDE_ERR:')
    || s.startsWith('WATCH_ERR:') || s.startsWith('CALC_ERR:') || s.startsWith('SPAM_ERR:')
    || s.startsWith('ALIAS_ERR:') || s.startsWith('NESTING_ERR:') || s.startsWith('LIMIT_ERR:')
    || s.startsWith('WASM_ERR:');
}

// ─── Inheritance pre-pass ─────────────────────────────────

/**
 * `production:inherit:base:extra` (or `production:inherit base`): the fields
 * of the parents, left to right, then the child's own. Root level only.
 */
function applyInheritance(root: SynxObject): void {
  const rootMeta = metaMapOf(root);
  if (!rootMeta) return;
  // perf: the keys of each parent, listed once. Every child listed them anew:
  // 16 000 children of a 16 000-field parent built 256 million array slots to
  // learn, 15 970 times, that the budget was spent (B-031.4).
  let parentKeys: Map<SynxObject, string[]> | null = null;
  // Root groups marked `:secret`, as the document declares them: what is
  // inherited from them stays secret.
  let secretGroups: Set<string> | null = null;
  for (const [name, meta] of rootMeta) {
    if ((bitsOf(meta) & MK_SECRET) !== 0) (secretGroups ?? (secretGroups = new Set<string>())).add(name);
  }

  for (const [key, meta] of rootMeta) {
    const idx = meta.markers.indexOf('inherit');
    if (idx === -1) continue;
    // Parents are every marker after `inherit`, else what the parser
    // promoted from the value.
    let parents = meta.markers.slice(idx + 1);
    if (parents.length === 0 && meta.args && meta.args.length > 0) parents = meta.args;
    if (parents.length === 0) continue;

    // spec: only an object inherits. A list or a scalar under `:inherit`
    // is left as written.
    const child = ownValue(root, key);
    if (!isPlainObject(child)) continue;

    // Each field keeps the declarations of the object its value came from: a
    // field the child redefines as a plain literal no longer runs the
    // parent's markers on that literal.
    //
    // safety: every inherited field is a copy, charged to the document's
    // budget — 3 000 children of a 3 000-field parent are nine million fields
    // from a 130 KB file.
    const merged: SynxObject = {};
    const order: string[] = [];
    // For each field: the parent it came from (`null` — the child itself)
    // and the declaration it keeps.
    const source = new Map<string, string | null>();
    const declared = new Map<string, SynxMeta | undefined>();
    // Fields whose whole value was read as a float, in the object they come from.
    const floats = new Set<string>();
    let indexed = false;
    let withinBudget = true;
    parents: for (const parentName of parents) {
      const parent = ownValue(root, parentName);
      if (!isPlainObject(parent)) continue;
      const parentObj = parent as SynxObject;
      const parentMeta = metaMapOf(parentObj);
      let fields = parentKeys === null ? undefined : parentKeys.get(parentObj);
      if (fields === undefined) {
        fields = keysOf(parentObj);
        (parentKeys ?? (parentKeys = new Map<SynxObject, string[]>())).set(parentObj, fields);
      }
      for (const k of fields) {
        // Measured only as far as the budget reaches.
        if (!measureWithin(parentObj[k], MAX_RESOLVE_EXPANSION_BYTES - expansion) || !chargeExpansion(costBytes + utf8Length(k))) {
          expansion = MAX_RESOLVE_EXPANSION_BYTES;
          withinBudget = false;
          break parents;
        }
        if (!source.has(k)) {
          order.push(k);
          if (!indexed && isIndexKey(k)) indexed = true;
        }
        // A copy, not the parent's own subtree: two children of one parent
        // shared it, so it was resolved twice and edits to one showed in both.
        setOwn(merged, k, cloneTree(parentObj[k]));
        source.set(k, parentName);
        declared.set(k, parentMeta?.get(k));
        if (isIntegralFloat(parentObj, k)) floats.add(k); else floats.delete(k);
      }
    }
    if (!withinBudget) {
      root[key] = LIMIT_ERR;
      continue;
    }
    const childObj = child as SynxObject;
    const childMeta = metaMapOf(childObj);
    for (const k of keysOf(childObj)) {
      if (!source.has(k)) {
        order.push(k);
        if (!indexed && isIndexKey(k)) indexed = true;
      }
      setOwn(merged, k, childObj[k]);
      source.set(k, null);
      declared.set(k, childMeta?.get(k));
      if (isIntegralFloat(childObj, k)) floats.add(k); else floats.delete(k);
    }
    // The declarations come with the copy and count against the budget as
    // they do in synx-core, field by field: the field's own, the `:secret` it
    // takes from its group, then those nested in its subtree, each filed again
    // under the child's path.
    const mergedMeta = newMetaMap();
    for (const k of order) {
      const parentName = source.get(k);
      let m = declared.get(k);
      if (parentName === null || parentName === undefined) {
        if (m) mergedMeta.set(k, m);
        continue;
      }
      if (m !== undefined && !chargeExpansion(metaCost(k, m))) {
        withinBudget = false;
        break;
      }
      // safety: a group under `:secret` is secret in every field, and a copy
      // of a field is that field. The copies are taken before the group is
      // masked and carried no mark of their own, so `child:inherit base`
      // printed everything `base:secret` hid (B-017.6). An inherited field
      // now says `:secret` itself — first in its chain, where it cannot be
      // read as the argument of another marker.
      if (secretGroups !== null && secretGroups.has(parentName) && (m === undefined || (bitsOf(m) & MK_SECRET) === 0)) {
        if (!chargeExpansion(2 * VALUE_COST + utf8Length(k))) {
          withinBudget = false;
          break;
        }
        m = {
          markers: m === undefined ? ['secret'] : ['secret', ...m.markers],
          args: m === undefined ? undefined : m.args,
          constraints: m === undefined ? undefined : m.constraints,
          typeHint: m === undefined ? undefined : m.typeHint,
          bits: 0,
        };
      }
      if (m) mergedMeta.set(k, m);
      const original = (root[parentName] as SynxObject)[k];
      const cost = nestedDeclarationCost(original, segmentBytes(parentName) + 1 + segmentBytes(k));
      if (cost > 0 && !chargeExpansion(cost)) {
        withinBudget = false;
        break;
      }
    }
    if (!withinBudget) {
      root[key] = LIMIT_ERR;
      continue;
    }
    if (mergedMeta.size > 0) setMetaMap(merged, mergedMeta);
    if (indexed) setKeyOrder(merged, order);
    for (const k of floats) noteIntegralFloat(merged, k, true);
    root[key] = merged;
  }
}

/** UTF-8 bytes of a key as one segment of a declarations path: `.` and `\` are escaped. */
function segmentBytes(key: string): number {
  let bytes = utf8Length(key);
  for (let i = 0; i < key.length; i++) {
    const c = key.charCodeAt(i);
    if (c === 46 || c === 92) bytes++;
  }
  return bytes;
}

/** What copying one key's declarations costs against the budget — `meta_cost` in synx-core. */
function metaCost(key: string, meta: SynxMeta): number {
  let cost = 2 * VALUE_COST + utf8Length(key);
  for (const s of meta.markers) cost += 24 + utf8Length(s);
  if (meta.args) for (const s of meta.args) cost += 24 + utf8Length(s);
  if (meta.typeHint !== undefined) cost += utf8Length(meta.typeHint);
  const c = meta.constraints;
  if (c) {
    if (c.type !== undefined) cost += 24 + utf8Length(c.type);
    if (c.pattern !== undefined) cost += 24 + utf8Length(c.pattern);
    if (c.enum) for (const s of c.enum) cost += 24 + utf8Length(s);
  }
  return cost;
}

/**
 * What the declarations inside `value` cost the expansion budget when
 * `:inherit` copies it — `meta_map_cost` of synx-core plus the path each map
 * is filed under, `pathBytes` long at `value` itself.
 */
function nestedDeclarationCost(value: SynxValue, pathBytes: number): number {
  if (value === null || typeof value !== 'object' || value instanceof SynxSecret) return 0;
  let cost = 0;
  if (Array.isArray(value)) {
    for (let i = 0; i < value.length; i++) {
      const item = value[i];
      // A list item is filed under `.#<index>`.
      if (isPlainObject(item)) cost += nestedDeclarationCost(item, pathBytes + 2 + String(i).length);
    }
    return cost;
  }
  const obj = value as SynxObject;
  const metaMap = metaMapOf(obj);
  if (metaMap !== undefined && metaMap.size > 0) {
    cost += VALUE_COST + pathBytes;
    for (const [k, meta] of metaMap) cost += metaCost(k, meta);
  }
  for (const k of Object.keys(obj)) {
    const child = obj[k];
    if (child !== null && typeof child === 'object') cost += nestedDeclarationCost(child, pathBytes + 1 + segmentBytes(k));
  }
  return cost;
}

/** Deep copy of a parsed subtree, its declarations included. */
function cloneTree(v: SynxValue): SynxValue {
  if (v === null || typeof v !== 'object' || v instanceof SynxSecret) return v;
  if (Array.isArray(v)) return v.map(cloneTree);
  const src = v as SynxObject;
  const out: SynxObject = {};
  for (const k of Object.keys(src)) setOwn(out, k, cloneTree(src[k]));
  const meta = metaMapOf(src);
  if (meta) setMetaMap(out, meta);
  const order = keyOrderOf(src);
  if (order) setKeyOrder(out, order.slice());
  copyIntegralFloats(src, out);
  return out;
}

// ─── Auto-{} interpolation ───────────────────────────────

/**
 * Resolve `{key}`, `{key.nested}`, `{key:alias}`, `{key:include}` in a
 * template — a port of `resolve_interpolation` in synx-core.
 *
 * A reference is `{`, a name of letters, digits, `_` and `.`, then `}` or
 * `:scope}`. The name is read first, so a `{` that opens nothing costs only
 * the characters up to the next one, and the `}` that closes a scope is
 * searched for once.
 *
 * `null` when the text would pass `cap` bytes.
 */
function resolveInterpolation(
  tpl: string,
  root: SynxObject,
  local: SynxObject,
  includes: Map<string, SynxObject>,
  cap: number,
): string | null {
  const len = tpl.length;
  let out = '';
  let outBytes = 0;
  let i = 0;
  // The first `}` after the last scope looked at, while it is still ahead.
  let closeCache = -1;
  let noMoreCloses = false;

  while (i < len && outBytes < MAX_ENGINE_SCRATCH_STRING) {
    if (tpl.charCodeAt(i) !== 123) {
      // Literal text up to the next `{`.
      let end = tpl.indexOf('{', i);
      if (end === -1) end = len;
      const run = takeBytes(tpl.substring(i, end), MAX_ENGINE_SCRATCH_STRING - outBytes);
      out += run;
      outBytes += takenBytes;
      i = end;
      continue;
    }

    const nameStart = i + 1;
    let nameEnd = nameStart;
    while (nameEnd < len) {
      const c = tpl.charCodeAt(nameEnd);
      if ((c >= 48 && c <= 57) || (c >= 65 && c <= 90) || (c >= 97 && c <= 122) || c === 95 || c === 46) nameEnd++;
      else if (c > 127 && isAlphanumericAt(tpl, nameEnd)) nameEnd += c >= 0xd800 && c <= 0xdbff ? 2 : 1;
      else break;
    }
    let close = -1;
    const after = tpl.charCodeAt(nameEnd);
    if (after === 125) close = nameEnd;
    else if (after === 58 && !noMoreCloses) {
      if (closeCache <= nameEnd) {
        closeCache = tpl.indexOf('}', nameEnd + 1);
        noMoreCloses = closeCache === -1;
      }
      close = closeCache;
    }
    if (close === -1) {
      // Not a reference: the brace is text.
      out += '{';
      outBytes++;
      i++;
      continue;
    }

    const name = tpl.substring(nameStart, nameEnd);
    let found: SynxValue | undefined;
    if (nameEnd < close) {
      const scopeName = tpl.substring(nameEnd + 1, close);
      if (scopeName === 'include') {
        // {key:include} — the one and only include
        if (includes.size === 1) found = deepGet(includes.values().next().value as SynxObject, name);
      } else {
        // {key:alias} — look up by alias
        const doc = includes.get(scopeName);
        if (doc !== undefined) found = deepGet(doc, name);
      }
    } else {
      // {key} — the root, then this object
      found = lookup(root, local, name);
    }
    const piece = found !== undefined ? valueToString(found) : tpl.substring(i, close + 1);
    out += takeBytes(piece, MAX_ENGINE_SCRATCH_STRING - outBytes);
    outBytes += takenBytes;
    if (outBytes > cap) return null;
    i = close + 1;
  }
  return outBytes > cap ? null : out;
}

/** UTF-8 bytes of the text the last `takeBytes` returned. */
let takenBytes = 0;

/** As much of `s` as fits in `room` bytes of UTF-8, whole characters only. */
function takeBytes(s: string, room: number): string {
  if (s.length * 3 <= room) {
    takenBytes = utf8Length(s);
    return s;
  }
  let bytes = 0;
  let i = 0;
  while (i < s.length) {
    const c = s.charCodeAt(i);
    let size = c < 0x80 ? 1 : c < 0x800 ? 2 : 3;
    let step = 1;
    if (c >= 0xd800 && c <= 0xdbff && i + 1 < s.length) {
      const lo = s.charCodeAt(i + 1);
      if (lo >= 0xdc00 && lo <= 0xdfff) { size = 4; step = 2; }
    }
    if (bytes + size > room) break;
    bytes += size;
    i += step;
  }
  takenBytes = bytes;
  return i === s.length ? s : s.substring(0, i);
}

let ALPHANUMERIC: RegExp | undefined;

/** Whether the character at `i` (beyond ASCII) is a letter or a digit, as `char::is_alphanumeric`. */
function isAlphanumericAt(s: string, i: number): boolean {
  // One character against one class: nothing here can backtrack.
  return (ALPHANUMERIC ??= /^[\p{Alphabetic}\p{N}]$/u).test(String.fromCodePoint(s.codePointAt(i) as number));
}

// ─── !include loader ──────────────────────────────────────

/** How a document is read on behalf of an active one (see `floats`). */
const INCLUDED = { floats: true };

function loadIncludes(directives: SynxInclude[], scope: Scope): void {
  if (!fs || !pathModule) return;
  if (scope.includeDepth >= scope.maxIncludeDepth) return;
  for (const inc of directives) {
    try {
      const fullPath = jailPath(scope.basePath, inc.path);
      checkFileSize(fullPath);
      includeEnter(fullPath, inc.path);
      try {
        scope.includes.set(inc.alias, resolveIncluded(readBudgeted(fullPath, inc.path), fullPath, scope));
      } finally {
        INCLUDE_CHAIN.pop();
      }
    } catch {
      // Include loading failed — skip silently
    }
  }
}

/**
 * Parse an included document and, when it is `!active`, resolve it as the
 * document it is: against its own root, with its own `!include` lines, private
 * keys removed and its fields validated.
 *
 * spec: it was resolved as a subtree of the including document — names in its
 * `:calc` and `{…}` were looked up in the parent's root, its `_private` keys
 * stayed, and its own `!include` lines were never loaded. synx-core resolves
 * it standalone.
 */
function resolveIncluded(text: string, fullPath: string, scope: Scope): SynxObject {
  // Its fields are checked against the types of the document that includes it.
  const parsed = parseData(text, INCLUDED);
  if (parsed.mode === 'active') {
    resolve(parsed.root, {
      ...scope.options,
      basePath: pathModule!.dirname(fullPath),
      _includeDepth: scope.includeDepth + 1,
      _includes: parsed.includes,
    } as any);
  }
  return parsed.root;
}

// ─── Global type and constraint validation (mirrors synx-core collect_registries / validate_fields) ───

/** The stricter of two declarations for one field name, as `merge_constraints` in synx-core. */
function mergeConstraints(base: SynxConstraints, incoming: SynxConstraints): SynxConstraints {
  const out: SynxConstraints = { ...base };
  if (incoming.required) out.required = true;
  if (incoming.readonly) out.readonly = true;
  if (incoming.min !== undefined) out.min = base.min === undefined ? incoming.min : Math.max(base.min, incoming.min);
  if (incoming.max !== undefined) out.max = base.max === undefined ? incoming.max : Math.min(base.max, incoming.max);
  if (out.type === undefined) out.type = incoming.type;
  if (out.pattern === undefined) out.pattern = incoming.pattern;
  if (out.enum === undefined) out.enum = incoming.enum;
  return out;
}

/** Both registries in one walk: each object's own declarations, then its children. */
function collectRegistries(
  obj: SynxObject,
  types: Map<string, string>,
  constraints: Map<string, SynxConstraints>,
  depth: number,
): void {
  if (depth > MAX_RESOLVE_DEPTH) return;
  const metaMap = metaMapOf(obj);
  if (metaMap !== undefined) {
    for (const [key, meta] of metaMap) {
      // The first declaration of a name wins, an empty `()` included: it is
      // a type no value fails.
      if (meta.typeHint !== undefined && !types.has(key)) types.set(key, meta.typeHint);
      const c = meta.constraints;
      if (c) {
        // perf: the declaration itself until a second one for the same name
        // has to be merged with it — it was copied for every field.
        const existing = constraints.get(key);
        constraints.set(key, existing ? mergeConstraints(existing, c) : c);
      }
    }
  }
  const keys = keysOf(obj);
  for (let i = 0; i < keys.length; i++) {
    const v = obj[keys[i]];
    if (v === null || typeof v !== 'object') continue;
    if (Array.isArray(v)) {
      for (const item of v) if (isPlainObject(item)) collectRegistries(item as SynxObject, types, constraints, depth + 1);
    } else if (!(v instanceof SynxSecret)) collectRegistries(v as SynxObject, types, constraints, depth + 1);
  }
}

/** Field constraints, then the field's type, for every field — one walk. */
function validateFields(
  obj: SynxObject,
  constraints: Map<string, SynxConstraints>,
  types: Map<string, string>,
  depth: number,
): void {
  if (depth > MAX_RESOLVE_DEPTH) return;
  for (const key of Object.keys(obj)) {
    const c = constraints.get(key);
    if (c !== undefined) {
      const current = obj[key];
      // A value the per-key pass already replaced is not re-validated: the
      // error string's own length would be checked against min/max.
      const errored = typeof current === 'string'
        && (current.startsWith('CONSTRAINT_ERR:') || current.startsWith('TYPE_ERR:'));
      if (!errored) validateConstraints(obj, key, c);
    }
    const expected = types.get(key);
    if (expected !== undefined && !valueMatchesType(obj, key, expected)) {
      obj[key] = `TYPE_ERR: '${key}' expected ${expected} but got ${valueTypeName(obj, key)}`;
    }
    const child = obj[key];
    if (child === null || typeof child !== 'object') continue;
    if (Array.isArray(child)) {
      for (const item of child) if (isPlainObject(item)) validateFields(item as SynxObject, constraints, types, depth + 1);
    } else if (!(child instanceof SynxSecret)) validateFields(child as SynxObject, constraints, types, depth + 1);
  }
}

function isPlainObject(v: unknown): boolean {
  return typeof v === 'object' && v !== null && !Array.isArray(v) && !(v instanceof SynxSecret);
}

/**
 * `obj[key]` is an int: a whole number not read as a float (E19), or a bigint.
 * An integer past 2^53 is always a bigint here, so a whole `number` that large
 * can only be a float — the result of a `:calc` beyond i64, `1e30` from the
 * environment.
 */
function isIntValue(obj: SynxObject, key: string, v: unknown): boolean {
  return (typeof v === 'number' && Number.isSafeInteger(v) && !isIntegralFloat(obj, key)) || typeof v === 'bigint';
}

function valueMatchesType(obj: SynxObject, key: string, t: string): boolean {
  const v = obj[key];
  switch (t) {
    case 'int':    return isIntValue(obj, key, v);
    case 'float':  return typeof v === 'number' || typeof v === 'bigint';
    case 'bool':   return typeof v === 'boolean';
    case 'string': return typeof v === 'string' || v instanceof SynxSecret;
    case 'array':  return Array.isArray(v);
    case 'object': return isPlainObject(v);
    default:       return true;
  }
}

function valueTypeName(obj: SynxObject, key: string): string {
  const v = obj[key];
  if (v === null || v === undefined) return 'null';
  if (v instanceof SynxSecret) return 'secret';
  if (Array.isArray(v)) return 'array';
  if (typeof v === 'number') return isIntValue(obj, key, v) ? 'int' : 'float';
  if (typeof v === 'bigint') return 'int';
  if (typeof v === 'boolean') return 'bool';
  if (typeof v === 'string') return 'string';
  return 'object';
}

// ─── Constraint enforcement ───────────────────────────────────────────────

function validateConstraints(obj: SynxObject, key: string, c: SynxConstraints): void {
  const val = obj[key];

  // required
  // A key written without a value parses as an empty group; for `[required]`
  // that is the missing value, not a present object.
  if (c.required && (val === null || val === undefined || val === '' ||
      (typeof val === 'object' && !Array.isArray(val) && !(val instanceof SynxSecret) && Object.keys(val).length === 0))) {
    obj[key] = `CONSTRAINT_ERR: '${key}' is required`;
    return;
  }
  // spec: the reference engine still checks enum for null, reading it as an
  // empty string; type, range and pattern checks skip an absent value.
  if (val === null || val === undefined) {
    if (c.enum && !c.enum.includes('')) {
      obj[key] = `CONSTRAINT_ERR: '${key}' must be one of [${c.enum.join('|')}]`;
    }
    return;
  }

  // type check
  if (c.type) {
    let ok = true;
    switch (c.type) {
      case 'int':    ok = isIntValue(obj, key, val); break;
      case 'float':  ok = typeof val === 'number' || typeof val === 'bigint'; break;
      case 'bool':   ok = typeof val === 'boolean'; break;
      // A `:secret` is a masked string: `[type:string]:secret:env` must accept it.
      case 'string': ok = typeof val === 'string' || val instanceof SynxSecret; break;
    }
    if (!ok) {
      obj[key] = `CONSTRAINT_ERR: '${key}' expected type '${c.type}'`;
      return;
    }
  }

  // A secret is checked by its text — its enum, its length and its pattern,
  // as in synx-core; no message quotes the value.
  const text = typeof val === 'string' ? val : val instanceof SynxSecret ? val.reveal() : null;

  // enum check
  if (c.enum) {
    // A list or a group is "" (`String([1])` let a one-item list pass `[enum:1]`).
    const shown = text !== null ? text : typeof val === 'object' ? '' : valueToString(val);
    if (!c.enum.includes(shown)) {
      obj[key] = `CONSTRAINT_ERR: '${key}' must be one of [${c.enum.join('|')}]`;
      return;
    }
  }

  // min / max  (numbers: value range; strings: length in Unicode scalar
  // values, as synx-core — `val.length` counted UTF-16 units).
  const n = typeof val === 'number' ? val
          : typeof val === 'bigint' ? Number(val)
          : text !== null && (c.min !== undefined || c.max !== undefined) ? scalarLength(text)
          : null;
  if (n !== null) {
    if (c.min !== undefined && n < c.min) {
      obj[key] = `CONSTRAINT_ERR: '${key}' value ${numberToText(n)} is below min ${numberToText(c.min)}`;
      return;
    }
    if (c.max !== undefined && n > c.max) {
      obj[key] = `CONSTRAINT_ERR: '${key}' value ${numberToText(n)} exceeds max ${numberToText(c.max)}`;
      return;
    }
  }

  // pattern — length-capped like synx-core; a pattern that nests one
  // quantifier inside another (`^(a+)+$`) is refused rather than run.
  if (c.pattern && text !== null && utf8Length(c.pattern) <= 256) {
    if (matchesLinear(c.pattern, text) === false) {
      obj[key] = `CONSTRAINT_ERR: '${key}' does not match pattern /${c.pattern}/`;
      return;
    }
  }
}

// ─── New-marker helpers ───────────────────────────────────

/** Widest field `:format` pads to, and most decimals it writes — the caps of synx-core. */
const MAX_FMT_WIDTH = 4096;
const MAX_FMT_PREC = 1024;

/**
 * `s.parse::<usize>()` of synx-core, already capped at `cap`: an optional `+`
 * and decimal digits within 64 bits, else `null`.
 */
function parseWidth(s: string, cap: number): number | null {
  let i = s.charCodeAt(0) === 43 ? 1 : 0;
  if (i >= s.length) return null;
  while (i < s.length - 1 && s.charCodeAt(i) === 48) i++;
  const digits = s.length - i;
  let n = 0;
  for (let j = i; j < s.length; j++) {
    const c = s.charCodeAt(j);
    if (c < 48 || c > 57) return null;
    if (n <= cap) n = n * 10 + (c - 48);
  }
  // u64::MAX is 18446744073709551615; anything longer does not parse.
  if (digits > 20 || (digits === 20 && s.substring(i) > '18446744073709551615')) return null;
  return Math.min(n, cap);
}

/** An integer's digits under `%5d`, `%05d` and their `i` forms; any other pattern leaves them as they are. */
function formatIntPattern(pattern: string, digits: string): string {
  if (pattern.charCodeAt(0) !== 37) return digits;
  const last = pattern.charCodeAt(pattern.length - 1);
  if (pattern.length < 2 || (last !== 100 && last !== 105)) return digits;
  const inner = pattern.substring(1, pattern.length - 1);
  if (inner.charCodeAt(0) === 48) {
    const width = parseWidth(inner.substring(1), MAX_FMT_WIDTH);
    if (width !== null) return digits.padStart(width, '0');
  }
  const width = parseWidth(inner, MAX_FMT_WIDTH);
  return width === null ? digits : digits.padStart(width, ' ');
}

const F64_VIEW = new DataView(new ArrayBuffer(8));

/** `|x| = mantissa × 2^exponent`, exactly, for a finite `x`. */
function decomposeFloat(x: number): { mantissa: bigint; exponent: number } {
  F64_VIEW.setFloat64(0, Math.abs(x));
  const hi = F64_VIEW.getUint32(0);
  const lo = F64_VIEW.getUint32(4);
  const biased = (hi >>> 20) & 0x7ff;
  const fraction = (BigInt(hi & 0xfffff) << BigInt(32)) | BigInt(lo);
  if (biased === 0) return { mantissa: fraction, exponent: -1074 };
  return { mantissa: fraction | (BigInt(1) << BigInt(52)), exponent: biased - 1075 };
}

/** `numerator / denominator` rounded to the nearest integer, a tie to the even one. */
function divideHalfEven(numerator: bigint, denominator: bigint): bigint {
  let q = numerator / denominator;
  const twice = (numerator % denominator) * BigInt(2);
  if (twice > denominator || (twice === denominator && (q & BigInt(1)) === BigInt(1))) q += BigInt(1);
  return q;
}

/** A number that is not finite, as synx-core writes it. */
function nonFiniteText(x: number): string {
  return Number.isNaN(x) ? 'NaN' : x > 0 ? 'inf' : '-inf';
}

/**
 * `format!("{:.prec$}", x)`: the exact decimal value of `x` to `prec` places.
 *
 * spec: `toFixed` refuses more than 100 places (a RangeError that left
 * `Synx.parse`) and breaks a tie upwards — 0.25 was `0.3` here and `0.2` in
 * synx-core.
 */
function fixedText(x: number, prec: number): string {
  if (!Number.isFinite(x)) return nonFiniteText(x);
  const negative = x < 0 || Object.is(x, -0);
  const { mantissa, exponent } = decomposeFloat(x);
  const scale = BigInt(10) ** BigInt(prec);
  const scaled = exponent >= 0
    ? (mantissa << BigInt(exponent)) * scale
    : divideHalfEven(mantissa * scale, BigInt(1) << BigInt(-exponent));
  let digits = scaled.toString();
  if (prec > 0) {
    if (digits.length <= prec) digits = '0'.repeat(prec - digits.length + 1) + digits;
    digits = `${digits.substring(0, digits.length - prec)}.${digits.substring(digits.length - prec)}`;
  }
  return negative ? `-${digits}` : digits;
}

/** `format!("{:.prec$e}", x)`: one digit, `prec` decimals, `e`, the exponent as it is. */
function exponentText(x: number, prec: number): string {
  if (!Number.isFinite(x)) return nonFiniteText(x);
  const negative = x < 0 || Object.is(x, -0);
  let digits: string;
  let exp10 = 0;
  if (x === 0) {
    digits = '0'.repeat(prec + 1);
  } else {
    const { mantissa, exponent } = decomposeFloat(x);
    const low = BigInt(10) ** BigInt(prec);
    const high = low * BigInt(10);
    exp10 = Math.floor(Math.log10(Math.abs(x)));
    let q: bigint;
    // |x| / 10^(exp10 - prec), as one exact fraction; the estimate of exp10
    // is off by at most one, so the loop settles in a step or two.
    for (;;) {
      const shift = prec - exp10;
      let numerator = mantissa;
      let denominator = BigInt(1);
      if (exponent >= 0) numerator <<= BigInt(exponent); else denominator <<= BigInt(-exponent);
      if (shift >= 0) numerator *= BigInt(10) ** BigInt(shift); else denominator *= BigInt(10) ** BigInt(-shift);
      const floor = numerator / denominator;
      if (floor < low) { exp10--; continue; }
      if (floor >= high) { exp10++; continue; }
      q = divideHalfEven(numerator, denominator);
      break;
    }
    if (q >= high) { q /= BigInt(10); exp10++; }
    digits = q.toString();
  }
  const body = prec > 0 ? `${digits.charAt(0)}.${digits.substring(1)}` : digits;
  return `${negative ? '-' : ''}${body}e${exp10}`;
}

/** A number under `%e`, `%.3f` and `%.3e`; any other pattern writes it plainly. */
function formatFloatPattern(pattern: string, x: number): string {
  if (pattern.charCodeAt(0) === 37) {
    if (pattern === '%e') {
      if (!Number.isFinite(x)) return nonFiniteText(x);
      return x === 0 ? '0e+0' : x.toExponential();
    }
    const last = pattern.charCodeAt(pattern.length - 1);
    if (pattern.length > 2 && (last === 102 || last === 101) && pattern.charCodeAt(1) === 46) {
      const prec = parseWidth(pattern.substring(2, pattern.length - 1), MAX_FMT_PREC);
      if (prec !== null) return last === 101 ? exponentText(x, prec) : fixedText(x, prec);
    }
  }
  return numberToText(x);
}

/**
 * `:format` — a port of `apply_format_pattern` in synx-core, pattern for
 * pattern: `%d` `%5d` `%05d` (and `i`), `%.Nf`, `%e`, `%.Ne`.
 *
 * spec: only a number is formatted — a string (a quoted `"3.14"`, an i64
 * overflow) passes through as written, and a group or list reads as empty.
 */
function applyFormatPattern(pattern: string, value: unknown): string {
  const wantsInt = pattern.indexOf('d') !== -1 || pattern.indexOf('i') !== -1;
  const wantsFloat = pattern.indexOf('f') !== -1 || pattern.indexOf('e') !== -1;
  if (typeof value === 'bigint') {
    // Integer patterns pad the exact digits; Number() would round them.
    if (wantsInt) return formatIntPattern(pattern, value.toString());
    return wantsFloat ? formatFloatPattern(pattern, Number(value)) : value.toString();
  }
  if (typeof value !== 'number') {
    if (value === null || value === undefined) return 'null';
    if (typeof value === 'object' && !(value instanceof SynxSecret)) return '';
    return String(value);
  }
  if (Number.isSafeInteger(value)) {
    // One number type here: a whole value is an integer to `%d` and a float to `%f`.
    if (wantsInt) return formatIntPattern(pattern, numberToText(value));
    return wantsFloat ? formatFloatPattern(pattern, value) : numberToText(value);
  }
  // A fractional number only takes a float pattern; with any other it is
  // written as it is, as synx-core does for a Float.
  return wantsFloat ? formatFloatPattern(pattern, value) : numberToText(value);
}

// ─── :once ────────────────────────────────────────────────

/**
 * The value `.synx.lock` records for `key`, or a new one recorded there.
 *
 * safety: the file is shared by every process that resolves documents in its
 * folder. It was read once, extended in memory and written whole at the end
 * of the resolve: of 216 values handed out by six processes at once, 189 were
 * different on the next read (B-065). As in synx-core, a new line is appended
 * with one write to a descriptor opened for appending, what others appended
 * is read first, and the first line of a key wins — nobody's line is lost.
 *
 * The format is unchanged — one `key value` line per entry.
 */
function lockedValue(lockPath: string, key: string, kind: string): string {
  let lock = LOCKS.get(lockPath);
  if (lock === undefined) {
    lock = { path: lockPath, values: new Map(), seen: 0, openLine: false, fd: -1, foreign: isForeignLock(lockPath) };
    if (fs && !lock.foreign) {
      let fd = -1;
      try {
        fd = fs.openSync(lockPath, 'r');
        readLockFrom(lock, fd);
      } catch { /* the file does not exist yet */ } finally {
        if (fd !== -1) try { fs.closeSync(fd); } catch { /* ok */ }
      }
    }
    LOCKS.set(lockPath, lock);
  }
  const known = lock.values.get(key);
  if (known !== undefined) return known;
  const value = kind === 'timestamp' ? String(Date.now())
    : kind === 'random' ? String(Math.floor(Math.random() * 2147483647))
    : generateUuid();
  return recordLock(lock, key, value);
}

/**
 * Whether `.synx.lock` is there and is not a plain file — a symbolic link, a
 * directory, anything else. Looked at without following a link.
 *
 * safety: the file was opened wherever its name led. A link named
 * `.synx.lock` in the base directory had `:once` read a file outside it for
 * its values and append a line holding a key of the document to it (B-089).
 * As in synx-core, such a file is neither read nor written: the values of
 * this resolve stay in memory. A file that is not there is created as before.
 */
function isForeignLock(lockPath: string): boolean {
  if (!fs) return false;
  try {
    return !fs.lstatSync(lockPath).isFile();
  } catch {
    return false; // not there, or not to be looked at: the open reports it
  }
}

/** Take in what the file has gained since it was last read. */
function readLockFrom(lock: LockFile, fd: number): void {
  if (!fs || lock.seen >= MAX_LOCK_FILE_BYTES) return;
  const chunks: Buffer[] = [];
  let total = 0;
  const room = MAX_LOCK_FILE_BYTES - lock.seen;
  for (;;) {
    const buf = Buffer.allocUnsafe(Math.min(65536, room - total));
    if (buf.length === 0) break;
    const n = fs.readSync(fd, buf, 0, buf.length, lock.seen + total);
    if (n <= 0) break;
    chunks.push(n === buf.length ? buf : buf.subarray(0, n));
    total += n;
  }
  if (total === 0) return;
  const bytes = chunks.length === 1 ? chunks[0] : Buffer.concat(chunks, total);
  lock.seen += total;
  lock.openLine = bytes[bytes.length - 1] !== 10;
  const lines = bytes.toString('utf-8').split('\n');
  for (let line of lines) {
    if (line.charCodeAt(line.length - 1) === 13) line = line.substring(0, line.length - 1);
    const space = line.indexOf(' ');
    if (space === -1) continue;
    const k = line.substring(0, space);
    if (!lock.values.has(k)) lock.values.set(k, trimSpaceStart(line.substring(space + 1)));
  }
}

/** Record `key value`, and return the value the file gives the key. */
function recordLock(lock: LockFile, key: string, value: string): string {
  if (fs && writesAllowed && !lock.foreign) {
    if (lock.fd === -1) {
      // Looked at again right before the write, as before the read.
      if (isForeignLock(lock.path)) lock.foreign = true;
      else try { lock.fd = fs.openSync(lock.path, 'a+'); } catch { lock.fd = -1; }
    }
    if (lock.fd !== -1) {
      try {
        // What others appended since the last read comes first.
        readLockFrom(lock, lock.fd);
        const theirs = lock.values.get(key);
        if (theirs !== undefined) return theirs;
        // One write: lines of two processes never interleave.
        fs.writeSync(lock.fd, `${lock.openLine ? '\n' : ''}${key} ${value}\n`);
        // Whoever appended between that read and this write is ahead of us.
        readLockFrom(lock, lock.fd);
      } catch { /* the value still holds for this resolve */ }
    }
  }
  const recorded = lock.values.get(key);
  if (recorded !== undefined) return recorded;
  lock.values.set(key, value);
  return value;
}

/** Close the lock files: every line is already written. */
function flushLocks(): void {
  if (fs) {
    for (const lock of LOCKS.values()) {
      if (lock.fd !== -1) try { fs.closeSync(lock.fd); } catch { /* ok */ }
    }
  }
  LOCKS.clear();
}

function trimSpaceStart(s: string): string {
  let i = 0;
  while (i < s.length && isUnicodeSpaceCode(s.charCodeAt(i))) i++;
  return i === 0 ? s : s.substring(i);
}

function generateUuid(): string {
  if (typeof crypto !== 'undefined' && typeof (crypto as any).randomUUID === 'function') {
    return (crypto as any).randomUUID();
  }
  // Fallback manual generation
  const hex = '0123456789abcdef';
  let out = '';
  for (let i = 0; i < 36; i++) {
    if (i === 8 || i === 13 || i === 18 || i === 23) out += '-';
    else if (i === 14) out += '4';
    else {
      const r = Math.random() * 16 | 0;
      out += hex[i === 19 ? (r & 0x3) | 0x8 : r];
    }
  }
  return out;
}

/** The integer a version part starts with, or 0. */
function leadingInt(part: string): number {
  return parseInt(part, 10) || 0;
}

function compareVersions(current: string, op: string, required: string): boolean {
  // spec: each dot-separated part is read for its leading integer, a part
  // with none counting as 0 — positions never shift.
  const cv = current.split('.');
  const rv = required.split('.');
  const len = Math.max(cv.length, rv.length);
  let cmp = 0;
  for (let i = 0; i < len; i++) {
    const a = i < cv.length ? leadingInt(cv[i]) : 0;
    const b = i < rv.length ? leadingInt(rv[i]) : 0;
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

/**
 * One value of a file by its dot-path — through the objects of a JSON
 * document or of a SYNX one. A list has no keys: `list.0` and `list.length`
 * are null, as in synx-core.
 */
function extractFromFileContent(content: string, keyPath: string, ext: string): SynxValue {
  let current: any;
  if (ext === 'json') {
    try {
      current = parseJson(content);
    } catch {
      return null;
    }
    // What synx-core's JSON reader refuses is no document here either.
    if (!isPortableJson(current, 0)) return null;
  } else {
    current = parseData(content, INCLUDED).root;
  }
  foundIn = null;
  for (const part of keyPath.split('.')) {
    // Own keys only: `constructor.name` walked into the built-ins and
    // returned "Object" for a key the file does not have.
    if (current === null || typeof current !== 'object' || Array.isArray(current) || !hasOwn.call(current, part)) return null;
    foundIn = current;
    foundKey = part;
    current = current[part];
  }
  return current === undefined ? null : current;
}

/** Whether `JSON.parse` hands its reviver the source text of a number; `undefined` until asked. */
let jsonSourceAccess: boolean | undefined;

/**
 * `JSON.parse` that keeps what a number said, where the runtime lets it be
 * read (the `source` a reviver is given — Node 21, Bun 1.1).
 *
 * spec: synx-core reads a JSON integer as an exact i64 and `8080.0` as a
 * float. `JSON.parse` alone rounds an id past 2^53 to another id and cannot
 * tell `8080.0` from `8080`: `:watch:id data.json` gave a different id here.
 */
function parseJson(content: string): unknown {
  if (jsonSourceAccess === undefined) {
    jsonSourceAccess = false;
    try {
      JSON.parse('1', ((_key: string, value: unknown, context: { source?: unknown } | undefined) => {
        if (context !== undefined && context !== null && context.source === '1') jsonSourceAccess = true;
        return value;
      }) as (key: string, value: unknown) => unknown);
    } catch {
      jsonSourceAccess = false;
    }
  }
  if (!jsonSourceAccess) return JSON.parse(content);
  return JSON.parse(content, (function (this: object, key: string, value: unknown, context: { source?: unknown }): unknown {
    if (typeof value !== 'number' || typeof context.source !== 'string') return value;
    // An integer literal within i64 is that integer, a `bigint` past 2^53.
    const exact = parseI64(context.source);
    if (exact !== null) return exact;
    if (Number.isSafeInteger(value) && this !== null && typeof this === 'object' && !Array.isArray(this)) {
      noteIntegralFloat(this, key, true);
    }
    return value;
  }) as (key: string, value: unknown) => unknown);
}

/** Deepest nesting of lists and objects synx-core's JSON reader accepts. */
const MAX_JSON_NESTING = 127;

/**
 * Whether a document `JSON.parse` accepted is one the JSON reader of
 * synx-core (serde_json) accepts as well: not nested past MAX_JSON_NESTING,
 * every number finite (`1e999` is out of range there), every string
 * well-formed (`"\ud800"` alone is refused there).
 *
 * safety: bounded here, the values `:watch` places are never deeper than the
 * walks over them can go.
 */
function isPortableJson(value: unknown, depth: number): boolean {
  if (typeof value === 'number') return Number.isFinite(value);
  if (typeof value === 'string') return isWellFormed(value);
  if (value === null || typeof value !== 'object') return true;
  if (depth >= MAX_JSON_NESTING) return false;
  if (Array.isArray(value)) {
    for (let i = 0; i < value.length; i++) {
      if (!isPortableJson(value[i], depth + 1)) return false;
    }
    return true;
  }
  const obj = value as Record<string, unknown>;
  for (const k of Object.keys(obj)) {
    if (!isWellFormed(k) || !isPortableJson(obj[k], depth + 1)) return false;
  }
  return true;
}

/** Whether a string has no surrogate without its pair. */
function isWellFormed(s: string): boolean {
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c < 0xd800 || c > 0xdfff) continue;
    if (c > 0xdbff) return false;
    const lo = s.charCodeAt(i + 1);
    if (!(lo >= 0xdc00 && lo <= 0xdfff)) return false;
    i++;
  }
  return true;
}

// ─── Helpers ──────────────────────────────────────────────

/** Serialize a value to SYNX format string (for :prompt marker).
 *  Keys are emitted in sorted order so that LLM prompts are deterministic
 *  across runs and across language bindings (matches the Rust engine). */
function stringifyValue(value: unknown, indent: number): string {
  const sp = ' '.repeat(indent);
  if (Array.isArray(value)) {
    let out = '';
    for (const item of value) out += `${sp}  - ${valueToString(item)}\n`;
    return out;
  }
  if (value !== null && typeof value === 'object' && !(value instanceof SynxSecret)) {
    const obj = value as Record<string, unknown>;
    let out = '';
    for (const k of Object.keys(obj).sort(compareUnicodeScalar)) {
      const v = obj[k];
      if (Array.isArray(v)) {
        out += `${sp}${k}\n`;
        for (const item of v) out += `${sp}  - ${valueToString(item)}\n`;
      } else if (v !== null && typeof v === 'object' && !(v instanceof SynxSecret)) {
        out += `${sp}${k}\n${stringifyValue(v, indent + 2)}`;
      } else {
        out += `${sp}${k} ${valueToString(v)}\n`;
      }
    }
    return out;
  }
  return `${sp}${valueToString(value)}\n`;
}

/**
 * Text of a value placed inside a string — interpolation, `:join`, `:map`,
 * `:prompt` lists. Mirrors synx-core's `value_to_string`: a secret stays
 * masked and a group or list contributes nothing (`String()` gave
 * "[object Object]" and "a,b"), `null` reads as "null".
 */
function valueToString(v: unknown): string {
  if (typeof v === 'string') return v;
  if (v === null || v === undefined) return 'null';
  if (typeof v === 'object') return v instanceof SynxSecret ? '[SECRET]' : '';
  if (typeof v === 'number') return numberToText(v);
  return String(v);
}

/**
 * A number as text, the way synx-core writes it (its `Display`): plain
 * decimal, never an exponent.
 *
 * spec: `String(1e-7)` is "1e-7" and `String(1e21)` is "1e+21"; Rust writes
 * "0.0000001" and "1000000000000000000000". Interpolation, `:join`, `:calc`
 * operands and enum checks of such a number differed between the two.
 */
function numberToText(n: number): string {
  // A `:calc` that overflows leaves a number that is not finite; synx-core
  // writes it `inf`, `-inf` or `NaN`.
  if (!Number.isFinite(n)) return nonFiniteText(n);
  return expandExponent(String(n));
}

/** A whole count as an integer's digits, within i64. */
function integerText(n: number): string {
  if (n >= 9223372036854775807) return '9223372036854775807';
  if (n <= -9223372036854775808) return '-9223372036854775808';
  return numberToText(n === 0 ? 0 : n);
}

/**
 * Whether text taken from outside the document (`:env`, `:default`) must stay
 * a string instead of going through `castPrimitive`. `(string)` always asked for
 * that; since 3.7.4 so does `[type:string]` — before, the cast ran first and the
 * constraint then rejected its own declared type. Mirrors `wants_string` in
 * synx-core.
 */
function wantsString(meta: SynxMeta): boolean {
  return meta.typeHint === 'string' || (meta.constraints !== undefined && meta.constraints.type === 'string');
}

/**
 * A value that came in as text from outside the document: a variable of the
 * environment or the argument of `:default`.
 *
 * spec: a secret is its text. Cast first, `00123` became the secret `123`,
 * `1e5` the secret `100000` and `+5` the secret `5` — a different key from
 * the one in the environment.
 */
function externalValue(text: string, asString: boolean, secret: boolean): SynxValue {
  if (asString) return text;
  const cast = castPrimitive(text);
  return secret && cast !== null && typeof cast !== 'string' ? text : cast;
}

/**
 * `obj[key] = externalValue(…)`, noting a whole-valued float.
 *
 * spec: `8080.0` from the environment is a float in synx-core, and
 * `[type:int]` refuses it (E19). Noted only when the chain holds nothing but
 * `:env` and `:default`: a later marker may turn the value into an int.
 */
function placeExternal(
  obj: SynxObject, key: string, text: string, asString: boolean, secret: boolean, markers: string[],
): void {
  const value = externalValue(text, asString, secret);
  obj[key] = value;
  let isFloat = isWholeFloat(value, text);
  if (isFloat) {
    const di = markers.indexOf('default');
    const end = di === -1 ? markers.length : di + 1;
    for (let i = 0; i < end && isFloat; i++) isFloat = markers[i] === 'env' || markers[i] === 'default';
  }
  noteIntegralFloat(obj, key, isFloat);
}

/** Whether `value`, cast from `text`, is a float with a whole value (`5.0`, `1e3`). */
function isWholeFloat(value: SynxValue, text: string): boolean {
  return typeof value === 'number' && Number.isInteger(value) && parseI64(text) === null;
}

/**
 * Casts text that came from outside the document (`:env`, `:default`, `:split`).
 * Mirrors `cast_primitive` in synx-core: surrounding quotes keep the literal,
 * an integer is an exact i64 (a `bigint` beyond 2^53 — `parseInt` turned a
 * 19-digit id into a different id), one that overflows i64 is tried as a
 * float, and only a finite float is a number: `inf`, `NaN` and `1e400` are
 * text.
 */
function castPrimitive(val: string): SynxValue {
  const len = val.length;
  if (len >= 2) {
    const first = val.charCodeAt(0);
    if ((first === 34 || first === 39) && val.charCodeAt(len - 1) === first) return val.substring(1, len - 1);
  }
  if (val === 'true') return true;
  if (val === 'false') return false;
  if (val === 'null') return null;
  const int = parseI64(val);
  if (int !== null) return int;
  if (floatKind(val) === FLOAT_DECIMAL) {
    const f = Number(val);
    if (Number.isFinite(f)) return f;
  }
  return val;
}

/** The text of an environment entry, or `undefined` when there is none.
 *  A name that is only a prototype member (`toString`) reads as a function. */
function envText(v: unknown): string | undefined {
  if (typeof v === 'string') return v;
  if (typeof v === 'number' || typeof v === 'boolean' || typeof v === 'bigint') return String(v);
  return undefined;
}

/**
 * An unsigned integer as synx-core's `parse::<usize>()` reads one — an
 * optional `+` and decimal digits, no more than `max` — or `null`.
 */
function parseUnsigned(s: string | undefined, max = U64_MAX): number | null {
  if (s === undefined) return null;
  const digits = unsignedDigits(s);
  if (digits === '') return null;
  // Compared as text: a double cannot tell 2^64 from 2^64 - 1.
  if (digits.length > max.length || (digits.length === max.length && digits > max)) return null;
  return +digits;
}

/** The largest count a 64-bit `usize` holds, and a `u32`. */
const U64_MAX = '18446744073709551615';
const U32_MAX = '4294967295';

/**
 * The digits of an unsigned integer written as `+007`: no sign, no leading
 * zeros. Empty when the text is not one.
 */
function unsignedDigits(s: string): string {
  let i = s.charCodeAt(0) === 43 ? 1 : 0;
  if (i >= s.length) return '';
  for (let j = i; j < s.length; j++) {
    const c = s.charCodeAt(j);
    if (c < 48 || c > 57) return '';
  }
  while (i < s.length - 1 && s.charCodeAt(i) === 48) i++;
  return i === 0 ? s : s.substring(i);
}

/**
 * CLDR-inspired plural category selection.
 * Mirrors the Rust `plural_category` table in `synx-core::engine`.
 */
function pluralCategory(lang: string, n: number): string {
  const absN = Math.abs(n);
  const n10 = absN % 10;
  const n100 = absN % 100;

  switch (lang) {
    case 'ru':
    case 'uk':
    case 'be':
    case 'pl':
      if (n10 === 1 && n100 !== 11) return 'one';
      if (n10 >= 2 && n10 <= 4 && !(n100 >= 12 && n100 <= 14)) return 'few';
      return 'many';
    case 'cs':
    case 'sk':
      if (absN === 1) return 'one';
      if (absN >= 2 && absN <= 4) return 'few';
      return 'other';
    case 'ar':
      if (absN === 0) return 'zero';
      if (absN === 1) return 'one';
      if (absN === 2) return 'two';
      if (n100 >= 3 && n100 <= 10) return 'few';
      if (n100 >= 11 && n100 <= 99) return 'many';
      return 'other';
    case 'fr':
    case 'pt':
      return absN <= 1 ? 'one' : 'other';
    case 'ja':
    case 'zh':
    case 'ko':
    case 'vi':
    case 'th':
      return 'other';
    default:
      return absN === 1 ? 'one' : 'other';
  }
}

function delimiterFromKeyword(keyword: string): string {
  // A switch, not a lookup table: `constructor` and `toString` are words a
  // table inherits, and the delimiter became a function.
  switch (keyword) {
    case 'space': return ' ';
    case 'pipe': return '|';
    case 'dash': return '-';
    case 'dot': return '.';
    case 'semi': return ';';
    case 'tab': return '\t';
    case 'slash': return '/';
    default: return keyword;
  }
}

function weightedRandom(items: SynxValue[], weights: number[]): SynxValue {
  const w: number[] = weights.filter(x => !Number.isNaN(x));
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

  let total = 0;
  for (let i = 0; i < w.length; i++) total += w[i];
  if (!(total > 0)) return items[Math.floor(Math.random() * items.length)];

  const rand = Math.random();
  let cumulative = 0;
  for (let i = 0; i < items.length; i++) {
    cumulative += w[i] / total;
    if (rand <= cumulative) return items[i];
  }
  return items[items.length - 1];
}

// ─── :calc names ──────────────────────────────────────────

function isWordChar(code: number): boolean {
  return (code >= 48 && code <= 57)   // 0-9
      || (code >= 65 && code <= 90)   // A-Z
      || (code >= 97 && code <= 122)  // a-z
      || code === 95;                 // _
}

/**
 * Replace whole-word occurrences of `word`; the result stops growing at
 * MAX_ENGINE_SCRATCH_STRING.
 */
function replaceWord(haystack: string, word: string, replacement: string): string {
  const wLen = word.length;
  const hLen = haystack.length;
  if (wLen === 0 || wLen > hLen) return haystack;
  // perf: pieces between matches, joined once — it was a character at a time.
  let result = '';
  let from = 0;
  let at = haystack.indexOf(word);
  while (at !== -1 && result.length < MAX_ENGINE_SCRATCH_STRING) {
    const before = at === 0 || !isWordChar(haystack.charCodeAt(at - 1));
    const after = at + wLen >= hLen || !isWordChar(haystack.charCodeAt(at + wLen));
    if (before && after) {
      result += haystack.substring(from, at) + replacement;
      from = at + wLen;
      at = haystack.indexOf(word, from);
    } else {
      at = haystack.indexOf(word, at + 1);
    }
  }
  return from === 0 ? haystack : result + haystack.substring(from);
}

/**
 * A key whole-word substitution can reach in ways a token lookup cannot: one
 * with a non-word character (`max-hp`, `a.b`, `цена`), or one a substituted
 * number can itself spell (`2`, `NaN`, `inf`).
 */
function isCalcSpecialKey(k: string): boolean {
  let digits = true;
  for (let i = 0; i < k.length; i++) {
    const c = k.charCodeAt(i);
    if (!isWordChar(c)) return true;
    if (c < 48 || c > 57) digits = false;
  }
  return digits || k === 'NaN' || k === 'inf';
}

const hasOwn = Object.prototype.hasOwnProperty;

/**
 * A number for arithmetic markers. An integer beyond 2^53 is a `bigint` here
 * and an i64 in synx-core, which uses it in `:calc`, `:clamp`, `:round`,
 * `:sum` and `:sort` as a float — so they did nothing with it in TypeScript.
 */
function isNum(v: unknown): v is number | bigint {
  return typeof v === 'number' || typeof v === 'bigint';
}

function positionsOf(scope: Scope, obj: SynxObject): Map<string, number> {
  const known = scope.positions ?? (scope.positions = new Map<object, Map<string, number>>());
  let positions = known.get(obj);
  if (positions === undefined) {
    positions = new Map<string, number>();
    const keys = keysOf(obj);
    for (let i = 0; i < keys.length; i++) positions.set(keys[i], i);
    known.set(obj, positions);
  }
  return positions;
}

/**
 * Substitute numeric variables into a `:calc` expression — a key of `local`
 * first (it shadows the root), then a root key. `null` when the result
 * exceeds MAX_CALC_RESOLVED_LEN.
 *
 * perf: one pass over the expression's words with a property lookup each.
 * The sequential form — whole-word replacement key by key, in document order
 * — survives only for documents that have a special key holding a number,
 * where it is the exact semantics; and there it visits just the keys that can
 * match: the special ones and the words of the expression. It used to build a
 * map of every numeric key of the root for every `:calc`: 4 000 of them next
 * to one `max-hp 5` took 12 s.
 */
function substituteCalcVars(expr: string, local: SynxObject, selfKey: string, scope: Scope, depth: number): string | null {
  const root = scope.root;
  const localIsRoot = local === root;
  const rootSpecial = scope.rootSpecial ?? (scope.rootSpecial = keysOf(root).filter(isCalcSpecialKey));
  const localSpecial = localIsRoot ? rootSpecial : specialKeysOf(scope, local, depth);

  let inPlay = false;
  for (let i = 0; i < rootSpecial.length && !inPlay; i++) inPlay = isNum(root[rootSpecial[i]]);
  if (!inPlay && !localIsRoot) {
    for (let i = 0; i < localSpecial.length && !inPlay; i++) {
      inPlay = localSpecial[i] !== selfKey && isNum(local[localSpecial[i]]);
    }
  }

  const len = expr.length;
  if (inPlay) {
    // Exact sequential whole-word replacement, object before root.
    const words: string[] = [];
    for (let i = 0; i < len;) {
      if (!isWordChar(expr.charCodeAt(i))) { i++; continue; }
      const start = i;
      while (i < len && isWordChar(expr.charCodeAt(i))) i++;
      words.push(expr.substring(start, i));
    }
    let resolved = expr;
    for (let pass = 0; pass < 2; pass++) {
      if (pass === 1 && localIsRoot) break;
      const map = pass === 0 ? local : root;
      const special = pass === 0 ? localSpecial : rootSpecial;
      const positions = positionsOf(scope, map);
      const candidates = new Set<string>(special);
      for (const w of words) if (positions.has(w)) candidates.add(w);
      const inOrder = [...candidates].sort((a, b) => (positions.get(a) as number) - (positions.get(b) as number));
      for (const k of inOrder) {
        if (pass === 0 && k === selfKey) continue;
        const v = map[k];
        if (!isNum(v)) continue;
        resolved = replaceWord(resolved, k, numberToText(Number(v)));
        if (utf8Length(resolved) > MAX_CALC_RESOLVED_LEN) return null;
      }
    }
    return resolved;
  }

  let out = '';
  // UTF-8 bytes of `out`: words and numbers are ASCII, only the runs between
  // them need measuring.
  let outBytes = 0;
  let i = 0;
  while (i < len) {
    const start = i;
    if (isWordChar(expr.charCodeAt(i))) {
      while (i < len && isWordChar(expr.charCodeAt(i))) i++;
      const token = expr.substring(start, i);
      let v: unknown;
      if (token !== selfKey && hasOwn.call(local, token) && isNum(local[token])) v = local[token];
      else if (hasOwn.call(root, token) && isNum(root[token])) v = root[token];
      const piece = v === undefined ? token : numberToText(Number(v));
      out += piece;
      outBytes += piece.length;
    } else {
      while (i < len && !isWordChar(expr.charCodeAt(i))) i++;
      const run = expr.substring(start, i);
      out += run;
      outBytes += utf8Length(run);
    }
    if (outBytes > MAX_CALC_RESOLVED_LEN) return null;
  }
  return out;
}

/**
 * Special `:calc` keys of the object being resolved at `depth`.
 *
 * perf: one slot per depth — an object's keys are resolved one after another,
 * with only deeper objects in between, so the slot of its depth is its own
 * until it is done.
 */
function specialKeysOf(scope: Scope, obj: SynxObject, depth: number): string[] {
  if (scope.specialOf[depth] !== obj) {
    scope.specialOf[depth] = obj;
    scope.special[depth] = keysOf(obj).filter(isCalcSpecialKey);
  }
  return scope.special[depth];
}

/**
 * Substitute dot-path references (`a.b.c`) in a calc expression with their
 * numeric values from the root tree. Tokens without a dot are left unchanged.
 * `null` past MAX_CALC_RESOLVED_LEN. Matches the `dot_resolved` pass of
 * synx-core.
 */
function replaceDotPaths(expr: string, root: SynxObject): string | null {
  if (expr.indexOf('.') === -1) return expr;
  let out = '';
  let outBytes = 0;
  let i = 0;
  const len = expr.length;
  while (i < len) {
    const start = i;
    if (isWordChar(expr.charCodeAt(i))) {
      let hasDot = false;
      while (i < len) {
        const c = expr.charCodeAt(i);
        if (c === 46) hasDot = true;
        else if (!isWordChar(c)) break;
        i++;
      }
      const token = expr.substring(start, i);
      const val = hasDot ? deepGet(root, token) : undefined;
      const piece = isNum(val) ? numberToText(Number(val)) : token;
      out += piece;
      outBytes += piece.length;
    } else {
      while (i < len && !isWordChar(expr.charCodeAt(i))) i++;
      const run = expr.substring(start, i);
      out += run;
      outBytes += utf8Length(run);
    }
    if (outBytes > MAX_CALC_RESOLVED_LEN) return null;
  }
  return out;
}

// ─── :spam ────────────────────────────────────────────────

/** Keys the process-wide `:spam` table keeps. */
const MAX_SPAM_BUCKETS = 1024;
/** Calls remembered per key; a limit above this is not enforced. */
const MAX_SPAM_CALLS_TRACKED = 4096;
/** A longer `key::target` is stored as its hash. */
const MAX_SPAM_KEY_CHARS = 256;

const SPAM_BUCKETS = new Map<string, number[]>();

function allowSpamAccess(bucketKey: string, maxCalls: number, windowSec: number): boolean {
  const now = Date.now();
  const windowMs = Math.max(1, windowSec) * 1000;

  // safety: the table outlives every parse. A long key is kept as its hash,
  // the table holds MAX_SPAM_BUCKETS keys (the one idle longest goes first)
  // and each key remembers MAX_SPAM_CALLS_TRACKED calls — a host that parses
  // other people's documents no longer grows it forever.
  if (bucketKey.length > MAX_SPAM_KEY_CHARS) {
    let h1 = 0x811c9dc5;
    let h2 = 0x01000193;
    for (let i = 0; i < bucketKey.length; i++) {
      const c = bucketKey.charCodeAt(i);
      h1 = Math.imul(h1 ^ c, 0x01000193);
      h2 = Math.imul(h2 + c, 0x85ebca6b) ^ (h2 >>> 13);
    }
    bucketKey = `#${(h1 >>> 0).toString(16)}${(h2 >>> 0).toString(16)}:${bucketKey.length}`;
  }
  let calls = SPAM_BUCKETS.get(bucketKey);
  if (calls === undefined) {
    if (SPAM_BUCKETS.size >= MAX_SPAM_BUCKETS) {
      let idleKey: string | undefined;
      let idleAt = Infinity;
      for (const [k, list] of SPAM_BUCKETS) {
        const last = list.length > 0 ? list[list.length - 1] : -Infinity;
        if (last < idleAt) { idleAt = last; idleKey = k; }
      }
      if (idleKey !== undefined) SPAM_BUCKETS.delete(idleKey);
    }
    calls = [];
    SPAM_BUCKETS.set(bucketKey, calls);
  } else {
    let kept = 0;
    for (let i = 0; i < calls.length; i++) if (now - calls[i] <= windowMs) calls[kept++] = calls[i];
    calls.length = kept;
  }

  if (calls.length >= maxCalls) return false;
  if (calls.length < MAX_SPAM_CALLS_TRACKED) calls.push(now);
  return true;
}

// ─── Lookups ──────────────────────────────────────────────

/**
 * The value at `path`: a key of `obj` spelled exactly so, else a dot-path
 * through nested objects. `undefined` when there is none.
 */
function deepGet(obj: SynxObject, path: string): SynxValue | undefined {
  // Try direct key first (own property only)
  if (hasOwn.call(obj, path)) {
    foundIn = obj;
    foundKey = path;
    return obj[path];
  }
  // Try dot-path
  let dot = path.indexOf('.');
  if (dot === -1) return undefined;
  let current: any = obj;
  let start = 0;
  for (;;) {
    const part = dot === -1 ? path.substring(start) : path.substring(start, dot);
    if (current === null || typeof current !== 'object' || Array.isArray(current) || current instanceof SynxSecret) return undefined;
    if (!hasOwn.call(current, part)) return undefined;
    if (dot === -1) {
      foundIn = current;
      foundKey = part;
      return current[part];
    }
    current = current[part];
    start = dot + 1;
    dot = path.indexOf('.', start);
  }
}

// The object and the key the last successful `deepGet` / `lookup` read.
let foundIn: SynxObject | null = null;
let foundKey = '';

/** Whether the value `deepGet` / `lookup` last found is a whole number read as a float. */
function foundIsFloat(): boolean {
  return foundIn !== null && isIntegralFloat(foundIn, foundKey);
}

/** A name as a marker reads it: a path from the root, else a key of this object. */
function lookup(root: SynxObject, local: SynxObject, name: string): SynxValue | undefined {
  const found = deepGet(root, name);
  if (found !== undefined) return found;
  if (!hasOwn.call(local, name)) return undefined;
  foundIn = local;
  foundKey = name;
  return local[name];
}

/** An object's own value for `key`; inherited members do not count. */
function ownValue(obj: SynxObject, key: string): SynxValue | undefined {
  return hasOwn.call(obj, key) ? obj[key] : undefined;
}
