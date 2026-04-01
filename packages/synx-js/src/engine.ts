/**
 * SYNX Engine — @aperturesyndicate/synx-format
 *
 * Resolves active markers (:random, :calc, :env, :alias, :secret, etc.)
 * in a parsed SYNX object tree. Only runs in !active mode.
 */

import type { SynxObject, SynxValue, SynxMetaMap, SynxOptions, SynxInclude } from './types';
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

// ─── Security constants ───────────────────────────────────

const MAX_CALC_EXPR_LEN = 4096;
const MAX_FILE_SIZE = 10 * 1024 * 1024; // 10 MB
const DEFAULT_MAX_INCLUDE_DEPTH = 16;

/** Maximum object nesting depth for active-mode resolution (prevents stack overflow). */
const MAX_RESOLVE_DEPTH = 512;

/** Ensure a file path stays inside the base directory (path jail). */
function jailPath(base: string, filePath: string): string {
  if (!pathModule) throw new Error('path module not available');
  // Block absolute paths
  if (pathModule.isAbsolute(filePath)) {
    throw new Error(`absolute path not allowed: ${filePath}`);
  }
  const resolved = pathModule.resolve(base, filePath);
  const normalizedBase = pathModule.resolve(base);
  // Ensure resolved path starts with the base directory
  if (!resolved.startsWith(normalizedBase + pathModule.sep) && resolved !== normalizedBase) {
    throw new Error(`path escapes base directory: ${filePath}`);
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

class SynxSecret {
  private _value: string;

  constructor(value: string) {
    this._value = value;
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

const SPAM_BUCKETS = new Map<string, number[]>();

// ─── Engine ───────────────────────────────────────────────

export function resolve(
  obj: SynxObject,
  options: SynxOptions = {},
  root?: SynxObject,
  includesMap?: Map<string, SynxObject>,
  _resolveDepth = 0,
  _currentPath = '',
): SynxObject {
  if (!root) {
    root = obj;
    // ── :inherit pre-pass (only at root level) ──
    applyInheritance(obj);
    // Remove private blocks (keys starting with _)
    for (const k of Object.keys(obj)) {
      if (k.startsWith('_')) delete obj[k];
    }
    // ── Load !include directives ──
    if (!includesMap && (options as any)._includes) {
      includesMap = loadIncludes((options as any)._includes as SynxInclude[], options);
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

    // ── :spam ──
    // Syntax: key:spam:MAX_CALLS:WINDOW_SEC target
    // WINDOW_SEC defaults to 1 when omitted.
    if (markers.includes('spam')) {
      const idx = markers.indexOf('spam');
      const maxCalls = parseInt(markers[idx + 1] ?? '0', 10);
      const windowSec = Math.max(1, parseInt(markers[idx + 2] ?? '1', 10) || 1);

      if (!Number.isFinite(maxCalls) || maxCalls <= 0) {
        obj[key] = 'SPAM_ERR: invalid limit, use :spam:MAX[:WINDOW_SEC]';
        continue;
      }

      const target = String(obj[key] ?? key);
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

    // ── :include ──
    if (markers.includes('include') && typeof obj[key] === 'string') {
      if (!fs || !pathModule) {
        obj[key] = 'INCLUDE_ERR: :include is not supported in browser';
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
      // Check if key has (string) type hint — if so, skip auto-detection
      const forceString = metaMap[key]?.typeHint === 'string';
      if (envVal !== undefined && envVal !== '') {
        obj[key] = forceString ? envVal : (isNaN(Number(envVal)) ? envVal : Number(envVal));
      } else if (defaultIdx !== -1 && markers.length > defaultIdx + 1) {
        // :env:default:VALUE — join all parts after 'default' back with ':'
        // to preserve IPs (0.0.0.0) and compound values
        const fallback = markers.slice(defaultIdx + 1).join(':');
        obj[key] = forceString ? fallback : (isNaN(Number(fallback)) ? fallback : Number(fallback));
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

    // ── :ref ──
    // Like :alias but feeds the resolved value into subsequent markers.
    // Supports :ref:calc shorthand: key:ref:calc:*2 base_rate → resolves base_rate, then applies "VALUE * 2".
    if (markers.includes('ref') && typeof obj[key] === 'string') {
      const target = obj[key] as string;
      const resolved = deepGet(root, target) ?? deepGet(obj, target);
      if (resolved !== undefined) {
        obj[key] = resolved;
        // If :calc follows, prepend the resolved value to the calc expression
        if (markers.includes('calc')) {
          const calcIdx = markers.indexOf('calc');
          const calcExpr = markers[calcIdx + 1] ?? '';
          if (calcExpr && typeof resolved === 'number') {
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
    // Syntax: name:i18n\n  en Plains\n  ru Равнины
    if (markers.includes('i18n') && obj[key] && typeof obj[key] === 'object' && !Array.isArray(obj[key])) {
      const translations = obj[key] as SynxObject;
      const lang = options.lang || 'en';
      const val = translations[lang] ?? translations['en'] ?? Object.values(translations)[0] ?? null;
      obj[key] = val;
    }

    // ── :calc ──
    if (markers.includes('calc') && typeof obj[key] === 'string') {
      let expr = obj[key] as string;
      if (expr.length > MAX_CALC_EXPR_LEN) {
        obj[key] = `CALC_ERR: expression too long (${expr.length} chars, max ${MAX_CALC_EXPR_LEN})`;
        continue;
      }
      // Collect already-resolved numeric variables from root + local scope.
      // Keys that appear later in iteration order and still hold a marker
      // value (e.g. an unresolved :env string) are not yet numbers and will
      // be absent from vars — place :calc keys after their dependencies.
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
          obj[key] = `ALIAS_ERR: circular alias detected: ${key} → ${target}`;
        } else {
          obj[key] = targetVal ?? null;
        }
      }
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

    // ── :template (legacy — handled by auto-{} below) ──

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
          const fallback = markers.slice(defaultIdx + 1).join(':');
          const forceStr = metaMap[key]?.typeHint === 'string';
          obj[key] = forceStr ? fallback : (isNaN(Number(fallback)) ? fallback : Number(fallback));
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
            obj[key] = `WATCH_ERR: ${e.message}`;
          }
        }
      }
    }

    // ── :prompt ──
    // Converts a subtree to a SYNX-formatted string wrapped in a labeled code fence.
    if (markers.includes('prompt')) {
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

  return obj;
}

// ─── Inheritance pre-pass ─────────────────────────────────

function applyInheritance(obj: SynxObject): void {
  const metaMap: SynxMetaMap | undefined = (obj as any).__synx;
  if (!metaMap) return;

  for (const key of Object.keys(obj)) {
    if (key === '__synx') continue;
    const meta = metaMap[key];
    if (!meta || !meta.markers.includes('inherit')) continue;

    const idx = meta.markers.indexOf('inherit');
    const parentName = meta.markers[idx + 1];
    if (!parentName) continue;

    const parentObj = obj[parentName];
    if (!parentObj || typeof parentObj !== 'object' || Array.isArray(parentObj)) continue;

    const childObj = obj[key];
    if (!childObj || typeof childObj !== 'object' || Array.isArray(childObj)) continue;

    // Merge: parent fields first, child fields override
    const merged: SynxObject = { ...(parentObj as SynxObject), ...(childObj as SynxObject) };
    // Copy over __synx metadata from both parent and child
    const parentMeta: SynxMetaMap | undefined = (parentObj as any).__synx;
    const childMeta: SynxMetaMap | undefined = (childObj as any).__synx;
    if (parentMeta || childMeta) {
      const mergedMeta = { ...(parentMeta || {}), ...(childMeta || {}) };
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
        return val != null ? String(val) : _match;
      }
      // Look up by alias
      if (includesMap) {
        const incl = includesMap.get(scope);
        if (incl) {
          const val = deepGet(incl, ref);
          return val != null ? String(val) : _match;
        }
      }
      return _match;
    }
    // {key} — local file
    const val = deepGet(root, ref) ?? deepGet(local, ref);
    return val != null ? String(val) : _match;
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

  // pattern (regex match — reject pathological patterns to prevent ReDoS)
  if (c.pattern && typeof val === 'string') {
    if (c.pattern.length > 128) return;
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
  // SYNX key lookup — parse the file and do a deep-get by dot-path
  try {
    const { root: parsed } = parseData(content);
    return deepGet(parsed, keyPath) ?? null;
  } catch { return null; }
}

// ─── Helpers ──────────────────────────────────────────────

/** Serialize a value to SYNX format string (for :prompt marker). */
function stringifyValue(value: unknown, indent: number): string {
  const sp = ' '.repeat(indent);
  if (value === null || value === undefined) return `${sp}null\n`;
  if (typeof value === 'boolean' || typeof value === 'number') return `${sp}${value}\n`;
  if (typeof value === 'string') return `${sp}${value}\n`;
  if (Array.isArray(value)) {
    let out = '';
    for (const item of value) out += `${sp}  - ${String(item)}\n`;
    return out;
  }
  if (typeof value === 'object') {
    let out = '';
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      if (k === '__synx') continue;
      if (v && typeof v === 'object' && !Array.isArray(v)) {
        out += `${sp}${k}\n`;
        out += stringifyValue(v, indent + 2);
      } else if (Array.isArray(v)) {
        out += `${sp}${k}\n`;
        for (const item of v) out += `${sp}  - ${String(item)}\n`;
      } else {
        out += `${sp}${k} ${v ?? 'null'}\n`;
      }
    }
    return out;
  }
  return `${sp}${String(value)}\n`;
}

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
