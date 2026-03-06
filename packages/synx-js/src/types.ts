/**
 * SYNX Types — @aperturesyndicate/synx
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
  | 'watch';

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
}

/** Map of key → metadata for a single object level */
export interface SynxMetaMap {
  [key: string]: SynxMeta;
}

/** Raw parse result before engine resolution */
export interface SynxParseResult {
  mode: SynxMode;
  root: SynxObject;
  locked?: boolean;
}

/** Options for Synx.parse() / Synx.load() */
export interface SynxOptions {
  /** Base directory for :include resolution (default: cwd) */
  basePath?: string;
  /** Override environment variables (for testing) */
  env?: Record<string, string>;
  /** Region code for :geo (e.g. "RU", "US") */
  region?: string;
}
