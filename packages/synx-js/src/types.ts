/**
 * SYNX Types — @aperturesyndicate/synx-format
 * Core type definitions for the SYNX parser.
 */

/**
 * Primitive value types that SYNX supports.
 *
 * `bigint` appears only for integers outside JavaScript's safe range that are
 * still inside i64 — the range the reference implementation represents exactly
 * and `number` cannot. Snowflake ids and BIGINT primary keys live there, and
 * rounding them to the nearest float silently rewrites the key a row is joined
 * on. Everything inside ±2^53-1 stays a plain `number`.
 */
export type SynxPrimitive = string | number | bigint | boolean | null;

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
  | 'audio'
  // 3.6.2 additions
  | 'replace'
  | 'sort'
  | 'sum';

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
  /** The engine's classification of `markers`, filled on first use. */
  bits?: number;
}

/** Declarations of one object's keys, in the order the document makes them. */
export type SynxMetaMap = Map<string, SynxMeta>;

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
  /** File declares `!tool` (LLM tool-call envelope). */
  tool?: boolean;
  /** File declares `!schema` (used with !tool for schema mode). */
  schema?: boolean;
  includes?: SynxInclude[];
  /** Input was dropped: it passed a size, line, block or list limit. */
  truncated?: boolean;
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
  /**
   * Resolve without writing to disk: `:once` returns the value `.synx.lock`
   * already holds, or a new one that is not recorded. For documents the host
   * does not trust with its files.
   */
  readOnly?: boolean;
}

/** Structural diff result from Synx.diff() */
export interface SynxDiff {
  added: Record<string, SynxValue>;
  removed: Record<string, SynxValue>;
  changed: Record<string, { from: SynxValue; to: SynxValue }>;
  unchanged: string[];
}

// ─── SYNXL — "SYNX Lines" record stream (SYNXL format version 1) ──────────

/** Recoverable parse observation kinds (SYNXL §11.2). */
export type SynxlDiagnosticKind =
  | 'MissingFields'
  | 'ExtraFields'
  | 'CastFailed'
  | 'UnknownBlockKey'
  | 'BlockFieldNotDeclared'
  | 'OrphanBlockLine'
  | 'ConstraintViolation'
  | 'RecordTruncated';

/** A non-fatal SYNXL parse observation (SYNXL §11.2). */
export interface SynxlDiagnostic {
  kind: SynxlDiagnosticKind;
  /** 0-based record index in document order. */
  record: number;
  /** 1-based source line number. */
  line: number;
  /** Human-readable description. */
  message: string;
  /** Field name the diagnostic refers to, when applicable. */
  field?: string;
}

/** One declaration from a `!fields` line (SYNXL §5). */
export interface SynxlField {
  /** Field name, compared by exact Unicode scalar sequence. */
  name: string;
  /** Declared `[block]` flag (SYNXL §5.3). */
  block: boolean;
  /** Declared type from `(type)` or `type:<name>`, if any. */
  type?: string;
  /** Declared constraints (SYNXL §5.2). Not enforced unless `validate` is set. */
  constraints?: SynxConstraints;
  /** Verbatim declaration text, re-emitted by the writer (SYNXL §14). */
  decl: string;
}

/** A `!fields` line in effect for a run of records (SYNXL §5). */
export interface SynxlFieldList {
  fields: SynxlField[];
  /** Number of non-block fields — the expected inline part count (SYNXL §5.3). */
  arity: number;
  /** 1-based source line of the `!fields` line. */
  line: number;
}

/** One parsed SYNXL record (record line plus its block). */
export interface SynxlRecord {
  /** 0-based index in document order. */
  index: number;
  /** 1-based source line of the record line. */
  line: number;
  /** Field name → value, restricted to the field list in effect. */
  values: SynxObject;
  /** The field list this record was parsed under. */
  fields: SynxlFieldList;
  /** Diagnostics produced by this record (SYNXL §11.2). */
  diagnostics: SynxlDiagnostic[];
}

/** Whole-document SYNXL parse result. */
export interface SynxlDocument {
  /** Declared format version from the prologue (SYNXL §4.1). */
  version: number;
  records: SynxlRecord[];
  /** Every field list encountered, in document order. */
  fieldLists: SynxlFieldList[];
  /** All diagnostics, in document order (flattened from the records). */
  diagnostics: SynxlDiagnostic[];
}

/** Options for the SYNXL reader. */
export interface SynxlOptions {
  /**
   * Enforce declared constraints and report `ConstraintViolation` diagnostics.
   * Off by default per SYNXL §8.4 (validation is opt-in).
   */
  validate?: boolean;
  /**
   * Per-record byte budget (line plus block). Defaults to the SYNXL §13 limit
   * of 16 MiB; overridable mainly for tests and memory-constrained consumers.
   * Exceeding it truncates the record and records `RecordTruncated`.
   */
  maxRecordBytes?: number;
  /**
   * Report a `RecordTruncated` diagnostic when the input does not end with a
   * newline, i.e. when the final record may have been cut mid-write
   * (SYNXL §15.2). Off by default because a missing final newline is normal.
   */
  reportTruncatedTail?: boolean;
}

/** Options for the SYNXL writer (SYNXL §14). */
export interface SynxlWriteOptions {
  /** Indentation width for block fields. Default 2. */
  indent?: number;
}

// perf: the class is declared in the parser — the one module every entry
// point loads — so that this file is types only and nothing loads it to run.
export { SynxError } from './parser';
