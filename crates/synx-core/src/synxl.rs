//! SYNXL — "SYNX Lines": the record-stream counterpart of JSONL and CSV.
//!
//! Reference implementation of `docs/spec/SYNXL-1-NORMATIVE.md` (format
//! version 1), which embeds the SYNX 3.7 language for the block portion of a
//! record. Section references in this file (`§7.1`, `§9.4`, …) are to that
//! document unless prefixed with `SYNX`.
//!
//! Three entry points:
//!
//! * [`parse_lines`] — whole-document parse into a [`SynxlDocument`].
//! * [`SynxlReader`] — streaming `Iterator` over records (§15.1).
//! * [`write_document`] / [`write_lines`] — canonical serialization (§14).
//!
//! ```rust
//! use synx_core::synxl;
//!
//! let doc = synxl::parse_lines("!synxl 1\n!fields id[type:int] ; name\n1 ; Wario\n").unwrap();
//! assert_eq!(doc.to_json(), r#"[{"id":1,"name":"Wario"}]"#);
//! ```

use std::collections::HashMap;
use std::fmt;

use memchr::memchr;

use crate::parser::{self, ParserOptions};
use crate::value::{Constraints, Value};

// ─── §13 Resource limits ─────────────────────────────────────

/// SYNXL format version implemented by this crate (§1.3).
pub const SYNXL_VERSION: u32 = 1;

/// Per **record** (record line plus block). An oversized record is truncated
/// at a valid UTF-8 boundary and reported as [`DiagnosticKind::RecordTruncated`]
/// — never a hard error, because one pathological row must not invalidate a
/// multi-gigabyte dataset (§11.1).
pub const MAX_SYNXL_RECORD_BYTES: usize = 16 * 1024 * 1024;

/// Fields per field list. Exceeding it is a hard error.
pub const MAX_SYNXL_FIELDS: usize = 4_096;

/// Field-name length in UTF-8 bytes. Exceeding it is a hard error.
pub const MAX_SYNXL_FIELD_NAME_BYTES: usize = 255;

/// Field lists per document. Exceeding it is a hard error.
pub const MAX_SYNXL_FIELD_LISTS: usize = 65_536;

/// Records per **in-memory** parse. [`SynxlReader`] has no record-count limit.
pub const MAX_SYNXL_RECORDS: usize = 16_777_216;

/// Nesting depth guard for the block writer (§14) — mirrors the crate's other
/// serializers, which all cap recursion rather than trusting the value tree.
const MAX_WRITE_DEPTH: usize = 64;

/// Decimal places used when a float has to be expanded out of exponent form so
/// that it survives a SYNX cast round-trip (see `write_float`). Large enough to
/// spell out the smallest subnormal `f64` exactly.
const FLOAT_EXPANSION_PRECISION: usize = 1_100;

// ─── §5 Field list ───────────────────────────────────────────

/// One declaration from a `!fields` line: `name(type)[constraints]`.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDecl {
    /// Field name. Compared by exact Unicode scalar sequence (§5.1).
    pub name: String,
    /// The `(type)` production only. `type:<name>` lives in `constraints`;
    /// use [`FieldDecl::type_name`] for the effective hint (§8.2).
    pub type_hint: Option<String>,
    /// Parsed by the SYNX constraint parser (§5.2). Not enforced unless the
    /// caller opts into validating mode (§8.4).
    pub constraints: Constraints,
    /// The SYNXL-only `block` flag (§5.3).
    pub block: bool,
}

impl FieldDecl {
    /// A plain, untyped, unconstrained field.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_hint: None,
            constraints: Constraints::default(),
            block: false,
        }
    }

    /// The same, flagged `[block]` (§5.3).
    pub fn new_block(name: impl Into<String>) -> Self {
        Self { block: true, ..Self::new(name) }
    }

    /// Effective type hint: `(type)` wins over `type:<name>` (§8.2).
    pub fn type_name(&self) -> Option<&str> {
        self.type_hint
            .as_deref()
            .or(self.constraints.type_name.as_deref())
    }
}

/// The ordered fields in effect for a run of records (§5).
#[derive(Debug, Clone, PartialEq)]
pub struct FieldList {
    fields: Vec<FieldDecl>,
    arity: usize,
    source: String,
    /// 1-based source line of the `!fields` line (0 when synthesised).
    pub line: usize,
}

impl FieldList {
    /// Build a field list, precomputing its arity.
    ///
    /// No validation is performed here; [`parse_lines`] rejects duplicate
    /// names and the other §11.1 conditions while reading the `!fields` line.
    pub fn new(fields: Vec<FieldDecl>) -> Self {
        let arity = fields.iter().filter(|f| !f.block).count();
        Self { fields, arity, source: String::new(), line: 0 }
    }

    /// The `!fields` line exactly as it appeared in the document, trimmed of
    /// surrounding whitespace — empty for a programmatically built list.
    ///
    /// A tool that splits or concatenates documents (§15.3) has to re-emit the
    /// field list in effect at the split point; replaying the original bytes
    /// keeps the shards byte-identical to the source, which re-deriving the
    /// declaration from [`FieldList::fields`] would not (§14 normalises the
    /// separator to `; ` and drops the author's spacing).
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Declarations in source order, block fields included.
    pub fn fields(&self) -> &[FieldDecl] {
        &self.fields
    }

    /// Number of fields **without** the `block` flag — the expected count of
    /// inline parts per record (§5.3).
    pub fn arity(&self) -> usize {
        self.arity
    }

    /// Look a declaration up by exact name.
    pub fn get(&self, name: &str) -> Option<&FieldDecl> {
        self.fields.iter().find(|f| f.name == name)
    }
}

// ─── §11 Error model ─────────────────────────────────────────

/// Hard-error conditions (§11.1). Every one of these aborts the parse; no
/// partial result is produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SynxlErrorKind {
    /// Missing or malformed `!synxl <version>` prologue (§4.1).
    MissingPrologue,
    /// Prologue declares a version this implementation does not support, or a
    /// repeated prologue declares a different version (§1.3, §4.1). For format
    /// version 1 the two conditions coincide: "different" implies "not 1".
    UnsupportedVersion,
    /// A line at indent 0 beginning with `!` that is neither a prologue nor a
    /// field list — including SYNX directives such as `!active` (§4.1).
    UnknownDirective,
    /// Record line encountered while no field list is in effect (§4.2).
    NoFieldList,
    /// Empty or unparsable `!fields` line (§5).
    MalformedFieldList,
    /// Duplicate field name within one field list (§5.1).
    DuplicateField,
    /// A marker run — chain (`:a:b`) or single (`:custom`) — in a field
    /// declaration (§5.2).
    MarkerChain,
    /// `random` / `random:int` / `random:float` / `random:bool` hint (§8.3).
    NonDeterministicHint,
    /// `block` combined with `(type)` or `type:` (§5.3).
    BlockWithType,
    /// A field list whose every field carries `block`, i.e. arity 0 (§5.3.4).
    ZeroArity,
    /// **Writer only.** A value has no SYNXL rendering: it must be promoted to
    /// a block (§14.3) but promoting it would leave the field list at arity 0
    /// (§5.3.4). §14.3 requires rejecting such a value loudly — emitting a
    /// document that reads back differently, or one that no reader accepts, is
    /// not permitted. Cannot arise for a value obtained by parsing (§14.1).
    Unwritable,
    /// A §13 limit other than `MAX_SYNXL_RECORD_BYTES` was exceeded.
    LimitExceeded,
}

impl SynxlErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SynxlErrorKind::MissingPrologue => "MissingPrologue",
            SynxlErrorKind::UnsupportedVersion => "UnsupportedVersion",
            SynxlErrorKind::UnknownDirective => "UnknownDirective",
            SynxlErrorKind::NoFieldList => "NoFieldList",
            SynxlErrorKind::MalformedFieldList => "MalformedFieldList",
            SynxlErrorKind::DuplicateField => "DuplicateField",
            SynxlErrorKind::MarkerChain => "MarkerChain",
            SynxlErrorKind::NonDeterministicHint => "NonDeterministicHint",
            SynxlErrorKind::BlockWithType => "BlockWithType",
            SynxlErrorKind::ZeroArity => "ZeroArity",
            SynxlErrorKind::Unwritable => "Unwritable",
            SynxlErrorKind::LimitExceeded => "LimitExceeded",
        }
    }
}

impl fmt::Display for SynxlErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A hard error (§11.1) with the 1-based source line that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynxlError {
    pub kind: SynxlErrorKind,
    pub line: usize,
    pub message: String,
}

impl SynxlError {
    /// Build an error.
    ///
    /// Public because wrappers around this crate — language bindings in
    /// particular — need to raise the same error type for conditions they
    /// enforce themselves (a writer rejecting `block` combined with a type,
    /// say) instead of assembling the struct literally.
    pub fn new(kind: SynxlErrorKind, line: usize, message: impl Into<String>) -> Self {
        Self { kind, line, message: message.into() }
    }
}

impl fmt::Display for SynxlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SYNXL {} at line {}: {}", self.kind, self.line, self.message)
    }
}

impl std::error::Error for SynxlError {}

/// Recoverable parse observations (§11.2). Reported, never dropped silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// Fewer inline parts than the arity; trailing fields set to `null` (§7.3).
    MissingFields,
    /// More inline parts than the arity; the surplus was discarded (§7.3).
    ExtraFields,
    /// Typed casting failed; the field was set to `null` (§8.2).
    CastFailed,
    /// A block key that matches no declared field (§9.3).
    UnknownBlockKey,
    /// A block key matching a field that is **not** declared `[block]` (§9.3).
    BlockFieldNotDeclared,
    /// A line at indent > 0 where no record is open — for example right after
    /// a field list. Discarded, and attached to the *following* record (§11.2).
    OrphanBlockLine,
    /// A declared constraint was violated — validating mode only (§8.4).
    ConstraintViolation,
    /// The record exceeded `MAX_SYNXL_RECORD_BYTES` and was truncated (§13).
    RecordTruncated,
}

impl DiagnosticKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticKind::MissingFields => "MissingFields",
            DiagnosticKind::ExtraFields => "ExtraFields",
            DiagnosticKind::CastFailed => "CastFailed",
            DiagnosticKind::UnknownBlockKey => "UnknownBlockKey",
            DiagnosticKind::BlockFieldNotDeclared => "BlockFieldNotDeclared",
            DiagnosticKind::OrphanBlockLine => "OrphanBlockLine",
            DiagnosticKind::ConstraintViolation => "ConstraintViolation",
            DiagnosticKind::RecordTruncated => "RecordTruncated",
        }
    }
}

impl fmt::Display for DiagnosticKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One diagnostic: record index (0-based), source line (1-based), kind, and a
/// human-readable message (§11.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub record_index: usize,
    pub line: usize,
    pub kind: DiagnosticKind,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} (record {}, line {}): {}",
            self.kind, self.record_index, self.line, self.message
        )
    }
}

// ─── Options ─────────────────────────────────────────────────

/// Parse options. Defaults follow §8.4: validation is opt-in.
#[derive(Debug, Clone, Default)]
pub struct SynxlOptions {
    /// Enforce declared constraints and report violations as
    /// [`DiagnosticKind::ConstraintViolation`] (§8.4).
    pub validate: bool,
}

// ─── Parse results ───────────────────────────────────────────

/// One record produced by [`SynxlReader`].
#[derive(Debug, Clone, PartialEq)]
pub struct SynxlRecord {
    /// 0-based position in the document.
    pub index: usize,
    /// 1-based source line of the record line.
    pub line: usize,
    /// Index into [`SynxlReader::field_lists`] of the list in effect.
    pub field_list: usize,
    /// Always a [`Value::Object`].
    pub value: Value,
    /// Diagnostics produced by this record alone (§11.2).
    pub diagnostics: Vec<Diagnostic>,
}

/// A fully materialised SYNXL document.
#[derive(Debug, Clone, PartialEq)]
pub struct SynxlDocument {
    /// Format version from the prologue (§4.1).
    pub version: u32,
    /// Records in document order; each is a [`Value::Object`].
    pub records: Vec<Value>,
    /// Every field list in declaration order (§4.2).
    pub field_lists: Vec<FieldList>,
    /// `records[i]` was parsed under `field_lists[record_field_lists[i]]`.
    pub record_field_lists: Vec<usize>,
    /// 1-based source line of each record's record line.
    pub record_lines: Vec<usize>,
    /// All diagnostics, in record order (§11.2).
    pub diagnostics: Vec<Diagnostic>,
}

impl SynxlDocument {
    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The field list that was in effect for record `index`.
    pub fn field_list_for(&self, index: usize) -> Option<&FieldList> {
        self.record_field_lists
            .get(index)
            .and_then(|i| self.field_lists.get(*i))
    }

    /// Canonical JSON **array** projection (§12.1).
    pub fn to_json(&self) -> String {
        records_to_json_array(&self.records)
    }

    /// Canonical **NDJSON** projection (§12.2).
    pub fn to_ndjson(&self) -> String {
        records_to_ndjson(&self.records)
    }

    /// Canonical SYNXL serialization (§14).
    ///
    /// Infallible in practice for a parsed document (§14.1); see
    /// [`write_document`] for the one condition that rejects.
    pub fn to_synxl(&self) -> Result<String, SynxlError> {
        write_document(self)
    }
}

/// Canonical JSON array projection of a record slice (§12.1).
pub fn records_to_json_array(records: &[Value]) -> String {
    let mut out = String::with_capacity(records.len() * 64 + 2);
    out.push('[');
    for (i, rec) in records.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        crate::write_json(&mut out, rec);
    }
    out.push(']');
    out
}

/// Canonical NDJSON projection of a record slice (§12.2): one canonical object
/// per line, `LF`-separated, no enclosing array.
pub fn records_to_ndjson(records: &[Value]) -> String {
    let mut out = String::with_capacity(records.len() * 64);
    for rec in records {
        crate::write_json(&mut out, rec);
        out.push('\n');
    }
    out
}

// ─── §15.1 Streaming reader ──────────────────────────────────

/// A single physical line, kept as offsets into the document.
///
/// Offsets rather than slices are what let one state machine serve both the
/// borrowing and the owning reader: the state carries no lifetime, so the
/// owning reader can keep its `String` beside it without `unsafe`.
#[derive(Debug, Clone, Copy)]
struct Line {
    /// Byte offset of the line's first byte.
    start: usize,
    /// Byte offset one past its last byte, `CR` / `LF` excluded.
    end: usize,
    /// 1-based line number.
    no: usize,
}

impl Line {
    /// Line content with `CR`/`LF` removed, indentation intact.
    #[inline]
    fn raw<'t>(&self, text: &'t str) -> &'t str {
        &text[self.start..self.end]
    }

    #[inline]
    fn indent(&self, text: &str) -> usize {
        let raw = self.raw(text);
        raw.len() - raw.trim_start().len()
    }
}

/// What a non-empty line at indent 0 turned out to be (§4.3, §6).
enum LineKind {
    /// A comment, a directive, or a field list — already applied, nothing to
    /// hand back to the caller.
    Skip,
    /// A record line; the caller must frame its block and call `build`.
    Record,
}

/// One record's text, already framed by whichever line source produced it.
///
/// Framing is the only part of reading that differs between an in-memory
/// document (slices) and an `io::BufRead` (a bounded buffer); everything the
/// format defines happens in [`ReaderCore::build`], which takes this.
struct RecordFrame<'t> {
    /// The record line, `CR`/`LF` excluded.
    line: &'t str,
    /// 1-based number of the record line.
    line_no: usize,
    /// The block's raw text, original indentation intact, `LF`-joined (§9.3).
    block: &'t str,
    /// 1-based number of the block's first line (0 when there is no block).
    block_first_no: usize,
    /// Total bytes before truncation, when §13's per-record cap was hit.
    truncated_total: Option<usize>,
}

/// The reader state machine, independent of how the document is owned.
///
/// Both [`SynxlReader`] (borrowing) and [`SynxlReaderOwned`] (owning) drive
/// this one type, handing it their text on each call. The parse path is shared
/// rather than forked, and neither wrapper — nor any binding built on them —
/// needs `unsafe` to keep a document alive alongside its reader.
#[derive(Debug)]
struct ReaderCore {
    /// Byte offset of the next line to read.
    pos: usize,
    /// 1-based number of the next line to read.
    line_no: usize,
    /// Look-ahead line pushed back by the block scanner.
    pending: Option<Line>,
    /// §11.2 — `OrphanBlockLine` diagnostics found while looking for the next
    /// record, which they are attached to.
    pending_diagnostics: Vec<Diagnostic>,
    in_block_comment: bool,
    field_lists: Vec<FieldList>,
    current: Option<usize>,
    record_index: usize,
    version: u32,
    done: bool,
    opts: SynxlOptions,
}

impl ReaderCore {
    /// Fresh state, before any input has been seen.
    fn empty(opts: SynxlOptions) -> Self {
        Self {
            pos: 0,
            line_no: 1,
            pending: None,
            pending_diagnostics: Vec::new(),
            in_block_comment: false,
            field_lists: Vec::new(),
            current: None,
            record_index: 0,
            version: 0,
            done: false,
            opts,
        }
    }

    /// Build the state and validate the prologue eagerly (§4.1).
    fn start(text: &str, opts: SynxlOptions) -> Result<Self, SynxlError> {
        let mut core = Self {
            // §3.1 — a leading U+FEFF is ignored. Skipping it by offset instead
            // of re-slicing keeps every later offset relative to the caller's
            // own text, which is what the owning reader indexes into.
            pos: if text.starts_with('\u{feff}') {
                '\u{feff}'.len_utf8()
            } else {
                0
            },
            line_no: 1,
            pending: None,
            pending_diagnostics: Vec::new(),
            in_block_comment: false,
            field_lists: Vec::new(),
            current: None,
            record_index: 0,
            version: 0,
            done: false,
            opts,
        };
        core.read_prologue(text)?;
        Ok(core)
    }

    /// Next physical line, honouring the block scanner's push-back.
    fn read_line(&mut self, text: &str) -> Option<Line> {
        if let Some(line) = self.pending.take() {
            return Some(line);
        }
        let bytes = text.as_bytes();
        if self.pos >= bytes.len() {
            return None;
        }
        let start = self.pos;
        let (mut end, next) = match memchr(b'\n', &bytes[start..]) {
            Some(rel) => (start + rel, start + rel + 1),
            None => (bytes.len(), bytes.len()),
        };
        // §3.2 — a CR immediately before LF is not part of the line.
        if end > start && bytes[end - 1] == b'\r' {
            end -= 1;
        }
        self.pos = next;
        let no = self.line_no;
        self.line_no += 1;
        Some(Line { start, end, no })
    }

    /// One iteration step: the body behind both readers' `Iterator::next`.
    fn next_item(&mut self, text: &str) -> Option<Result<SynxlRecord, SynxlError>> {
        if self.done {
            return None;
        }
        match self.next_record(text) {
            Ok(Some(rec)) => Some(Ok(rec)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(err) => {
                // §11.1 — a hard error ends the document.
                self.done = true;
                Some(Err(err))
            }
        }
    }

    /// §4.1 — does the prologue scan skip this line? Toggles `###` state.
    ///
    /// Shared by every line source: the prologue must be found the same way
    /// whether the document is a `&str` or an `io::BufRead`.
    fn prologue_skip(&mut self, trimmed: &str) -> bool {
        if trimmed.is_empty() {
            return true;
        }
        // `###` is matched before the `#` line-comment rule (§4.3).
        if trimmed == "###" {
            self.in_block_comment = !self.in_block_comment;
            return true;
        }
        if self.in_block_comment {
            return true;
        }
        trimmed.starts_with('#') || trimmed.starts_with("//")
    }

    /// The error raised when input ends before a prologue is found (§4.1).
    fn missing_prologue(last_line: usize) -> SynxlError {
        SynxlError::new(
            SynxlErrorKind::MissingPrologue,
            last_line.max(1),
            "document has no `!synxl <version>` prologue",
        )
    }

    /// §4.1 — the first non-empty, non-comment line MUST be the prologue.
    fn read_prologue(&mut self, text: &str) -> Result<(), SynxlError> {
        while let Some(line) = self.read_line(text) {
            let trimmed = line.raw(text).trim();
            if self.prologue_skip(trimmed) {
                continue;
            }
            self.version = parse_prologue(trimmed, line.no)?;
            return Ok(());
        }
        Err(Self::missing_prologue(self.line_no.saturating_sub(1)))
    }

    /// §11.2 — record an indented line that has no open record.
    fn note_orphan(&mut self, trimmed: &str, no: usize) {
        self.pending_diagnostics.push(Diagnostic {
            record_index: self.record_index,
            line: no,
            kind: DiagnosticKind::OrphanBlockLine,
            message: format!("indented line `{}` has no open record; discarded", elide(trimmed)),
        });
    }

    /// Decide what a non-empty line at indent 0 is (§4.3, §6).
    ///
    /// This is the whole structural vocabulary of the format in one place, so
    /// every line source classifies identically; only the byte plumbing around
    /// it differs.
    fn classify(&mut self, trimmed: &str, no: usize) -> Result<LineKind, SynxlError> {
        if trimmed == "###" {
            self.in_block_comment = !self.in_block_comment;
            return Ok(LineKind::Skip);
        }
        if self.in_block_comment {
            return Ok(LineKind::Skip);
        }

        // §4.3 — the `!` forms are matched before the comment rules.
        if trimmed.starts_with('!') {
            if let Some(rest) = trimmed.strip_prefix("!fields") {
                if rest.is_empty() || starts_with_wsp(rest) {
                    let list = parse_field_list(rest.trim(), trimmed, no)?;
                    self.push_field_list(list)?;
                    return Ok(LineKind::Skip);
                }
            }
            if let Some(rest) = trimmed.strip_prefix("!synxl") {
                if rest.is_empty() || starts_with_wsp(rest) {
                    // §4.1 — a repeated prologue is accepted and ignored when it
                    // declares the same version (shards are concatenable), and
                    // rejected when it does not.
                    parse_prologue(trimmed, no)?;
                    return Ok(LineKind::Skip);
                }
            }
            // §4.1 — anything else beginning with `!` at indent 0 is a hard
            // error. Ignoring it would let a typo (`!filds …`) leave the
            // previous field list in effect and mis-populate every subsequent
            // record without any signal.
            return Err(SynxlError::new(
                SynxlErrorKind::UnknownDirective,
                no,
                format!(
                    "`{}` is neither a prologue nor a field list; a record starting with `!` must quote it",
                    elide(trimmed)
                ),
            ));
        }
        if trimmed.starts_with('#') || trimmed.starts_with("//") {
            return Ok(LineKind::Skip);
        }

        // §6 — anything else at indent 0 is a record line. The SYNX §7
        // first-character filter deliberately does NOT apply here.
        Ok(LineKind::Record)
    }

    /// The field list in effect, or the §4.2 hard error.
    fn require_field_list(&self, no: usize) -> Result<usize, SynxlError> {
        self.current.ok_or_else(|| {
            SynxlError::new(
                SynxlErrorKind::NoFieldList,
                no,
                "record line with no `!fields` in effect",
            )
        })
    }

    /// Install a freshly parsed field list, enforcing §13's list cap.
    fn push_field_list(&mut self, list: FieldList) -> Result<(), SynxlError> {
        if self.field_lists.len() >= MAX_SYNXL_FIELD_LISTS {
            return Err(SynxlError::new(
                SynxlErrorKind::LimitExceeded,
                list.line,
                format!("more than {} field lists in one document", MAX_SYNXL_FIELD_LISTS),
            ));
        }
        self.field_lists.push(list);
        self.current = Some(self.field_lists.len() - 1);
        Ok(())
    }

    /// Advance to the next record, consuming structural lines on the way.
    fn next_record(&mut self, text: &str) -> Result<Option<SynxlRecord>, SynxlError> {
        loop {
            let line = match self.read_line(text) {
                Some(l) => l,
                None => return Ok(None),
            };
            let trimmed = line.raw(text).trim();
            if trimmed.is_empty() {
                continue;
            }
            // §3.4 — indent > 0 belongs to the current record's block. Reaching
            // this loop means no record is open, so the line is orphaned: it is
            // discarded and reported against the *following* record (§11.2).
            if line.indent(text) > 0 {
                self.note_orphan(trimmed, line.no);
                continue;
            }
            if let LineKind::Skip = self.classify(trimmed, line.no)? {
                continue;
            }

            let fl_idx = self.require_field_list(line.no)?;

            // §9.1 — the block is the maximal run of following lines up to the
            // next non-empty line at indent 0. Empty lines do not terminate it
            // (§3.5).
            let mut block_start: Option<usize> = None;
            let mut block_end: Option<usize> = None;
            let mut block_first_no = 0usize;
            loop {
                let next = match self.read_line(text) {
                    Some(l) => l,
                    None => break,
                };
                if next.raw(text).trim().is_empty() {
                    continue;
                }
                if next.indent(text) == 0 {
                    self.pending = Some(next);
                    break;
                }
                if block_start.is_none() {
                    block_start = Some(next.start);
                    block_first_no = next.no;
                }
                block_end = Some(next.end);
            }
            // Block lines are contiguous in the source, so the whole block is
            // one slice — no per-line copying, and `|+` base-indent locking
            // still sees the original indentation (§9.3).
            let block_raw = match (block_start, block_end) {
                (Some(s), Some(e)) => &text[s..e],
                _ => "",
            };

            // §13 — truncate rather than fail. The record line is kept first:
            // it carries the inline fields, the more valuable half.
            let mut record_line = line.raw(text);
            let mut block_text = block_raw;
            let total = record_line.len().saturating_add(block_text.len());
            let mut truncated_total = None;
            if total > MAX_SYNXL_RECORD_BYTES {
                truncated_total = Some(total);
                if record_line.len() >= MAX_SYNXL_RECORD_BYTES {
                    record_line = truncate_utf8(record_line, MAX_SYNXL_RECORD_BYTES);
                    block_text = "";
                } else {
                    block_text =
                        truncate_utf8(block_text, MAX_SYNXL_RECORD_BYTES - record_line.len());
                }
            }

            return self
                .build(
                    fl_idx,
                    RecordFrame {
                        line: record_line,
                        line_no: line.no,
                        block: block_text,
                        block_first_no,
                        truncated_total,
                    },
                )
                .map(Some);
        }
    }

    /// Turn one framed record into a [`SynxlRecord`] (§7, §8, §9, §11.2).
    ///
    /// Framing — finding the record line and its block — belongs to the line
    /// source; everything the format defines happens here, once.
    fn build(&mut self, fl_idx: usize, frame: RecordFrame<'_>) -> Result<SynxlRecord, SynxlError> {
        let RecordFrame {
            line: record_line,
            line_no,
            block: block_text,
            block_first_no,
            truncated_total,
        } = frame;

        let index = self.record_index;
        self.record_index += 1;
        // Orphan lines physically precede the record they are attached to, so
        // they lead its diagnostic list (§11.2 orders the other kinds).
        let mut diagnostics: Vec<Diagnostic> = std::mem::take(&mut self.pending_diagnostics);
        if let Some(total) = truncated_total {
            diagnostics.push(Diagnostic {
                record_index: index,
                line: line_no,
                kind: DiagnosticKind::RecordTruncated,
                message: format!(
                    "record is {} bytes, truncated to the {} byte limit",
                    total, MAX_SYNXL_RECORD_BYTES
                ),
            });
        }

        // §9.3 — delegate the block to the SYNX parser *before* borrowing the
        // field list, so the borrow checker keeps `self` free for the read.
        let block_root = if block_text.trim().is_empty() {
            None
        } else {
            // §9.4 — directives are disabled inside the embedded parse, and
            // §9.5 — no `!active` mode means no metadata capture.
            match parser::parse_with(block_text, ParserOptions { directives: false }).root {
                Value::Object(map) => Some(map),
                _ => None,
            }
        };

        let fl = &self.field_lists[fl_idx];
        let mut obj: HashMap<String, Value> = HashMap::with_capacity(fl.fields.len());

        if record_line.trim() == ";" {
            // §7.2 — the all-null record. Matched *before* the §7.1 split,
            // whose result would otherwise depend on the arity: two null parts
            // at arity 2, but a spurious `MissingFields` at arity 3. At arity 1
            // this form is the only representation an all-null record has, an
            // empty line being invisible (§3.5).
            for field in fl.fields.iter().filter(|f| !f.block) {
                obj.insert(field.name.clone(), Value::Null);
            }
        } else {
            // §7 — inline fields, positionally matched against non-block fields.
            let mut inline_fields = fl.fields.iter().filter(|f| !f.block);
            let mut parts = PartSplitter::new(record_line);
            let mut part_count = 0usize;
            loop {
                let part = match parts.next() {
                    Some(p) => p,
                    None => break,
                };
                part_count += 1;
                match inline_fields.next() {
                    Some(field) => {
                        let value = cast_part(field, part, index, line_no, &mut diagnostics);
                        obj.insert(field.name.clone(), value);
                    }
                    // §7.3 — surplus parts are discarded but still counted.
                    None => {}
                }
            }
            let mut missing = 0usize;
            for field in inline_fields {
                obj.insert(field.name.clone(), Value::Null);
                missing += 1;
            }
            if missing > 0 {
                diagnostics.push(Diagnostic {
                    record_index: index,
                    line: line_no,
                    kind: DiagnosticKind::MissingFields,
                    message: format!(
                        "record has {} inline part(s), field list declares {}; {} trailing field(s) set to null",
                        part_count,
                        fl.arity(),
                        missing
                    ),
                });
            } else if part_count > fl.arity() {
                diagnostics.push(Diagnostic {
                    record_index: index,
                    line: line_no,
                    kind: DiagnosticKind::ExtraFields,
                    message: format!(
                        "record has {} inline part(s), field list declares {}; {} discarded",
                        part_count,
                        fl.arity(),
                        part_count - fl.arity()
                    ),
                });
            }
        }

        // §9.2 — a block field with no block value is null.
        for field in fl.fields.iter().filter(|f| f.block) {
            obj.insert(field.name.clone(), Value::Null);
        }

        // §11.2 — block diagnostics report the line *inside the block* that
        // carries the offending key, so the block's key/line map is built the
        // first time one is needed (never for a clean record).
        let mut key_lines: Option<HashMap<&str, usize>> = None;

        // §9.3 — match block keys by exact name, visiting them in lexicographic
        // order so the diagnostic sequence is reproducible despite the HashMap.
        if let Some(map) = block_root {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            for (key, value) in entries {
                match fl.get(&key) {
                    Some(field) if field.block => {
                        obj.insert(field.name.clone(), value);
                    }
                    Some(_) => {
                        let at = key_lines
                            .get_or_insert_with(|| block_key_lines(block_text, block_first_no))
                            .get(key.as_str())
                            .copied()
                            .unwrap_or(line_no);
                        diagnostics.push(Diagnostic {
                            record_index: index,
                            line: at,
                            kind: DiagnosticKind::BlockFieldNotDeclared,
                            message: format!(
                                "block key `{}` matches a field that is not declared [block]; inline value kept",
                                key
                            ),
                        });
                    }
                    None => {
                        let at = key_lines
                            .get_or_insert_with(|| block_key_lines(block_text, block_first_no))
                            .get(key.as_str())
                            .copied()
                            .unwrap_or(line_no);
                        diagnostics.push(Diagnostic {
                            record_index: index,
                            line: at,
                            kind: DiagnosticKind::UnknownBlockKey,
                            message: format!("block key `{}` matches no declared field", key),
                        });
                    }
                }
            }
        }

        // §8.4 — validation is opt-in.
        if self.opts.validate {
            for field in fl.fields.iter() {
                if let Some(value) = obj.get(&field.name) {
                    if let Some(msg) = check_constraints(&field.name, value, &field.constraints) {
                        // §11.2 — the record line for an inline field, the
                        // offending block line for a block field.
                        let at = if field.block {
                            key_lines
                                .get_or_insert_with(|| block_key_lines(block_text, block_first_no))
                                .get(field.name.as_str())
                                .copied()
                                .unwrap_or(line_no)
                        } else {
                            line_no
                        };
                        diagnostics.push(Diagnostic {
                            record_index: index,
                            line: at,
                            kind: DiagnosticKind::ConstraintViolation,
                            message: msg,
                        });
                    }
                }
            }
        }

        Ok(SynxlRecord {
            index,
            line: line_no,
            field_list: fl_idx,
            value: Value::Object(obj),
            diagnostics,
        })
    }
}

/// Streaming record reader over borrowed text (§15.1).
///
/// Record boundaries are decidable from a single byte (§3.4), so records are
/// produced incrementally without materialising the document. The prologue is
/// validated eagerly by [`SynxlReader::new`]; every later hard error surfaces
/// as an `Err` item, after which iteration stops.
///
/// Use [`SynxlReaderOwned`] when the reader has to outlive the expression that
/// produced the text — an FFI wrapper, for instance.
///
/// ```rust
/// use synx_core::synxl::SynxlReader;
///
/// let src = "!synxl 1\n!fields a ; b\n1 ; 2\n3 ; 4\n";
/// let mut n = 0;
/// for rec in SynxlReader::new(src).unwrap() {
///     assert!(rec.unwrap().diagnostics.is_empty());
///     n += 1;
/// }
/// assert_eq!(n, 2);
/// ```
#[derive(Debug)]
pub struct SynxlReader<'a> {
    text: &'a str,
    core: ReaderCore,
}

impl<'a> SynxlReader<'a> {
    /// Open a reader, validating the prologue (§4.1).
    pub fn new(text: &'a str) -> Result<Self, SynxlError> {
        Self::with_options(text, SynxlOptions::default())
    }

    /// Open a reader with explicit options (§8.4).
    pub fn with_options(text: &'a str, opts: SynxlOptions) -> Result<Self, SynxlError> {
        Ok(Self { text, core: ReaderCore::start(text, opts)? })
    }

    /// Declared format version (§4.1).
    pub fn version(&self) -> u32 {
        self.core.version
    }

    /// Field lists seen so far, in declaration order.
    pub fn field_lists(&self) -> &[FieldList] {
        &self.core.field_lists
    }

    /// The field list currently in effect, if any (§4.2).
    pub fn field_list(&self) -> Option<&FieldList> {
        self.core.current.map(|i| &self.core.field_lists[i])
    }

    /// Consume the reader, keeping the field lists it collected.
    pub fn into_field_lists(self) -> Vec<FieldList> {
        self.core.field_lists
    }

    /// Diagnostics that were found after the last record and therefore had no
    /// record to attach to — `OrphanBlockLine` at end of input (§11.2). Empty
    /// until iteration finishes.
    pub fn trailing_diagnostics(&self) -> &[Diagnostic] {
        &self.core.pending_diagnostics
    }
}

impl Iterator for SynxlReader<'_> {
    type Item = Result<SynxlRecord, SynxlError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.core.next_item(self.text)
    }
}

/// Streaming record reader that **owns** its document (§15.1).
///
/// Identical to [`SynxlReader`] in behaviour and code path — the two share one
/// state machine — but it carries the text itself, so it has no lifetime
/// parameter and can be stored in a struct, returned from a function, or
/// handed to an FFI object without the caller keeping the source alive by hand.
/// That is the shape language bindings need, and it removes the only reason
/// they would otherwise have had to reach for `unsafe`.
///
/// ```rust
/// use synx_core::synxl::SynxlReaderOwned;
///
/// fn open() -> SynxlReaderOwned {
///     // The `String` is moved into the reader; nothing outlives it.
///     SynxlReaderOwned::new(String::from("!synxl 1\n!fields a\n1\n2\n")).unwrap()
/// }
///
/// let records: Vec<_> = open().map(|r| r.unwrap()).collect();
/// assert_eq!(records.len(), 2);
/// ```
#[derive(Debug)]
pub struct SynxlReaderOwned {
    text: String,
    core: ReaderCore,
}

impl SynxlReaderOwned {
    /// Open a reader over an owned document, validating the prologue (§4.1).
    pub fn new(text: String) -> Result<Self, SynxlError> {
        Self::with_options(text, SynxlOptions::default())
    }

    /// Open an owning reader with explicit options (§8.4).
    pub fn with_options(text: String, opts: SynxlOptions) -> Result<Self, SynxlError> {
        let core = ReaderCore::start(&text, opts)?;
        Ok(Self { text, core })
    }

    /// Declared format version (§4.1).
    pub fn version(&self) -> u32 {
        self.core.version
    }

    /// Field lists seen so far, in declaration order.
    pub fn field_lists(&self) -> &[FieldList] {
        &self.core.field_lists
    }

    /// The field list currently in effect, if any (§4.2).
    pub fn field_list(&self) -> Option<&FieldList> {
        self.core.current.map(|i| &self.core.field_lists[i])
    }

    /// Consume the reader, keeping the field lists it collected.
    pub fn into_field_lists(self) -> Vec<FieldList> {
        self.core.field_lists
    }

    /// Diagnostics recorded after the last record (§11.2). See
    /// [`SynxlReader::trailing_diagnostics`].
    pub fn trailing_diagnostics(&self) -> &[Diagnostic] {
        &self.core.pending_diagnostics
    }

    /// The document being read.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Consume the reader and give the document back.
    pub fn into_text(self) -> String {
        self.text
    }
}

impl Iterator for SynxlReaderOwned {
    type Item = Result<SynxlRecord, SynxlError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.core.next_item(&self.text)
    }
}

// ─── §15.1 Streaming from `io::BufRead` ──────────────────────

/// What can go wrong while streaming from an [`io::BufRead`].
///
/// The two arms are kept apart deliberately: a [`SynxlError`] says the document
/// is malformed and is the same verdict every conforming implementation must
/// reach, while an [`io::Error`] says nothing about the document at all — the
/// disk, socket, or pipe failed. Folding the second into the first would make
/// a transient network hiccup indistinguishable from a spec violation.
#[derive(Debug)]
pub enum SynxlStreamError {
    /// A §11.1 hard error in the document.
    Format(SynxlError),
    /// The underlying reader failed, or produced bytes that are not UTF-8 (§3.1).
    Io(std::io::Error),
}

impl SynxlStreamError {
    /// The format error, if this is one.
    pub fn as_format(&self) -> Option<&SynxlError> {
        match self {
            SynxlStreamError::Format(e) => Some(e),
            SynxlStreamError::Io(_) => None,
        }
    }

    /// The I/O error, if this is one.
    pub fn as_io(&self) -> Option<&std::io::Error> {
        match self {
            SynxlStreamError::Io(e) => Some(e),
            SynxlStreamError::Format(_) => None,
        }
    }
}

impl fmt::Display for SynxlStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SynxlStreamError::Format(e) => write!(f, "{}", e),
            SynxlStreamError::Io(e) => write!(f, "I/O error while reading SYNXL: {}", e),
        }
    }
}

impl std::error::Error for SynxlStreamError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SynxlStreamError::Format(e) => Some(e),
            SynxlStreamError::Io(e) => Some(e),
        }
    }
}

impl From<SynxlError> for SynxlStreamError {
    fn from(e: SynxlError) -> Self {
        SynxlStreamError::Format(e)
    }
}

impl From<std::io::Error> for SynxlStreamError {
    fn from(e: std::io::Error) -> Self {
        SynxlStreamError::Io(e)
    }
}

/// Streaming record reader over an [`io::BufRead`] — the reader §15.1 asks for.
///
/// [`SynxlReader`] and [`SynxlReaderOwned`] stream *records* out of a document
/// that is already in memory; this one streams the *document* too. §13
/// deliberately removed the whole-file byte cap ("datasets are routinely
/// gigabytes"), which only means something if a reader never has to hold the
/// file, and §3.4 made record boundaries decidable from a single byte precisely
/// so that buffering one record is enough.
///
/// Live memory is therefore one record — its line plus its block — and
/// `MAX_SYNXL_RECORD_BYTES` (§13) is the real bound on it: an oversized record
/// is cut at the same byte offset the in-memory readers cut it at, reported
/// with the same `RecordTruncated` diagnostic, and the rest of the document
/// keeps parsing.
///
/// Parsing is the same code as the other two readers; only the framing — how
/// bytes become a record line and a block — differs.
///
/// ```rust
/// use std::io::Cursor;
/// use synx_core::synxl::SynxlStreamReader;
///
/// let src = Cursor::new("!synxl 1\n!fields a ; b\n1 ; 2\n3 ; 4\n");
/// let mut n = 0;
/// for rec in SynxlStreamReader::new(src).unwrap() {
///     assert!(rec.unwrap().diagnostics.is_empty());
///     n += 1;
/// }
/// assert_eq!(n, 2);
/// ```
#[derive(Debug)]
pub struct SynxlStreamReader<R: std::io::BufRead> {
    inner: R,
    core: ReaderCore,
    /// Byte scratch for the line being read.
    scratch: Vec<u8>,
    /// The current line, `CR`/`LF` excluded.
    cur: String,
    /// 1-based number of `cur`.
    cur_no: usize,
    /// Bytes the §13 cap dropped from `cur`.
    cur_dropped: usize,
    /// The indent-0 line that ended the previous record's block, pushed back.
    lookahead: Option<(String, usize, usize)>,
    /// 1-based number of the last line read from `inner`.
    line_no: usize,
    /// The record being assembled (§13 bounds both buffers).
    record: String,
    block: String,
    /// Blank lines seen inside a block, held back until content follows so a
    /// trailing run of them stays out of the block (§9.1).
    deferred: String,
    done: bool,
}

impl<R: std::io::BufRead> SynxlStreamReader<R> {
    /// Open a streaming reader, validating the prologue (§4.1).
    pub fn new(inner: R) -> Result<Self, SynxlStreamError> {
        Self::with_options(inner, SynxlOptions::default())
    }

    /// Open a streaming reader with explicit options (§8.4).
    pub fn with_options(inner: R, opts: SynxlOptions) -> Result<Self, SynxlStreamError> {
        let mut reader = Self {
            inner,
            core: ReaderCore::empty(opts),
            scratch: Vec::with_capacity(256),
            cur: String::with_capacity(256),
            cur_no: 0,
            cur_dropped: 0,
            lookahead: None,
            line_no: 0,
            record: String::new(),
            block: String::new(),
            deferred: String::new(),
            done: false,
        };
        reader.read_prologue()?;
        Ok(reader)
    }

    /// Declared format version (§4.1).
    pub fn version(&self) -> u32 {
        self.core.version
    }

    /// Field lists seen so far, in declaration order.
    pub fn field_lists(&self) -> &[FieldList] {
        &self.core.field_lists
    }

    /// The field list currently in effect, if any (§4.2).
    pub fn field_list(&self) -> Option<&FieldList> {
        self.core.current.map(|i| &self.core.field_lists[i])
    }

    /// Consume the reader, keeping the field lists it collected.
    pub fn into_field_lists(self) -> Vec<FieldList> {
        self.core.field_lists
    }

    /// Diagnostics recorded after the last record (§11.2). See
    /// [`SynxlReader::trailing_diagnostics`].
    pub fn trailing_diagnostics(&self) -> &[Diagnostic] {
        &self.core.pending_diagnostics
    }

    /// Consume the reader and give the underlying source back.
    pub fn into_inner(self) -> R {
        self.inner
    }

    /// Advance `cur` to the next line. `Ok(false)` at end of input.
    fn advance(&mut self) -> Result<bool, SynxlStreamError> {
        if let Some((text, no, dropped)) = self.lookahead.take() {
            self.cur = text;
            self.cur_no = no;
            self.cur_dropped = dropped;
            return Ok(true);
        }
        let dropped =
            match read_capped_line(&mut self.inner, &mut self.scratch, MAX_SYNXL_RECORD_BYTES)? {
                Some(d) => d,
                None => return Ok(false),
            };
        // §13 — a cut must land on a valid UTF-8 boundary; the cap can only
        // have split a scalar, so at most three bytes come back off.
        if dropped > 0 {
            for _ in 0..3 {
                if std::str::from_utf8(&self.scratch).is_ok() {
                    break;
                }
                self.scratch.pop();
            }
        }
        let text = std::str::from_utf8(&self.scratch).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("line {} is not valid UTF-8 (§3.1): {}", self.line_no + 1, e),
            )
        })?;
        self.cur.clear();
        // §3.1 — a byte order mark at the start of input is ignored.
        self.cur.push_str(if self.line_no == 0 {
            text.strip_prefix('\u{feff}').unwrap_or(text)
        } else {
            text
        });
        self.line_no += 1;
        self.cur_no = self.line_no;
        self.cur_dropped = dropped;
        Ok(true)
    }

    /// Push the current line back so the next `advance` returns it again.
    fn push_back(&mut self) {
        self.lookahead = Some((std::mem::take(&mut self.cur), self.cur_no, self.cur_dropped));
    }

    /// §4.1 — the first non-empty, non-comment line MUST be the prologue.
    fn read_prologue(&mut self) -> Result<(), SynxlStreamError> {
        loop {
            if !self.advance()? {
                return Err(ReaderCore::missing_prologue(self.line_no).into());
            }
            let trimmed = self.cur.trim();
            if self.core.prologue_skip(trimmed) {
                continue;
            }
            self.core.version = parse_prologue(trimmed, self.cur_no)?;
            return Ok(());
        }
    }

    /// Advance to the next record, framing it into `record` / `block`.
    fn next_record(&mut self) -> Result<Option<SynxlRecord>, SynxlStreamError> {
        loop {
            if !self.advance()? {
                return Ok(None);
            }
            let trimmed = self.cur.trim();
            if trimmed.is_empty() {
                continue;
            }
            // §3.4 — indent > 0 with no record open is an orphan (§11.2).
            if self.cur.len() - self.cur.trim_start().len() > 0 {
                let no = self.cur_no;
                self.core.note_orphan(trimmed, no);
                continue;
            }
            if let LineKind::Skip = self.core.classify(trimmed, self.cur_no)? {
                continue;
            }

            let record_no = self.cur_no;
            let fl_idx = self.core.require_field_list(record_no)?;
            self.record.clear();
            self.record.push_str(&self.cur);
            // §13 — the budget is the record line plus its block, and the line
            // is kept first: it carries the inline fields.
            let mut total = self.record.len() + self.cur_dropped;
            self.block.clear();
            self.deferred.clear();
            let mut block_first_no = 0usize;

            // §9.1 — the block runs to the next non-empty line at indent 0.
            loop {
                if !self.advance()? {
                    break;
                }
                if self.cur.trim().is_empty() {
                    // §3.5 — blank lines never terminate a block, but a
                    // trailing run of them is not part of it either, so they
                    // wait here until content proves they were interior.
                    if !self.block.is_empty() {
                        self.deferred.push('\n');
                        self.deferred.push_str(&self.cur);
                    }
                    continue;
                }
                if self.cur.len() - self.cur.trim_start().len() == 0 {
                    self.push_back();
                    break;
                }
                if block_first_no == 0 {
                    block_first_no = self.cur_no;
                }
                let sep = usize::from(!self.block.is_empty());
                total += self.deferred.len() + sep + self.cur.len() + self.cur_dropped;
                // Cut at exactly the offset the in-memory readers cut at, so
                // the two paths agree byte-for-byte on a truncated record.
                let room = (MAX_SYNXL_RECORD_BYTES + self.deferred.len())
                    .saturating_sub(self.record.len() + self.block.len());
                if room > self.deferred.len() + sep {
                    let budget = room - self.deferred.len() - sep;
                    self.block.push_str(&self.deferred);
                    if sep == 1 {
                        self.block.push('\n');
                    }
                    let piece_len = truncate_utf8(&self.cur, budget).len();
                    self.block.push_str(&self.cur[..piece_len]);
                }
                self.deferred.clear();
            }

            let truncated_total = if total > MAX_SYNXL_RECORD_BYTES { Some(total) } else { None };
            let frame = RecordFrame {
                line: &self.record,
                line_no: record_no,
                block: &self.block,
                block_first_no,
                truncated_total,
            };
            return self.core.build(fl_idx, frame).map(Some).map_err(Into::into);
        }
    }
}

impl<R: std::io::BufRead> Iterator for SynxlStreamReader<R> {
    type Item = Result<SynxlRecord, SynxlStreamError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.next_record() {
            Ok(Some(rec)) => Some(Ok(rec)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(err) => {
                // §11.1 — a hard error ends the document; an I/O failure means
                // there is nothing left to read from either.
                self.done = true;
                Some(Err(err))
            }
        }
    }
}

/// Read one line into `out`, keeping at most `cap` bytes of it.
///
/// The trailing `LF` and a `CR` before it are not stored (§3.2). Returns the
/// number of bytes dropped by the cap, or `None` at end of input — which is
/// what bounds live memory when a hostile document has no newline in it.
fn read_capped_line<R: std::io::BufRead>(
    reader: &mut R,
    out: &mut Vec<u8>,
    cap: usize,
) -> std::io::Result<Option<usize>> {
    out.clear();
    let mut dropped = 0usize;
    let mut saw_any = false;
    loop {
        let (consumed, done) = {
            let available = match reader.fill_buf() {
                Ok(b) => b,
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if available.is_empty() {
                break;
            }
            saw_any = true;
            match memchr(b'\n', available) {
                Some(i) => {
                    push_capped(out, &available[..i], cap, &mut dropped);
                    (i + 1, true)
                }
                None => {
                    let len = available.len();
                    push_capped(out, available, cap, &mut dropped);
                    (len, false)
                }
            }
        };
        reader.consume(consumed);
        if done {
            break;
        }
    }
    if !saw_any {
        return Ok(None);
    }
    if out.last() == Some(&b'\r') {
        out.pop();
    }
    Ok(Some(dropped))
}

fn push_capped(out: &mut Vec<u8>, chunk: &[u8], cap: usize, dropped: &mut usize) {
    let room = cap.saturating_sub(out.len());
    let take = room.min(chunk.len());
    out.extend_from_slice(&chunk[..take]);
    *dropped += chunk.len() - take;
}

// ─── Whole-document parse ────────────────────────────────────

/// Parse a whole SYNXL document (§4–§10).
///
/// Hard errors (§11.1) are returned as `Err`; recoverable observations live in
/// [`SynxlDocument::diagnostics`] (§11.2).
pub fn parse_lines(text: &str) -> Result<SynxlDocument, SynxlError> {
    parse_lines_with(text, &SynxlOptions::default())
}

/// [`parse_lines`] with explicit options (§8.4).
pub fn parse_lines_with(text: &str, opts: &SynxlOptions) -> Result<SynxlDocument, SynxlError> {
    let mut reader = SynxlReader::with_options(text, opts.clone())?;
    let mut records = Vec::new();
    let mut record_field_lists = Vec::new();
    let mut record_lines = Vec::new();
    let mut diagnostics = Vec::new();

    loop {
        let item = match reader.next() {
            Some(item) => item,
            None => break,
        };
        let mut rec = item?;
        // §13 — the record cap applies to the in-memory parse only.
        if records.len() >= MAX_SYNXL_RECORDS {
            return Err(SynxlError::new(
                SynxlErrorKind::LimitExceeded,
                rec.line,
                format!("more than {} records in an in-memory parse", MAX_SYNXL_RECORDS),
            ));
        }
        records.push(rec.value);
        record_field_lists.push(rec.field_list);
        record_lines.push(rec.line);
        diagnostics.append(&mut rec.diagnostics);
    }

    // §11.2 — orphan lines after the last record have no record to attach to,
    // but must still be reported.
    diagnostics.extend_from_slice(reader.trailing_diagnostics());

    let version = reader.version();
    Ok(SynxlDocument {
        version,
        records,
        field_lists: reader.into_field_lists(),
        record_field_lists,
        record_lines,
        diagnostics,
    })
}

/// Map each top-level block key to the 1-based source line that declares it.
///
/// §11.2 requires `UnknownBlockKey` / `BlockFieldNotDeclared` to report the
/// line inside the block, but the SYNX parser returns no positions. The keys
/// that end up at the root of the embedded document are those at the block's
/// shallowest indent (SYNX §8.6 stack repair), so re-scanning that one level is
/// enough — and it only runs when a diagnostic is actually being emitted.
fn block_key_lines(block: &str, first_line_no: usize) -> HashMap<&str, usize> {
    let mut min_indent = usize::MAX;
    for line in block.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.trim().is_empty() {
            continue;
        }
        min_indent = min_indent.min(line.len() - line.trim_start().len());
    }

    let mut map: HashMap<&str, usize> = HashMap::new();
    for (i, line) in block.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let trimmed = line.trim();
        if trimmed.is_empty() || line.len() - line.trim_start().len() != min_indent {
            continue;
        }
        // Lines SYNX never turns into a key (SYNX §7) plus `!` lines, which
        // SYNXL discards inside a block (§9.4).
        if trimmed.starts_with("- ") || trimmed.starts_with('!') {
            continue;
        }
        match trimmed.as_bytes()[0] {
            b'[' | b':' | b'-' | b'#' | b'/' | b'(' => continue,
            _ => {}
        }
        let key_end = trimmed
            .find(|c: char| c == ' ' || c == '\t' || c == '[' || c == ':' || c == '(')
            .unwrap_or(trimmed.len());
        map.entry(&trimmed[..key_end]).or_insert(first_line_no + i);
    }
    map
}

// ─── §4.1 Prologue ───────────────────────────────────────────

fn parse_prologue(trimmed: &str, line: usize) -> Result<u32, SynxlError> {
    let rest = match trimmed.strip_prefix("!synxl") {
        Some(r) if starts_with_wsp(r) => r,
        _ => {
            return Err(SynxlError::new(
                SynxlErrorKind::MissingPrologue,
                line,
                format!("expected `!synxl <version>`, found `{}`", elide(trimmed)),
            ))
        }
    };
    let version = rest.trim();
    // The grammar is `"!synxl" 1*WSP version LF` — nothing may follow.
    if version.is_empty() || !version.bytes().all(|b| b.is_ascii_digit()) {
        return Err(SynxlError::new(
            SynxlErrorKind::MissingPrologue,
            line,
            format!("prologue version must be a decimal integer, found `{}`", elide(version)),
        ));
    }
    match version.parse::<u32>() {
        Ok(v) if v == SYNXL_VERSION => Ok(v),
        _ => Err(SynxlError::new(
            SynxlErrorKind::UnsupportedVersion,
            line,
            format!("SYNXL version `{}` is not supported (this build implements {})", version, SYNXL_VERSION),
        )),
    }
}

// ─── §5 Field-list parsing ───────────────────────────────────

/// Parse the text following `!fields` into a [`FieldList`] (§5).
///
/// `source` is the whole `!fields` line, kept verbatim for tools that re-emit
/// it (see [`FieldList::source`]).
fn parse_field_list(rest: &str, source: &str, line: usize) -> Result<FieldList, SynxlError> {
    if rest.is_empty() {
        return Err(SynxlError::new(
            SynxlErrorKind::MalformedFieldList,
            line,
            "`!fields` declares no fields",
        ));
    }

    let mut fields: Vec<FieldDecl> = Vec::new();
    for decl in rest.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            return Err(SynxlError::new(
                SynxlErrorKind::MalformedFieldList,
                line,
                "empty field declaration",
            ));
        }
        if fields.len() >= MAX_SYNXL_FIELDS {
            return Err(SynxlError::new(
                SynxlErrorKind::LimitExceeded,
                line,
                format!("more than {} fields in one field list", MAX_SYNXL_FIELDS),
            ));
        }
        let field = parse_field_decl(decl, line)?;
        // §5.1 — duplicates are rejected rather than resolved.
        if fields.iter().any(|f| f.name == field.name) {
            return Err(SynxlError::new(
                SynxlErrorKind::DuplicateField,
                line,
                format!("duplicate field name `{}`", field.name),
            ));
        }
        fields.push(field);
    }

    let list = FieldList::new(fields);
    // §5.3.4 — a zero-arity field list has no representation: its record line
    // would be empty, and §3.5 makes empty lines invisible, so records would
    // have no detectable boundary.
    if list.arity() == 0 {
        return Err(SynxlError::new(
            SynxlErrorKind::ZeroArity,
            line,
            "field list has arity 0 — every field is declared [block]; at least one inline field is required",
        ));
    }
    let mut list = list;
    list.line = line;
    list.source = source.to_string();
    Ok(list)
}

/// `name [ "(" type ")" ] [ "[" constraints "]" ]` (§5).
fn parse_field_decl(decl: &str, line: usize) -> Result<FieldDecl, SynxlError> {
    let bytes = decl.as_bytes();
    let len = bytes.len();

    // §5.1 — the name runs until `[`, `(`, `:`, or whitespace.
    let mut pos = 0usize;
    while pos < len {
        let ch = bytes[pos];
        if ch == b' ' || ch == b'\t' || ch == b'[' || ch == b'(' || ch == b':' {
            break;
        }
        pos += 1;
    }
    let name = &decl[..pos];
    if name.is_empty() {
        return Err(SynxlError::new(
            SynxlErrorKind::MalformedFieldList,
            line,
            format!("field declaration `{}` has no name", elide(decl)),
        ));
    }
    if name.len() > MAX_SYNXL_FIELD_NAME_BYTES {
        return Err(SynxlError::new(
            SynxlErrorKind::LimitExceeded,
            line,
            format!(
                "field name is {} bytes, limit is {}",
                name.len(),
                MAX_SYNXL_FIELD_NAME_BYTES
            ),
        ));
    }

    // Optional `(type)`.
    let mut type_hint = None;
    if pos < len && bytes[pos] == b'(' {
        let start = pos + 1;
        match decl[start..].find(')') {
            Some(rel) => {
                type_hint = Some(decl[start..start + rel].trim().to_string());
                pos = start + rel + 1;
            }
            None => {
                return Err(SynxlError::new(
                    SynxlErrorKind::MalformedFieldList,
                    line,
                    format!("unterminated `(` in field declaration `{}`", elide(decl)),
                ))
            }
        }
    }

    // Optional `[constraints]` — balanced scan, so `pattern:^[A-Z]$` survives.
    let mut constraints = Constraints::default();
    let mut block = false;
    if pos < len && bytes[pos] == b'[' {
        let cstart = pos + 1;
        let mut depth = 1usize;
        let mut scan = cstart;
        while scan < len {
            match bytes[scan] {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            scan += 1;
        }
        if depth != 0 {
            return Err(SynxlError::new(
                SynxlErrorKind::MalformedFieldList,
                line,
                format!("unterminated `[` in field declaration `{}`", elide(decl)),
            ));
        }
        let raw = &decl[cstart..scan];
        // §5.2 — reuse the SYNX constraint parser verbatim.
        constraints = parser::parse_constraints(raw);
        // §5.3 — `block` is SYNXL-only, so the SYNX parser drops it as an
        // unrecognised bare flag; pick it up here.
        block = raw
            .split(',')
            .map(|p| p.trim())
            .any(|p| p == "block");
        pos = scan + 1;
    }

    // Nothing but whitespace may follow.
    let tail = decl[pos..].trim();
    if !tail.is_empty() {
        // §5.2 — marker chains are reserved for a future version.
        if tail.starts_with(':') {
            return Err(SynxlError::new(
                SynxlErrorKind::MarkerChain,
                line,
                format!("marker chain `{}` is not allowed in a field declaration", elide(tail)),
            ));
        }
        return Err(SynxlError::new(
            SynxlErrorKind::MalformedFieldList,
            line,
            format!("trailing `{}` in field declaration `{}`", elide(tail), elide(decl)),
        ));
    }

    // §8.3 — a dataset whose parse result varies between reads is not
    // interchangeable, so the non-deterministic hints are rejected outright.
    for hint in [type_hint.as_deref(), constraints.type_name.as_deref()]
        .into_iter()
        .flatten()
    {
        if is_non_deterministic_hint(hint) {
            return Err(SynxlError::new(
                SynxlErrorKind::NonDeterministicHint,
                line,
                format!("non-deterministic type hint `{}` on field `{}`", hint, name),
            ));
        }
    }

    // §5.3.2 — the shape of a block value comes from the embedded document.
    if block && (type_hint.is_some() || constraints.type_name.is_some()) {
        return Err(SynxlError::new(
            SynxlErrorKind::BlockWithType,
            line,
            format!("field `{}` combines `block` with a type", name),
        ));
    }

    Ok(FieldDecl {
        name: name.to_string(),
        type_hint,
        constraints,
        block,
    })
}

fn is_non_deterministic_hint(hint: &str) -> bool {
    matches!(hint, "random" | "random:int" | "random:float" | "random:bool")
}

// ─── §7.1 Record-line splitting ──────────────────────────────

/// One inline part: the text plus whether it came from a quoted run (§7.4).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Part<'a> {
    text: &'a str,
    quoted: bool,
}

/// Splits a record line on `;` while recognising quotes in the same pass (§7.1).
///
/// The algorithm is normative and reproduced literally: skip horizontal
/// whitespace, try a quoted run, and fall back to an unquoted part on any
/// failure — no opening quote, no matching close, or garbage after the close.
struct PartSplitter<'a> {
    s: &'a str,
    b: &'a [u8],
    pos: usize,
    finished: bool,
}

impl<'a> PartSplitter<'a> {
    fn new(s: &'a str) -> Self {
        Self { s, b: s.as_bytes(), pos: 0, finished: false }
    }
}

impl<'a> Iterator for PartSplitter<'a> {
    type Item = Part<'a>;

    fn next(&mut self) -> Option<Part<'a>> {
        if self.finished {
            return None;
        }
        let len = self.b.len();
        let mut i = self.pos;

        // 1. Skip spaces and horizontal tabs.
        while i < len && (self.b[i] == b' ' || self.b[i] == b'\t') {
            i += 1;
        }

        // 2. Quoted candidate.
        if i < len && (self.b[i] == b'"' || self.b[i] == b'\'') {
            let quote = self.b[i];
            if let Some(rel) = memchr(quote, &self.b[i + 1..]) {
                let close = i + 1 + rel;
                let mut j = close + 1;
                while j < len && (self.b[j] == b' ' || self.b[j] == b'\t') {
                    j += 1;
                }
                if j >= len || self.b[j] == b';' {
                    // §7.4 — the value is the text strictly between the quotes,
                    // untrimmed and uninterpreted.
                    let part = Part { text: &self.s[i + 1..close], quoted: true };
                    if j >= len {
                        self.finished = true;
                        self.pos = len;
                    } else {
                        self.pos = j + 1;
                    }
                    return Some(part);
                }
            }
        }

        // 3. Unquoted: up to the next `;` or end of line; quotes inside are
        //    ordinary content. 4. Trimmed of leading/trailing space and tab.
        let end = match memchr(b';', &self.b[i..]) {
            Some(rel) => i + rel,
            None => len,
        };
        let text = trim_wsp(&self.s[i..end]);
        if end >= len {
            self.finished = true;
            self.pos = len;
        } else {
            self.pos = end + 1;
        }
        Some(Part { text, quoted: false })
    }
}

// ─── §8 Casting ──────────────────────────────────────────────

/// Cast one inline part for `field`, recording a diagnostic on failure.
fn cast_part(
    field: &FieldDecl,
    part: Part<'_>,
    record_index: usize,
    line: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> Value {
    // §7.4 — a quoted part is literal inner text: no casting at all.
    if part.quoted {
        return Value::String(part.text.to_string());
    }
    // §7.2 — an empty unquoted part is null, not an empty string.
    if part.text.is_empty() {
        return Value::Null;
    }
    match field.type_name() {
        // §8.2 — typed casting; a failure nulls the cell but keeps the row.
        Some(hint) => match cast_typed_checked(part.text, hint) {
            Some(v) => v,
            None => {
                diagnostics.push(Diagnostic {
                    record_index,
                    line,
                    kind: DiagnosticKind::CastFailed,
                    message: format!(
                        "field `{}`: `{}` is not a valid {}",
                        field.name,
                        elide(part.text),
                        hint
                    ),
                });
                Value::Null
            }
        },
        // §8.1 — automatic casting.
        None => cast_inline(part.text),
    }
}

/// SYNX §8.3 automatic casting minus quote stripping (§8.1 + §7.1 step 3).
///
/// A part that survived §7.1 as *unquoted* keeps its quote characters as
/// ordinary content, so SYNX's "surrounded by quotes → literal inner text"
/// step must not run a second time; otherwise `"a"b"` would silently lose two
/// characters and break the §14.1 round-trip.
fn cast_inline(raw: &str) -> Value {
    if is_quote_wrapped(raw) {
        return Value::String(raw.to_string());
    }
    parser::cast(raw)
}

/// SYNX §8.3 typed casting with explicit failure (§8.2).
fn cast_typed_checked(raw: &str, hint: &str) -> Option<Value> {
    match hint {
        "int" => raw.parse::<i64>().ok().map(Value::Int),
        // Non-finite floats have no JSON form, so they count as failures
        // rather than becoming a `null` with no diagnostic attached.
        "float" => raw.parse::<f64>().ok().filter(|f| f.is_finite()).map(Value::Float),
        "bool" => match raw {
            "true" => Some(Value::Bool(true)),
            "false" => Some(Value::Bool(false)),
            _ => None,
        },
        "string" => Some(Value::String(raw.to_string())),
        // SYNX §8.3: an unknown hint falls back to automatic casting, which
        // cannot fail.
        _ => Some(cast_inline(raw)),
    }
}

// ─── §8.4 Validation (opt-in) ────────────────────────────────

/// Mirror of the engine's constraint enforcement, reported instead of applied.
fn check_constraints(key: &str, value: &Value, c: &Constraints) -> Option<String> {
    if c.required {
        let empty = matches!(value, Value::Null)
            || matches!(value, Value::String(s) if s.is_empty());
        if empty {
            return Some(format!("`{}` is required", key));
        }
    }
    // An unset optional field is not checked any further.
    if matches!(value, Value::Null) {
        return None;
    }

    if let Some(ref type_name) = c.type_name {
        let ok = match type_name.as_str() {
            "int" => matches!(value, Value::Int(_)),
            "float" => matches!(value, Value::Float(_) | Value::Int(_)),
            "bool" => matches!(value, Value::Bool(_)),
            "string" => matches!(value, Value::String(_)),
            _ => true,
        };
        if !ok {
            return Some(format!("`{}` expected type `{}`", key, type_name));
        }
    }

    if let Some(ref enum_values) = c.enum_values {
        let as_str = match value {
            Value::String(s) => s.clone(),
            Value::Int(n) => n.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Bool(b) => b.to_string(),
            _ => String::new(),
        };
        if !enum_values.contains(&as_str) {
            return Some(format!("`{}` must be one of [{}]", key, enum_values.join("|")));
        }
    }

    // Numbers compare by value, strings by length — same rule as the engine.
    let num = match value {
        Value::Int(n) => Some(*n as f64),
        Value::Float(f) => Some(*f),
        Value::String(s) if c.min.is_some() || c.max.is_some() => Some(s.len() as f64),
        _ => None,
    };
    if let Some(n) = num {
        if let Some(min) = c.min {
            if n < min {
                return Some(format!("`{}` value {} is below min {}", key, n, min));
            }
        }
        if let Some(max) = c.max {
            if n > max {
                return Some(format!("`{}` value {} exceeds max {}", key, n, max));
            }
        }
    }

    if let Some(ref pattern) = c.pattern {
        if pattern.len() <= 256 {
            if let Value::String(ref s) = value {
                // An invalid regex is skipped silently, matching the engine.
                if let Ok(re) = regex::Regex::new(pattern) {
                    if !re.is_match(s) {
                        return Some(format!("`{}` does not match pattern /{}/", key, pattern));
                    }
                }
            }
        }
    }

    None
}

// ─── §14 Writer ──────────────────────────────────────────────

/// Serialize a document, emitting a `!fields` line per run of records that
/// share a field list (§14, §4.2).
///
/// Fails with [`SynxlErrorKind::Unwritable`] when a value has no SYNXL
/// rendering (§14.3). That cannot happen for a document obtained by parsing —
/// §14.1 guarantees the round trip — so only a programmatically built value
/// can trigger it.
pub fn write_document(doc: &SynxlDocument) -> Result<String, SynxlError> {
    let mut out = String::with_capacity(doc.records.len() * 64 + 64);
    out.push_str("!synxl 1\n");

    if doc.records.is_empty() {
        // §4.2 — a document still declares its schema even with no rows.
        if let Some(fl) = doc.field_lists.first() {
            write_group(&mut out, fl.fields(), &[])?;
        }
        return Ok(out);
    }

    let mut start = 0usize;
    while start < doc.records.len() {
        let fl_idx = doc.record_field_lists.get(start).copied().unwrap_or(0);
        let mut end = start + 1;
        while end < doc.records.len()
            && doc.record_field_lists.get(end).copied().unwrap_or(0) == fl_idx
        {
            end += 1;
        }
        let empty = FieldList::new(Vec::new());
        let fl = doc.field_lists.get(fl_idx).unwrap_or(&empty);
        write_group(&mut out, fl.fields(), &doc.records[start..end])?;
        start = end;
    }
    Ok(out)
}

/// Serialize a single-schema record set (§14). See [`write_document`] for the
/// failure condition.
pub fn write_lines(fields: &[FieldDecl], records: &[Value]) -> Result<String, SynxlError> {
    let mut out = String::with_capacity(records.len() * 64 + 64);
    out.push_str("!synxl 1\n");
    write_group(&mut out, fields, records)?;
    Ok(out)
}

/// Emit one `!fields` line and the records that use it.
fn write_group(
    out: &mut String,
    fields: &[FieldDecl],
    records: &[Value],
) -> Result<(), SynxlError> {
    // §14.3 — a value that cannot live inline is promoted, which is a property
    // of the whole column: block-ness is declared once, in the field list.
    let mut block: Vec<bool> = fields
        .iter()
        .map(|f| {
            f.block
                || records.iter().any(|r| {
                    r.as_object()
                        .and_then(|m| m.get(&f.name))
                        .map(needs_block)
                        .unwrap_or(false)
                })
        })
        .collect();

    // §5.3.4 — arity 0 is a hard error, so promotion must leave one column
    // inline. The candidate must be a declared-inline column none of whose
    // values contain `LF`, `CR`, or `;`; such a value survives as an unquoted
    // part byte-for-byte, losing at most significant edge whitespace, which
    // §14.1's scope note already exempts. One always exists for a document
    // that came from a parse — an inline part can hold none of those bytes.
    //
    // If none exists (only reachable for a programmatically built value), the
    // value has no rendering at all: §14.3 requires rejecting it loudly, and
    // emitting the zero-arity document anyway would be the forbidden half of
    // that rule — a document this crate's own parser refuses.
    if !fields.is_empty() && block.iter().all(|b| *b) {
        let candidate = (0..fields.len()).filter(|i| !fields[*i].block).find(|i| {
            !records.iter().any(|r| {
                matches!(
                    r.as_object().and_then(|m| m.get(&fields[*i].name)),
                    Some(Value::String(s))
                        if s.contains('\n') || s.contains('\r') || s.contains(';')
                )
            })
        });
        match candidate {
            Some(i) => block[i] = false,
            None => {
                return Err(SynxlError::new(
                    SynxlErrorKind::Unwritable,
                    0,
                    "every field would have to be promoted to a block, which leaves the field list at arity 0 (§5.3.4); the value has no SYNXL rendering",
                ))
            }
        }
    }

    out.push_str("!fields ");
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            out.push_str("; ");
        }
        write_field_decl(out, field, block[i]);
    }
    out.push('\n');

    for record in records {
        let map = record.as_object();
        // §7.2 — an all-null record is written as the single token `;`, which
        // is arity-independent and diagnostic-free. Joining empty parts would
        // produce an invisible line at arity 1 and a `MissingFields` at 3.
        let all_null = fields.iter().enumerate().all(|(i, f)| {
            block[i]
                || map
                    .and_then(|m| m.get(&f.name))
                    .map(Value::is_null)
                    .unwrap_or(true)
        });
        if all_null {
            out.push(';');
        } else {
            let mut first = true;
            for (i, field) in fields.iter().enumerate() {
                if block[i] {
                    continue;
                }
                if !first {
                    out.push_str("; ");
                }
                first = false;
                let value = map.and_then(|m| m.get(&field.name)).unwrap_or(&Value::Null);
                write_inline(out, value);
            }
        }
        out.push('\n');

        for (i, field) in fields.iter().enumerate() {
            if !block[i] {
                continue;
            }
            let value = match map.and_then(|m| m.get(&field.name)) {
                Some(v) if !v.is_null() => v,
                // §9.2 — an absent block field needs no lines at all.
                _ => continue,
            };
            write_synx_entry(out, &field.name, value, 2, 0);
        }
    }
    Ok(())
}

fn write_field_decl(out: &mut String, field: &FieldDecl, block: bool) {
    out.push_str(&field.name);
    // §5.3.2 — a promoted field drops its type; the block document carries the
    // shape instead.
    if !block {
        if let Some(ref hint) = field.type_hint {
            out.push('(');
            out.push_str(hint);
            out.push(')');
        }
    }

    let c = &field.constraints;
    let mut parts: Vec<String> = Vec::new();
    if !block {
        if let Some(ref t) = c.type_name {
            parts.push(format!("type:{}", t));
        }
    }
    if c.required {
        parts.push("required".to_string());
    }
    if c.readonly {
        parts.push("readonly".to_string());
    }
    if let Some(min) = c.min {
        parts.push(format!("min:{}", trim_float(min)));
    }
    if let Some(max) = c.max {
        parts.push(format!("max:{}", trim_float(max)));
    }
    if let Some(ref p) = c.pattern {
        parts.push(format!("pattern:{}", p));
    }
    if let Some(ref e) = c.enum_values {
        parts.push(format!("enum:{}", e.join("|")));
    }
    if block {
        parts.push("block".to_string());
    }
    if !parts.is_empty() {
        out.push('[');
        out.push_str(&parts.join(", "));
        out.push(']');
    }
}

/// §14.3 — values that cannot be written as an inline part.
fn needs_block(value: &Value) -> bool {
    match value {
        // Structure has no inline form at all.
        Value::Object(_) | Value::Array(_) => true,
        Value::String(s) | Value::Secret(s) => {
            s.contains('\n')
                || s.contains('\r')
                // Quoting has no escapes (§7.4/§16.5), so a value that needs
                // quoting and contains both quote characters — the `;` + quote
                // case of §14.3 among others — has to be promoted.
                || (inline_needs_quote(s) && pick_quote(s).is_none())
        }
        _ => false,
    }
}

/// Would this string be misread if written as a bare inline part? (§14.3)
fn inline_needs_quote(s: &str) -> bool {
    s.is_empty()
        || s != trim_wsp(s)
        || s.contains(';')
        || s.starts_with('#')
        || s.starts_with("//")
        || s.starts_with('!')
        // A leading quote would make §7.1 step 2 read the part as quoted.
        || s.starts_with('"')
        || s.starts_with('\'')
        || cast_inline(s) != Value::String(s.to_string())
}

/// A quote character that does not occur in `s`, or `None` if both do.
fn pick_quote(s: &str) -> Option<char> {
    if !s.contains('"') {
        Some('"')
    } else if !s.contains('\'') {
        Some('\'')
    } else {
        None
    }
}

/// §14.2 / §14.3 — one inline part.
fn write_inline(out: &mut String, value: &Value) {
    match value {
        // §14.2 — an unset field is an empty part.
        Value::Null => {}
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(n) => {
            let mut buf = itoa::Buffer::new();
            out.push_str(buf.format(*n));
        }
        Value::Float(f) => write_float(out, *f),
        Value::String(s) | Value::Secret(s) => {
            if inline_needs_quote(s) {
                match pick_quote(s) {
                    Some(q) => {
                        out.push(q);
                        out.push_str(s);
                        out.push(q);
                    }
                    // Unreachable for a promoted column; emit the raw text
                    // rather than losing the value outright.
                    None => out.push_str(s),
                }
            } else {
                out.push_str(s);
            }
        }
        // Promoted by `needs_block`; nothing sensible to write here.
        Value::Object(_) | Value::Array(_) => {}
    }
}

/// Write a float so that the SYNX cast reads it back as the same float.
///
/// SYNX §8.3 only recognises `-?digits.digits`, so shortest-form output such
/// as `1e300` or `5` would come back as a string or an int. Non-finite values
/// have no JSON form either (`write_json` emits `null` for them), so they are
/// written as an empty part — whose projection is the same `null`.
fn write_float(out: &mut String, f: f64) {
    if !f.is_finite() {
        return;
    }
    let s = f.to_string();
    if !s.contains('e') && !s.contains('E') {
        out.push_str(&s);
        if !s.contains('.') {
            out.push_str(".0");
        }
        return;
    }
    // Exponent form: spell the value out in full. f64 decimals terminate, so
    // the expansion is exact.
    let expanded = format!("{:.*}", FLOAT_EXPANSION_PRECISION, f);
    let trimmed = expanded.trim_end_matches('0');
    let trimmed = if trimmed.ends_with('.') { &expanded[..trimmed.len() + 1] } else { trimmed };
    out.push_str(trimmed);
}

fn trim_float(f: f64) -> String {
    let s = f.to_string();
    s.strip_suffix(".0").map(|s| s.to_string()).unwrap_or(s)
}

// ─── §14 Block serialization (SYNX subset) ───────────────────

/// Write `key: value` as SYNX at `indent` columns.
fn write_synx_entry(out: &mut String, key: &str, value: &Value, indent: usize, depth: usize) {
    if depth > MAX_WRITE_DEPTH {
        return;
    }
    match value {
        Value::Object(map) => {
            push_indent(out, indent);
            out.push_str(key);
            out.push('\n');
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            for k in keys {
                write_synx_entry(out, k, &map[k], indent + 2, depth + 1);
            }
        }
        Value::Array(items) => {
            push_indent(out, indent);
            out.push_str(key);
            if items.is_empty() {
                // A bare key parses back as an empty object; the `:join` list
                // marker is the only SYNX surface that yields an empty array.
                out.push_str(":join");
            }
            out.push('\n');
            for item in items {
                write_synx_item(out, item, indent + 2, depth + 1);
            }
        }
        Value::String(s) if s.contains('\n') => write_synx_multiline(out, key, s, indent),
        _ => {
            let scalar = synx_scalar(value);
            match scalar {
                Some(text) => {
                    push_indent(out, indent);
                    out.push_str(key);
                    out.push(' ');
                    out.push_str(&text);
                    out.push('\n');
                }
                // Not representable as a single SYNX token — fall back to an
                // indent-preserving block, whose body is never comment-stripped
                // nor cast.
                None => {
                    let s = value.as_str().unwrap_or_default().to_string();
                    write_synx_multiline(out, key, &s, indent);
                }
            }
        }
    }
}

/// Write one `- item` list entry.
fn write_synx_item(out: &mut String, item: &Value, indent: usize, depth: usize) {
    if depth > MAX_WRITE_DEPTH {
        return;
    }
    match item {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            // The `- ` line has to carry a key whose value fits on it; SYNX
            // attaches everything deeper to the *item*, not to that key.
            let head = keys
                .iter()
                .position(|k| dash_line_safe(&map[*k]))
                .unwrap_or(0);
            let head_key = keys[head];
            push_indent(out, indent);
            out.push_str("- ");
            out.push_str(head_key);
            match synx_scalar(&map[head_key]) {
                Some(text) if !text.is_empty() => {
                    out.push(' ');
                    out.push_str(&text);
                }
                // An empty object is written as a bare key, which is exactly
                // how SYNX produced it in the first place.
                _ => {}
            }
            out.push('\n');
            for (i, k) in keys.iter().enumerate() {
                if i == head {
                    continue;
                }
                write_synx_entry(out, k, &map[*k], indent + 2, depth + 1);
            }
        }
        _ => {
            push_indent(out, indent);
            out.push_str("- ");
            match synx_scalar(item) {
                Some(text) => out.push_str(&text),
                None => out.push_str(item.as_str().unwrap_or_default()),
            }
            out.push('\n');
        }
    }
}

/// May this value share the `- ` line of a list item?
fn dash_line_safe(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.is_empty(),
        Value::Array(_) => false,
        Value::String(s) => !s.contains('\n') && synx_scalar(value).is_some() && !s.is_empty(),
        _ => true,
    }
}

/// `key |+` plus an indented body (§14.3).
fn write_synx_multiline(out: &mut String, key: &str, body: &str, indent: usize) {
    push_indent(out, indent);
    out.push_str(key);
    out.push_str(" |+\n");
    for line in body.split('\n') {
        push_indent(out, indent + 2);
        out.push_str(line);
        out.push('\n');
    }
}

/// Render a scalar as a SYNX value token, or `None` when no token can carry it.
///
/// A SYNX value is comment-stripped at ` #` / ` //` *before* casting, so a
/// value containing either sequence cannot be rescued by quoting.
fn synx_scalar(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some("null".to_string()),
        Value::Bool(b) => Some(if *b { "true" } else { "false" }.to_string()),
        Value::Int(n) => Some(n.to_string()),
        Value::Float(f) => {
            // A non-finite float has no JSON form either — `write_json` emits
            // `null` for it, so `null` is the projection-preserving token.
            if !f.is_finite() {
                return Some("null".to_string());
            }
            let mut s = String::new();
            write_float(&mut s, *f);
            Some(s)
        }
        Value::String(s) | Value::Secret(s) => {
            if s.contains('\n') || s.contains(" #") || s.contains(" //") {
                return None;
            }
            if !synx_value_needs_quote(s) {
                return Some(s.clone());
            }
            pick_quote(s).map(|q| format!("{}{}{}", q, s, q))
        }
        Value::Object(_) | Value::Array(_) => None,
    }
}

/// Would this string be misread as a bare SYNX value?
fn synx_value_needs_quote(s: &str) -> bool {
    s.is_empty()
        || s != s.trim()
        || s == "|"
        || s == "|+"
        || is_quote_wrapped(s)
        || !matches!(parser::cast(s), Value::String(ref v) if v == s)
}

// ─── Small helpers ───────────────────────────────────────────

#[inline]
fn starts_with_wsp(s: &str) -> bool {
    s.starts_with(' ') || s.starts_with('\t')
}

#[inline]
fn trim_wsp(s: &str) -> &str {
    s.trim_matches(|c| c == ' ' || c == '\t')
}

/// Does `s` both start and end with the same ASCII quote, length ≥ 2?
fn is_quote_wrapped(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2
        && ((b[0] == b'"' && b[b.len() - 1] == b'"') || (b[0] == b'\'' && b[b.len() - 1] == b'\''))
}

fn push_indent(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push(' ');
    }
}

/// Truncate to at most `max` bytes at a valid UTF-8 boundary (§13).
fn truncate_utf8(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Shorten a fragment for an error message.
fn elide(s: &str) -> String {
    const MAX: usize = 48;
    if s.len() <= MAX {
        return s.to_string();
    }
    let cut = truncate_utf8(s, MAX);
    format!("{}…", cut)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> SynxlDocument {
        parse_lines(src).expect("expected a successful parse")
    }

    fn kinds(d: &SynxlDocument) -> Vec<DiagnosticKind> {
        d.diagnostics.iter().map(|x| x.kind).collect()
    }

    // ── §4 document structure ───────────────────────────────

    #[test]
    fn parses_prologue_and_simple_records() {
        let d = doc("!synxl 1\n!fields id[type:int] ; name\n1 ; Wario\n2 ; Mario\n");
        assert_eq!(d.version, 1);
        assert_eq!(d.len(), 2);
        assert_eq!(d.to_json(), r#"[{"id":1,"name":"Wario"},{"id":2,"name":"Mario"}]"#);
        assert!(d.diagnostics.is_empty());
    }

    #[test]
    fn bom_and_crlf_are_tolerated() {
        let d = doc("\u{feff}!synxl 1\r\n!fields a ; b\r\n1 ; 2\r\n");
        assert_eq!(d.to_json(), r#"[{"a":1,"b":2}]"#);
    }

    #[test]
    fn comments_and_blank_lines_before_prologue() {
        let d = doc("# hi\n\n// there\n###\n!synxl 999\n###\n!synxl 1\n!fields a\nx\n");
        assert_eq!(d.to_json(), r#"[{"a":"x"}]"#);
    }

    #[test]
    fn missing_prologue_is_a_hard_error() {
        let e = parse_lines("!fields a\nx\n").unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::MissingPrologue);
        let e = parse_lines("").unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::MissingPrologue);
        let e = parse_lines("!synxl\n!fields a\n").unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::MissingPrologue);
        // Trailing garbage after the version is not a prologue.
        let e = parse_lines("!synxl 1 extra\n!fields a\n").unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::MissingPrologue);
    }

    #[test]
    fn unsupported_version_is_a_hard_error() {
        let e = parse_lines("!synxl 2\n!fields a\nx\n").unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::UnsupportedVersion);
        let e = parse_lines("!synxl 99999999999999999999\n!fields a\n").unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::UnsupportedVersion);
    }

    #[test]
    fn record_without_field_list_is_a_hard_error() {
        let e = parse_lines("!synxl 1\nrogue record\n").unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::NoFieldList);
        assert_eq!(e.line, 2);
    }

    #[test]
    fn field_list_can_be_redeclared_mid_document() {
        // §4.2 / §10 — records keep the schema they were parsed under.
        let src = "!synxl 1\n\
                   !fields id[type:int] ; score[type:float]\n\
                   1 ; 0.91\n\
                   # schema evolution\n\
                   !fields id[type:int] ; score[type:float] ; lang\n\
                   3 ; 0.55 ; ru\n";
        let d = doc(src);
        assert_eq!(
            d.to_json(),
            r#"[{"id":1,"score":0.91},{"id":3,"lang":"ru","score":0.55}]"#
        );
        assert_eq!(d.field_lists.len(), 2);
        assert_eq!(d.record_field_lists, vec![0, 1]);
        assert_eq!(d.field_list_for(1).unwrap().arity(), 3);
    }

    // ── §5 field list ───────────────────────────────────────

    #[test]
    fn duplicate_field_name_is_a_hard_error() {
        let e = parse_lines("!synxl 1\n!fields a ; b ; a\nx ; y ; z\n").unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::DuplicateField);
    }

    #[test]
    fn marker_chain_is_a_hard_error() {
        let e = parse_lines("!synxl 1\n!fields port:env:default:3000\n1\n").unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::MarkerChain);
        let e = parse_lines("!synxl 1\n!fields id[required]:env\n1\n").unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::MarkerChain);
        // §5.2 — a single marker run is forbidden too, not just a chain.
        let e = parse_lines("!synxl 1\n!fields id:custom\n1\n").unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::MarkerChain);
    }

    #[test]
    fn non_deterministic_hint_is_a_hard_error() {
        for src in [
            "!synxl 1\n!fields id(random)\n1\n",
            "!synxl 1\n!fields id(random:float)\n1\n",
            "!synxl 1\n!fields id[type:random:bool]\n1\n",
        ] {
            let e = parse_lines(src).unwrap_err();
            assert_eq!(e.kind, SynxlErrorKind::NonDeterministicHint, "{src}");
        }
    }

    #[test]
    fn block_with_type_is_a_hard_error() {
        let e = parse_lines("!synxl 1\n!fields m(int)[block]\n\n").unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::BlockWithType);
        let e = parse_lines("!synxl 1\n!fields m[block, type:int]\n\n").unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::BlockWithType);
    }

    #[test]
    fn malformed_field_lists_are_hard_errors() {
        for src in [
            "!synxl 1\n!fields\nx\n",
            "!synxl 1\n!fields a ; ; b\nx\n",
            "!synxl 1\n!fields a[required\nx\n",
            "!synxl 1\n!fields a(int\nx\n",
        ] {
            let e = parse_lines(src).unwrap_err();
            assert_eq!(e.kind, SynxlErrorKind::MalformedFieldList, "{src}");
        }
    }

    #[test]
    fn field_name_length_limit() {
        let long = "n".repeat(MAX_SYNXL_FIELD_NAME_BYTES + 1);
        let e = parse_lines(&format!("!synxl 1\n!fields {long}\nx\n")).unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::LimitExceeded);
    }

    #[test]
    fn field_count_limit() {
        let mut src = String::from("!synxl 1\n!fields ");
        for i in 0..(MAX_SYNXL_FIELDS + 1) {
            if i > 0 {
                src.push_str(" ; ");
            }
            src.push_str(&format!("f{i}"));
        }
        src.push('\n');
        let e = parse_lines(&src).unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::LimitExceeded);
    }

    #[test]
    fn unrecognised_constraint_parts_are_tolerated() {
        // §5.2 — a version-1 parser stays readable for later producers.
        let d = doc("!synxl 1\n!fields a[required, futureflag, future:thing]\n5\n");
        assert_eq!(d.to_json(), r#"[{"a":5}]"#);
        assert!(d.field_lists[0].get("a").unwrap().constraints.required);
    }

    #[test]
    fn arity_ignores_block_fields() {
        let d = doc("!synxl 1\n!fields a ; b ; m[block]\n1 ; 2\n");
        assert_eq!(d.field_lists[0].arity(), 2);
        assert!(d.diagnostics.is_empty());
        assert_eq!(d.to_json(), r#"[{"a":1,"b":2,"m":null}]"#);
    }

    // ── §6 record lines ─────────────────────────────────────

    #[test]
    fn synx_first_character_filter_does_not_apply() {
        let src = "!synxl 1\n!fields v\n-5\n/var/log/app\n@kaiserberg\n[unparsed]\n:marker\n(paren)\n";
        let d = doc(src);
        assert_eq!(
            d.to_json(),
            r#"[{"v":-5},{"v":"/var/log/app"},{"v":"@kaiserberg"},{"v":"[unparsed]"},{"v":":marker"},{"v":"(paren)"}]"#
        );
    }

    #[test]
    fn reserved_prefixes_at_indent_zero() {
        // `//` is a comment, a single `/` is data, `#` is a comment, and a
        // record meant to start with a reserved prefix must quote it.
        let d = doc("!synxl 1\n!fields v\n//comment\n/data\n# comment\n\"!bang\"\n'#quoted'\n");
        assert_eq!(d.to_json(), r##"[{"v":"/data"},{"v":"!bang"},{"v":"#quoted"}]"##);
    }

    #[test]
    fn unknown_bang_line_is_a_hard_error() {
        // §4.1 — the `!filds` typo must not silently leave the old schema in
        // effect; SYNX directives are rejected for the same reason.
        for src in [
            "!synxl 1\n!fields a\n1\n!filds a ; b\n2 ; 3\n",
            "!synxl 1\n!active\n!fields a\n1\n",
            "!synxl 1\n!fields a\n!include /etc/passwd\n1\n",
            "!synxl 1\n!fieldsx a\n1\n",
        ] {
            let e = parse_lines(src).unwrap_err();
            assert_eq!(e.kind, SynxlErrorKind::UnknownDirective, "{src}");
        }
    }

    #[test]
    fn repeated_prologue() {
        // §4.1 — shards are concatenable when the version matches.
        let d = doc("!synxl 1\n!fields a\n1\n!synxl 1\n!fields a\n2\n");
        assert_eq!(d.to_json(), r#"[{"a":1},{"a":2}]"#);
        let e = parse_lines("!synxl 1\n!fields a\n1\n!synxl 2\n!fields a\n2\n").unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::UnsupportedVersion);
        assert_eq!(e.line, 4);
    }

    #[test]
    fn zero_arity_field_list_is_a_hard_error() {
        // §5.3.4 — an all-block field list has no record line to write.
        let e = parse_lines("!synxl 1\n!fields m[block]\n\n  m\n    k v\n").unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::ZeroArity);
        let e = parse_lines("!synxl 1\n!fields a[block] ; b[block]\n").unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::ZeroArity);
    }

    // ── §7 inline fields ────────────────────────────────────

    #[test]
    fn quoting_and_the_unquoted_fallback() {
        let d = doc(
            "!synxl 1\n!fields a ; b ; c\n\
             \"has ; semi\" ; 'single' ; \"a\"b\" \n",
        );
        let rec = d.records[0].as_object().unwrap();
        assert_eq!(rec["a"], Value::String("has ; semi".into()));
        assert_eq!(rec["b"], Value::String("single".into()));
        // §7.1 step 3: trailing garbage after the close quote ⇒ unquoted, and
        // §8.1 must not strip the quotes a second time.
        assert_eq!(rec["c"], Value::String("\"a\"b\"".into()));
    }

    #[test]
    fn unterminated_quote_falls_back_to_unquoted() {
        let d = doc("!synxl 1\n!fields a ; b\n\"open ; still\n");
        let rec = d.records[0].as_object().unwrap();
        assert_eq!(rec["a"], Value::String("\"open".into()));
        assert_eq!(rec["b"], Value::String("still".into()));
    }

    #[test]
    fn quoted_values_bypass_casting() {
        let d = doc("!synxl 1\n!fields a ; b ; c\n\"42\" ; \"true\" ; \"null\"\n");
        assert_eq!(d.to_json(), r#"[{"a":"42","b":"true","c":"null"}]"#);
    }

    #[test]
    fn null_versus_empty_string() {
        // §7.2 — the ambiguity CSV never resolved. Note the record line cannot
        // start with a space: indent > 0 would make it block content (§3.4).
        let d = doc("!synxl 1\n!fields a ; b ; c\n; \"\" ; x\n");
        assert_eq!(d.to_json(), r#"[{"a":null,"b":"","c":"x"}]"#);
    }

    #[test]
    fn quoted_part_keeps_interior_whitespace() {
        let d = doc("!synxl 1\n!fields a ; b\n\"  padded  \"  ;   bare  \n");
        let rec = d.records[0].as_object().unwrap();
        assert_eq!(rec["a"], Value::String("  padded  ".into()));
        assert_eq!(rec["b"], Value::String("bare".into()));
    }

    #[test]
    fn only_spaces_and_tabs_are_trimmed_from_a_part() {
        // §7.1 step 4 — Unicode whitespace is content, not padding. The NBSP
        // must not lead the *line*, though: §3.3 counts it as indentation
        // (SYNX §4 ltrims Unicode whitespace), which would make the line block
        // content rather than a record.
        let d = doc("!synxl 1\n!fields a ; b\nx ; \u{a0}hello\u{a0}\n");
        assert_eq!(
            d.records[0].as_object().unwrap()["b"],
            Value::String("\u{a0}hello\u{a0}".into())
        );
        // A leading NBSP is indentation, so this line is not a record at all.
        let d = doc("!synxl 1\n!fields a\n\u{a0}hello\n");
        assert_eq!(d.to_json(), "[]");
        assert_eq!(kinds(&d), vec![DiagnosticKind::OrphanBlockLine]);
    }

    #[test]
    fn arity_mismatch_in_both_directions() {
        let d = doc("!synxl 1\n!fields a ; b ; c\n1 ; 2\n1 ; 2 ; 3 ; 4 ; 5\n");
        assert_eq!(kinds(&d), vec![DiagnosticKind::MissingFields, DiagnosticKind::ExtraFields]);
        assert_eq!(d.diagnostics[0].record_index, 0);
        assert_eq!(d.diagnostics[0].line, 3);
        assert_eq!(d.diagnostics[1].record_index, 1);
        assert_eq!(d.diagnostics[1].line, 4);
        assert_eq!(d.to_json(), r#"[{"a":1,"b":2,"c":null},{"a":1,"b":2,"c":3}]"#);
    }

    #[test]
    fn all_null_record() {
        // §7.2 — `;` sets every inline field to null at any arity, silently.
        let d = doc("!synxl 1\n!fields a\n;\n");
        assert_eq!(d.to_json(), r#"[{"a":null}]"#);
        assert!(d.diagnostics.is_empty());

        let d = doc("!synxl 1\n!fields a ; b\n;\n");
        assert_eq!(d.to_json(), r#"[{"a":null,"b":null}]"#);
        assert!(d.diagnostics.is_empty());

        // Arity 3 would otherwise raise a spurious `MissingFields`.
        let d = doc("!synxl 1\n!fields a ; b ; c\n;\t\n");
        assert_eq!(d.to_json(), r#"[{"a":null,"b":null,"c":null}]"#);
        assert!(d.diagnostics.is_empty());

        // Block fields are unaffected — the block is still parsed.
        let d = doc("!synxl 1\n!fields a ; m[block]\n;\n  m\n    k v\n");
        assert_eq!(d.to_json(), r#"[{"a":null,"m":{"k":"v"}}]"#);
        assert!(d.diagnostics.is_empty());

        // Only the exact `;` form is special: `;;` is an ordinary line of three
        // empty parts and keeps the §7.1 / §7.3 treatment.
        let d = doc("!synxl 1\n!fields a ; b\n;;\n");
        assert_eq!(d.to_json(), r#"[{"a":null,"b":null}]"#);
        assert_eq!(kinds(&d), vec![DiagnosticKind::ExtraFields]);
    }

    #[test]
    fn all_null_record_round_trips_at_every_arity() {
        // §7.2 — the writer MUST emit this form; §14.1 then holds at arity 1,
        // where no other representation exists.
        for fields in ["a", "a ; b", "a ; b ; c", "a ; b ; m[block]"] {
            let src = format!("!synxl 1\n!fields {fields}\n;\n");
            let a = doc(&src);
            let written = a.to_synxl().unwrap();
            assert!(
                written.lines().any(|l| l == ";"),
                "expected an all-null record line for `{fields}`:\n{written}"
            );
            assert_eq!(a.to_json(), doc(&written).to_json(), "{written}");
            assert!(doc(&written).diagnostics.is_empty(), "{written}");
        }
    }

    #[test]
    fn trailing_semicolon_yields_a_part() {
        let d = doc("!synxl 1\n!fields a ; b\n1 ;\n");
        assert_eq!(d.to_json(), r#"[{"a":1,"b":null}]"#);
        assert!(d.diagnostics.is_empty());
    }

    #[test]
    fn inline_comments_are_not_stripped() {
        // §7.5 — `#` and `//` are content in a dataset.
        let d = doc("!synxl 1\n!fields text\nsee https://x.dev #hashtag // not a comment\n");
        assert_eq!(
            d.records[0].as_object().unwrap()["text"],
            Value::String("see https://x.dev #hashtag // not a comment".into())
        );
    }

    // ── §8 casting ──────────────────────────────────────────

    #[test]
    fn automatic_casting() {
        let d = doc("!synxl 1\n!fields a ; b ; c ; d ; e ; f\n1 ; -2.5 ; true ; false ; null ; text\n");
        assert_eq!(
            d.to_json(),
            r#"[{"a":1,"b":-2.5,"c":true,"d":false,"e":null,"f":"text"}]"#
        );
    }

    #[test]
    fn typed_cast_failure_nulls_the_cell_only() {
        let d = doc("!synxl 1\n!fields id[type:int] ; score(float)\nnope ; 0.5\n7 ; nope\n");
        assert_eq!(kinds(&d), vec![DiagnosticKind::CastFailed, DiagnosticKind::CastFailed]);
        assert_eq!(d.to_json(), r#"[{"id":null,"score":0.5},{"id":7,"score":null}]"#);
    }

    #[test]
    fn typed_bool_and_string() {
        let d = doc("!synxl 1\n!fields a[type:bool] ; b(string) ; c[type:bool]\ntrue ; 42 ; yes\n");
        assert_eq!(d.to_json(), r#"[{"a":true,"b":"42","c":null}]"#);
        assert_eq!(kinds(&d), vec![DiagnosticKind::CastFailed]);
    }

    #[test]
    fn validation_is_opt_in() {
        let src = "!synxl 1\n!fields id[type:int, min:10] ; name[required]\n5 ; \n";
        let d = doc(src);
        assert!(d.diagnostics.is_empty());

        let d = parse_lines_with(src, &SynxlOptions { validate: true }).unwrap();
        assert_eq!(
            kinds(&d),
            vec![DiagnosticKind::ConstraintViolation, DiagnosticKind::ConstraintViolation]
        );
    }

    // ── §9 blocks ───────────────────────────────────────────

    #[test]
    fn worked_example_from_the_spec() {
        // NB: a plain (non-continued) literal — a `\` line continuation would
        // strip the leading whitespace that makes these lines a block.
        let src = "!synxl 1
!fields id[type:int, required] ; score[type:float] ; messages[block]

1 ; 0.91
  messages
    - role system
      content You are a helpful assistant.
    - role user
      content |+
          def f(x):
              return x + 1

2 ; 0.74
  messages
    - role user
      content Привет
";
        let d = doc(src);
        assert_eq!(d.len(), 2);
        assert!(d.diagnostics.is_empty());
        let mut expected = String::from(
            r#"{"id":1,"messages":[{"content":"You are a helpful assistant.","role":"system"},"#,
        );
        expected.push_str(r#"{"content":"def f(x):\n    return x + 1","role":"user"}],"score":0.91}"#);
        let mut json = String::new();
        crate::write_json(&mut json, &d.records[0]);
        assert_eq!(json, expected);
        assert_eq!(
            d.to_ndjson().lines().count(),
            2,
            "NDJSON projection is one object per line"
        );
    }

    #[test]
    fn empty_block_yields_null_block_fields() {
        let d = doc("!synxl 1\n!fields a ; m[block]\n1\n\n2\n");
        assert_eq!(d.to_json(), r#"[{"a":1,"m":null},{"a":2,"m":null}]"#);
    }

    #[test]
    fn blank_lines_do_not_terminate_a_block() {
        let d = doc("!synxl 1\n!fields a ; m[block]\n1\n\n  m\n    x 1\n\n2\n");
        assert_eq!(d.to_json(), r#"[{"a":1,"m":{"x":1}},{"a":2,"m":null}]"#);
    }

    #[test]
    fn block_key_diagnostics() {
        let d = doc("!synxl 1\n!fields a ; m[block]\n1\n  bogus 1\n  a 99\n");
        // §9.3 — sorted key order keeps this deterministic despite the HashMap.
        assert_eq!(
            kinds(&d),
            vec![DiagnosticKind::BlockFieldNotDeclared, DiagnosticKind::UnknownBlockKey]
        );
        // §11.2 — these two report the *block* line carrying the key, not the
        // record line: `a` is on line 5, `bogus` on line 4.
        assert_eq!(d.diagnostics[0].line, 5);
        assert_eq!(d.diagnostics[1].line, 4);
        assert!(d.diagnostics.iter().all(|x| x.record_index == 0));
        // The inline value stays authoritative.
        assert_eq!(d.to_json(), r#"[{"a":1,"m":null}]"#);
    }

    #[test]
    fn block_diagnostic_line_survives_nesting_and_multiline() {
        let src = "!synxl 1
!fields id ; m[block]
1
  m
    - role user
      content |+
        text
  zzz here
";
        let d = doc(src);
        assert_eq!(kinds(&d), vec![DiagnosticKind::UnknownBlockKey]);
        // `zzz` is the only unmatched top-level key, on line 8.
        assert_eq!(d.diagnostics[0].line, 8);
    }

    #[test]
    fn type_hints_inside_a_cell_are_not_interpreted() {
        // §8.3 — casting is driven exclusively by the field list.
        let d = doc("!synxl 1\n!fields a ; b\n(random) ; (int)5\n");
        assert_eq!(d.to_json(), r#"[{"a":"(random)","b":"(int)5"}]"#);
    }

    #[test]
    fn directives_inside_a_block_are_ignored() {
        // §9.4 — outside a multiline block a `!` line is discarded entirely.
        let src = "!synxl 1\n!fields a ; m[block]\n1\n  m\n    !include /etc/passwd\n    k v\n";
        let d = doc(src);
        assert_eq!(d.to_json(), r#"[{"a":1,"m":{"k":"v"}}]"#);
    }

    #[test]
    fn directives_inside_a_multiline_body_are_preserved() {
        // §9.4 — inside `|+` the very same line is data.
        let src = "!synxl 1\n!fields a ; m[block]\n1\n  m |+\n    !include /etc/passwd\n    !active\n";
        let d = doc(src);
        assert_eq!(
            d.records[0].as_object().unwrap()["m"],
            Value::String("!include /etc/passwd\n!active".into())
        );
    }

    #[test]
    fn active_mode_cannot_be_switched_on_from_a_block() {
        // §9.5 — no metadata surface, no marker resolution.
        let src = "!synxl 1\n!fields a ; m[block]\n1\n  !active\n  m\n    tax:calc 2 * 2\n";
        let d = doc(src);
        assert_eq!(d.to_json(), r#"[{"a":1,"m":{"tax":"2 * 2"}}]"#);
    }

    #[test]
    fn orphan_block_lines_are_reported_against_the_next_record() {
        // §11.2 — discarded, but never silently.
        let d = doc("!synxl 1\n!fields a\n  stray one\n5\n!fields a\n  stray two\n");
        assert_eq!(d.to_json(), r#"[{"a":5}]"#);
        assert_eq!(
            kinds(&d),
            vec![DiagnosticKind::OrphanBlockLine, DiagnosticKind::OrphanBlockLine]
        );
        // Line 3 belongs to record 0, which has not been read yet; line 6 has
        // no following record and is attached to the index one past the end.
        assert_eq!((d.diagnostics[0].record_index, d.diagnostics[0].line), (0, 3));
        assert_eq!((d.diagnostics[1].record_index, d.diagnostics[1].line), (1, 6));
    }

    // ── §13 limits ──────────────────────────────────────────

    #[test]
    fn oversized_record_is_truncated_not_rejected() {
        let mut src = String::from("!synxl 1\n!fields a ; b\n");
        src.push_str("head ; ");
        src.push_str(&"x".repeat(MAX_SYNXL_RECORD_BYTES + 16));
        src.push_str("\ntail ; ok\n");
        let d = doc(&src);
        assert_eq!(d.len(), 2, "the following record still parses");
        assert_eq!(kinds(&d), vec![DiagnosticKind::RecordTruncated]);
        assert_eq!(d.diagnostics[0].record_index, 0);
        let b = d.records[0].as_object().unwrap()["b"].as_str().unwrap();
        assert!(b.len() < MAX_SYNXL_RECORD_BYTES);
        assert_eq!(d.records[1].as_object().unwrap()["a"], Value::String("tail".into()));
    }

    #[test]
    fn field_list_count_limit() {
        let mut src = String::from("!synxl 1\n");
        for _ in 0..(MAX_SYNXL_FIELD_LISTS + 1) {
            src.push_str("!fields a\n");
        }
        let e = parse_lines(&src).unwrap_err();
        assert_eq!(e.kind, SynxlErrorKind::LimitExceeded);
    }

    #[test]
    fn truncation_respects_utf8_boundaries() {
        // §13 — "truncated at a valid UTF-8 boundary".
        let s = "aя";
        assert_eq!(truncate_utf8(s, 2), "a");
        assert_eq!(truncate_utf8(s, 3), "aя");
        assert_eq!(truncate_utf8(s, 99), "aя");
        assert_eq!(truncate_utf8("😀", 3), "");
    }

    #[test]
    fn streaming_and_whole_document_agree() {
        let src = "!synxl 1
!fields id[type:int] ; note ; m[block]
1 ; ok
  m
    k v
2 ; oops ; surplus
!fields id[type:int] ; note
3
";
        let whole = doc(src);
        let mut records = Vec::new();
        let mut diagnostics = Vec::new();
        for item in SynxlReader::new(src).unwrap() {
            let mut rec = item.unwrap();
            records.push(rec.value);
            diagnostics.append(&mut rec.diagnostics);
        }
        assert_eq!(whole.to_json(), records_to_json_array(&records));
        assert_eq!(whole.diagnostics, diagnostics);
        assert_eq!(
            kinds(&whole),
            vec![DiagnosticKind::ExtraFields, DiagnosticKind::MissingFields]
        );
    }

    #[test]
    fn a_document_may_exceed_the_synx_input_cap() {
        // §13 — the SYNX 16 MiB whole-input cap MUST NOT apply to SYNXL.
        let mut src = String::from("!synxl 1\n!fields a ; b\n");
        let row = "0123456789 ; abcdefghij\n";
        let rows = (17 * 1024 * 1024) / row.len() + 1;
        src.reserve(rows * row.len());
        for _ in 0..rows {
            src.push_str(row);
        }
        assert!(src.len() > 16 * 1024 * 1024);
        let d = doc(&src);
        assert_eq!(d.len(), rows);
        assert!(d.diagnostics.is_empty());
    }

    // ── §15.1 streaming ─────────────────────────────────────

    #[test]
    fn streaming_reader_yields_records_incrementally() {
        let src = "!synxl 1\n!fields a ; m[block]\n1\n  m\n    k v\n2 ; ignored\n";
        let mut reader = SynxlReader::new(src).unwrap();
        let first = reader.next().unwrap().unwrap();
        assert_eq!(first.index, 0);
        assert_eq!(first.line, 3);
        assert!(first.diagnostics.is_empty());
        let second = reader.next().unwrap().unwrap();
        assert_eq!(second.index, 1);
        assert_eq!(second.line, 6);
        assert_eq!(second.diagnostics.len(), 1);
        assert_eq!(second.diagnostics[0].kind, DiagnosticKind::ExtraFields);
        assert!(reader.next().is_none());
    }

    #[test]
    fn owned_reader_matches_the_borrowing_one() {
        let src = "\u{feff}!synxl 1\r\n!fields id[type:int] ; note ; m[block]\r\n1 ; ok\r\n  m\r\n    k v\r\n2 ; oops ; surplus\r\n  bogus 1\r\n";

        let borrowed: Vec<SynxlRecord> = SynxlReader::new(src)
            .unwrap()
            .map(Result::unwrap)
            .collect();

        // The reader owns the text, so it can outlive the value it was built
        // from — no `unsafe`, no keeping the source alive by hand.
        let mut owned = {
            let moved = String::from(src);
            SynxlReaderOwned::new(moved).unwrap()
        };
        assert_eq!(owned.version(), 1);
        let mut collected = Vec::new();
        while let Some(item) = owned.next() {
            collected.push(item.unwrap());
        }

        assert_eq!(collected, borrowed);
        assert_eq!(collected.len(), 2);
        assert_eq!(collected[1].line, 6, "BOM and CRLF must not shift line numbers");
        assert_eq!(owned.field_lists().len(), 1);
        assert_eq!(owned.text(), src);
    }

    #[test]
    fn owned_reader_is_returnable_from_a_function() {
        fn open(src: &str) -> SynxlReaderOwned {
            SynxlReaderOwned::with_options(src.to_string(), SynxlOptions { validate: true })
                .unwrap()
        }
        let mut reader = open("!synxl 1\n!fields a[required]\n;\n");
        let rec = reader.next().unwrap().unwrap();
        assert_eq!(rec.diagnostics.len(), 1);
        assert_eq!(rec.diagnostics[0].kind, DiagnosticKind::ConstraintViolation);
        assert!(reader.next().is_none());
        assert_eq!(reader.into_text().lines().count(), 3);
    }

    #[test]
    fn owned_reader_reports_the_prologue_error_eagerly() {
        let err = SynxlReaderOwned::new("nope\n".to_string()).unwrap_err();
        assert_eq!(err.kind, SynxlErrorKind::MissingPrologue);
    }

    // ── §15.1 streaming from io::BufRead ────────────────────

    /// Read a document three ways — borrowed, owned, and off a `BufRead` — and
    /// require identical records and diagnostics from all three.
    fn all_three_agree(src: &str) -> Vec<SynxlRecord> {
        let borrowed: Vec<SynxlRecord> =
            SynxlReader::new(src).unwrap().map(Result::unwrap).collect();
        let owned: Vec<SynxlRecord> = SynxlReaderOwned::new(src.to_string())
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let streamed: Vec<SynxlRecord> = SynxlStreamReader::new(std::io::Cursor::new(src))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(owned, borrowed, "owned reader diverged");
        assert_eq!(streamed, borrowed, "io reader diverged");
        borrowed
    }

    #[test]
    fn io_streaming_matches_the_in_memory_readers() {
        let recs = all_three_agree(
            "\u{feff}!synxl 1\r
!fields id[type:int] ; note ; m[block]\r
\r
1 ; ok\r
  m\r
    - role user\r
      content |+\r
        line one\r
\r
        line two\r
2 ; oops ; surplus\r
  bogus 1\r
!fields id[type:int]\r
  orphan\r
;\r
",
        );
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[0].line, 4, "BOM and CRLF must not shift line numbers");
        assert_eq!(recs[1].diagnostics.len(), 2);
        assert_eq!(recs[2].value.as_object().unwrap()["id"], Value::Null);
    }

    #[test]
    fn io_streaming_agrees_on_blocks_comments_and_diagnostics() {
        all_three_agree(
            "!synxl 1
###
!fields hidden
###
!fields a ; b ; m[block]
# comment
1 ; 2
  m
    k v

    j w
//comment
3 ; 4 ; 5
  m
    !include /etc/passwd
    q |+
      !active
;
",
        );
    }

    #[test]
    fn io_streaming_bounds_memory_on_an_oversized_record() {
        // §13 — the per-record cap is the real memory bound for a stream, and
        // it must cut at the same offset the in-memory readers cut at.
        let mut src = String::from("!synxl 1\n!fields a ; b\n");
        src.push_str("head ; ");
        src.push_str(&"x".repeat(MAX_SYNXL_RECORD_BYTES + 4096));
        src.push_str("\ntail ; ok\n");

        let streamed: Vec<SynxlRecord> = SynxlStreamReader::new(std::io::Cursor::new(&src))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let borrowed: Vec<SynxlRecord> =
            SynxlReader::new(&src).unwrap().map(Result::unwrap).collect();

        assert_eq!(streamed.len(), 2, "the record after the oversized one survives");
        assert_eq!(streamed[0].diagnostics[0].kind, DiagnosticKind::RecordTruncated);
        assert_eq!(
            streamed[0].value.as_object().unwrap()["b"],
            borrowed[0].value.as_object().unwrap()["b"],
            "both readers must cut at the same byte"
        );
        assert_eq!(streamed[1].value, borrowed[1].value);
    }

    #[test]
    fn io_streaming_bounds_memory_on_an_oversized_block() {
        let mut src = String::from("!synxl 1\n!fields a ; m[block]\n1\n  m |+\n");
        let line = format!("    {}\n", "y".repeat(4095));
        for _ in 0..((MAX_SYNXL_RECORD_BYTES / line.len()) + 2) {
            src.push_str(&line);
        }
        src.push_str("2\n");

        let streamed: Vec<SynxlRecord> = SynxlStreamReader::new(std::io::Cursor::new(&src))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let borrowed: Vec<SynxlRecord> =
            SynxlReader::new(&src).unwrap().map(Result::unwrap).collect();

        assert_eq!(streamed.len(), 2);
        assert_eq!(streamed[0].diagnostics[0].kind, DiagnosticKind::RecordTruncated);
        assert_eq!(streamed[0].value, borrowed[0].value);
        assert_eq!(streamed[1].value, borrowed[1].value);
    }

    #[test]
    fn io_streaming_survives_a_line_without_a_newline() {
        // A hostile document with no `LF` at all cannot be allowed to grow the
        // line buffer without bound; the cap applies to a single line too.
        let mut src = String::from("!synxl 1\n!fields a\n");
        src.push_str(&"z".repeat(MAX_SYNXL_RECORD_BYTES + 1024));
        let recs: Vec<SynxlRecord> = SynxlStreamReader::new(std::io::Cursor::new(&src))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].diagnostics[0].kind, DiagnosticKind::RecordTruncated);
        let a = recs[0].value.as_object().unwrap()["a"].as_str().unwrap();
        assert_eq!(a.len(), MAX_SYNXL_RECORD_BYTES);
    }

    #[test]
    fn io_streaming_separates_format_errors_from_io_errors() {
        // Format condition.
        let err = SynxlStreamReader::new(std::io::Cursor::new("nope\n")).unwrap_err();
        assert_eq!(err.as_format().unwrap().kind, SynxlErrorKind::MissingPrologue);
        assert!(err.as_io().is_none());

        let mut reader =
            SynxlStreamReader::new(std::io::Cursor::new("!synxl 1\n!fields a\n1\n!filds a\n2\n"))
                .unwrap();
        assert!(reader.next().unwrap().is_ok());
        let err = reader.next().unwrap().unwrap_err();
        assert_eq!(err.as_format().unwrap().kind, SynxlErrorKind::UnknownDirective);
        assert!(reader.next().is_none(), "iteration stops after a hard error");

        // I/O condition — invalid UTF-8 is not a format verdict (§3.1).
        let bad = [b'!', b's', b'y', b'n', b'x', b'l', b' ', b'1', b'\n', 0xff, 0xfe, b'\n'];
        let mut reader = SynxlStreamReader::new(std::io::Cursor::new(&bad[..])).unwrap();
        let err = reader.next().unwrap().unwrap_err();
        assert!(err.as_format().is_none());
        assert_eq!(err.as_io().unwrap().kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn io_streaming_reports_a_failing_source() {
        #[derive(Debug)]
        struct Boom;
        impl std::io::Read for Boom {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "boom"))
            }
        }
        let err = SynxlStreamReader::new(std::io::BufReader::new(Boom)).unwrap_err();
        assert_eq!(err.as_io().unwrap().kind(), std::io::ErrorKind::BrokenPipe);
    }

    // ── §15.3 field-list source ─────────────────────────────

    #[test]
    fn field_list_keeps_its_source_line() {
        // A splitting tool must re-emit the field list in effect verbatim.
        let d = doc("!synxl 1\n!fields  id[type:int]  ;  name \n1 ; x\n!fields a\n2\n");
        assert_eq!(d.field_lists[0].source(), "!fields  id[type:int]  ;  name");
        assert_eq!(d.field_lists[0].line, 2);
        assert_eq!(d.field_lists[1].source(), "!fields a");
        assert_eq!(d.field_lists[1].line, 4);
        // Programmatically built lists have none.
        assert_eq!(FieldList::new(vec![FieldDecl::new("a")]).source(), "");

        // The same text reaches a streaming consumer, which is where §15.3
        // shard emission actually happens.
        let mut reader = SynxlReader::new("!synxl 1\n!fields  a ; b\n1 ; 2\n").unwrap();
        reader.next();
        assert_eq!(reader.field_list().unwrap().source(), "!fields  a ; b");
    }

    #[test]
    fn streaming_reader_reports_a_mid_document_hard_error_once() {
        let src = "!synxl 1\n!fields a\n1\n!fields a ; a\n2\n";
        let mut reader = SynxlReader::new(src).unwrap();
        assert!(reader.next().unwrap().is_ok());
        let err = reader.next().unwrap().unwrap_err();
        assert_eq!(err.kind, SynxlErrorKind::DuplicateField);
        assert!(reader.next().is_none(), "iteration stops after a hard error");
    }

    // ── §14 writer ──────────────────────────────────────────

    fn round_trip(src: &str) {
        let a = doc(src);
        let written = a.to_synxl().unwrap();
        let b = doc(&written);
        assert_eq!(
            a.to_json(),
            b.to_json(),
            "round-trip mismatch\n--- written ---\n{written}"
        );
    }

    #[test]
    fn round_trip_scalars_and_quoting() {
        round_trip(
            "!synxl 1\n!fields a ; b ; c ; d ; e ; f ; g\n\
             1 ; \"42\" ; \"has ; semi\" ; ; \"\" ; -2.5 ; true\n",
        );
    }

    #[test]
    fn round_trip_reserved_prefixes_and_padding() {
        round_trip(
            "!synxl 1\n!fields a ; b ; c ; d\n\
             \"#tag\" ; \"//path\" ; \"!bang\" ; \"  padded  \"\n",
        );
    }

    #[test]
    fn round_trip_blocks_and_schema_evolution() {
        round_trip(
            "!synxl 1
!fields id[type:int, required] ; score[type:float] ; messages[block]
1 ; 0.91
  messages
    - role system
      content You are a helpful assistant.
    - role user
      content |+
          def f(x):
              return x + 1
!fields id[type:int] ; lang ; messages[block]
3 ; ru
  messages
    - role user
      content Как дела?
",
        );
    }

    #[test]
    fn writer_promotes_multiline_values_to_a_block() {
        // §14.3 — an LF forces promotion even though the field is inline.
        let fields = vec![FieldDecl::new("id"), FieldDecl::new("text")];
        let mut rec = HashMap::new();
        rec.insert("id".to_string(), Value::Int(1));
        rec.insert("text".to_string(), Value::String("line one\n  line two".into()));
        let records = vec![Value::Object(rec)];
        let out = write_lines(&fields, &records).unwrap();
        assert!(out.contains("text[block]"), "{out}");
        assert!(out.contains("|+"), "{out}");
        let back = doc(&out);
        assert_eq!(back.to_json(), records_to_json_array(&records));
    }

    #[test]
    fn writer_promotes_values_that_cannot_be_quoted() {
        // A value with a `;` and both quote characters has no inline form.
        let fields = vec![FieldDecl::new("id"), FieldDecl::new("a")];
        let mut rec = HashMap::new();
        rec.insert("id".to_string(), Value::Int(1));
        rec.insert("a".to_string(), Value::String("mix \" and ' and ; here".into()));
        let records = vec![Value::Object(rec)];
        let out = write_lines(&fields, &records).unwrap();
        assert!(out.contains("a[block]"), "{out}");
        let back = doc(&out);
        assert_eq!(back.to_json(), records_to_json_array(&records));
    }

    #[test]
    fn writer_rejects_an_unwritable_value() {
        // §14.3 — the value needs quoting, holds both quote characters (so no
        // quoting is available) and a `;` (so it cannot fall back to a bare
        // part either). Promoting it is the only option, and promoting the one
        // and only column would leave arity 0 — which §5.3.4 rejects. The
        // writer must say so, not emit a document its own parser refuses.
        let fields = vec![FieldDecl::new("a")];
        let mut rec = HashMap::new();
        rec.insert("a".to_string(), Value::String("mix \" and ' and ; here".into()));
        let records = vec![Value::Object(rec)];

        let err = write_lines(&fields, &records).unwrap_err();
        assert_eq!(err.kind, SynxlErrorKind::Unwritable);

        // Same value through a document.
        let doc = SynxlDocument {
            version: 1,
            records,
            field_lists: vec![FieldList::new(fields)],
            record_field_lists: vec![0],
            record_lines: vec![3],
            diagnostics: Vec::new(),
        };
        assert_eq!(doc.to_synxl().unwrap_err().kind, SynxlErrorKind::Unwritable);
    }

    #[test]
    fn writer_keeps_one_column_inline_instead_of_zero_arity() {
        // §5.3.4 — promoting every column would produce an unparsable document,
        // so the last inline column stays inline even when it wants quoting.
        let fields = vec![FieldDecl::new("a")];
        let mut rec = HashMap::new();
        rec.insert("a".to_string(), Value::String("'x\"y".into()));
        let records = vec![Value::Object(rec)];
        let out = write_lines(&fields, &records).unwrap();
        assert!(!out.contains("[block]"), "{out}");
        let back = doc(&out);
        assert_eq!(back.to_json(), records_to_json_array(&records));
    }

    #[test]
    fn writer_output_is_a_valid_document_without_records() {
        let d = doc("!synxl 1\n!fields a ; b\n");
        assert_eq!(d.to_synxl().unwrap(), "!synxl 1\n!fields a; b\n");
        assert_eq!(doc(&d.to_synxl().unwrap()).to_json(), "[]");
    }

    #[test]
    fn write_lines_emits_the_canonical_shape() {
        let d = doc("!synxl 1\n!fields id[type:int] ; name\n1 ; Wario\n");
        let out = d.to_synxl().unwrap();
        assert_eq!(out, "!synxl 1\n!fields id[type:int]; name\n1; Wario\n");
    }

    #[test]
    fn float_round_trips_through_exponent_form() {
        let fields = vec![FieldDecl::new("a"), FieldDecl::new("b"), FieldDecl::new("c")];
        let mut rec = HashMap::new();
        rec.insert("a".to_string(), Value::Float(1e300));
        rec.insert("b".to_string(), Value::Float(5.0));
        rec.insert("c".to_string(), Value::Float(f64::NAN));
        let records = vec![Value::Object(rec)];
        let out = write_lines(&fields, &records).unwrap();
        let back = doc(&out);
        assert_eq!(back.to_json(), records_to_json_array(&records));
    }

    // ── §12 projections ─────────────────────────────────────

    #[test]
    fn ndjson_projection() {
        let d = doc("!synxl 1\n!fields a ; b\n1 ; x\n2 ; y\n");
        assert_eq!(d.to_ndjson(), "{\"a\":1,\"b\":\"x\"}\n{\"a\":2,\"b\":\"y\"}\n");
    }

    #[test]
    fn keys_are_sorted_in_the_projection() {
        let d = doc("!synxl 1\n!fields z ; a ; m\n1 ; 2 ; 3\n");
        assert_eq!(d.to_json(), r#"[{"a":2,"m":3,"z":1}]"#);
    }

    // ── splitter unit coverage ──────────────────────────────

    #[test]
    fn splitter_edge_cases() {
        let parts: Vec<Part> = PartSplitter::new("a;b").collect();
        assert_eq!(parts.len(), 3 - 1);
        let parts: Vec<Part> = PartSplitter::new("").collect();
        assert_eq!(parts, vec![Part { text: "", quoted: false }]);
        let parts: Vec<Part> = PartSplitter::new("'x' ; \"y\"").collect();
        assert_eq!(
            parts,
            vec![
                Part { text: "x", quoted: true },
                Part { text: "y", quoted: true }
            ]
        );
        // Close quote followed by garbage ⇒ unquoted, garbage included.
        let parts: Vec<Part> = PartSplitter::new("'x'y ; z").collect();
        assert_eq!(parts[0], Part { text: "'x'y", quoted: false });
        // A `;` inside quotes is not a delimiter.
        let parts: Vec<Part> = PartSplitter::new("\"a;b\"").collect();
        assert_eq!(parts, vec![Part { text: "a;b", quoted: true }]);
    }
}
