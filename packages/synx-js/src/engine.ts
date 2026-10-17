/**
 * SYNX Engine — @aperturesyndicate/synx-format
 *
 * Resolves active markers (:random, :calc, :env, :alias, :secret, etc.)
 * in a parsed SYNX object tree. Only runs in !active mode.
 */

import type { SynxObject, SynxValue, SynxConstraints, SynxMeta, SynxMetaMap, SynxOptions, SynxInclude } from './types';
import { safeCalc } from './calc';
import { parseData, parseI64 } from './parser';
import { matchesLinear, scalarLength, expandExponent } from './synxl';
import { compareUnicodeScalar } from './json';

// Lazy-loaded Node.js modules (not available in browser)
let fs: typeof import('fs') | undefined;
let pathModule: typeof import('path') | undefined;
try {
  fs = require('fs');
  pathModule = require('path');
} catch {
  // Browser environment — :include will not work
}

// ─── Security constants ───────────────────────────────────

const MAX_CALC_EXPR_LEN = 4096;
const MAX_FILE_SIZE = 10 * 1024 * 1024; // 10 MB
const DEFAULT_MAX_INCLUDE_DEPTH = 16;

/** Maximum object nesting depth for active-mode resolution (prevents stack overflow). */
const MAX_RESOLVE_DEPTH = 512;

/** Ensure a file path stays inside the base directory (path jail).
 *  Error messages are kept in lock-step with the Rust engine's `jail_path`. */
function jailPath(base: string, filePath: string): string {
  if (!pathModule) throw new Error('path module not available');
  // Always check leading "/" or "\" first so the message is portable: POSIX
  // absolute paths and Windows rooted paths both produce the same string.
  const first = filePath.charCodeAt(0);
  if (first === 0x2f /* / */ || first === 0x5c /* \ */) {
    throw new Error(`SECURITY: rooted paths are not allowed: '${filePath}'`);
  }
  if (pathModule.isAbsolute(filePath)) {
    throw new Error(`SECURITY: absolute paths are not allowed: '${filePath}'`);
  }
  const resolved = pathModule.resolve(base, filePath);
  const normalizedBase = pathModule.resolve(base);
  if (filePath.includes('..') &&
      !resolved.startsWith(normalizedBase + pathModule.sep) &&
      resolved !== normalizedBase) {
    throw new Error(`SECURITY: path traversal detected: '${filePath}'`);
  }
  if (!resolved.startsWith(normalizedBase + pathModule.sep) && resolved !== normalizedBase) {
    throw new Error(`SECURITY: path escapes base directory: '${filePath}'`);
  }
  return resolved;
}

/** Check that a file does not exceed MAX_FILE_SIZE. */
function checkFileSize(filePath: string): void {
  if (!fs) return;
  const stat = fs.statSync(filePath);
  if (stat.size > MAX_FILE_SIZE) {
    throw new Error(`file too large (${stat.size} bytes, max ${MAX_FILE_SIZE})`);
  }
}

// ─── Secret wrapper ───────────────────────────────────────

export class SynxSecret {
  // Runtime-private: a TS `private` field is an ordinary own property, so
  // util.inspect / console.log / Object.entries / spread would print it.
  #value: string;

  constructor(value: string) {
    this.#value = value;
    // Brand under a global symbol so the runtime-agnostic canonical JSON writer
    // (json.ts) can recognise a secret and emit `"[SECRET]"` without importing
    // this module (which would pull the whole engine, and fs/path, into it).
    Object.defineProperty(this, Symbol.for('synx.secret'), { value: true, enumerable: false });
  }

  toString(): string {
    return '[SECRET]';
  }

  toJSON(): string {
    return '[SECRET]';
  }

  /** Call this explicitly to get the real value */
  reveal(): string {
    return this.#value;
  }

  [Symbol.for('nodejs.util.inspect.custom')](): string {
    return '[SECRET]';
  }

  [Symbol.toPrimitive](hint: string): string | number {
    if (hint === 'number') return NaN;
    return '[SECRET]';
  }
}

const SPAM_BUCKETS = new Map<string, number[]>();

/**
 * Normalise Node.js fs error messages to OS-agnostic strings so that
 * INCLUDE_ERR / WATCH_ERR text matches the Rust engine. Without this, the
 * surface depends on the host OS ("ENOENT: no such file or directory" on Linux,
 * "ENOENT…" + Windows variant) — diff-noisy and not portable.
 */
function fmtIoErr(err: any, ctx: string): string {
  const code = err && err.code;
  if (code === 'ENOENT') return `file not found: ${ctx}`;
  if (code === 'EACCES' || code === 'EPERM') return `permission denied: ${ctx}`;
  return err && err.message ? err.message : String(err);
}

// ─── Engine ───────────────────────────────────────────────

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

/** Classify a marker chain into `MK_*` bits; arguments and unknown names add nothing. */
function markerBits(markers: string[]): number {
  let bits = 0;
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

export function resolve(
  obj: SynxObject,
  options: SynxOptions = {},
  root?: SynxObject,
  includesMap?: Map<string, SynxObject>,
  _resolveDepth = 0,
  _currentPath = '',
): SynxObject {
  const isTop = !root;
  let typeRegistry: Map<string, string> | undefined;
  let constraintRegistry: Map<string, SynxConstraints> | undefined;
  if (!root) {
    root = obj;
    ROOT_CALC_SPECIAL.delete(obj);
    // perf: one walk fills both registries (it was a walk each).
    typeRegistry = new Map<string, string>();
    constraintRegistry = new Map<string, SynxConstraints>();
    collectRegistries(obj, typeRegistry, constraintRegistry);
    // ── :inherit pre-pass (only at root level) ──
    applyInheritance(obj);
    // Remove private blocks (keys starting with _)
    for (const k of Object.keys(obj)) {
      if (k.startsWith('_')) delete obj[k];
    }
    // ── :secret pre-pass ──
    // Mask literal `:secret` values before any resolution so a field that
    // interpolates `{that_key}` can never read the plaintext. Marker
    // application runs after child recursion, so a nested field referencing a
    // root-level secret would otherwise observe it unmasked.
    maskSecretLiterals(obj);
    // ── Load !include directives ──
    if (!includesMap) {
      const inc = (options as any)._includes;
      includesMap = inc
        ? loadIncludes(inc as SynxInclude[], options)
        : new Map<string, SynxObject>();
    }
  }

  // Guard: prevent stack overflow from deeply nested objects
  if (_resolveDepth >= MAX_RESOLVE_DEPTH) {
    for (const k of Object.keys(obj)) {
      if (k !== '__synx') {
        obj[k] = 'NESTING_ERR: maximum object nesting depth exceeded';
      }
    }
    return obj;
  }

  const metaMap: SynxMetaMap | undefined = (obj as any).__synx;

  for (const key of Object.keys(obj)) {
    if (key === '__synx') continue;

    let value = obj[key];

    // Recurse into nested objects
    if (value && typeof value === 'object' && !Array.isArray(value)) {
      resolve(value as SynxObject, options, root, includesMap, _resolveDepth + 1, _currentPath ? `${_currentPath}.${key}` : key);
    }

    // Recurse into arrays of objects
    if (Array.isArray(value)) {
      for (const item of value) {
        if (item && typeof item === 'object' && !Array.isArray(item)) {
          resolve(item as SynxObject, options, root, includesMap, _resolveDepth + 1, _currentPath ? `${_currentPath}.${key}` : key);
        }
      }
    }

    // Apply markers
    if (!metaMap || !metaMap[key]) continue;
    const { markers, args } = metaMap[key];
    // perf: one pass classifies the chain; each marker test below is a bit
    // test instead of a linear `includes` (about 30 of them per key before).
    const mb = markerBits(markers);

    // ── :spam ──
    // Syntax: key:spam:MAX_CALLS:WINDOW_SEC target
    // WINDOW_SEC defaults to 1 when omitted.
    if ((mb & MK_SPAM) !== 0) {
      const idx = markers.indexOf('spam');
      const maxCalls = parseInt(markers[idx + 1] ?? '0', 10);
      const windowSec = Math.max(1, parseInt(markers[idx + 2] ?? '1', 10) || 1);

      if (!Number.isFinite(maxCalls) || maxCalls <= 0) {
        obj[key] = 'SPAM_ERR: invalid limit, use :spam:MAX[:WINDOW_SEC]';
        continue;
      }

      const target = obj[key] === undefined ? key : valueToString(obj[key]);
      const bucketKey = `${key}::${target}`;
      if (!allowSpamAccess(bucketKey, maxCalls, windowSec)) {
        obj[key] = `SPAM_ERR: '${target}' exceeded ${maxCalls} calls per ${windowSec}s`;
        continue;
      }

      const resolvedTarget = deepGet(root, target) ?? deepGet(obj, target);
      if (resolvedTarget !== undefined) {
        obj[key] = resolvedTarget;
      }
    }

    // ── :include / :import ──
    if (((mb & MK_INCLUDE) !== 0 || (mb & MK_IMPORT) !== 0) && typeof obj[key] === 'string') {
      if (!fs || !pathModule) {
        obj[key] = 'INCLUDE_ERR: :include/:import is not supported in browser';
        continue;
      }
      const maxDepth = options.maxIncludeDepth ?? DEFAULT_MAX_INCLUDE_DEPTH;
      const currentDepth = (options as any)._includeDepth ?? 0;
      if (currentDepth >= maxDepth) {
        obj[key] = `INCLUDE_ERR: max include depth (${maxDepth}) exceeded`;
        continue;
      }
      const includePath = String(obj[key]);
      const basePath = options.basePath || (typeof process !== 'undefined' ? process.cwd() : '.');
      try {
        const fullPath = jailPath(basePath, includePath);
        checkFileSize(fullPath);
        const text = fs.readFileSync(fullPath, 'utf-8');
        const { root: included, mode: includedMode } = parseData(text);
        if (includedMode === 'active') {
          resolve(included, { ...options, basePath: pathModule.dirname(fullPath), _includeDepth: currentDepth + 1 } as any, root);
        }
        obj[key] = included;
        // Register the loaded subtree under the key as an alias so
        // `{leaf:<key>}` interpolation finds it (parity with !include directives).
        if (includesMap) {
          includesMap.set(key, included as SynxObject);
        }
      } catch (e: any) {
        obj[key] = `INCLUDE_ERR: ${fmtIoErr(e, includePath)}`;
      }
      continue;
    }

    // ── :env ──
    // Only resolve when the value is the string variable name. Mirrors the
    // Rust engine's `if let Some(Value::String(var_name)) = map.get(key)` guard
    // so that `key:env:default:foo` with no value (which is parsed as a group)
    // keeps its `{}` shape instead of accidentally applying the default.
    if ((mb & MK_ENV) !== 0 && typeof value === 'string') {
      const varName = value;
      const envSource = options.env || (typeof process !== 'undefined' ? process.env : {});
      const envVal = envSource[varName];

      const defaultIdx = markers.indexOf('default');
      const forceString = wantsString(metaMap[key]);
      if (envVal !== undefined && envVal !== '') {
        obj[key] = forceString ? envVal : castPrimitive(envVal);
      } else if (defaultIdx !== -1 && markers.length > defaultIdx + 1) {
        // :env:default:VALUE — join all parts after 'default' back with ':'
        // to preserve IPs (0.0.0.0) and compound values
        const fallback = markers.slice(defaultIdx + 1).join(':');
        obj[key] = forceString ? fallback : castPrimitive(fallback);
      } else {
        obj[key] = null;
      }
    }

    // ── :random ──
    if ((mb & MK_RANDOM) !== 0 && Array.isArray(obj[key])) {
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

    // ── :ref ──
    // Like :alias but feeds the resolved value into subsequent markers.
    // Supports :ref:calc shorthand: key:ref:calc:*2 base_rate → resolves base_rate, then applies "VALUE * 2".
    if ((mb & MK_REF) !== 0 && typeof obj[key] === 'string') {
      const target = obj[key] as string;
      const resolved = deepGet(root, target) ?? deepGet(obj, target);
      if (resolved !== undefined) {
        obj[key] = resolved;
        // If :calc follows, prepend the resolved value to the calc expression
        if ((mb & MK_CALC) !== 0) {
          const calcIdx = markers.indexOf('calc');
          const calcExpr = markers[calcIdx + 1] ?? '';
          if (calcExpr && isNum(resolved)) {
            // Shorthand: :ref:calc:*2 → VALUE * 2, :ref:calc:+10 → VALUE + 10
            const first = calcExpr.charAt(0);
            if ('+-*/%'.includes(first)) {
              const fullExpr = `${resolved} ${calcExpr}`;
              try {
                obj[key] = safeCalc(fullExpr);
              } catch {
                obj[key] = resolved;
              }
            }
          }
        }
      } else {
        obj[key] = null;
      }
    }

    // ── :i18n ──
    // Selects a localized value from a nested object based on options.lang.
    // Plain syntax:        name:i18n\n  en Plains\n  ru Равнины
    // Plural syntax:       title:i18n:item_count\n  en\n    one {count} item\n    other {count} items\n  ru\n    one ...\n    few ...\n    many ...\n    other ...
    if ((mb & MK_I18N) !== 0 && obj[key] && typeof obj[key] === 'object' && !Array.isArray(obj[key])) {
      const translations = obj[key] as SynxObject;
      const lang = options.lang || 'en';
      let val: SynxValue = translations[lang] ?? translations['en'] ?? Object.values(translations)[0] ?? null;

      const i18nIdx = markers.indexOf('i18n');
      const countField = markers[i18nIdx + 1];
      // Pluralisation: when the picked language entry is itself an object
      // (one/few/many/other), choose by count.
      if (countField && val && typeof val === 'object' && !Array.isArray(val)) {
        const pluralForms = val as SynxObject;
        const countRaw = (deepGet(root, countField) ?? deepGet(obj, countField)) as SynxValue;
        const count = typeof countRaw === 'number' ? countRaw
                    : typeof countRaw === 'string' && !isNaN(Number(countRaw)) ? Number(countRaw)
                    : 0;
        const category = pluralCategory(lang, Math.trunc(count));
        const chosen = pluralForms[category] ?? pluralForms['other'] ?? Object.values(pluralForms)[0] ?? null;
        if (typeof chosen === 'string') {
          obj[key] = chosen.split('{count}').join(String(Math.trunc(count)));
        } else {
          obj[key] = chosen;
        }
      } else {
        obj[key] = val;
      }
    }

    // ── :calc ──
    if ((mb & MK_CALC) !== 0 && typeof obj[key] === 'string') {
      let expr = obj[key] as string;
      if (expr.length > MAX_CALC_EXPR_LEN) {
        obj[key] = `CALC_ERR: expression too long (${expr.length} chars, max ${MAX_CALC_EXPR_LEN})`;
        continue;
      }
      // Already-resolved numeric variables: a key of this object shadows a
      // root key. Keys later in document order that still hold a marker value
      // (an unresolved :env string) are not numbers yet and stay as written.
      expr = substituteCalcVars(expr, obj, key, root);
      // Substitute dot-path references (e.g. base.hp, server.port) before evaluating.
      expr = replaceDotPaths(expr, root);
      try {
        obj[key] = safeCalc(expr);
      } catch (e: any) {
        obj[key] = `CALC_ERR: ${e.message}`;
      }
    }

    // ── :alias ──
    if ((mb & MK_ALIAS) !== 0 && typeof obj[key] === 'string') {
      const target = obj[key] as string;
      // Detect direct self-reference (bare key or full dot-path)
      const currentKeyPath = _currentPath ? `${_currentPath}.${key}` : key;
      if (target === key || target === currentKeyPath) {
        obj[key] = `ALIAS_ERR: self-referential alias: ${currentKeyPath} → ${target}`;
      } else {
        // Detect one-hop cycle: a → b → a
        // Only flag as cycle if the target key ALSO has an :alias marker.
        // Without this check, plain string values that happen to match the current
        // key name would produce false-positive ALIAS_ERR results.
        const targetVal = deepGet(root, target);
        // Check if the target key has an :alias marker in metadata.
        // Must look up the target's PARENT object's __synx, not the root's,
        // to correctly handle nested keys like "section.foo".
        const lastDot = target.lastIndexOf('.');
        const targetParentPath = lastDot >= 0 ? target.slice(0, lastDot) : '';
        const targetLeafKey = lastDot >= 0 ? target.slice(lastDot + 1) : target;
        const targetParentObj = targetParentPath ? deepGet(root, targetParentPath) : root;
        const targetParentMeta = (targetParentObj != null && typeof targetParentObj === 'object')
          ? (targetParentObj as any).__synx as Record<string, any> | undefined
          : undefined;
        const targetHasAlias: boolean =
          targetParentMeta?.[targetLeafKey]?.markers?.includes('alias') ?? false;
        const isCycle = targetHasAlias && typeof targetVal === 'string' && targetVal === key;
        if (isCycle) {
          // Stable, order-independent message: sort participants lexicographically
          // so both keys produce the same string regardless of iteration order.
          const a = currentKeyPath <= target ? currentKeyPath : target;
          const b = currentKeyPath <= target ? target : currentKeyPath;
          obj[key] = `ALIAS_ERR: circular alias detected: ${a} → ${b}`;
        } else {
          obj[key] = targetVal ?? null;
        }
      }
    }

    // ── :secret ──
    // Idempotent: the pre-pass (maskSecretLiterals) may already have wrapped a
    // literal secret. Re-wrapping would mask the mask and destroy reveal().
    if ((mb & MK_SECRET) !== 0 && !(obj[key] instanceof SynxSecret)) {
      obj[key] = new SynxSecret(String(obj[key])) as any;
    }

    // ── :unique ──
    if ((mb & MK_UNIQUE) !== 0 && Array.isArray(obj[key])) {
      const seen = new Set<string>();
      obj[key] = (obj[key] as SynxValue[]).filter((item) => {
        const s = valueToString(item);
        if (seen.has(s)) return false;
        seen.add(s);
        return true;
      });
    }

    // ── :geo ──
    if ((mb & MK_GEO) !== 0 && Array.isArray(obj[key])) {
      const region = options.region || 'US';
      const arr = obj[key] as string[];
      const found = arr.find((item) => typeof item === 'string' && item.startsWith(region + ' '));
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

    // ── :template (legacy — handled by auto-{} below) ──

    // ── :split ──
    if ((mb & MK_SPLIT) !== 0 && typeof obj[key] === 'string') {
      const splitIdx = markers.indexOf('split');
      const delimArg = (splitIdx + 1 < markers.length) ? markers[splitIdx + 1] : ',';
      const sep = delimiterFromKeyword(delimArg);
      obj[key] = (obj[key] as string).split(sep).map(s => s.trim()).filter(s => s !== '').map(s => castPrimitive(s));
    }

    // ── :join ──
    if ((mb & MK_JOIN) !== 0 && Array.isArray(obj[key])) {
      const joinIdx = markers.indexOf('join');
      const delimArg = (joinIdx + 1 < markers.length) ? markers[joinIdx + 1] : ',';
      const sep = delimiterFromKeyword(delimArg);
      obj[key] = (obj[key] as SynxValue[]).map(valueToString).join(sep);
    }

    // ── :default (standalone, not combined with :env) ──
    if ((mb & MK_DEFAULT) !== 0 && (mb & MK_ENV) === 0) {
      if (obj[key] === null || obj[key] === undefined || obj[key] === '') {
        const defaultIdx = markers.indexOf('default');
        if (defaultIdx !== -1 && markers.length > defaultIdx + 1) {
          const fallback = markers.slice(defaultIdx + 1).join(':');
          const forceStr = wantsString(metaMap[key]);
          obj[key] = forceStr ? fallback : castPrimitive(fallback);
        }
      }
    }

    // ── :clamp ──
    // Syntax: key:clamp:MIN:MAX value
    if ((mb & MK_CLAMP) !== 0) {
      const idx = markers.indexOf('clamp');
      // A bound is a finite number in the synx-core grammar: `parseFloat`
      // read `5abc` as 5 and `Infinity` as a bound, where Rust skips both.
      const lo = clampBound(markers[idx + 1] ?? '');
      const hi = clampBound(markers[idx + 2] ?? '');
      if (!isNaN(lo) && !isNaN(hi)) {
        if (lo > hi) {
          obj[key] = `CONSTRAINT_ERR: clamp min (${lo}) > max (${hi})`;
        } else if (isNum(obj[key])) {
          obj[key] = Math.min(hi, Math.max(lo, Number(obj[key])));
        }
      }
    }

    // ── :round ──
    // Syntax: key:round:N value  (N = decimal places, default 0)
    if ((mb & MK_ROUND) !== 0) {
      const idx = markers.indexOf('round');
      const decimals = parseInt(markers[idx + 1] ?? '0', 10) || 0;
      if (isNum(obj[key])) {
        const factor = Math.pow(10, decimals);
        obj[key] = Math.round(Number(obj[key]) * factor) / factor;
      }
    }

    // ── :map ──
    // Syntax: key:map:source_key\n  - lookup_val result_text
    if ((mb & MK_MAP) !== 0 && Array.isArray(obj[key])) {
      const idx = markers.indexOf('map');
      const sourceKey = markers[idx + 1] ?? '';
      const lookupVal = sourceKey ? valueToString(deepGet(root, sourceKey) ?? deepGet(obj, sourceKey) ?? '') : '';
      const arr = obj[key] as string[];
      const found = arr.find((item) => {
        if (typeof item !== 'string') return false;
        const s = item;
        const sep = s.indexOf(' ');
        return sep !== -1 && s.substring(0, sep).trim() === lookupVal;
      });
      obj[key] = found
        ? castPrimitive(found.substring(found.indexOf(' ') + 1).trim())
        : null;
    }

    // ── :format ──
    // Syntax: key:format:PATTERN value  (e.g. %.2f, %05d)
    if ((mb & MK_FORMAT) !== 0) {
      const idx = markers.indexOf('format');
      const pattern = markers[idx + 1] ?? '%s';
      obj[key] = applyFormatPattern(pattern, obj[key]);
    }

    // ── :replace:FROM:TO ──    @since 3.6.2
    // Literal substring replacement on a string value. `to` defaults to "" (deletion).
    // Limitation: `from` and `to` cannot themselves contain `:` since the marker
    // chain is colon-delimited; for those cases use {interpolation} instead.
    if ((mb & MK_REPLACE) !== 0 && typeof obj[key] === 'string') {
      const idx = markers.indexOf('replace');
      const from = markers[idx + 1];
      const to = markers[idx + 2] ?? '';
      if (from !== undefined && from !== '') {
        obj[key] = (obj[key] as string).split(from).join(to);
      }
    }

    // ── :sort  /  :sort:desc ──     @since 3.6.2
    // Sort an array. Default direction is ascending; `:sort:desc` reverses.
    // Numbers compare numerically; any other pair compares as text by code
    // point, as synx-core does.
    // spec: `localeCompare` ordered by the machine's locale — `{a}` before
    // `list` here, after it in Rust — and a group as "[object Object]".
    if ((mb & MK_SORT) !== 0 && Array.isArray(obj[key])) {
      const idx = markers.indexOf('sort');
      const dir = markers[idx + 1];
      const sorted = (obj[key] as SynxValue[]).slice().sort((a, b) => {
        if (isNum(a) && isNum(b)) return Number(a) - Number(b);
        return compareUnicodeScalar(valueToString(a), valueToString(b));
      });
      if (dir === 'desc') sorted.reverse();
      obj[key] = sorted;
    }

    // ── :sum ──     @since 3.6.2
    // Sum the numeric items of an array. Non-numeric entries are ignored.
    // Returns an integer when all summands are integers, otherwise a float.
    if ((mb & MK_SUM) !== 0 && Array.isArray(obj[key])) {
      let total = 0;
      let allInt = true;
      for (const v of obj[key] as SynxValue[]) {
        if (isNum(v)) {
          total += Number(v);
          if (!Number.isInteger(Number(v))) allInt = false;
        } else if (typeof v === 'string' && v !== '' && !isNaN(Number(v))) {
          const n = Number(v);
          total += n;
          if (!Number.isInteger(n)) allInt = false;
        }
      }
      obj[key] = allInt ? Math.trunc(total) : total;
    }

    // ── :fallback ──
    // Syntax: key:fallback:DEFAULT_PATH value
    // If value is empty OR file does not exist, use the fallback.
    if ((mb & MK_FALLBACK) !== 0) {
      const idx = markers.indexOf('fallback');
      const defaultVal = markers[idx + 1] ?? '';
      const current = obj[key];
      let useFallback = current === null || current === undefined || current === '';
      if (!useFallback && typeof current === 'string' && fs && pathModule && options.basePath) {
        try {
          const fullPath = jailPath(options.basePath, current);
          useFallback = !fs.existsSync(fullPath);
        } catch {
          useFallback = true; // path escapes jail → treat as missing → use fallback
        }
      }
      if (useFallback && defaultVal) {
        obj[key] = defaultVal;
      }
    }

    // ── :once ──
    // Syntax: key:once  or  key:once:uuid  or  key:once:random  or  key:once:timestamp
    // Generates a value once and persists it in a .synx.lock file.
    if ((mb & MK_ONCE) !== 0) {
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
    if ((mb & MK_VERSION) !== 0 && typeof obj[key] === 'string') {
      const idx = markers.indexOf('version');
      const op = markers[idx + 1] ?? '>=';
      const required = markers[idx + 2] ?? '0';
      obj[key] = compareVersions(obj[key] as string, op, required) as any;
    }

    // ── :watch ──
    // Syntax: key:watch:KEY_PATH ./file  (reads at parse time)
    if ((mb & MK_WATCH) !== 0 && typeof obj[key] === 'string') {
      if (!fs || !pathModule) {
        obj[key] = 'WATCH_ERR: not supported in browser';
      } else {
        const maxDepth = options.maxIncludeDepth ?? DEFAULT_MAX_INCLUDE_DEPTH;
        const currentDepth = (options as any)._includeDepth ?? 0;
        if (currentDepth >= maxDepth) {
          obj[key] = `WATCH_ERR: max include depth (${maxDepth}) exceeded`;
        } else {
          const filePath = obj[key] as string;
          const basePath = options.basePath || (typeof process !== 'undefined' ? process.cwd() : '.');
          const idx = markers.indexOf('watch');
          const keyPath = markers[idx + 1];
          try {
            const fullPath = jailPath(basePath, filePath);
            checkFileSize(fullPath);
            const content = fs.readFileSync(fullPath, 'utf-8');
            const ext = pathModule.extname(fullPath).slice(1);
            if (keyPath) {
              obj[key] = extractFromFileContent(content, keyPath, ext) as any;
            } else {
              obj[key] = content.trim();
            }
          } catch (e: any) {
            obj[key] = `WATCH_ERR: ${fmtIoErr(e, filePath)}`;
          }
        }
      }
    }

    // ── :prompt ──
    // Converts a subtree to a SYNX-formatted string wrapped in a labeled code fence.
    if ((mb & MK_PROMPT) !== 0) {
      const idx = markers.indexOf('prompt');
      const label = markers[idx + 1] ?? key;
      const val = obj[key];
      const synxText = stringifyValue(val, 0);
      obj[key] = `${label} (SYNX):\n\`\`\`synx\n${synxText}\`\`\``;
    }

    // ── :vision ──
    // Metadata-only marker. Recognized (no error), value passes through.

    // ── :audio ──
    // Metadata-only marker. Recognized (no error), value passes through.

    // ── Constraint validation (always last, after all markers resolved) ──
    if (metaMap && metaMap[key]?.constraints) {
      validateConstraints(obj, key, metaMap[key].constraints!);
    }
  }

  // ── Auto-{} interpolation (separate pass, runs on ALL string values) ──
  for (const key of Object.keys(obj)) {
    if (key === '__synx') continue;
    if (typeof obj[key] === 'string' && (obj[key] as string).includes('{')) {
      obj[key] = resolveInterpolation(obj[key] as string, root, obj, includesMap);
    }
  }

  // spec: the reference engine then checks every field against the merged
  // `[...]` constraints declared for its name anywhere in the document
  // (`validate_field_constraints`), and finally `(type)` hints against the
  // resolved value. The constraint pass was missing here until 3.7.4.
  // perf: one walk checks both, constraints then type per field — the same
  // order the two separate walks gave each field.
  if (isTop && ((constraintRegistry && constraintRegistry.size > 0) || (typeRegistry && typeRegistry.size > 0))) {
    validateFields(obj, constraintRegistry, typeRegistry);
  }

  return obj;
}

// ─── Inheritance pre-pass ─────────────────────────────────

/** Markers whose value must be resolved before it becomes a secret — masked by
 *  `:secret` in place, not pre-masked. */
const RESOLVING_BEFORE_SECRET = new Set([
  'env', 'include', 'import', 'watch', 'ref', 'alias', 'calc', 'map', 'i18n',
  'default', 'fallback', 'once', 'split', 'join', 'format', 'replace',
]);

/** Convert every literal `:secret`-marked value to a SynxSecret before
 *  resolution, so no later interpolation can read the plaintext. */
function maskSecretLiterals(obj: SynxObject): void {
  const metaMap: SynxMetaMap | undefined = (obj as any).__synx;
  for (const key of Object.keys(obj)) {
    if (key === '__synx') continue;
    const meta = metaMap?.[key];
    if (
      meta &&
      meta.markers.includes('secret') &&
      !meta.markers.some((m) => RESOLVING_BEFORE_SECRET.has(m))
    ) {
      const v = obj[key];
      if (!(v instanceof SynxSecret) && (v === null || typeof v !== 'object')) {
        obj[key] = new SynxSecret(valueToString(v)) as any;
        continue;
      }
    }
    const child = obj[key];
    if (child && typeof child === 'object' && !Array.isArray(child) && !(child instanceof SynxSecret)) {
      maskSecretLiterals(child as SynxObject);
    }
  }
}

function applyInheritance(obj: SynxObject): void {
  const metaMap: SynxMetaMap | undefined = (obj as any).__synx;
  if (!metaMap) return;

  for (const key of Object.keys(obj)) {
    if (key === '__synx') continue;
    const meta = metaMap[key];
    if (!meta || !meta.markers.includes('inherit')) continue;

    // Two supported syntaxes, as in synx-core:
    //   production:inherit:base:extra  → parents are every marker after `inherit`
    //   production:inherit base        → parser promotes "base" into meta.args
    // Only the first parent was used here; synx-core merges them all in order.
    const idx = meta.markers.indexOf('inherit');
    let parents = meta.markers.slice(idx + 1);
    if (parents.length === 0 && meta.args && meta.args.length > 0) parents = meta.args.slice();
    if (parents.length === 0) continue;

    const childObj = obj[key];
    if (!isPlainObject(childObj)) continue;

    // Parents left to right, then the child. Each field keeps the metadata of
    // the object its value came from: a field the child redefines as a plain
    // literal no longer runs the parent's markers on that literal.
    const merged: SynxObject = {};
    const mergedMeta: SynxMetaMap = {};
    for (const parentName of parents) {
      const parentObj = obj[parentName];
      if (!isPlainObject(parentObj)) continue;
      const parentMeta: SynxMetaMap | undefined = (parentObj as any).__synx;
      for (const k of Object.keys(parentObj as SynxObject)) {
        // A copy, not the parent's own subtree: two children of one parent
        // shared it, so it was resolved twice and edits to one showed in both.
        merged[k] = cloneTree((parentObj as SynxObject)[k]);
        if (parentMeta?.[k]) mergedMeta[k] = parentMeta[k];
        else delete mergedMeta[k];
      }
    }
    const childMeta: SynxMetaMap | undefined = (childObj as any).__synx;
    for (const k of Object.keys(childObj as SynxObject)) {
      merged[k] = (childObj as SynxObject)[k];
      if (childMeta?.[k]) mergedMeta[k] = childMeta[k];
      else delete mergedMeta[k];
    }
    if (Object.keys(mergedMeta).length > 0) {
      Object.defineProperty(merged, '__synx', {
        value: mergedMeta,
        enumerable: false,
        writable: true,
        configurable: true,
      });
    }
    obj[key] = merged;
  }
}

/** Deep copy of a parsed subtree, `__synx` metadata included. */
function cloneTree(v: SynxValue): SynxValue {
  if (Array.isArray(v)) return v.map(cloneTree);
  if (v === null || typeof v !== 'object') return v;
  const out: SynxObject = {};
  for (const k of Object.keys(v)) out[k] = cloneTree((v as SynxObject)[k]);
  const meta = (v as any).__synx;
  if (meta) {
    Object.defineProperty(out, '__synx', { value: meta, enumerable: false, writable: true, configurable: true });
  }
  return out;
}

// ─── Auto-{} interpolation ───────────────────────────────

/**
 * Resolve {key}, {key.nested}, {key:alias}, {key:include} placeholders.
 * - {key}           — look up in root, then local scope
 * - {key:alias}     — look up in included file with that alias
 * - {key:include}   — look up in the first (only) included file
 */
function resolveInterpolation(
  tpl: string,
  root: SynxObject,
  local: SynxObject,
  includesMap?: Map<string, SynxObject>,
): string {
  return tpl.replace(/\{(\w+(?:\.\w+)*)(?::(\w+(?:[./\\][\w./\\]*)?))?\}/g, (_match, ref: string, scope: string | undefined) => {
    if (scope) {
      // {key:alias} or {key:include}
      if (scope === 'include') {
        if (!includesMap || includesMap.size === 0) return _match;
        if (includesMap.size > 1) return 'INCLUDE_ERR: multiple !include — specify alias';
        const firstInclude = includesMap.values().next().value!;
        const val = deepGet(firstInclude, ref);
        return val !== undefined ? valueToString(val) : _match;
      }
      // Look up by alias
      if (includesMap) {
        const incl = includesMap.get(scope);
        if (incl) {
          const val = deepGet(incl, ref);
          return val !== undefined ? valueToString(val) : _match;
        }
      }
      return _match;
    }
    // {key} — local file
    const val = deepGet(root, ref) ?? deepGet(local, ref);
    return val !== undefined ? valueToString(val) : _match;
  });
}

// ─── !include loader ──────────────────────────────────────

function loadIncludes(
  includes: SynxInclude[],
  options: SynxOptions,
): Map<string, SynxObject> {
  const map = new Map<string, SynxObject>();
  if (!fs || !pathModule) return map;
  const basePath = options.basePath || (typeof process !== 'undefined' ? process.cwd() : '.');
  const maxDepth = options.maxIncludeDepth ?? DEFAULT_MAX_INCLUDE_DEPTH;
  const currentDepth = (options as any)._includeDepth ?? 0;
  if (currentDepth >= maxDepth) return map;
  for (const inc of includes) {
    try {
      const fullPath = jailPath(basePath, inc.path);
      checkFileSize(fullPath);
      const text = fs.readFileSync(fullPath, 'utf-8');
      const { root, mode } = parseData(text);
      if (mode === 'active') {
        resolve(root, { ...options, basePath: pathModule.dirname(fullPath), _includeDepth: currentDepth + 1 } as any, undefined, map);
      }
      map.set(inc.alias, root);
    } catch (e: any) {
      // Include loading failed — skip silently
    }
  }
  return map;
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
  depth = 0,
): void {
  if (depth > MAX_RESOLVE_DEPTH) return;
  const metaMap: SynxMetaMap | undefined = (obj as any).__synx;
  if (metaMap) {
    for (const key of Object.keys(metaMap)) {
      const meta = metaMap[key];
      if (!meta) continue;
      if (meta.typeHint && !types.has(key)) types.set(key, meta.typeHint);
      const c = meta.constraints;
      if (c) {
        const existing = constraints.get(key);
        constraints.set(key, existing ? mergeConstraints(existing, c) : { ...c });
      }
    }
  }
  for (const key of Object.keys(obj)) {
    const v = obj[key];
    if (Array.isArray(v)) {
      for (const item of v) if (isPlainObject(item)) collectRegistries(item as SynxObject, types, constraints, depth + 1);
    } else if (isPlainObject(v)) collectRegistries(v as SynxObject, types, constraints, depth + 1);
  }
}

/** Field constraints, then the field's type, for every field — one walk. */
function validateFields(
  obj: SynxObject,
  constraints: Map<string, SynxConstraints> | undefined,
  types: Map<string, string> | undefined,
  depth = 0,
): void {
  if (depth > MAX_RESOLVE_DEPTH) return;
  for (const key of Object.keys(obj)) {
    if (key === '__synx') continue;
    const c = constraints?.get(key);
    if (c) {
      const current = obj[key];
      // A value the per-key pass already replaced is not re-validated: the
      // error string's own length would be checked against min/max.
      const errored = typeof current === 'string'
        && (current.startsWith('CONSTRAINT_ERR:') || current.startsWith('TYPE_ERR:'));
      if (!errored) validateConstraints(obj, key, c);
    }
    const expected = types?.get(key);
    if (expected !== undefined && !valueMatchesType(obj[key], expected)) {
      obj[key] = `TYPE_ERR: '${key}' expected ${expected} but got ${valueTypeName(obj[key])}`;
    }
    const child = obj[key];
    if (Array.isArray(child)) {
      for (const item of child) if (isPlainObject(item)) validateFields(item as SynxObject, constraints, types, depth + 1);
    } else if (isPlainObject(child)) validateFields(child as SynxObject, constraints, types, depth + 1);
  }
}

function isPlainObject(v: unknown): boolean {
  return typeof v === 'object' && v !== null && !Array.isArray(v) && !(v instanceof SynxSecret);
}

function valueMatchesType(v: unknown, t: string): boolean {
  switch (t) {
    case 'int':    return (typeof v === 'number' && Number.isInteger(v)) || typeof v === 'bigint';
    case 'float':  return typeof v === 'number' || typeof v === 'bigint';
    case 'bool':   return typeof v === 'boolean';
    case 'string': return typeof v === 'string' || v instanceof SynxSecret;
    case 'array':  return Array.isArray(v);
    case 'object': return isPlainObject(v);
    default:       return true;
  }
}

function valueTypeName(v: unknown): string {
  if (v === null || v === undefined) return 'null';
  if (v instanceof SynxSecret) return 'secret';
  if (Array.isArray(v)) return 'array';
  if (typeof v === 'number') return Number.isInteger(v) ? 'int' : 'float';
  if (typeof v === 'bigint') return 'int';
  if (typeof v === 'boolean') return 'bool';
  if (typeof v === 'string') return 'string';
  return 'object';
}

// ─── Constraint enforcement ───────────────────────────────────────────────

function validateConstraints(obj: SynxObject, key: string, c: import('./types').SynxConstraints): void {
  const val = obj[key];

  // required
  if (c.required && (val === null || val === undefined || val === '')) {
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
    const ok = (() => {
      switch (c.type) {
        case 'int':    return (typeof val === 'number' && Number.isInteger(val)) || typeof val === 'bigint';
        case 'float':  return typeof val === 'number' || typeof val === 'bigint';
        case 'bool':   return typeof val === 'boolean';
        // A `:secret` is a masked string: `[type:string]:secret:env` must accept it.
        case 'string': return typeof val === 'string' || val instanceof SynxSecret;
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
    // As synx-core reads it: a list, group or secret is "" (`String([1])` let
    // a one-item list pass `[enum:1]`).
    const strVal = typeof val === 'object' ? '' : valueToString(val);
    if (!c.enum.includes(strVal)) {
      obj[key] = `CONSTRAINT_ERR: '${key}' must be one of [${c.enum.join('|')}]`;
      return;
    }
  }

  // min / max  (numbers: value range; strings: length in Unicode scalar
  // values, as synx-core — `val.length` counted UTF-16 units).
  const n = typeof val === 'number' ? val
          : typeof val === 'bigint' ? Number(val)
          : typeof val === 'string' && (c.min !== undefined || c.max !== undefined)
            ? scalarLength(val) : null;
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

  // pattern — length-capped like synx-core; a pattern that nests one
  // quantifier inside another (`^(a+)+$`) is refused rather than run, as in
  // SYNXL since 3.7.2: active mode had no such guard, so the same 88-byte
  // document that 3.7.2 defused for SYNXL still hung the event loop here.
  if (c.pattern && typeof val === 'string' && c.pattern.length <= 256) {
    if (matchesLinear(c.pattern, val) === false) {
      obj[key] = `CONSTRAINT_ERR: '${key}' does not match pattern /${c.pattern}/`;
      return;
    }
  }
}

// ─── New-marker helpers ───────────────────────────────────

function applyFormatPattern(pattern: string, value: unknown): string {
  // spec: only a number is formatted, as in synx-core — a string (a quoted
  // `"3.14"`, an i64 overflow) passes through as written, and a group or list
  // reads as empty instead of "[object Object]".
  if (typeof value === 'bigint') {
    // Integer patterns pad the exact digits; Number() would round them.
    const z = pattern.match(/^%0(\d+)d$/) ?? pattern.match(/^%(\d+)d$/);
    if (z) return String(value).padStart(parseInt(z[1], 10), pattern.charAt(1) === '0' ? '0' : ' ');
    if (!/[fe]/.test(pattern)) return String(value);
    value = Number(value);
  }
  if (typeof value !== 'number') {
    if (value === null || value === undefined) return 'null';
    if (typeof value === 'object' && !(value instanceof SynxSecret)) return '';
    return String(value);
  }
  const n = value;
  // A fractional number only takes a float pattern; with any other it is
  // written as it is, as synx-core does for a Float.
  if (!Number.isInteger(n) && !/[fe]/.test(pattern)) return numberToText(n);

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
  // SYNX key lookup — parse the file and do a deep-get by dot-path
  try {
    const { root: parsed } = parseData(content);
    return deepGet(parsed, keyPath) ?? null;
  } catch { return null; }
}

// ─── Helpers ──────────────────────────────────────────────

/** Serialize a value to SYNX format string (for :prompt marker).
 *  Keys are emitted in sorted order so that LLM prompts are deterministic
 *  across runs and across language bindings (matches the Rust engine). */
function stringifyValue(value: unknown, indent: number): string {
  const sp = ' '.repeat(indent);
  if (value === null || value === undefined) return `${sp}null\n`;
  // SynxSecret is an object, but must serialize as its [SECRET] mask — never recurse into _value.
  if (value instanceof SynxSecret) return `${sp}${String(value)}\n`;
  if (typeof value === 'boolean' || typeof value === 'number') return `${sp}${valueToString(value)}\n`;
  if (typeof value === 'string') return `${sp}${value}\n`;
  if (Array.isArray(value)) {
    let out = '';
    for (const item of value) out += `${sp}  - ${valueToString(item)}\n`;
    return out;
  }
  if (typeof value === 'object') {
    let out = '';
    const obj = value as Record<string, unknown>;
    const keys = Object.keys(obj).filter(k => k !== '__synx').sort();
    for (const k of keys) {
      const v = obj[k];
      if (v instanceof SynxSecret) {
        out += `${sp}${k} ${String(v)}\n`;
      } else if (v && typeof v === 'object' && !Array.isArray(v)) {
        out += `${sp}${k}\n`;
        out += stringifyValue(v, indent + 2);
      } else if (Array.isArray(v)) {
        out += `${sp}${k}\n`;
        for (const item of v) out += `${sp}  - ${valueToString(item)}\n`;
      } else {
        out += `${sp}${k} ${valueToString(v)}\n`;
      }
    }
    return out;
  }
  return `${sp}${String(value)}\n`;
}

/**
 * Whether text taken from outside the document (`:env`, `:default`) must stay
 * a string instead of going through `castPrimitive`. `(string)` always asked for
 * that; since 3.7.4 so does `[type:string]` — before, the cast ran first and the
 * constraint then rejected its own declared type. Mirrors `wants_string` in
 * synx-core.
 */
/**
 * Text of a value placed inside a string — interpolation, `:join`, `:map`,
 * `:prompt` lists. Mirrors synx-core's `value_to_string`: a secret stays
 * masked and a group or list contributes nothing (`String()` gave
 * "[object Object]" and "a,b"), `null` reads as "null".
 */
const CLAMP_BOUND_RE = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/;

/** A `:clamp` bound: a finite number written as synx-core reads one, else NaN. */
function clampBound(s: string): number {
  if (!CLAMP_BOUND_RE.test(s)) return NaN;
  const n = Number(s);
  return Number.isFinite(n) ? n : NaN;
}

function valueToString(v: unknown): string {
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
  return expandExponent(String(n));
}

function wantsString(meta: SynxMeta | undefined): boolean {
  return meta?.typeHint === 'string' || meta?.constraints?.type === 'string';
}

/**
 * Casts text that came from outside the document (`:env`, `:default`, `:split`).
 * Mirrors `cast_primitive` in synx-core: surrounding quotes keep the literal,
 * an integer is an exact i64 (a `bigint` beyond 2^53 — `parseInt` turned a
 * 19-digit id into a different id), and one that overflows i64 is tried as a
 * float, like Rust's `parse::<i64>()` then `parse::<f64>()`.
 */
function castPrimitive(val: string): SynxValue {
  if (val.length >= 2) {
    const first = val[0];
    const last = val[val.length - 1];
    if ((first === '"' && last === '"') || (first === "'" && last === "'")) {
      return val.slice(1, -1);
    }
  }
  if (val === 'true') return true;
  if (val === 'false') return false;
  if (val === 'null') return null;
  if (/^[+-]?\d+$/.test(val)) {
    const int = parseI64(val.startsWith('+') ? val.slice(1) : val);
    if (int !== null) return int;
  }
  if (/^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(val)) {
    const f = Number(val);
    if (Number.isFinite(f)) return f;
  }
  return val;
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

/**
 * Root keys a single-pass token lookup cannot reach, per root object. The root
 * key set is fixed once resolution starts (values change, keys do not).
 */
const ROOT_CALC_SPECIAL = new WeakMap<object, string[]>();

/**
 * A key whole-word substitution can reach in ways a token lookup cannot: one
 * with a non-word character (`max-hp`, `a.b`, `цена`), or one a substituted
 * number can itself spell (`2`, `1e` of `1e+21`, `NaN`, `Infinity`).
 */
function isCalcSpecialKey(k: string): boolean {
  for (let i = 0; i < k.length; i++) if (!isWordChar(k.charCodeAt(i))) return true;
  return /^\d+e?$/.test(k) || k === 'NaN' || k === 'Infinity';
}

function rootCalcSpecial(root: SynxObject): string[] {
  let special = ROOT_CALC_SPECIAL.get(root);
  if (!special) {
    special = Object.keys(root).filter(isCalcSpecialKey);
    ROOT_CALC_SPECIAL.set(root, special);
  }
  return special;
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

/**
 * Substitute numeric variables into a `:calc` expression — a key of `local`
 * first (it shadows the root), then a root key.
 *
 * perf: one pass over the expression's words with a property lookup each. The
 * previous version collected every numeric key of the root into a map and ran
 * a whole-expression `replaceWord` per key, so N calcs over a root of N keys
 * were quadratic (an active document of 2000 sections took 0.6 s). The
 * sequential form remains for documents with a special key holding a number,
 * where it is the exact semantics.
 */
function substituteCalcVars(expr: string, local: SynxObject, selfKey: string, root: SynxObject): string {
  const localIsRoot = local === root;
  let special = false;
  for (const k of rootCalcSpecial(root)) {
    if (isNum(root[k]) && !(localIsRoot && k === selfKey)) { special = true; break; }
  }
  if (!special && !localIsRoot) {
    for (const k of Object.keys(local)) {
      if (k !== selfKey && isNum(local[k]) && isCalcSpecialKey(k)) { special = true; break; }
    }
  }
  if (special) {
    const vars = new Map<string, string>();
    for (const rKey of Object.keys(root)) {
      if (isNum(root[rKey])) vars.set(rKey, String(Number(root[rKey])));
    }
    for (const rKey of Object.keys(local)) {
      if (rKey !== selfKey && isNum(local[rKey])) vars.set(rKey, String(Number(local[rKey])));
    }
    // Longer keys first so `hp` cannot eat into `base_hp`.
    const sorted = [...vars.entries()].sort((a, b) => b[0].length - a[0].length);
    let result = expr;
    for (const [k, v] of sorted) result = replaceWord(result, k, v);
    return result;
  }

  let out = '';
  let i = 0;
  const len = expr.length;
  while (i < len) {
    if (isWordChar(expr.charCodeAt(i))) {
      const start = i;
      while (i < len && isWordChar(expr.charCodeAt(i))) i++;
      const token = expr.substring(start, i);
      let v: unknown;
      if (token !== selfKey && hasOwn.call(local, token) && isNum(local[token])) v = Number(local[token]);
      else if (hasOwn.call(root, token) && isNum(root[token])) v = Number(root[token]);
      out += v === undefined ? token : numberToText(v as number);
    } else {
      const start = i;
      while (i < len && !isWordChar(expr.charCodeAt(i))) i++;
      out += expr.substring(start, i);
    }
  }
  return out;
}

/**
 * Substitute dot-path references (`a.b.c`) in a calc expression with their
 * numeric values from the root tree. Tokens without a dot are left unchanged.
 * Matches the Rust `dot_resolved` pass in `synx-core::engine`.
 */
function replaceDotPaths(expr: string, root: SynxObject): string {
  let out = '';
  let i = 0;
  const len = expr.length;
  while (i < len) {
    const code = expr.charCodeAt(i);
    if (isWordChar(code)) {
      const start = i;
      let hasDot = false;
      while (i < len) {
        const c = expr.charCodeAt(i);
        if (isWordChar(c)) {
          i++;
        } else if (c === 46) { // '.'
          hasDot = true;
          i++;
        } else {
          break;
        }
      }
      const token = expr.substring(start, i);
      if (hasDot && token.indexOf('.') !== -1) {
        const val = deepGet(root, token);
        if (isNum(val)) {
          out += String(Number(val));
          continue;
        }
      }
      out += token;
    } else {
      out += expr[i];
      i++;
    }
  }
  return out;
}

function allowSpamAccess(bucketKey: string, maxCalls: number, windowSec: number): boolean {
  const now = Date.now();
  const windowMs = windowSec * 1000;
  const calls = SPAM_BUCKETS.get(bucketKey) ?? [];
  const filtered = calls.filter((ts) => now - ts <= windowMs);

  if (filtered.length >= maxCalls) {
    SPAM_BUCKETS.set(bucketKey, filtered);
    return false;
  }

  if (filtered.length === 0 && calls.length > 0) {
    SPAM_BUCKETS.delete(bucketKey);
  }

  filtered.push(now);
  SPAM_BUCKETS.set(bucketKey, filtered);
  return true;
}

function deepGet(obj: SynxObject, path: string): SynxValue | undefined {
  // Try direct key first (own property only)
  if (Object.prototype.hasOwnProperty.call(obj, path)) return obj[path];
  // Try dot-path
  const parts = path.split('.');
  let current: any = obj;
  for (const part of parts) {
    if (current == null || typeof current !== 'object') return undefined;
    if (!Object.prototype.hasOwnProperty.call(current, part)) return undefined;
    current = current[part];
  }
  return current;
}
