/**
 * SYNX Types — @aperturesyndicate/synx-format
 * Core type definitions for the SYNX parser.
 */

/** Primitive value types that SYNX supports */
export type SynxPrimitive = string | number | boolean | null;

/** A SYNX value can be a primitive, an array, or a nested object */
export type SynxValue = SynxPrimitive | SynxArray | SynxObject;

/** SYNX array (list of values) */
export type SynxArray = SynxValue[];

/** SYNX object (key-value map) */
export interface SynxObject {
  [key: string]: SynxValue;
}

/** File mode: static (no functions) or active (functions + constraints enabled) */
export type SynxMode = 'static' | 'active';

/** Supported function markers */
export type SynxMarker =
  | 'random'
  | 'calc'
  | 'env'
  | 'alias'
  | 'ref'
  | 'inherit'
  | 'i18n'
  | 'secret'
  | 'default'
  | 'unique'
  | 'include'
  | 'geo'
  | 'template'
  | 'split'
  | 'join'
  | 'clamp'
  | 'round'
  | 'map'
  | 'format'
  | 'fallback'
  | 'once'
  | 'version'
  | 'watch'
  | 'prompt'
  | 'vision'
  | 'audio';

/** Constraint types for [] validation */
export interface SynxConstraints {
  min?: number;
  max?: number;
  type?: string;
  required?: boolean;
  pattern?: string;
  enum?: string[];
  readonly?: boolean;
}

/** Internal metadata attached to a key (non-enumerable) */
export interface SynxMeta {
  markers: string[];
  args?: string[];          // e.g. percentages for :random
  constraints?: SynxConstraints;
  typeHint?: string;        // e.g. 'string', 'int', 'float'
}

/** Map of key → metadata for a single object level */
export interface SynxMetaMap {
  [key: string]: SynxMeta;
}

/** Include directive parsed from !include */
export interface SynxInclude {
  path: string;
  alias: string;
}

/** Raw parse result before engine resolution */
export interface SynxParseResult {
  mode: SynxMode;
  root: SynxObject;
  locked?: boolean;
  /** File declares `!llm` (LLM envelope hint; data tree unchanged). @since 3.6.0 */
  llm?: boolean;
  includes?: SynxInclude[];
}

/** Options for Synx.parse() / Synx.load() */
export interface SynxOptions {
  /** Base directory for :include resolution (default: cwd) */
  basePath?: string;
  /** Override environment variables (for testing) */
  env?: Record<string, string>;
  /** Region code for :geo (e.g. "RU", "US") */
  region?: string;
  /** Language code for :i18n (e.g. "en", "ru", "de") */
  lang?: string;
  /** Throw if marker resolution produces runtime error strings (INCLUDE_ERR, WATCH_ERR, etc.) */
  strict?: boolean;
  /** Maximum include/import nesting depth (default: 16) */
  maxIncludeDepth?: number;
}

/** Structural diff result from Synx.diff() */
export interface SynxDiff {
  added: Record<string, SynxValue>;
  removed: Record<string, SynxValue>;
  changed: Record<string, { from: SynxValue; to: SynxValue }>;
  unchanged: string[];
}

/**
 * Typed error thrown by SYNX in strict mode.
 * The `code` field contains the error prefix (e.g. "CALC_ERR", "ALIAS_ERR").
 */
export class SynxError extends Error {
  readonly code: string;

  constructor(message: string) {
    super(message);
    this.name = 'SynxError';
    // Extract prefix up to first ':'
    const colonIdx = message.indexOf(':');
    this.code = colonIdx !== -1 ? message.slice(0, colonIdx) : 'SYNX_ERR';
  }
}
