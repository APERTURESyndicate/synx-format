//! SYNX Engine — resolves active markers (:random, :calc, :env, :alias, :secret, etc.)
//! in a parsed SYNX value tree. Only runs in !active mode.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use crate::calc::safe_calc;
use crate::parser;
use crate::rng;
use crate::value::*;

static SPAM_BUCKETS: OnceLock<Mutex<HashMap<String, Vec<Instant>>>> = OnceLock::new();

/// Maximum expression length accepted by :calc (prevents ReDoS/stack abuse).
const MAX_CALC_EXPR_LEN: usize = 4096;
/// Maximum resolved expression length produced by :calc substitutions.
/// Prevents pathological inputs from growing the expression until OOM.
const MAX_CALC_RESOLVED_LEN: usize = 64 * 1024;
/// Maximum file size for :include / :watch reads (10 MB).
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
/// Default maximum include depth.
const DEFAULT_MAX_INCLUDE_DEPTH: usize = 16;
/// Maximum object nesting depth for active-mode resolution (prevents stack overflow).
const MAX_RESOLVE_DEPTH: usize = 512;

/// Upper bound for single `String` scratch buffers built from hostile `:template` / replace paths.
const MAX_ENGINE_SCRATCH_STRING: usize = 4 * 1024 * 1024;

/// Validate that `full` path stays within the `base` directory (jail).
/// Returns `Ok(canonical)` or an `Err` describing the violation.
fn jail_path(base: &str, file_path: &str) -> Result<std::path::PathBuf, String> {
    // Always check leading "/" or "\" first so the message is portable:
    // POSIX absolute paths and Windows rooted paths both produce the same
    // "rooted paths are not allowed" string.
    if let Some(first) = file_path.chars().next() {
        if first == '/' || first == '\\' {
            return Err(format!("SECURITY: rooted paths are not allowed: '{}'", file_path));
        }
    }
    // Block any other absolute paths (Windows drive letters, UNC, etc.).
    let fp = std::path::Path::new(file_path);
    if fp.is_absolute() {
        return Err(format!("SECURITY: absolute paths are not allowed: '{}'", file_path));
    }

    // safety: a bare file name gives an empty parent directory; `""` does not
    // canonicalize, and every path `starts_with("")`, so the jail let
    // `../outside/x.synx` through. An empty base is the current directory, and
    // a base that cannot be resolved refuses instead of jailing nothing.
    let base = if base.is_empty() { "." } else { base };
    let base_canonical = match std::fs::canonicalize(base) {
        Ok(p) => p,
        Err(_) => return Err(format!("SECURITY: base directory not found: '{}'", base)),
    };
    let full = base_canonical.join(file_path);
    let full_canonical = match std::fs::canonicalize(&full) {
        Ok(p) => p,
        Err(_) => {
            // File may not exist yet — at least verify no ".." escapes.
            // Judged by the path as the document wrote it: the joined path
            // differs by platform (Windows folds `..` into a canonical base).
            if file_path.split(['/', '\\']).any(|seg| seg == "..") {
                return Err(format!("SECURITY: path traversal detected: '{}'", file_path));
            }
            // Without canonicalisation we cannot prove containment; only
            // accept if the un-canonicalised join still starts with the base.
            if !full.starts_with(&base_canonical) {
                return Err(format!("SECURITY: path escapes base directory: '{}'", file_path));
            }
            return Ok(full);
        }
    };
    if !full_canonical.starts_with(&base_canonical) {
        return Err(format!("SECURITY: path escapes base directory: '{}'", file_path));
    }
    Ok(full_canonical)
}

/// Whether `path` names something inside its base directory: relative, with
/// no root, drive or `..` segment. Judged by its text, so a package directory
/// that is itself a symlink (a linked local package) keeps working.
///
/// safety: `!use` joined the package name and the manifest's `main` / `wasm`
/// onto the packages directory as written, so `!use @a/../../secret` read
/// `<anywhere>/src/main.synx` into the document — the escape `!include`
/// already refused through `jail_path`.
fn is_contained_relative(path: &str) -> bool {
    if path.is_empty()
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains(':')
        || std::path::Path::new(path).is_absolute()
    {
        return false;
    }
    path.split(|c| c == '/' || c == '\\').all(|seg| seg != "..")
}

/// Most included documents one top-level resolve may read and resolve.
const MAX_INCLUDE_READS: usize = 1024;

/// Most bytes one top-level resolve may read from disk through `:include`,
/// `!include`, `!use` and `:watch`.
///
/// safety: every file is capped at `MAX_FILE_SIZE`, but a document may name
/// the same file from a thousand keys; each read is parsed and kept.
const MAX_RESOLVE_READ_BYTES: usize = 64 * 1024 * 1024;

/// Most bytes the markers of one top-level resolve may add to the document:
/// values copied by `:alias`, `:ref`, `:spam` and `:inherit`, and text built
/// by `{interpolation}`, `:replace`, `:join`, `:format` and `:prompt`.
///
/// safety: each of them can multiply what it reads. `l:alias a` / `r:alias a`
/// repeated over 22 objects is 2^22 copies from 700 bytes; 3 000 children of a
/// 3 000-field `:inherit` parent are nine million fields; one 4 MiB value
/// interpolated from 600 keys is 2.4 GiB. A copied value costs
/// `VALUE_COST` plus the bytes of its key and text — close to what it
/// occupies, so the budget is also the memory the copies can take.
const MAX_RESOLVE_EXPANSION_BYTES: usize = 64 * 1024 * 1024;

/// What one copied value costs against `MAX_RESOLVE_EXPANSION_BYTES`, text
/// aside: its slot in the object, the key's slot and their bookkeeping.
const VALUE_COST: usize = 128;

/// The prefixes of the strings the engine leaves in place of a value it
/// could not produce.
pub const RUNTIME_ERROR_PREFIXES: &[&str] = &[
    "INCLUDE_ERR:",
    "WATCH_ERR:",
    "CALC_ERR:",
    "SPAM_ERR:",
    "CONSTRAINT_ERR:",
    "ALIAS_ERR:",
    "NESTING_ERR:",
    "LIMIT_ERR:",
    "TYPE_ERR:",
    "WASM_ERR:",
];

/// What the keys under `:secret` held before the pre-pass masked them, by
/// object path and key (see [`mask_secret_literals`]).
///
/// perf: the document's own map type. With the standard one every key of an
/// object that held a secret was hashed with SipHash on its turn.
type SecretOriginals = Map<Map<Value>>;

/// The value a marker is left with once the document's budget is spent.
const LIMIT_ERR: &str = "LIMIT_ERR: resolution budget exceeded";

/// The value left where a marker would nest objects past `MAX_RESOLVE_DEPTH`.
const NESTING_ERR: &str = "NESTING_ERR: maximum object nesting depth exceeded";

/// Most bytes of a `.synx.lock` one resolve reads.
const MAX_LOCK_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// A `.synx.lock` sidecar as one resolve has seen it.
///
/// safety: the file is shared by every process that resolves a document in
/// the directory, and none of them may undo another. It was read whole,
/// extended in memory and written back whole: of 800 values handed out by
/// eight processes, 52 were still in the file afterwards, and the next start
/// gave the other 748 keys new values. Now a line is only ever appended, in
/// one write, and the first line for a key is its value for everyone — a
/// process that has just appended reads what came before its own line and
/// takes that if another process was first.
struct LockFile {
    path: std::path::PathBuf,
    /// First value recorded for each key, in file order.
    values: HashMap<String, String>,
    /// How much of the file `values` covers.
    seen: u64,
    /// What was read ends inside a line (a file edited by hand).
    open_line: bool,
    /// Open for appending once this resolve has a value of its own to record.
    writer: Option<std::fs::File>,
    /// The path is a link, a folder or anything else that is not a plain
    /// file: it is left alone.
    foreign: bool,
}

impl LockFile {
    fn open(path: &std::path::Path) -> Self {
        // safety: the lock file is the one thing a resolve writes, and it
        // writes where the document's folder says. A `.synx.lock` that is a
        // link — a cloned repository can carry one — led out of that folder:
        // `:once` appended `<key> <value>` lines, the key chosen by the
        // document, to whatever file the link named, and read that file's
        // lines back as values. A lock that is not a plain file of its own
        // is neither read nor written; the values of this resolve are then
        // kept in memory only, as `resolve_read_only` keeps them.
        let foreign = match std::fs::symlink_metadata(path) {
            Ok(meta) => !meta.file_type().is_file(),
            Err(e) => e.kind() != std::io::ErrorKind::NotFound,
        };
        let mut lock =
            LockFile { path: path.to_path_buf(), values: HashMap::new(), seen: 0, open_line: false, writer: None, foreign };
        if !foreign {
            if let Ok(mut file) = std::fs::File::open(path) {
                lock.read_from(&mut file);
            }
        }
        lock
    }

    /// Take in what the file has gained since it was last read.
    fn read_from(&mut self, file: &mut std::fs::File) {
        use std::io::{Read, Seek, SeekFrom};
        if self.seen >= MAX_LOCK_FILE_BYTES || file.seek(SeekFrom::Start(self.seen)).is_err() {
            return;
        }
        let mut bytes = Vec::new();
        if (&mut *file).take(MAX_LOCK_FILE_BYTES - self.seen).read_to_end(&mut bytes).is_err() || bytes.is_empty() {
            return;
        }
        self.seen += bytes.len() as u64;
        self.open_line = bytes.last() != Some(&b'\n');
        for line in String::from_utf8_lossy(&bytes).lines() {
            if let Some((key, rest)) = line.split_once(' ') {
                self.values.entry(key.to_string()).or_insert_with(|| rest.trim_start().to_string());
            }
        }
    }

    /// Record `key value`, and return the value the file gives the key.
    fn record(&mut self, key: &str, value: String, read_only: bool) -> String {
        use std::io::Write;
        if !read_only && !self.foreign {
            if self.writer.is_none() {
                self.writer = std::fs::OpenOptions::new().read(true).append(true).create(true).open(&self.path).ok();
            }
            if let Some(mut file) = self.writer.take() {
                // What others appended since the last read comes first.
                self.read_from(&mut file);
                if let Some(theirs) = self.values.get(key) {
                    self.writer = Some(file);
                    return theirs.clone();
                }
                let line = format!("{}{} {}\n", if self.open_line { "\n" } else { "" }, key, value);
                // One write: lines of two processes never interleave.
                if file.write_all(line.as_bytes()).is_ok() {
                    // Whoever appended between that read and this write is ahead of us.
                    self.read_from(&mut file);
                }
                self.writer = Some(file);
            }
        }
        self.values.entry(key.to_string()).or_insert(value).clone()
    }
}

/// State shared by a top-level `resolve` and the nested resolves it starts for
/// included documents and packages.
#[derive(Default)]
struct ResolveState {
    /// `resolve` calls in progress on this thread.
    depth: usize,
    /// Files whose resolution is in progress, outermost first.
    chain: Vec<std::path::PathBuf>,
    /// Documents read so far.
    reads: usize,
    /// Bytes read so far.
    read_bytes: usize,
    /// Bytes added so far (see `MAX_RESOLVE_EXPANSION_BYTES`).
    expansion: usize,
    /// Lock files `:once` has touched.
    locks: Vec<LockFile>,
    /// The outermost resolve may not write to disk.
    read_only: bool,
}

thread_local! {
    static RESOLVE_STATE: std::cell::RefCell<ResolveState> = std::cell::RefCell::new(ResolveState::default());
}

/// Held for the length of one `resolve` call. The outermost one starts the
/// budgets afresh and, on the way out, writes the lock files `:once` changed.
struct ResolveScope;

impl ResolveScope {
    fn enter(read_only: bool) -> Self {
        RESOLVE_STATE.with(|s| {
            let mut s = s.borrow_mut();
            if s.depth == 0 {
                s.chain.clear();
                s.reads = 0;
                s.read_bytes = 0;
                s.expansion = 0;
                s.locks.clear();
                s.read_only = read_only;
            }
            s.depth += 1;
        });
        ResolveScope
    }
}

impl Drop for ResolveScope {
    fn drop(&mut self) {
        let locks = RESOLVE_STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.depth = s.depth.saturating_sub(1);
            if s.depth == 0 { std::mem::take(&mut s.locks) } else { Vec::new() }
        });
        // Closing the files is all that is left: every line is already there.
        drop(locks);
    }
}

/// Take `bytes` from the expansion budget; `false` once it is spent.
fn charge_expansion(bytes: usize) -> bool {
    RESOLVE_STATE.with(|s| {
        let mut s = s.borrow_mut();
        match s.expansion.checked_add(bytes) {
            Some(total) if total <= MAX_RESOLVE_EXPANSION_BYTES => {
                s.expansion = total;
                true
            }
            _ => {
                s.expansion = MAX_RESOLVE_EXPANSION_BYTES;
                false
            }
        }
    })
}

/// Bytes of the expansion budget not yet spent.
fn expansion_room() -> usize {
    RESOLVE_STATE.with(|s| MAX_RESOLVE_EXPANSION_BYTES.saturating_sub(s.borrow().expansion))
}

/// What copying one key's declarations costs against the budget.
fn meta_cost(key: &str, meta: &Meta) -> usize {
    let mut cost = (2 * VALUE_COST).saturating_add(key.len());
    for s in meta.markers.iter().chain(meta.args.iter()) {
        cost = cost.saturating_add(24 + s.len());
    }
    if let Some(hint) = &meta.type_hint {
        cost = cost.saturating_add(hint.len());
    }
    if let Some(c) = &meta.constraints {
        for s in c.type_name.iter().chain(c.pattern.iter()).chain(c.enum_values.iter().flatten()) {
            cost = cost.saturating_add(24 + s.len());
        }
    }
    cost
}

/// What copying one object's declarations costs against the budget.
fn meta_map_cost(mm: &MetaMap) -> usize {
    let mut cost = VALUE_COST;
    for (key, meta) in mm {
        cost = cost.saturating_add(meta_cost(key, meta));
    }
    cost
}

/// What copying `value` costs, and how many levels of objects and lists it nests.
fn value_cost(value: &Value) -> (usize, usize) {
    match value {
        Value::String(s) | Value::Secret(s) => (VALUE_COST.saturating_add(s.len()), 0),
        Value::Array(items) => {
            let mut cost = VALUE_COST;
            let mut depth = 0;
            for item in items {
                let (c, d) = value_cost(item);
                cost = cost.saturating_add(c);
                depth = depth.max(d);
            }
            (cost, depth + 1)
        }
        Value::Object(map) => {
            let mut cost = VALUE_COST;
            let mut depth = 0;
            for (k, v) in map {
                let (c, d) = value_cost(v);
                cost = cost.saturating_add(c).saturating_add(k.len());
                depth = depth.max(d);
            }
            (cost, depth + 1)
        }
        _ => (VALUE_COST, 0),
    }
}

/// [`value_cost`] for a copy that has `room` bytes of budget left: `None` as
/// soon as the cost passes `room`, without measuring the rest.
///
/// safety: the measure was taken in full before the budget was asked. Once
/// the budget was spent every further `:alias` of a large group still walked
/// all of it to learn it would not fit — 96 000 aliases of a 96 000-field
/// group took 25 s to answer `LIMIT_ERR` 95 995 times.
fn value_cost_within(value: &Value, room: usize) -> Option<(usize, usize)> {
    fn walk(value: &Value, left: &mut usize) -> Option<usize> {
        let own = match value {
            Value::String(s) | Value::Secret(s) => VALUE_COST.saturating_add(s.len()),
            _ => VALUE_COST,
        };
        *left = left.checked_sub(own)?;
        match value {
            Value::Array(items) => {
                let mut depth = 0;
                for item in items {
                    depth = depth.max(walk(item, left)?);
                }
                Some(depth + 1)
            }
            Value::Object(map) => {
                let mut depth = 0;
                for (k, v) in map {
                    *left = left.checked_sub(k.len())?;
                    depth = depth.max(walk(v, left)?);
                }
                Some(depth + 1)
            }
            _ => Some(0),
        }
    }
    let mut left = room;
    let nesting = walk(value, &mut left)?;
    Some((room - left, nesting))
}

/// A copy of `value` for a marker to place in an object `depth` levels below
/// the root, or the error it leaves instead: the copy is charged to the
/// document's budget and may not nest past `MAX_RESOLVE_DEPTH`.
///
/// spec: the budget is asked first. A value that neither fits the budget nor
/// the depth is a `LIMIT_ERR`, and after it every copy is — it used to be
/// measured whole to report `NESTING_ERR` if it was also too deep.
fn budgeted_copy(value: &Value, depth: usize) -> Value {
    let Some((cost, nesting)) = value_cost_within(value, expansion_room()) else {
        charge_expansion(usize::MAX);
        return Value::String(LIMIT_ERR.to_string());
    };
    if depth.saturating_add(nesting) >= MAX_RESOLVE_DEPTH {
        return Value::String(NESTING_ERR.to_string());
    }
    if !charge_expansion(cost) {
        return Value::String(LIMIT_ERR.to_string());
    }
    value.clone()
}

/// Held while an included file resolves; leaving the scope closes it.
struct IncludeGuard;

impl Drop for IncludeGuard {
    fn drop(&mut self) {
        RESOLVE_STATE.with(|s| {
            s.borrow_mut().chain.pop();
        });
    }
}

/// Opens `full` for resolution, or refuses a file already being resolved
/// (a cycle) and reads past the per-document budget.
///
/// safety: the depth cap alone let `a:include self.synx` / `b:include
/// self.synx` resolve 2^16 times — 22 s for a three-line file, and a third
/// key made it hang. A cycle now stops at its first repeat, and the budget
/// bounds an acyclic fan-out.
fn include_enter(full: &std::path::Path, shown: &str) -> Result<IncludeGuard, String> {
    RESOLVE_STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.chain.iter().any(|p| p == full) {
            return Err(format!("circular include: '{}'", shown));
        }
        if s.reads >= MAX_INCLUDE_READS {
            return Err(format!("include budget ({} files) exceeded", MAX_INCLUDE_READS));
        }
        s.reads += 1;
        s.chain.push(full.to_path_buf());
        Ok(IncludeGuard)
    })
}

/// Read a file for `:include`, `!include`, `!use` or `:watch`, counting its
/// size against `MAX_RESOLVE_READ_BYTES`. `shown` is the path as the document
/// wrote it, for the error text.
fn read_budgeted(full: &std::path::Path, shown: &str) -> Result<String, String> {
    let size = std::fs::metadata(full).map(|m| m.len()).unwrap_or(0);
    let fits = RESOLVE_STATE.with(|s| {
        let s = s.borrow();
        (s.read_bytes as u64).saturating_add(size) <= MAX_RESOLVE_READ_BYTES as u64
    });
    if !fits {
        return Err(format!("read budget ({} bytes) exceeded", MAX_RESOLVE_READ_BYTES));
    }
    // A directory reads as "permission denied" on Windows and "is a
    // directory" elsewhere; it is neither.
    if full.is_dir() {
        return Err(format!("not a file: {}", shown));
    }
    let text = std::fs::read_to_string(full).map_err(|e| fmt_io_err(&e, shown))?;
    RESOLVE_STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.read_bytes = s.read_bytes.saturating_add(text.len());
    });
    Ok(text)
}

/// Check file size before reading.
fn check_file_size(path: &std::path::Path) -> Result<(), String> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() > MAX_FILE_SIZE => {
            Err(format!("SECURITY: file too large ({} bytes, max {})", meta.len(), MAX_FILE_SIZE))
        }
        _ => Ok(()),
    }
}

/// Normalise std::io::Error kinds to a portable, OS-agnostic message so that
/// INCLUDE_ERR / WATCH_ERR strings don't drift between Windows ("The system
/// cannot find the file specified. (os error 2)") and POSIX ("No such file or
/// directory").
///
/// spec: the text never carries the operating system's own wording, so the
/// same document resolves to the same string on every machine. A name the
/// platform cannot hold at all (a NUL, or `|` on Windows) is a file that is
/// not there.
fn fmt_io_err(e: &std::io::Error, ctx: &str) -> String {
    use std::io::ErrorKind;
    let invalid_name = cfg!(windows) && e.raw_os_error() == Some(123);
    match e.kind() {
        ErrorKind::NotFound | ErrorKind::InvalidInput => format!("file not found: {}", ctx),
        _ if invalid_name => format!("file not found: {}", ctx),
        ErrorKind::PermissionDenied => format!("permission denied: {}", ctx),
        ErrorKind::InvalidData => format!("file is not valid UTF-8: {}", ctx),
        _ => format!("cannot read file: {}", ctx),
    }
}

/// Whether a marker chain contains `name`.
///
/// perf: replaces `markers.contains(&"name".to_string())`, which allocated
/// two strings per check — about seventy per marked key.
#[inline]
fn has_marker(markers: &[String], name: &str) -> bool {
    markers.iter().any(|m| m == name)
}

/// Resolve all active-mode markers in a ParseResult.
/// Returns the resolved root Value.
pub fn resolve(result: &mut ParseResult, options: &Options) {
    resolve_scoped(result, options, false);
}

/// [`resolve`] for a document the host does not trust with its disk: nothing
/// is written. `:once` returns the value `.synx.lock` already holds, or a new
/// one that is not recorded.
pub fn resolve_read_only(result: &mut ParseResult, options: &Options) {
    resolve_scoped(result, options, true);
}

fn resolve_scoped(result: &mut ParseResult, options: &Options, read_only: bool) {
    if result.mode != Mode::Active {
        return;
    }
    let _scope = ResolveScope::enter(read_only);
    let mut metadata = std::mem::take(&mut result.metadata);
    let includes_directives = std::mem::take(&mut result.includes);
    let use_directives = std::mem::take(&mut result.uses);

    // ── Load !use packages (before includes, so packages are available) ──
    #[cfg(feature = "wasm")]
    let mut wasm_runtime = crate::wasm::WasmMarkerRuntime::new();
    let packages_map = load_packages(
        &use_directives,
        options,
        #[cfg(feature = "wasm")]
        &mut wasm_runtime,
    );

    // If wasm feature is enabled and markers were loaded, create options with runtime
    #[cfg(feature = "wasm")]
    let wasm_options;
    #[cfg(feature = "wasm")]
    let options = if !wasm_runtime.marker_names().is_empty() {
        wasm_options = Options {
            wasm_runtime: Some(std::sync::Arc::new(wasm_runtime)),
            ..options.clone()
        };
        &wasm_options
    } else {
        options
    };

    // ── Load !include files ──
    let mut includes_map = load_includes(&includes_directives, options);
    // Root `:include` keys the pre-pass below already resolved, as
    // `[path, root]`; kept apart from `includes_map`, whose size and first
    // value `{key:include}` reads.
    let mut preresolved: Map = Map::default();

    // ── Pre-pass: also register `:include`/`:import` marker keys as aliases ──
    // This makes `{leaf:<key>}` interpolation work for the README pattern:
    //
    //   db:include ./common.synx
    //   greeting Hello, {site_name:db}!
    //
    // Without this, only `!include` directives feed the alias map.
    // perf: only a root key marked `:include` or `:import` has anything to do
    // here. Every root key used to be looked up among the declarations.
    let root_meta = metadata.get("").filter(|mm| {
        mm.values().any(|meta| meta.markers.iter().any(|m| m == "include" || m == "import"))
    });
    if let (Value::Object(root_map), Some(root_meta)) = (&result.root, root_meta) {
        for (key, _) in root_map.iter() {
            let meta = match root_meta.get(key) {
                Some(m) => m,
                None => continue,
            };
            let is_inc = meta.markers.iter().any(|m| m == "include" || m == "import");
            if !is_inc { continue; }
            // safety: the pre-pass resolves the included document, whose own
            // pre-pass includes again; without the depth cap every other
            // include path has, a file that includes itself overflowed the
            // stack and took the process down.
            if options._include_depth >= options.max_include_depth.unwrap_or(DEFAULT_MAX_INCLUDE_DEPTH) {
                continue;
            }
            // The current value is the path string from the parser.
            let path = match root_map.get(key) {
                Some(Value::String(s)) => s.clone(),
                _ => continue,
            };
            let base = options.base_path.as_deref().unwrap_or(".");
            let full = match jail_path(base, &path) {
                Ok(p) => p,
                Err(_) => continue,
            };
            if check_file_size(&full).is_err() { continue; }
            let _open = match include_enter(&full, &path) {
                Ok(g) => g,
                Err(_) => continue,
            };
            let text = match read_budgeted(&full, &path) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let mut included = parser::parse(&text);
            if included.mode == Mode::Active {
                let mut child_opts = options.clone();
                child_opts._include_depth += 1;
                if let Some(parent) = full.parent() {
                    child_opts.base_path = Some(parent.to_string_lossy().into_owned());
                }
                resolve(&mut included, &child_opts);
            }
            // perf: the `:include` marker of this key takes this result instead
            // of reading and resolving the file again. Each level resolved it
            // twice, so a chain of includes cost 2^depth resolutions.
            // safety: `{name:key}` reads this copy. Under `:secret` it is
            // masked like the one the key itself is given — `db:secret:include
            // creds.synx` / `x {password:db}` handed `x` the password.
            let mut named = included.root.clone();
            if meta.markers.iter().any(|m| m == "secret") {
                mask_secret_value(&mut named);
            }
            preresolved.insert(
                key.clone(),
                Value::Array(vec![Value::String(path.clone()), included.root]),
            );
            includes_map.entry(key.clone()).or_insert(named);
        }
    }

    // ── Merge packages into root before resolution ──
    if let Value::Object(ref mut root_map) = result.root {
        for (alias, pkg_value) in &packages_map {
            root_map.entry(alias.clone()).or_insert_with(|| pkg_value.clone());
        }
    }

    // ── Type and constraint registries ──
    // spec: built from the document as it stands, before inheritance, walking
    // each object's own declarations and then its children — the order the
    // TypeScript engine uses, which decides the winner when one field name is
    // declared with different types. A declaration whose object was later
    // overwritten (`a` / `  x(int) 1` / `a 5`) no longer counts.
    let mut type_registry: Map<String> = Map::default();
    let mut constraint_registry: Map<FieldRule> = Map::default();
    // Inheritance rewrites the declarations; a document that inherits takes
    // its registry as copies, any other as references into them.
    let inherits = metadata
        .get("")
        .is_some_and(|mm| mm.values().any(|meta| has_marker(&meta.markers, "inherit")));
    collect_registries(&result.root, &metadata, &mut String::new(), &mut type_registry, &mut constraint_registry, inherits, 0);

    // ── :inherit pre-pass ──
    if inherits {
        apply_inheritance(&mut result.root, &mut metadata);
    }
    // Remove private blocks (keys starting with _)
    if let Value::Object(ref mut root_map) = result.root {
        root_map.retain(|k, _| !k.starts_with('_'));
    }

    // ── :secret pre-pass ──
    // safety: every value under `:secret` is masked *before* any resolution,
    // so no field that names it — `{that_key}`, `:alias`, `:map` — can read
    // the plain text, wherever in the document it stands. The value comes
    // back for the key's own turn (its subtree and its marker chain), and
    // what that produces is masked again.
    let mut secret_originals = SecretOriginals::default();
    mask_declared_secrets(&mut result.root, &metadata, &mut secret_originals);

    let calc_scope = CalcScope::of(&result.root);
    let mut path = String::new();
    let mut at: Vec<Step> = Vec::new();
    resolve_value(&mut result.root, &mut at, options, &metadata, &mut path, &includes_map, &preresolved, &calc_scope, &mut secret_originals);

    // ── Validate field constraints, then types (global, by field name) ──
    // perf: one walk for both; per field the order is the same as two walks.
    validate_fields(&mut result.root, &constraint_registry, &type_registry, &metadata);

    result.metadata = metadata;
    result.includes = includes_directives;
}

/// Append `key` to a metadata path as one segment.
///
/// spec: a key may itself contain a dot (`a.b`). Written as is, the object
/// `a.b` and the object `b` inside `a` had one path, so the markers of one
/// applied to the other. `.` and `\` in a key are escaped with `\`; a key
/// without them — nearly every key — is copied as it is.
pub(crate) fn push_key_segment(path: &mut String, key: &str) {
    if memchr::memchr2(b'.', b'\\', key.as_bytes()).is_none() {
        path.push_str(key);
        return;
    }
    for c in key.chars() {
        if c == '.' || c == '\\' {
            path.push('\\');
        }
        path.push(c);
    }
}

/// `key` as one metadata-path segment (see [`push_key_segment`]).
pub(crate) fn key_segment(key: &str) -> std::borrow::Cow<'_, str> {
    if memchr::memchr2(b'.', b'\\', key.as_bytes()).is_none() {
        return std::borrow::Cow::Borrowed(key);
    }
    let mut out = String::with_capacity(key.len() + 2);
    push_key_segment(&mut out, key);
    std::borrow::Cow::Owned(out)
}

/// The segments of a metadata path, unescaped: `a\.b.c` is `["a.b", "c"]`.
pub fn meta_path_segments(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    if path.is_empty() {
        return out;
    }
    let mut seg = String::new();
    let mut chars = path.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    seg.push(n);
                }
            }
            '.' => out.push(std::mem::take(&mut seg)),
            _ => seg.push(c),
        }
    }
    out.push(seg);
    out
}

/// Append the metadata-path segment of list item `idx`: `.#<idx>`.
///
/// A key never starts with `#` (that line is a comment), so the segment
/// cannot collide with an object key.
pub(crate) fn push_item_segment(path: &mut String, idx: usize) {
    use std::fmt::Write as _;
    path.push_str(".#");
    let _ = write!(path, "{}", idx);
}

/// One descent from an object into an object it holds, by position.
#[derive(Clone, Copy)]
enum Step {
    /// Into the value of the key at this position.
    Key(usize),
    /// Into an item of the list under a key: `(position of the key, index of the item)`.
    Item(usize, usize),
}

/// Stands in for an object that is not where its positions say.
fn empty_object() -> &'static Map {
    static EMPTY: OnceLock<Map> = OnceLock::new();
    EMPTY.get_or_init(Map::default)
}

/// The object `at` leads to from the root.
///
/// safety: a marker reads the whole document while it rewrites one object of
/// it. The engine used to hold `&mut` to that object and read the rest
/// through a raw pointer to the root: two live references to the same
/// memory, which the language does not allow whatever the code then does
/// with them. Now nothing is held. A read borrows the document, a write
/// borrows it again, and the object is found each time by the positions that
/// lead to it — positions do not change while a document resolves (nothing
/// is removed, and a rewritten key keeps its place).
fn object_at<'a>(root: &'a Value, at: &[Step]) -> &'a Map {
    let mut current: &Map = match root {
        Value::Object(map) => map,
        _ => return empty_object(),
    };
    for step in at {
        let next = match *step {
            Step::Key(slot) => current.get_index(slot).map(|(_, value)| value),
            Step::Item(slot, item) => match current.get_index(slot) {
                Some((_, Value::Array(items))) => items.get(item),
                _ => None,
            },
        };
        current = match next {
            Some(Value::Object(map)) => map,
            _ => return empty_object(),
        };
    }
    current
}

/// [`object_at`] for writing; `None` when there is no object there.
fn object_at_mut<'a>(root: &'a mut Value, at: &[Step]) -> Option<&'a mut Map> {
    let mut current: &mut Map = match root {
        Value::Object(map) => map,
        _ => return None,
    };
    for step in at {
        let next = match *step {
            Step::Key(slot) => current.get_index_mut(slot).map(|(_, value)| value),
            Step::Item(slot, item) => match current.get_index_mut(slot) {
                Some((_, Value::Array(items))) => items.get_mut(item),
                _ => None,
            },
        };
        current = match next {
            Some(Value::Object(map)) => map,
            _ => return None,
        };
    }
    Some(current)
}

/// The value of the key at position `slot` of the object `at` leads to.
///
/// perf: a key being resolved is named by its position. Its markers used to
/// look it up by name each time they read or wrote it — five or six hashes
/// of the key and as many probes for one `:env:default`.
#[inline]
fn value_at<'a>(root: &'a Value, at: &[Step], slot: usize) -> Option<&'a Value> {
    object_at(root, at).get_index(slot).map(|(_, value)| value)
}

/// [`value_at`] for writing.
#[inline]
fn value_at_mut<'a>(root: &'a mut Value, at: &[Step], slot: usize) -> Option<&'a mut Value> {
    object_at_mut(root, at)?.get_index_mut(slot).map(|(_, value)| value)
}

/// Give the key at position `slot` a new value.
#[inline]
fn put_at(root: &mut Value, at: &[Step], slot: usize, value: Value) {
    if let Some(place) = value_at_mut(root, at, slot) {
        *place = value;
    }
}

/// What a key holds, for the walk below.
enum Holds {
    Object,
    List(usize),
    Other,
}

#[allow(clippy::too_many_arguments)]
fn resolve_value(
    root: &mut Value,
    at: &mut Vec<Step>,
    options: &Options,
    metadata: &Map<MetaMap>,
    path: &mut String,
    includes: &Map,
    preresolved: &Map,
    calc_scope: &CalcScope,
    secret_originals: &mut SecretOriginals,
) {
    // One step per level: how deep the object being resolved sits.
    let depth = at.len();
    // Guard: prevent stack overflow from deeply nested objects
    if depth >= MAX_RESOLVE_DEPTH {
        if let Some(map) = object_at_mut(root, at) {
            for val in map.values_mut() {
                *val = Value::String(
                    "NESTING_ERR: maximum object nesting depth exceeded".to_string()
                );
            }
        }
        return;
    }

    let Some(len) = object_at_mut(root, at).map(|map| map.len()) else { return };

    // One pass in document order: each key's subtree, then the key's own
    // markers, then the next key — so a marker sees every key before it fully
    // resolved, at any depth.
    //
    // spec: until 3.7.5 every nested object was resolved before any marker of
    // its parent, so `a:calc …` followed by `b` / `  x {a}` gave `x` the
    // unresolved expression in Rust and the result in TypeScript.
    // perf: one path buffer for the whole walk instead of a `format!` per
    // child; metadata is borrowed, not cloned per object.
    let mm = metadata.get(path.as_str());
    let base_len = path.len();
    let local_calc = LocalCalc::default();
    // What this object's `:secret` keys held before the pre-pass.
    let mut originals_here = if secret_originals.is_empty() { None } else { secret_originals.swap_remove(path.as_str()) };
    for i in 0..len {
        // The key's value, with its secret back in place for its own turn;
        // for a group or a list `path` now names it.
        let holds = {
            let Some(map) = object_at_mut(root, at) else { return };
            match map.get_index_mut(i) {
                None => Holds::Other,
                Some((key, child)) => {
                    if let Some(original) = originals_here.as_mut().and_then(|o| o.swap_remove(key.as_str())) {
                        *child = original;
                        if matches!(child, Value::Object(_) | Value::Array(_)) {
                            // Secrets nested in it stay masked until their own turn.
                            if base_len > 0 {
                                path.push('.');
                            }
                            push_key_segment(path, key);
                            mask_secret_subtree(child, metadata, path, secret_originals);
                            path.truncate(base_len);
                        }
                    }
                    let holds = match child {
                        Value::Object(_) => Holds::Object,
                        Value::Array(items) => Holds::List(items.len()),
                        _ => Holds::Other,
                    };
                    if !matches!(holds, Holds::Other) {
                        if base_len > 0 {
                            path.push('.');
                        }
                        push_key_segment(path, key);
                    }
                    holds
                }
            }
        };
        match holds {
            Holds::Object => {
                at.push(Step::Key(i));
                resolve_value(root, at, options, metadata, path, includes, preresolved, calc_scope, secret_originals);
                at.pop();
                path.truncate(base_len);
            }
            Holds::List(count) => {
                let list_len = path.len();
                for idx in 0..count {
                    at.push(Step::Item(i, idx));
                    // Only an item that is an object has anything to resolve.
                    if object_at_mut(root, at).is_some() {
                        push_item_segment(path, idx);
                        resolve_value(root, at, options, metadata, path, includes, preresolved, calc_scope, secret_originals);
                        path.truncate(list_len);
                    }
                    at.pop();
                }
                path.truncate(base_len);
            }
            Holds::Other => {}
        }
        let Some(mm) = mm else { continue };
        // perf: the key's name is borrowed from its declaration, which lives
        // in the metadata and not in the document being rewritten. It was
        // copied out of the document for every marked key.
        let (key, meta) = {
            let Some((k, _)) = object_at(root, at).get_index(i) else { break };
            let Some((key, meta)) = mm.get_key_value(k.as_str()) else { continue };
            (key.as_str(), meta)
        };
        apply_markers(root, at, i, key, meta, options, path, metadata, preresolved, calc_scope, &local_calc, depth);
    }

    // Third pass: auto-{} interpolation on all string values
    let len = object_at(root, at).len();
    for i in 0..len {
        let replaced = {
            let map = object_at(root, at);
            match map.get_index(i) {
                Some((_, Value::String(s))) if memchr::memchr(b'{', s.as_bytes()).is_some() => {
                    let root_ref: &Value = root;
                    // safety: the text may grow by what is left of the budget and
                    // no further — one 4 MiB value named from 600 keys was 2.4 GiB.
                    let cap = s.len().saturating_add(expansion_room());
                    match resolve_interpolation(s, root_ref, map, includes, cap) {
                        None => {
                            charge_expansion(usize::MAX);
                            Some(LIMIT_ERR.to_string())
                        }
                        Some(result) if result == *s => None,
                        Some(result) => {
                            charge_expansion(result.len().saturating_sub(s.len()));
                            Some(result)
                        }
                    }
                }
                _ => None,
            }
        };
        if let Some(result) = replaced {
            if let Some((_, v)) = object_at_mut(root, at).and_then(|map| map.get_index_mut(i)) {
                *v = Value::String(result);
            }
        }
    }
}

/// Bits for the built-in markers `apply_markers` dispatches on.
mod mk {
    pub const ALIAS: u32 = 1 << 0;
    pub const CALC: u32 = 1 << 1;
    pub const CLAMP: u32 = 1 << 2;
    pub const DEFAULT: u32 = 1 << 3;
    pub const ENV: u32 = 1 << 4;
    pub const FALLBACK: u32 = 1 << 5;
    pub const FORMAT: u32 = 1 << 6;
    pub const GEO: u32 = 1 << 7;
    pub const I18N: u32 = 1 << 8;
    pub const IMPORT: u32 = 1 << 9;
    pub const INCLUDE: u32 = 1 << 10;
    pub const JOIN: u32 = 1 << 11;
    pub const MAP: u32 = 1 << 12;
    pub const ONCE: u32 = 1 << 13;
    pub const PROMPT: u32 = 1 << 14;
    pub const RANDOM: u32 = 1 << 15;
    pub const REF: u32 = 1 << 16;
    pub const REPLACE: u32 = 1 << 17;
    pub const ROUND: u32 = 1 << 18;
    pub const SECRET: u32 = 1 << 19;
    pub const SORT: u32 = 1 << 20;
    pub const SPAM: u32 = 1 << 21;
    pub const SPLIT: u32 = 1 << 22;
    pub const SUM: u32 = 1 << 23;
    pub const UNIQUE: u32 = 1 << 24;
    pub const VERSION: u32 = 1 << 25;
    pub const WATCH: u32 = 1 << 26;
}

/// Classify a marker chain into [`mk`] bits. Arguments (`3000` in
/// `:default:3000`) and unknown names contribute nothing.
fn marker_bits(markers: &[String]) -> u32 {
    let mut bits = 0;
    for m in markers {
        bits |= match m.as_str() {
            "alias" => mk::ALIAS,
            "calc" => mk::CALC,
            "clamp" => mk::CLAMP,
            "default" => mk::DEFAULT,
            "env" => mk::ENV,
            "fallback" => mk::FALLBACK,
            "format" => mk::FORMAT,
            "geo" => mk::GEO,
            "i18n" => mk::I18N,
            "import" => mk::IMPORT,
            "include" => mk::INCLUDE,
            "join" => mk::JOIN,
            "map" => mk::MAP,
            "once" => mk::ONCE,
            "prompt" => mk::PROMPT,
            "random" => mk::RANDOM,
            "ref" => mk::REF,
            "replace" => mk::REPLACE,
            "round" => mk::ROUND,
            "secret" => mk::SECRET,
            "sort" => mk::SORT,
            "spam" => mk::SPAM,
            "split" => mk::SPLIT,
            "sum" => mk::SUM,
            "unique" => mk::UNIQUE,
            "version" => mk::VERSION,
            "watch" => mk::WATCH,
            _ => 0,
        };
    }
    bits
}

/// Run the markers of one key: `key` sits at position `slot` of the object
/// `at` leads to, and stays there — resolution replaces values, it never
/// adds or removes a key of the object it is working through.
#[allow(clippy::too_many_arguments)]
fn apply_markers(
    root: &mut Value,
    at: &[Step],
    slot: usize,
    key: &str,
    meta: &Meta,
    options: &Options,
    path: &str,
    metadata: &Map<MetaMap>,
    preresolved: &Map,
    calc_scope: &CalcScope,
    local_calc: &LocalCalc,
    depth: usize,
) {
    let markers = &meta.markers;
    // perf: one pass classifies the chain; every marker test below is a bit
    // test instead of a scan with string compares (~35 scans per key before).
    let mb = marker_bits(markers);

    // ── :spam ──
    // Syntax: key:spam:MAX_CALLS:WINDOW_SEC target
    // WINDOW_SEC defaults to 1 when omitted.
    // If target is a key path, resolves its value after passing the limit check.
    if mb & mk::SPAM != 0 {
        let spam_idx = markers.iter().position(|m| m == "spam").unwrap();
        let max_calls = markers
            .get(spam_idx + 1)
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        let window_sec = markers
            .get(spam_idx + 2)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(1);

        if max_calls == 0 {
            put_at(root, at, slot,
                Value::String("SPAM_ERR: invalid limit, use :spam:MAX[:WINDOW_SEC]".to_string()),
            );
            return;
        }

        let target = value_at(root, at, slot)
            .map(value_to_string)
            .unwrap_or_else(|| key.to_string());
        let bucket_key = format!("{}::{}", key, target);

        if !allow_spam_access(&bucket_key, max_calls, window_sec) {
            // safety: the target is the key's own value, and under `:secret`
            // that value is the secret. The error is left in the clear (it is
            // a failure to report), so it must not carry what it names: the
            // second resolve of `k:spam:1:60:secret hunter2` in one process
            // printed `SPAM_ERR: 'hunter2' exceeded …`.
            let shown = if mb & mk::SECRET != 0 { "[SECRET]" } else { target.as_str() };
            put_at(root, at, slot,
                Value::String(format!(
                    "SPAM_ERR: '{}' exceeded {} calls per {}s",
                    shown, max_calls, window_sec
                )),
            );
            return;
        }

        if let Some(resolved) = value_at(root, at, slot)
            .and_then(|v| {
                let t = value_to_string(v);
                let root_ref = &*root;
                deep_ref(root_ref, &t)
                    .or_else(|| object_at(root, at).get(t.as_str()))
                    .map(|found| budgeted_copy(found, depth))
            })
        {
            put_at(root, at, slot, resolved);
        }
    }

    // ── :include / :import ──
    if (mb & mk::INCLUDE != 0) || (mb & mk::IMPORT != 0) {
        if let Some(Value::String(file_path)) = value_at(root, at, slot) {
            let max_depth = options.max_include_depth.unwrap_or(DEFAULT_MAX_INCLUDE_DEPTH);
            if options._include_depth >= max_depth {
                put_at(root, at, slot,
                    Value::String(format!("INCLUDE_ERR: max include depth ({}) exceeded", max_depth)),
                );
                return;
            }
            if path.is_empty() {
                if let Some(Value::Array(pre)) = preresolved.get(key) {
                    if let [Value::String(pre_path), included] = pre.as_slice() {
                        if pre_path == file_path {
                            let placed = placed_document(included.clone(), depth, mb & mk::SECRET != 0);
                            put_at(root, at, slot, placed);
                            return;
                        }
                    }
                }
            }
            let base = options
                .base_path
                .as_deref()
                .unwrap_or(".");
            let full = match jail_path(base, file_path) {
                Ok(p) => p,
                Err(e) => {
                    put_at(root, at, slot, Value::String(format!("INCLUDE_ERR: {}", e)));
                    return;
                }
            };
            if let Err(e) = check_file_size(&full) {
                put_at(root, at, slot, Value::String(format!("INCLUDE_ERR: {}", e)));
                return;
            }
            let _open = match include_enter(&full, file_path) {
                Ok(g) => g,
                Err(e) => {
                    put_at(root, at, slot, Value::String(format!("INCLUDE_ERR: {}", e)));
                    return;
                }
            };
            match read_budgeted(&full, file_path) {
                Ok(text) => {
                    let mut included = parser::parse(&text);
                    if included.mode == Mode::Active {
                        let mut child_opts = options.clone();
                        child_opts._include_depth += 1;
                        if let Some(parent) = full.parent() {
                            child_opts.base_path = Some(parent.to_string_lossy().into_owned());
                        }
                        resolve(&mut included, &child_opts);
                    }
                    let placed = placed_document(included.root, depth, mb & mk::SECRET != 0);
                    put_at(root, at, slot, placed);
                }
                Err(e) => {
                    put_at(root, at, slot, Value::String(format!("INCLUDE_ERR: {}", e)));
                }
            }
        }
        // safety: the chain ends here, so `:secret` is applied here — a
        // value that was not a path (`k:secret:include 5`) left in the clear.
        if mb & mk::SECRET != 0 {
            if let Some(v) = value_at_mut(root, at, slot) {
                mask_secret_value(v);
            }
        }
        return;
    }

    // ── :env ──
    if mb & mk::ENV != 0 {
        if let Some(Value::String(var_name)) = value_at(root, at, slot) {
            // perf: the variable is read where it lies. A copy was made of
            // it first — and thrown away whenever the text turned out to be
            // a number or a bool, as a port or a flag does.
            let from_process;
            let env_val: Option<&str> = if let Some(ref env_map) = options.env {
                env_map.get(var_name.as_str()).map(String::as_str)
            } else {
                from_process = std::env::var(var_name).ok();
                from_process.as_deref()
            };

            let force_string = wants_string(meta);
            let secret = mb & mk::SECRET != 0;
            let resolved = if let Some(val) = env_val.filter(|v| !v.is_empty()) {
                external_value(val, force_string, secret)
            } else {
                default_argument(markers).map_or(Value::Null, |fallback| external_value(&fallback, force_string, secret))
            };
            put_at(root, at, slot, resolved);
        }
    }

    // ── :random ──
    if mb & mk::RANDOM != 0 {
        if let Some(Value::Array(arr)) = value_at(root, at, slot) {
            if arr.is_empty() {
                put_at(root, at, slot, Value::Null);
                return;
            }
            let picked = if !meta.args.is_empty() {
                let weights: Vec<f64> = meta.args.iter().filter_map(|s| s.parse().ok()).collect();
                weighted_random(arr, &weights)
            } else {
                arr[rng::random_usize(arr.len())].clone()
            };
            put_at(root, at, slot, picked);
        }
    }

    // ── :ref ──
    // Like :alias but feeds the resolved value into subsequent markers.
    // Supports :ref:calc shorthand: key:ref:calc:*2 base_rate → resolves base_rate, then applies "VALUE * 2".
    if mb & mk::REF != 0 {
        if let Some(Value::String(target)) = value_at(root, at, slot) {
            let root_ref = &*root;
            let resolved = deep_ref(root_ref, target)
                .or_else(|| object_at(root, at).get(target.as_str()))
                .map(|found| budgeted_copy(found, depth))
                .unwrap_or(Value::Null);

            // If :calc follows with a shorthand expression
            if mb & mk::CALC != 0 {
                if let Some(n) = value_as_number(&resolved) {
                    let calc_idx = markers.iter().position(|m| m == "calc").unwrap();
                    if let Some(calc_expr) = markers.get(calc_idx + 1) {
                        let first = calc_expr.chars().next().unwrap_or(' ');
                        if "+-*/%".contains(first) {
                            let expr = format!("{} {}", format_number(n), calc_expr);
                            match safe_calc(&expr) {
                                Ok(result) => {
                                    let v = if result.fract() == 0.0 && result.abs() < i64::MAX as f64 {
                                        Value::Int(result as i64)
                                    } else {
                                        Value::Float(result)
                                    };
                                    put_at(root, at, slot, v);
                                }
                                Err(e) => {
                                    put_at(root, at, slot, Value::String(format!("CALC_ERR: {}", e)));
                                }
                            }
                        } else {
                            put_at(root, at, slot, resolved);
                        }
                    } else {
                        put_at(root, at, slot, resolved);
                    }
                } else {
                    put_at(root, at, slot, resolved);
                }
            } else {
                put_at(root, at, slot, resolved);
            }
        }
    }

    // ── :i18n ──
    // Selects a localized value from a nested object based on options.lang.
    // Supports pluralization: key:i18n:COUNT_FIELD
    //   When count field is specified, the language entry must contain plural forms:
    //   title:i18n:item_count
    //     en
    //       one {count} item
    //       other {count} items
    //     ru
    //       one {count} предмет
    //       few {count} предмета
    //       many {count} предметов
    //       other {count} предметов
    if mb & mk::I18N != 0 {
        if let Some(Value::Object(translations)) = value_at(root, at, slot) {
            let lang = options.lang.as_deref().unwrap_or("en");
            let val = translations.get(lang)
                .or_else(|| translations.get("en"))
                .or_else(|| translations.values().next())
                .cloned()
                .unwrap_or(Value::Null);

            // Check for pluralization: i18n:count_field
            let i18n_idx = markers.iter().position(|m| m == "i18n").unwrap();
            let count_field = markers.get(i18n_idx + 1).cloned();

            if let (Some(ref cf), Value::Object(ref plural_forms)) = (&count_field, &val) {
                // Look up count value from current map or root
                let count_val = object_at(root, at).get(cf)
                    .and_then(value_as_number)
                    .or_else(|| {
                        let root_ref = &*root;
                        deep_ref(root_ref, cf).and_then(value_as_number)
                    })
                    .unwrap_or(0.0) as i64;

                let category = plural_category(lang, count_val);
                let chosen = plural_forms.get(category)
                    .or_else(|| plural_forms.get("other"))
                    .or_else(|| plural_forms.values().next())
                    .cloned()
                    .unwrap_or(Value::Null);

                // Substitute {count} in the result string
                if let Value::String(ref s) = chosen {
                    let replaced = s.replace("{count}", &count_val.to_string());
                    put_at(root, at, slot, Value::String(replaced));
                } else {
                    put_at(root, at, slot, chosen);
                }
            } else {
                put_at(root, at, slot, val);
            }
        }
    }

    // ── :calc ──
    if mb & mk::CALC != 0 {
        if let Some(Value::String(expr)) = value_at(root, at, slot) {
            if expr.len() > MAX_CALC_EXPR_LEN {
                put_at(root, at, slot,
                    Value::String(format!("CALC_ERR: expression too long ({} chars, max {})", expr.len(), MAX_CALC_EXPR_LEN)),
                );
                return;
            }
            let root_ref = &*root;
            let root_map = match root_ref {
                Value::Object(m) => Some(m),
                _ => None,
            };
            let mut resolved = match substitute_calc_vars(expr, object_at(root, at), key, root_map, calc_scope, local_calc) {
                Some(r) => r,
                None => {
                    put_at(root, at, slot,
                        Value::String(format!(
                            "CALC_ERR: resolved expression too long (max {} bytes)",
                            MAX_CALC_RESOLVED_LEN
                        )),
                    );
                    return;
                }
            };

            // Substitute dot-path references (e.g., base.hp, server.port)
            let root_ref2 = &*root;
            let mut dot_resolved = String::new();
            let bytes = resolved.as_bytes();
            let len = bytes.len();
            let mut i = 0;
            while i < len {
                if is_word_char(bytes[i]) {
                    let start = i;
                    let mut has_dot = false;
                    while i < len && (is_word_char(bytes[i]) || bytes[i] == b'.') {
                        if bytes[i] == b'.' { has_dot = true; }
                        i += 1;
                    }
                    let token = &resolved[start..i];
                    if has_dot && token.contains('.') {
                        if let Some(val) = deep_ref(root_ref2, token) {
                            if let Some(n) = value_as_number(val) {
                                dot_resolved.push_str(&format_number(n));
                                if dot_resolved.len() > MAX_CALC_RESOLVED_LEN {
                                    put_at(root, at, slot,
                                        Value::String(format!(
                                            "CALC_ERR: resolved expression too long (max {} bytes)",
                                            MAX_CALC_RESOLVED_LEN
                                        )),
                                    );
                                    return;
                                }
                                continue;
                            }
                        }
                    }
                    dot_resolved.push_str(token);
                    if dot_resolved.len() > MAX_CALC_RESOLVED_LEN {
                        put_at(root, at, slot,
                            Value::String(format!(
                                "CALC_ERR: resolved expression too long (max {} bytes)",
                                MAX_CALC_RESOLVED_LEN
                            )),
                        );
                        return;
                    }
                } else {
                    // A whole character: `bytes[i] as char` read each UTF-8
                    // byte as Latin-1, so `цена` came out as mojibake.
                    let ch = resolved[i..].chars().next().unwrap_or('\u{fffd}');
                    dot_resolved.push(ch);
                    i += ch.len_utf8();
                    if dot_resolved.len() > MAX_CALC_RESOLVED_LEN {
                        put_at(root, at, slot,
                            Value::String(format!(
                                "CALC_ERR: resolved expression too long (max {} bytes)",
                                MAX_CALC_RESOLVED_LEN
                            )),
                        );
                        return;
                    }
                }
            }
            resolved = dot_resolved;

            match safe_calc(&resolved) {
                Ok(result) => {
                    let v = if result.fract() == 0.0 && result.abs() < i64::MAX as f64 {
                        Value::Int(result as i64)
                    } else {
                        Value::Float(result)
                    };
                    put_at(root, at, slot, v);
                }
                Err(e) => {
                    put_at(root, at, slot,
                        Value::String(format!("CALC_ERR: {}", e)),
                    );
                }
            }
        }
    }

    // ── :alias ──
    if mb & mk::ALIAS != 0 {
        if let Some(Value::String(target)) = value_at(root, at, slot) {
            let target = target.clone();
            // Build the full dot-path of the current key. A list item's
            // `#<index>` segment is a metadata detail, not part of the path a
            // document can name, so it is left out (as in TypeScript).
            let mut current_path = String::with_capacity(path.len() + key.len() + 1);
            for seg in meta_path_segments(path).iter().filter(|seg| !seg.starts_with('#')) {
                current_path.push_str(seg);
                current_path.push('.');
            }
            current_path.push_str(key);
            // Detect direct self-reference: key:alias key
            if target == key || target == current_path {
                put_at(root, at, slot,
                    Value::String(format!("ALIAS_ERR: self-referential alias: {} → {}", current_path, target)),
                );
            } else {
                // Detect one-hop cycle: a → b → a
                // Only flag as cycle if the target key ALSO has an :alias marker.
                // Without this check, plain string values that happen to match the current
                // key name would produce false-positive ALIAS_ERR results.
                let root_ref = &*root;
                let target_val = deep_ref(root_ref, &target);
                // Determine the metadata path of the target key
                let (target_parent, target_key_name) = if let Some(dot) = target.rfind('.') {
                    (target[..dot].to_string(), target[dot + 1..].to_string())
                } else {
                    (String::new(), target.clone())
                };
                let target_has_alias = metadata
                    .get(&target_parent)
                    .and_then(|mm| mm.get(&target_key_name))
                    .map(|m| has_marker(&m.markers, "alias"))
                    .unwrap_or(false);
                let is_cycle = target_has_alias && match &target_val {
                    Some(Value::String(s)) => s == key || s == &current_path,
                    _ => false,
                };
                if is_cycle {
                    // Stable, order-independent message: sort the participants
                    // lexicographically so both keys produce the same string
                    // regardless of HashMap iteration order.
                    let (a, b) = if current_path <= target {
                        (current_path.as_str(), target.as_str())
                    } else {
                        (target.as_str(), current_path.as_str())
                    };
                    put_at(root, at, slot,
                        Value::String(format!("ALIAS_ERR: circular alias detected: {} → {}", a, b)),
                    );
                } else {
                    let val = target_val.map(|found| budgeted_copy(found, depth)).unwrap_or(Value::Null);
                    put_at(root, at, slot, val);
                }
            }
        }
    }

    // ── :unique ──
    if mb & mk::UNIQUE != 0 {
        if let Some(Value::Array(arr)) = value_at(root, at, slot) {
            // A HashSet membership test keeps this linear; the previous
            // `Vec::contains` made de-duping an N-item list O(N²).
            let mut seen = std::collections::HashSet::new();
            let mut unique = Vec::new();
            for item in arr {
                let s = value_to_string(item);
                if seen.insert(s) {
                    unique.push(item.clone());
                }
            }
            put_at(root, at, slot, Value::Array(unique));
        }
    }

    // ── :geo ──
    if mb & mk::GEO != 0 {
        if let Some(Value::Array(arr)) = value_at(root, at, slot) {
            let region = options.region.as_deref().unwrap_or("US");
            let prefix = format!("{} ", region);
            let found = arr.iter().find(|item| {
                if let Value::String(s) = item {
                    s.starts_with(&prefix)
                } else {
                    false
                }
            });

            let result = if let Some(Value::String(s)) = found {
                Value::String(s[prefix.len()..].trim().to_string())
            } else if let Some(first) = arr.first() {
                if let Value::String(s) = first {
                    if let Some(space) = s.find(' ') {
                        Value::String(s[space + 1..].trim().to_string())
                    } else {
                        first.clone()
                    }
                } else {
                    first.clone()
                }
            } else {
                Value::Null
            };
            put_at(root, at, slot, result);
        }
    }

    // ── :template (legacy — handled by auto-{} in resolve_value) ──

    // ── :split ──
    if mb & mk::SPLIT != 0 {
        if let Some(Value::String(s)) = value_at(root, at, slot) {
            let split_idx = markers.iter().position(|m| m == "split").unwrap();
            let sep = if split_idx + 1 < markers.len() {
                delimiter_from_keyword(&markers[split_idx + 1])
            } else {
                ",".to_string()
            };
            let items: Vec<Value> = s
                .split(&sep)
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .map(|p| cast_primitive(p))
                .collect();
            put_at(root, at, slot, Value::Array(items));
        }
    }

    // ── :join ──
    if mb & mk::JOIN != 0 {
        if let Some(Value::Array(arr)) = value_at(root, at, slot) {
            let join_idx = markers.iter().position(|m| m == "join").unwrap();
            let sep = if join_idx + 1 < markers.len() {
                delimiter_from_keyword(&markers[join_idx + 1])
            } else {
                ",".to_string()
            };
            // safety: the separator is as long as the document likes and is
            // written once per item.
            let separators = sep.len().saturating_mul(arr.len().saturating_sub(1));
            let joined = if separators > arr.len() && !charge_expansion(separators) {
                LIMIT_ERR.to_string()
            } else {
                let text: usize = arr.iter().map(|v| if let Value::String(s) = v { s.len() } else { 24 }).sum();
                let mut joined = String::with_capacity(separators.saturating_add(text));
                for (i, item) in arr.iter().enumerate() {
                    if i > 0 {
                        joined.push_str(&sep);
                    }
                    joined.push_str(&value_text(item));
                }
                joined
            };
            put_at(root, at, slot, Value::String(joined));
        }
    }

    // ── :default (standalone, without :env) ──
    if (mb & mk::DEFAULT != 0) && !(mb & mk::ENV != 0) {
        let is_empty = match value_at(root, at, slot) {
            Some(Value::Null) | None => true,
            Some(Value::String(s)) if s.is_empty() => true,
            _ => false,
        };
        if is_empty {
            if let Some(fallback) = default_argument(markers) {
                put_at(root, at, slot, external_value(&fallback, wants_string(meta), mb & mk::SECRET != 0));
            }
        }
    }

    // ── :clamp ──
    // Syntax: key:clamp:MIN:MAX value
    // Clamps a numeric value to [MIN, MAX].
    if mb & mk::CLAMP != 0 {
        let clamp_idx = markers.iter().position(|m| m == "clamp").unwrap();
        let min_s = markers.get(clamp_idx + 1).cloned().unwrap_or_default();
        let max_s = markers.get(clamp_idx + 2).cloned().unwrap_or_default();
        // safety: a bound must be a finite number. `f64::clamp` panics on a
        // NaN bound, so `:clamp:nan:5` crashed the process; `inf` and `nan`
        // are not SYNX numbers, and TypeScript skipped them.
        let bound = |s: &str| s.parse::<f64>().ok().filter(|v| v.is_finite());
        if let (Some(lo), Some(hi)) = (bound(&min_s), bound(&max_s)) {
            if lo > hi {
                put_at(root, at, slot, Value::String(
                    format!("CONSTRAINT_ERR: clamp min ({}) > max ({})", lo, hi),
                ));
            } else if let Some(n) = value_at(root, at, slot).and_then(value_as_number) {
                let clamped = n.clamp(lo, hi);
                let v = if clamped.fract() == 0.0 && clamped.abs() < i64::MAX as f64 {
                    Value::Int(clamped as i64)
                } else {
                    Value::Float(clamped)
                };
                put_at(root, at, slot, v);
            }
        }
    }

    // ── :round ──
    // Syntax: key:round:N value  (N = decimal places, default 0)
    // Works standalone or after :calc: key:calc:round:2 expr
    if mb & mk::ROUND != 0 {
        let round_idx = markers.iter().position(|m| m == "round").unwrap();
        let decimals: u32 = markers.get(round_idx + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if let Some(n) = value_at(root, at, slot).and_then(value_as_number) {
            // A scale past the range of f64 rounds nothing: the number stays.
            let factor = 10f64.powi(decimals.min(400) as i32);
            let scaled = n * factor;
            let rounded = if scaled.is_finite() { scaled.round() / factor } else { n };
            // safety: only a whole number that fits is an integer. The cast
            // alone saturates: `:round` of 1e30 was 9223372036854775807 and
            // of NaN was 0 — invented values, where `:calc` and `:clamp`
            // keep the float.
            let v = if decimals == 0 && rounded.fract() == 0.0 && rounded.abs() < i64::MAX as f64 {
                Value::Int(rounded as i64)
            } else {
                Value::Float(rounded)
            };
            put_at(root, at, slot, v);
        }
    }

    // ── :map ──
    // Syntax: key:map:source_key\n  - lookup_val result
    // Looks up `source_key` in root, finds matching "lookup_val result" entry in the array.
    if mb & mk::MAP != 0 {
        if let Some(Value::Array(arr)) = value_at(root, at, slot) {
            let map_idx = markers.iter().position(|m| m == "map").unwrap();
            let source_key = markers.get(map_idx + 1).cloned().unwrap_or_default();
            let lookup_val = if !source_key.is_empty() {
                let root_ref = &*root;
                deep_ref(root_ref, &source_key)
                    .or_else(|| object_at(root, at).get(&source_key))
                    .map(value_to_string)
                    .unwrap_or_default()
            } else {
                // Use the current string value as lookup key
                match value_at(root, at, slot) {
                    Some(Value::String(s)) => s.clone(),
                    _ => String::new(),
                }
            };

            // Find matching entry: "lookup_val result_text"
            let result = arr.iter().find_map(|item| {
                if let Value::String(s) = item {
                    if let Some(space) = s.find(' ') {
                        if s[..space].trim() == lookup_val {
                            return Some(cast_primitive(s[space + 1..].trim()));
                        }
                    }
                }
                None
            });
            put_at(root, at, slot, result.unwrap_or(Value::Null));
        }
    }

    // ── :format ──
    // Syntax: key:format:PATTERN value  (printf-style: %.2f, %d, %05d, %e)
    // Converts numeric or string value to a formatted string.
    if mb & mk::FORMAT != 0 {
        let fmt_idx = markers.iter().position(|m| m == "format").unwrap();
        let pattern = markers.get(fmt_idx + 1).cloned().unwrap_or_else(|| "%s".to_string());
        if let Some(current) = value_at(root, at, slot) {
            let mut formatted = apply_format_pattern(&pattern, current);
            // safety: a width of 4096 on a twenty-byte line is a 200-fold growth.
            if matches!(current, Value::Int(_) | Value::Float(_)) && !charge_expansion(formatted.len()) {
                formatted = LIMIT_ERR.to_string();
            }
            put_at(root, at, slot, Value::String(formatted));
        }
    }

    // ── :replace:FROM:TO ──    (since 3.6.2)
    // Literal substring replacement on a string value. `TO` defaults to "" (deletion).
    // `FROM`/`TO` cannot contain ':' because the marker chain is colon-delimited;
    // for those cases use `{interpolation}` instead.
    if mb & mk::REPLACE != 0 {
        if let Some(Value::String(s)) = value_at(root, at, slot) {
            let idx = markers.iter().position(|m| m == "replace").unwrap();
            let from = markers.get(idx + 1).cloned().unwrap_or_default();
            let to = markers.get(idx + 2).cloned().unwrap_or_default();
            if !from.is_empty() {
                // safety: `:replace:a:<long>` on a run of `a` multiplies the two
                // lengths; the growth is known before a byte is written.
                let grown = if to.len() > from.len() {
                    s.matches(from.as_str()).count().saturating_mul(to.len() - from.len())
                } else {
                    0
                };
                let replaced = if grown > 0 && !charge_expansion(grown) {
                    LIMIT_ERR.to_string()
                } else if grown > 0 {
                    // perf: `str::replace` grows its buffer by doubling — up
                    // to three times the result in flight.
                    let mut out = String::with_capacity(s.len() + grown);
                    let mut last = 0;
                    for (at, _) in s.match_indices(from.as_str()) {
                        out.push_str(&s[last..at]);
                        out.push_str(&to);
                        last = at + from.len();
                    }
                    out.push_str(&s[last..]);
                    out
                } else {
                    s.replace(&from, &to)
                };
                put_at(root, at, slot, Value::String(replaced));
            }
        }
    }

    // ── :sort  /  :sort:desc ──    (since 3.6.2)
    // Sort an array. Numeric items compare numerically; otherwise lexicographic.
    if mb & mk::SORT != 0 {
        if let Some(Value::Array(arr)) = value_at(root, at, slot) {
            let idx = markers.iter().position(|m| m == "sort").unwrap();
            let desc = matches!(markers.get(idx + 1).map(|s| s.as_str()), Some("desc"));
            let mut sorted = arr.clone();
            // safety: a total order — numbers first, by value, then everything
            // else by its text. Comparing a number with text as text made
            // `10 < 1a < 9 < 10` a cycle, and `sort_by` panics on one: a list
            // of fifty mixed items took the process down.
            sorted.sort_by(|a, b| {
                use std::cmp::Ordering;
                match (value_as_number(a), value_as_number(b)) {
                    (Some(an), Some(bn)) => an
                        .partial_cmp(&bn)
                        .unwrap_or_else(|| an.is_nan().cmp(&bn.is_nan())),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => value_to_string(a).cmp(&value_to_string(b)),
                }
            });
            if desc { sorted.reverse(); }
            put_at(root, at, slot, Value::Array(sorted));
        }
    }

    // ── :sum ──    (since 3.6.2)
    // Sum the numeric items of an array. Non-numeric items are ignored.
    // Returns Int when all summands are integers, Float otherwise.
    if mb & mk::SUM != 0 {
        if let Some(Value::Array(arr)) = value_at(root, at, slot) {
            let mut total: f64 = 0.0;
            let mut all_int = true;
            for v in arr {
                match v {
                    Value::Int(n) => total += *n as f64,
                    Value::Float(f) => {
                        total += *f;
                        if f.fract() != 0.0 { all_int = false; }
                    }
                    Value::String(s) => {
                        // `inf` and `nan` are text here as everywhere (see `cast_primitive`).
                        if let Some(f) = s.parse::<f64>().ok().filter(|f| f.is_finite()) {
                            total += f;
                            if f.fract() != 0.0 { all_int = false; }
                        }
                    }
                    _ => {}
                }
            }
            let result = if all_int && total.fract() == 0.0 && total.abs() < i64::MAX as f64 {
                Value::Int(total as i64)
            } else {
                Value::Float(total)
            };
            put_at(root, at, slot, result);
        }
    }

    // ── :fallback ──
    // Syntax: key:fallback:DEFAULT_PATH value
    // If the value (treated as a file path) doesn't exist on disk, use the fallback.
    // Falls back to default if value is also null/empty.
    if mb & mk::FALLBACK != 0 {
        let fb_idx = markers.iter().position(|m| m == "fallback").unwrap();
        let default_val = markers.get(fb_idx + 1).cloned().unwrap_or_default();
        let use_fallback = match value_at(root, at, slot) {
            None | Some(Value::Null) => true,
            Some(Value::String(s)) if s.is_empty() => true,
            Some(Value::String(s)) => {
                let base = options.base_path.as_deref().unwrap_or(".");
                match jail_path(base, s) {
                    Ok(safe) => !safe.exists(),
                    Err(_) => true, // path escapes jail → treat as missing → use fallback
                }
            }
            _ => false,
        };
        if use_fallback && !default_val.is_empty() {
            put_at(root, at, slot, Value::String(default_val));
        }
    }

    // ── :once ──
    // Syntax: key:once  or  key:once:uuid  or  key:once:random  or  key:once:timestamp
    // Generates a value once and persists it in a .synx.lock sidecar file.
    if mb & mk::ONCE != 0 {
        let once_idx = markers.iter().position(|m| m == "once").unwrap();
        let gen_type = markers.get(once_idx + 1).map(|s| s.as_str()).unwrap_or("uuid");
        let lock_path = options.base_path.as_deref()
            .map(|b| std::path::Path::new(b).join(".synx.lock"))
            .unwrap_or_else(|| std::path::Path::new(".synx.lock").to_path_buf());

        // The value recorded in the lock file, or a new one recorded there.
        let value = locked_value(&lock_path, key, || match gen_type {
            "uuid" => rng::generate_uuid(),
            "timestamp" => std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .to_string(),
            "random" => rng::random_usize(u32::MAX as usize).to_string(),
            _ => rng::generate_uuid(),
        });
        put_at(root, at, slot, Value::String(value));
    }

    // ── :version ──
    // Syntax: key:version:OP:REQUIRED value
    // Compares the value (current version) against REQUIRED using OP (>=, <=, >, <, ==, !=).
    // Returns a bool.
    if mb & mk::VERSION != 0 {
        if let Some(Value::String(current_ver)) = value_at(root, at, slot) {
            let ver_idx = markers.iter().position(|m| m == "version").unwrap();
            let op = markers.get(ver_idx + 1).map(|s| s.as_str()).unwrap_or(">=");
            let required = markers.get(ver_idx + 2).cloned().unwrap_or_default();
            let result = compare_versions(current_ver, op, &required);
            put_at(root, at, slot, Value::Bool(result));
        }
    }

    // ── :watch ──
    // Syntax: key:watch:KEY_PATH ./file.json  (or ./file.synx)
    // Reads the referenced file at parse time. Optionally extracts a key path (JSON/SYNX).
    if mb & mk::WATCH != 0 {
        if let Some(Value::String(file_path)) = value_at(root, at, slot) {
            let max_depth = options.max_include_depth.unwrap_or(DEFAULT_MAX_INCLUDE_DEPTH);
            if options._include_depth >= max_depth {
                put_at(root, at, slot,
                    Value::String(format!("WATCH_ERR: max include depth ({}) exceeded", max_depth)),
                );
                return;
            }
            let base = options.base_path.as_deref().unwrap_or(".");
            let full = match jail_path(base, file_path) {
                Ok(p) => p,
                Err(e) => {
                    put_at(root, at, slot, Value::String(format!("WATCH_ERR: {}", e)));
                    return;
                }
            };
            if let Err(e) = check_file_size(&full) {
                put_at(root, at, slot, Value::String(format!("WATCH_ERR: {}", e)));
                return;
            }
            let watch_idx = markers.iter().position(|m| m == "watch").unwrap();
            let key_path = markers.get(watch_idx + 1).cloned();

            match read_budgeted(&full, file_path) {
                Ok(content) => {
                    let value = if let Some(ref kp) = key_path {
                        extract_from_file_content(&content, kp, full.extension().and_then(|e| e.to_str()).unwrap_or("")).unwrap_or(Value::Null)
                    } else {
                        Value::String(content.trim().to_string())
                    };
                    put_at(root, at, slot, placed_document(value, depth, false));
                }
                Err(e) => {
                    put_at(root, at, slot, Value::String(format!("WATCH_ERR: {}", e)));
                }
            }
        }
    }

    // ── :secret ──
    // Last of the markers that produce the value, so whatever they resolved
    // is what gets masked. It used to sit after `:alias`: `t:secret:once`
    // came out as plain text, `:secret:default:x` and `:secret:watch` masked
    // the argument instead of the result, and `:secret:format` formatted the
    // mask. `:prompt` and package markers below still see only the mask.
    if mb & mk::SECRET != 0 {
        if let Some(v) = value_at_mut(root, at, slot) {
            mask_secret_value(v);
        }
    }

    // ── :prompt ──
    // Syntax: key:prompt:LABEL subtree
    // Converts the resolved subtree (object) into a SYNX-formatted string
    // wrapped in a labeled code fence, ready for LLM prompt embedding.
    if mb & mk::PROMPT != 0 {
        let prompt_idx = markers.iter().position(|m| m == "prompt").unwrap();
        let label = markers.get(prompt_idx + 1).cloned().unwrap_or_else(|| key.to_string());
        if let Some(val) = value_at(root, at, slot) {
            let synx_text = stringify_value(val, 0);
            let mut block = format!("{} (SYNX):\n```synx\n{}```", label, synx_text);
            if !charge_expansion(block.len()) {
                block = LIMIT_ERR.to_string();
            }
            put_at(root, at, slot, Value::String(block));
        }
    }

    // ── :vision ──
    // Metadata-only marker. Recognized by the engine (no error), value passes through.
    // Applications detect this marker via metadata to dispatch image generation.

    // ── :audio ──
    // Metadata-only marker. Recognized by the engine (no error), value passes through.
    // Applications detect this marker via metadata to dispatch audio generation.

    // ── WASM custom markers ──
    // If a marker is not built-in and a WASM runtime is loaded, dispatch to it.
    #[cfg(feature = "wasm")]
    if let Some(ref wasm_rt) = options.wasm_runtime {
        for marker in markers {
            if crate::wasm::BUILTIN_MARKERS.contains(&marker.as_str()) {
                continue;
            }
            if wasm_rt.has_marker(marker) {
                // Collect args: all marker parts after this marker name
                let marker_idx = markers.iter().position(|m| m == marker).unwrap();
                let args: Vec<String> = markers[marker_idx + 1..].to_vec();
                let current_value = value_at(root, at, slot).cloned().unwrap_or(Value::Null);
                match wasm_rt.apply_marker(marker, &current_value, &args) {
                    Ok(result) => {
                        put_at(root, at, slot, result);
                    }
                    Err(e) => {
                        put_at(root, at, slot, Value::String(format!("WASM_ERR: {}", e)));
                    }
                }
                break; // Only apply one WASM marker per key
            }
        }
    }

    // ── Constraint validation (always last, after all markers resolved) ──
    if let Some(ref c) = meta.constraints {
        if let Some(v) = value_at_mut(root, at, slot) {
            if let Some(err) = constraint_violation(key, v, c) {
                *v = Value::String(err);
            }
        }
    }
}

// ─── Constraint enforcement ───────────────────────────────

/// The error a value earns against its constraints, or `None` when it passes.
///
/// perf: reads the value in place — the previous version cloned it first.
fn constraint_violation(key: &str, val: &Value, c: &Constraints) -> Option<String> {
    // required
    if c.required {
        // A key written without a value parses as an empty group; for
        // `[required]` that is the missing value, not a present object.
        let empty = matches!(val, Value::Null)
            || matches!(val, Value::String(s) if s.is_empty())
            || matches!(val, Value::Object(m) if m.is_empty());
        if empty {
            return Some(format!("CONSTRAINT_ERR: '{}' is required", key));
        }
    }

    // spec: an absent value (null) is checked against `enum` only, read as an
    // empty string; type, range and pattern apply to values that exist. An
    // optional `port[type:int]:env PORT` with PORT unset is null, not an error
    // — the behaviour the TypeScript engine documents and implements.
    if matches!(val, Value::Null) {
        if let Some(ref enum_vals) = c.enum_values {
            if !enum_vals.iter().any(|e| e.is_empty()) {
                return Some(format!("CONSTRAINT_ERR: '{}' must be one of [{}]", key, enum_vals.join("|")));
            }
        }
        return None;
    }

    // type check
    if let Some(ref type_name) = c.type_name {
        let ok = match type_name.as_str() {
            "int"    => matches!(val, Value::Int(_)),
            "float"  => matches!(val, Value::Float(_) | Value::Int(_)),
            "bool"   => matches!(val, Value::Bool(_)),
            // A `:secret` is a masked string: `[type:string]:secret:env` must accept it.
            "string" => matches!(val, Value::String(_) | Value::Secret(_)),
            _        => true,
        };
        if !ok {
            return Some(format!("CONSTRAINT_ERR: '{}' expected type '{}'", key, type_name));
        }
    }

    // enum check
    if let Some(ref enum_vals) = c.enum_values {
        let owned;
        let val_str: &str = match val {
            // A secret is checked by its text; no message quotes the value.
            Value::String(s) | Value::Secret(s) => s,
            Value::Int(n)    => { owned = n.to_string(); &owned }
            Value::Float(f)  => { owned = f.to_string(); &owned }
            Value::Bool(b)   => if *b { "true" } else { "false" },
            _                => "",
        };
        if !enum_vals.iter().any(|e| e == val_str) {
            return Some(format!("CONSTRAINT_ERR: '{}' must be one of [{}]", key, enum_vals.join("|")));
        }
    }

    // min / max  (numbers: value range; strings: length range).
    // A string's length is its count of Unicode scalar values: until 3.7.4 it
    // was UTF-8 bytes, so `[max:4]` rejected the three-letter `мир` (6 bytes)
    // while TypeScript accepted it.
    let num = match val {
        Value::Int(n)    => Some(*n as f64),
        Value::Float(f)  => Some(*f),
        Value::String(s) | Value::Secret(s) if c.min.is_some() || c.max.is_some() => Some(s.chars().count() as f64),
        _                => None,
    };
    if let Some(n) = num {
        if let Some(min) = c.min {
            if n < min {
                return Some(format!("CONSTRAINT_ERR: '{}' value {} is below min {}", key, n, min));
            }
        }
        if let Some(max) = c.max {
            if n > max {
                return Some(format!("CONSTRAINT_ERR: '{}' value {} exceeds max {}", key, n, max));
            }
        }
    }

    // pattern (regex match — bounded length to avoid pathological compilation costs)
    if let Some(ref pat) = c.pattern {
        if pat.len() <= 256 {
            // Invalid and backtracking-prone patterns are skipped silently, as in
            // every other implementation.
            if let Value::String(s) | Value::Secret(s) = val {
                if pattern_rejects(pat, s) {
                    return Some(format!("CONSTRAINT_ERR: '{}' does not match pattern /{}/", key, pat));
                }
            }
        }
    }
    None
}

/// Whether `s` fails `[pattern:…]`. Invalid and backtracking-prone patterns
/// never reject.
///
/// perf: compiled patterns are cached per thread — a pattern declared on a
/// field name was recompiled for every value checked against it.
pub(crate) fn pattern_rejects(pat: &str, s: &str) -> bool {
    use std::cell::RefCell;
    thread_local! {
        static PATTERNS: RefCell<HashMap<String, Option<regex::Regex>>> = RefCell::new(HashMap::new());
    }
    // safety: bounded — a document full of distinct patterns cannot grow it forever.
    const MAX_CACHED_PATTERNS: usize = 256;
    PATTERNS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.contains_key(pat) {
            if cache.len() >= MAX_CACHED_PATTERNS {
                cache.clear();
            }
            let compiled = if has_nested_quantifier(pat) { None } else { regex::Regex::new(pat).ok() };
            cache.insert(pat.to_string(), compiled);
        }
        match cache.get(pat) {
            Some(Some(re)) => !re.is_match(s),
            _ => false,
        }
    })
}

/// Does the pattern repeat something that already repeats — the `(a+)+`
/// shape behind exponential backtracking?
///
/// The `regex` crate is linear and would run such a pattern safely, but every
/// backtracking implementation (TypeScript, Java, .NET, Dart, Swift, C++)
/// must refuse it, so the reference refuses it too: a `[pattern]` constraint
/// with this shape is skipped everywhere, and the projection stays identical.
/// Mirrors `hasNestedQuantifier` in `packages/synx-js/src/synxl.ts`.
pub(crate) fn has_nested_quantifier(pattern: &str) -> bool {
    let chars: Vec<char> = pattern.chars().collect();
    let mut opens: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '[' => {
                // A character class is skipped wholesale; quantifiers in it are literal.
                while i < chars.len() && chars[i] != ']' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            '(' => opens.push(i),
            ')' => {
                if let Some(start) = opens.pop() {
                    if matches!(chars.get(i + 1), Some('*' | '+' | '{')) {
                        let mut body = String::new();
                        let mut j = start + 1;
                        while j < i {
                            if chars[j] == '\\' {
                                j += 2;
                                continue;
                            }
                            body.push(chars[j]);
                            j += 1;
                        }
                        let bytes = body.as_bytes();
                        let quantified = bytes.iter().enumerate().any(|(k, &b)| {
                            b == b'*' || b == b'+' || (b == b'{' && bytes.get(k + 1).is_some_and(u8::is_ascii_digit))
                        });
                        if quantified {
                            return true;
                        }
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

// ─── New-marker helpers ───────────────────────────────────

/// Apply a printf-style format pattern to a value.
fn apply_format_pattern(pattern: &str, value: &Value) -> String {
    match value {
        Value::Int(n) => {
            if pattern.contains('d') || pattern.contains('i') {
                format_int_pattern(pattern, *n)
            } else if pattern.contains('f') || pattern.contains('e') {
                format_float_pattern(pattern, *n as f64)
            } else {
                n.to_string()
            }
        }
        Value::Float(f) => {
            if pattern.contains('f') || pattern.contains('e') {
                format_float_pattern(pattern, *f)
            } else if (pattern.contains('d') || pattern.contains('i'))
                && f.fract() == 0.0
                && f.abs() < 9.0e15
            {
                // spec: a whole float takes an integer pattern — `12.0` under
                // `%05d` is `00012`, as in TypeScript, where it is one number.
                format_int_pattern(pattern, *f as i64)
            } else {
                format_number(*f)
            }
        }
        Value::String(s) => s.clone(),
        other => value_to_string(other),
    }
}

fn format_int_pattern(pattern: &str, n: i64) -> String {
    // Guardrail: user-controlled width can be enormous (esp. under fuzzing).
    // Large widths can cause pathological allocations or panics in formatting internals.
    const MAX_FMT_WIDTH: usize = 4096;
    if let Some(s) = pattern.strip_prefix('%') {
        if let Some(inner) = s.strip_suffix('d').or_else(|| s.strip_suffix('i')) {
            if let Some(w) = inner.strip_prefix('0') {
                if let Ok(width) = w.parse::<usize>() {
                    let width = width.min(MAX_FMT_WIDTH);
                    return format!("{:0>width$}", n, width = width);
                }
            }
            if let Ok(width) = inner.parse::<usize>() {
                let width = width.min(MAX_FMT_WIDTH);
                return format!("{:>width$}", n, width = width);
            }
        }
    }
    n.to_string()
}

fn format_float_pattern(pattern: &str, f: f64) -> String {
    // Same rationale as MAX_FMT_WIDTH: avoid pathological precision values.
    const MAX_FMT_PREC: usize = 1024;
    if let Some(s) = pattern.strip_prefix('%') {
        // %e — exponential form, matches the JS `Number.toExponential()` shape:
        //   e.g. 123456.789 → "1.23456789e+5"
        if s == "e" {
            if f == 0.0 {
                return "0e+0".to_string();
            }
            // Use Rust's exponential formatter (which uses ryu-like shortest
            // round-trip digits) and then reshape the exponent token to match
            // JS `toExponential()` (always signed, no leading zeros).
            let raw = format!("{:e}", f); // e.g. "1.23456789e5" or "1.23456789e-5"
            if let Some(epos) = raw.rfind('e') {
                let mantissa = &raw[..epos];
                let exp_part = &raw[epos + 1..];
                let (sign, digits) = match exp_part.chars().next() {
                    Some('+') => ('+', &exp_part[1..]),
                    Some('-') => ('-', &exp_part[1..]),
                    _ => ('+', exp_part),
                };
                // Strip the decimal point if the mantissa is an integer (1.0 → 1)
                let mantissa_clean = if mantissa.ends_with(".0") {
                    &mantissa[..mantissa.len() - 2]
                } else {
                    mantissa
                };
                return format!("{}e{}{}", mantissa_clean, sign, digits);
            }
            return raw;
        }
        if let Some(inner) = s.strip_suffix('f').or_else(|| s.strip_suffix('e')) {
            if let Some(prec_s) = inner.strip_prefix('.') {
                if let Ok(prec) = prec_s.parse::<usize>() {
                    let prec = prec.min(MAX_FMT_PREC);
                    if s.ends_with('e') {
                        return format!("{:.prec$e}", f, prec = prec);
                    }
                    return format!("{:.prec$}", f, prec = prec);
                }
            }
        }
    }
    f.to_string()
}

/// The value `.synx.lock` records for `key`, or `generate()` recorded there.
///
/// perf: the file is read once per resolve and written once at its end. Each
/// `:once` key used to read the whole file and, when new, rewrite it: a
/// document with 3 000 such keys spent over 25 s in file I/O.
///
/// The format is unchanged — one `key value` line per entry, the first line
/// for a key wins.
fn locked_value(lock_path: &std::path::Path, key: &str, generate: impl FnOnce() -> String) -> String {
    let found = RESOLVE_STATE.with(|s| {
        let mut s = s.borrow_mut();
        let idx = match s.locks.iter().position(|l| l.path == lock_path) {
            Some(i) => i,
            None => {
                s.locks.push(LockFile::open(lock_path));
                s.locks.len() - 1
            }
        };
        s.locks[idx].values.get(key).cloned()
    });
    if let Some(value) = found {
        return value;
    }
    let value = generate();
    RESOLVE_STATE.with(|s| {
        let mut s = s.borrow_mut();
        let read_only = s.read_only;
        match s.locks.iter_mut().find(|l| l.path == lock_path) {
            Some(lock) => lock.record(key, value, read_only),
            None => value,
        }
    })
}

/// The integer at the start of `s` (after whitespace, with an optional sign),
/// or 0 — JavaScript's `parseInt(s, 10) || 0`.
fn leading_int(s: &str) -> i64 {
    let t = s.trim_start();
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let mut n: i64 = 0;
    for b in digits.bytes().take_while(u8::is_ascii_digit) {
        n = n.saturating_mul(10).saturating_add((b - b'0') as i64);
    }
    if neg { -n } else { n }
}

/// Compare two version strings using a comparison operator.
fn compare_versions(current: &str, op: &str, required: &str) -> bool {
    // spec: each dot-separated part is read for its leading integer, a part
    // with none counting as 0 — positions never shift. Dropping unparsable
    // parts made `1.2.3-beta` compare as `1.2` and `1.x.3` as `1.3`, and
    // disagreed with the TypeScript engine.
    let parse_ver = |s: &str| -> Vec<i64> { s.split('.').map(leading_int).collect() };
    let cv = parse_ver(current);
    let rv = parse_ver(required);
    let len = cv.len().max(rv.len());
    let mut ord = std::cmp::Ordering::Equal;
    for i in 0..len {
        let a = cv.get(i).copied().unwrap_or(0);
        let b = rv.get(i).copied().unwrap_or(0);
        if a != b {
            ord = a.cmp(&b);
            break;
        }
    }
    match op {
        ">=" => ord != std::cmp::Ordering::Less,
        "<=" => ord != std::cmp::Ordering::Greater,
        ">"  => ord == std::cmp::Ordering::Greater,
        "<"  => ord == std::cmp::Ordering::Less,
        "==" | "=" => ord == std::cmp::Ordering::Equal,
        "!=" => ord != std::cmp::Ordering::Equal,
        _ => false,
    }
}

/// Keys the process-wide `:spam` table keeps.
const MAX_SPAM_BUCKETS: usize = 1024;
/// Calls remembered per key; a limit above this is not enforced.
const MAX_SPAM_CALLS_TRACKED: usize = 4096;
/// A longer `key::target` is stored as its hash.
const MAX_SPAM_KEY_BYTES: usize = 256;

fn allow_spam_access(bucket_key: &str, max_calls: usize, window_sec: u64) -> bool {
    let now = Instant::now();
    let window = Duration::from_secs(window_sec.max(1));

    let buckets = SPAM_BUCKETS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = match buckets.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };

    // safety: the table outlives every parse. A long key is kept as its hash,
    // the table holds `MAX_SPAM_BUCKETS` keys (the one idle longest goes
    // first) and each key remembers `MAX_SPAM_CALLS_TRACKED` calls — a host
    // that parses other people's documents no longer grows it forever.
    let short;
    let bucket_key = if bucket_key.len() > MAX_SPAM_KEY_BYTES {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        bucket_key.hash(&mut h);
        short = format!("#{:016x}", h.finish());
        short.as_str()
    } else {
        bucket_key
    };
    if !guard.contains_key(bucket_key) && guard.len() >= MAX_SPAM_BUCKETS {
        let idle = guard
            .iter()
            .min_by_key(|(_, calls)| calls.last().copied())
            .map(|(k, _)| k.clone());
        if let Some(k) = idle {
            guard.remove(&k);
        }
    }

    let calls = guard.entry(bucket_key.to_string()).or_default();
    calls.retain(|ts| now.duration_since(*ts) <= window);

    if calls.len() >= max_calls {
        return false;
    }

    if calls.len() < MAX_SPAM_CALLS_TRACKED {
        calls.push(now);
    }
    true
}

#[cfg(test)]
fn clear_spam_buckets() {
    let buckets = SPAM_BUCKETS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut guard) = buckets.lock() {
        guard.clear();
    }
}

/// Extract a value from file content by key path (JSON dot-path or SYNX key).
fn extract_from_file_content(content: &str, key_path: &str, ext: &str) -> Option<Value> {
    if ext == "json" {
        // Real JSON parse via serde_json — supports dot-path traversal.
        let parsed: serde_json::Value = serde_json::from_str(content).ok()?;
        let mut current = &parsed;
        for part in key_path.split('.') {
            current = current.get(part)?;
        }
        return Some(json_value_to_synx(current));
    }

    // SYNX file: parse it and follow the dot-path through the resulting tree.
    let parsed = crate::parser::parse(content);
    let mut current = &parsed.root;
    for part in key_path.split('.') {
        match current {
            Value::Object(map) => {
                current = map.get(part)?;
            }
            _ => return None,
        }
    }
    Some(current.clone())
}

/// Convert a `serde_json::Value` to `Value` (best-effort).
fn json_value_to_synx(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Null
            }
        }
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            Value::Array(arr.iter().map(json_value_to_synx).collect())
        }
        serde_json::Value::Object(map) => {
            let mut out = Map::default();
            for (k, v) in map {
                out.insert(k.clone(), json_value_to_synx(v));
            }
            Value::Object(out)
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────

/// Serialize a Value to SYNX format string (for :prompt marker).
fn stringify_value(value: &Value, indent: usize) -> String {
    let spaces = " ".repeat(indent);
    match value {
        Value::Object(map) => {
            let mut out = String::new();
            let mut keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
            keys.sort_unstable();
            for key in keys {
                let val = &map[key];
                match val {
                    Value::Object(_) => {
                        out.push_str(&format!("{}{}\n", spaces, key));
                        out.push_str(&stringify_value(val, indent + 2));
                    }
                    Value::Array(arr) => {
                        out.push_str(&format!("{}{}\n", spaces, key));
                        for item in arr {
                            out.push_str(&format!("{}  - {}\n", spaces, value_to_string(item)));
                        }
                    }
                    _ => {
                        out.push_str(&format!("{}{} {}\n", spaces, key, value_to_string(val)));
                    }
                }
            }
            out
        }
        // A list prompts as its items, as a list inside an object does. It
        // rendered as one empty line, while TypeScript listed the items.
        Value::Array(arr) => {
            let mut out = String::new();
            for item in arr {
                out.push_str(&format!("{}  - {}\n", spaces, value_to_string(item)));
            }
            out
        }
        _ => format!("{}{}\n", spaces, value_to_string(value)),
    }
}

/// Whether text taken from outside the document (`:env`, `:default`) must stay
/// a string instead of going through [`cast_primitive`]. `(string)` always
/// asked for that; since 3.7.4 so does `[type:string]` — before, the cast ran
/// first and the constraint then rejected its own declared type, so a numeric
/// id (`1550184245153112174`) or a zero-padded code (`007`) could not be read
/// from the environment as text at all.
fn wants_string(meta: &Meta) -> bool {
    meta.type_hint.as_deref() == Some("string")
        || meta
            .constraints
            .as_ref()
            .and_then(|c| c.type_name.as_deref())
            == Some("string")
}

/// The argument of `:default` in a marker chain: everything after it, with
/// the colons that split it put back — `:default:0.0.0.0`, `:default:a:b`.
/// `None` when `default` is the last marker or is not there.
///
/// perf: one segment — a port, a flag, a name — is borrowed. Every default
/// used to be joined into a new string, taken or not.
fn default_argument(markers: &[String]) -> Option<std::borrow::Cow<'_, str>> {
    let at = markers.iter().position(|m| m == "default")?;
    match &markers[at + 1..] {
        [] => None,
        [one] => Some(std::borrow::Cow::Borrowed(one.as_str())),
        parts => Some(std::borrow::Cow::Owned(parts.join(":"))),
    }
}

/// A value that came in as text from outside the document: a variable of the
/// environment or the argument of `:default`.
fn external_value(text: &str, as_string: bool, secret: bool) -> Value {
    if as_string {
        return Value::String(text.to_string());
    }
    match cast_primitive(text) {
        // spec: a secret is its text. Cast first, `00123` became the secret
        // `123`, `1e5` the secret `100000` and `+5` the secret `5` — a
        // different key from the one in the environment.
        Value::Int(_) | Value::Float(_) | Value::Bool(_) if secret => Value::String(text.to_string()),
        cast => cast,
    }
}

pub(crate) fn cast_primitive(val: &str) -> Value {
    // Quoted strings preserve literal value
    if val.len() >= 2 {
        let bytes = val.as_bytes();
        if (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
        {
            return Value::String(val[1..val.len() - 1].to_string());
        }
    }
    match val {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "null" => Value::Null,
        _ => {
            if let Ok(i) = val.parse::<i64>() {
                Value::Int(i)
            } else if let Some(f) = val.parse::<f64>().ok().filter(|f| f.is_finite()) {
                // spec: `inf`, `NaN` and `1e400` are text. Rust reads them as
                // floats, which JSON then writes as `null`: a host named
                // `inf` or a `Nan, Ina` list lost its value, and TypeScript,
                // which keeps the text, disagreed.
                Value::Float(f)
            } else {
                Value::String(val.to_string())
            }
        }
    }
}

fn delimiter_from_keyword(keyword: &str) -> String {
    match keyword {
        "space" => " ".to_string(),
        "pipe" => "|".to_string(),
        "dash" => "-".to_string(),
        "dot" => ".".to_string(),
        "semi" => ";".to_string(),
        "tab" => "\t".to_string(),
        "slash" => "/".to_string(),
        other => other.to_string(),
    }
}

fn value_as_number(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

/// [`value_to_string`] without the copy when the value is text already.
fn value_text(v: &Value) -> std::borrow::Cow<'_, str> {
    match v {
        Value::String(s) => std::borrow::Cow::Borrowed(s),
        other => std::borrow::Cow::Owned(value_to_string(other)),
    }
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => format_number(*f as f64),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        // safety: a `:secret` value MUST NOT reach a string sink as plaintext.
        // value_to_string feeds interpolation (`{key}`), `:join`, `:prompt`,
        // `:map`, spam bucket keys — every one of which would otherwise leak the
        // secret. The masked form matches `write_json` (`"[SECRET]"`); callers
        // that legitimately need the raw value use the typed `Value::Secret` API.
        Value::Secret(_) => "[SECRET]".to_string(),
        Value::Array(_) | Value::Object(_) => String::new(),
    }
}

fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
        (n as i64).to_string()
    } else {
        n.to_string()
    }
}

/// What `:calc` needs to know about the root once per resolution.
struct CalcScope {
    /// Positions of the root keys a single-pass token lookup cannot find
    /// (see [`is_calc_special_key`]).
    root_special: Vec<usize>,
}

impl CalcScope {
    fn of(root: &Value) -> Self {
        // spec: computed after the pre-passes; resolution replaces values but
        // never adds or removes keys, so the root key set is fixed from here.
        CalcScope { root_special: root.as_object().map(special_key_positions).unwrap_or_default() }
    }
}

/// The same for the object being resolved, found on its first `:calc`.
#[derive(Default)]
struct LocalCalc {
    special: std::cell::OnceCell<Vec<usize>>,
}

fn special_key_positions(map: &Map) -> Vec<usize> {
    map.keys().enumerate().filter(|(_, k)| is_calc_special_key(k)).map(|(i, _)| i).collect()
}

/// A key that whole-word substitution can reach in ways a token lookup cannot:
/// one containing a non-word byte (`max-hp`, `a.b`, `цена`), or one that a
/// substituted number can itself spell (`2`, `NaN`, `inf`).
fn is_calc_special_key(k: &str) -> bool {
    !k.bytes().all(is_word_char)
        || k.bytes().all(|b| b.is_ascii_digit())
        || k == "NaN"
        || k == "inf"
}

/// Substitute numeric variables into a `:calc` expression: a key of the
/// current object first (it shadows the root), then a root key. `None` when
/// the result exceeds `MAX_CALC_RESOLVED_LEN`.
///
/// perf: one pass over the expression's words with a map lookup each. The
/// sequential form — whole-word replacement key by key, in document order —
/// survives only for documents that have a special key with a numeric value,
/// where it is the exact semantics; and there it visits just the keys that
/// can match: the special ones and the words of the expression. A substituted
/// number is delimited by non-word characters and spells no ordinary key, so
/// no other key can ever match. It used to scan every numeric key of the
/// object and of the root for every `:calc`: 8 000 of them next to one
/// `max-hp 10` took 3.4 s.
///
/// spec: until 3.7.5 the root was substituted first, so a root key beat a
/// same-named key of the object — the opposite of the TypeScript engine.
fn substitute_calc_vars(
    expr: &str,
    local: &Map,
    self_key: &str,
    root: Option<&Map>,
    scope: &CalcScope,
    local_calc: &LocalCalc,
) -> Option<String> {
    let local_is_root = root.map_or(false, |r| std::ptr::eq(r, local));
    let numeric = |m: &Map, k: &str| m.get(k).and_then(value_as_number);
    let numeric_at = |m: &Map, i: usize| m.get_index(i).and_then(|(_, v)| value_as_number(v));
    let lookup = |name: &str| -> Option<f64> {
        if name != self_key {
            if let Some(n) = numeric(local, name) {
                return Some(n);
            }
        }
        root.and_then(|r| numeric(r, name))
    };

    let local_special: &[usize] = if local_is_root {
        &scope.root_special
    } else {
        local_calc.special.get_or_init(|| special_key_positions(local))
    };
    let special_in_play = scope
        .root_special
        .iter()
        .any(|&i| root.map_or(false, |r| numeric_at(r, i).is_some()))
        || (!local_is_root
            && local_special.iter().any(|&i| {
                local.get_index(i).map_or(false, |(k, v)| k != self_key && value_as_number(v).is_some())
            }));

    if special_in_play {
        // Exact sequential whole-word replacement, object before root.
        let mut resolved = expr.to_string();
        let mut words: Vec<&str> = Vec::new();
        let bytes = expr.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if is_word_char(bytes[i]) {
                let start = i;
                while i < bytes.len() && is_word_char(bytes[i]) {
                    i += 1;
                }
                words.push(&expr[start..i]);
            } else {
                i += 1;
            }
        }
        let maps: [(Option<&Map>, &[usize]); 2] = [
            (Some(local), local_special),
            (if local_is_root { None } else { root }, &scope.root_special),
        ];
        for (m, special) in maps {
            let Some(m) = m else { continue };
            let is_local = std::ptr::eq(m, local);
            let mut candidates: Vec<usize> = special.to_vec();
            candidates.extend(words.iter().filter_map(|w| m.get_index_of(*w)));
            candidates.sort_unstable();
            candidates.dedup();
            for idx in candidates {
                let Some((rk, rv)) = m.get_index(idx) else { continue };
                if is_local && rk == self_key {
                    continue;
                }
                if let Some(n) = value_as_number(rv) {
                    resolved = replace_word(&resolved, rk, &format_number(n));
                    if resolved.len() > MAX_CALC_RESOLVED_LEN {
                        return None;
                    }
                }
            }
        }
        return Some(resolved);
    }

    let bytes = expr.as_bytes();
    let mut out = String::with_capacity(expr.len() + 16);
    let mut i = 0;
    while i < bytes.len() {
        if is_word_char(bytes[i]) {
            let start = i;
            while i < bytes.len() && is_word_char(bytes[i]) {
                i += 1;
            }
            let token = &expr[start..i];
            match lookup(token) {
                Some(n) => out.push_str(&format_number(n)),
                None => out.push_str(token),
            }
        } else {
            let ch = expr[i..].chars().next().unwrap_or('\u{fffd}');
            out.push(ch);
            i += ch.len_utf8();
        }
        if out.len() > MAX_CALC_RESOLVED_LEN {
            return None;
        }
    }
    Some(out)
}

/// Replace whole-word occurrences of `word` with `replacement`.
fn replace_word(haystack: &str, word: &str, replacement: &str) -> String {
    let word_bytes = word.as_bytes();
    let word_len = word_bytes.len();
    let hay_bytes = haystack.as_bytes();
    let hay_len = hay_bytes.len();

    if word_len > hay_len {
        return haystack.to_string();
    }

    let mut result = String::with_capacity(hay_len.min(MAX_ENGINE_SCRATCH_STRING));
    let mut i = 0;

    while i <= hay_len - word_len {
        if result.len() >= MAX_ENGINE_SCRATCH_STRING {
            break;
        }
        if &hay_bytes[i..i + word_len] == word_bytes {
            let before_ok = i == 0 || !is_word_char(hay_bytes[i - 1]);
            let after_ok = i + word_len >= hay_len || !is_word_char(hay_bytes[i + word_len]);
            if before_ok && after_ok {
                let room = MAX_ENGINE_SCRATCH_STRING.saturating_sub(result.len());
                if room > 0 {
                    let take = replacement.len().min(room);
                    let end = replacement.floor_char_boundary(take);
                    result.push_str(&replacement[..end]);
                }
                i += word_len;
                continue;
            }
        }
        // A whole character: `hay_bytes[i] as char` read each UTF-8 byte as
        // Latin-1. A match of a UTF-8 word starts on a boundary, so `i` stays
        // on one.
        let ch = haystack[i..].chars().next().unwrap_or('\u{fffd}');
        if result.len() < MAX_ENGINE_SCRATCH_STRING {
            result.push(ch);
        }
        i += ch.len_utf8();
    }
    if i < hay_len {
        let room = MAX_ENGINE_SCRATCH_STRING.saturating_sub(result.len());
        let end = i + haystack[i..].floor_char_boundary(room.min(hay_len - i));
        result.push_str(&haystack[i..end]);
    }
    result
}

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn weighted_random(items: &[Value], weights: &[f64]) -> Value {
    let mut w: Vec<f64> = weights.to_vec();
    if w.len() < items.len() {
        let assigned: f64 = w.iter().sum();
        // If the explicit weights already exceed 100, give unassigned items
        // the same average weight as the assigned ones so they remain visible.
        // If there is room left under 100, distribute the remainder equally.
        let per_item = if assigned < 100.0 {
            (100.0 - assigned) / (items.len() - w.len()) as f64
        } else {
            assigned / w.len() as f64
        };
        while w.len() < items.len() {
            w.push(per_item);
        }
    }
    let total: f64 = w.iter().sum();
    if total <= 0.0 {
        return items[rng::random_usize(items.len())].clone();
    }

    let rand_val = rng::random_f64_01();
    let mut cumulative = 0.0;
    for (i, item) in items.iter().enumerate() {
        cumulative += w[i] / total;
        if rand_val <= cumulative {
            return item.clone();
        }
    }
    items.last().cloned().unwrap_or(Value::Null)
}

// ─── Inheritance pre-pass ─────────────────────────────────

/// Mask every value under `:secret` before resolution, keeping what it held
/// in `originals` for the key's own turn (see `resolve_value`).
///
/// safety: a value was pre-masked only when its chain had none of sixteen
/// "resolving" markers, so `password:secret:default:x hunter2` stayed plain
/// text until its turn — and `leak:alias password` on the line above read
/// it. A group under `:secret` was not masked at all. Now nothing waits in
/// the clear: scalars become secrets, a list or a group has every scalar in
/// it masked.
fn mask_secret_literals(value: &mut Value, metadata: &Map<MetaMap>, path: &mut String, originals: &mut SecretOriginals) {
    let map = match value {
        Value::Object(m) => m,
        _ => return,
    };
    if let Some(mm) = metadata.get(path.as_str()) {
        mask_secret_keys(map, mm, path, originals);
    }
    let base_len = path.len();
    for (key, child) in map.iter_mut() {
        if !matches!(child, Value::Object(_) | Value::Array(_)) {
            continue;
        }
        if base_len > 0 {
            path.push('.');
        }
        push_key_segment(path, key);
        mask_secret_subtree(child, metadata, path, originals);
        path.truncate(base_len);
    }
}

/// Mask the keys of one object that its declarations `mm` mark `:secret`;
/// `path` is where the object sits.
fn mask_secret_keys(map: &mut Map, mm: &MetaMap, path: &str, originals: &mut SecretOriginals) {
    for (key, meta) in mm {
        if !has_marker(&meta.markers, "secret") {
            continue;
        }
        if let Some(v) = map.get_mut(key.as_str()) {
            if !matches!(v, Value::Secret(_) | Value::Null) {
                // perf: the mask is built beside the value, which then moves
                // out whole; the value was copied, the copy masked in place,
                // and the path copied again for every secret key.
                let masked = masked_copy(v);
                let original = std::mem::replace(v, masked);
                match originals.get_mut(path) {
                    Some(kept) => {
                        kept.insert(key.clone(), original);
                    }
                    None => {
                        let mut kept = Map::default();
                        kept.insert(key.clone(), original);
                        originals.insert(path.to_string(), kept);
                    }
                }
            }
        }
    }
}

/// What [`mask_secret_value`] makes of a copy of `v`.
fn masked_copy(v: &Value) -> Value {
    match v {
        Value::Secret(_) | Value::Null => v.clone(),
        Value::Array(items) => Value::Array(items.iter().map(masked_copy).collect()),
        Value::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), masked_copy(v))).collect()),
        Value::String(s) if is_engine_error(s) => v.clone(),
        other => Value::Secret(value_to_string(other)),
    }
}

/// [`mask_secret_literals`] for a whole document, led by its declarations.
///
/// perf: the walk above visits every group and every list item, building a
/// path and looking it up for each, to reach the few keys marked `:secret` —
/// a tenth of the time a service config took to resolve, and paid in full by
/// a document with no secret in it. The declarations already say where the
/// secrets are: each path that has one is followed to its object.
///
/// A group under `:secret` has to be masked before the secrets declared
/// inside it are looked at, as the walk does on its way down. A path is
/// longer than every path above it, so the shortest go first.
fn mask_declared_secrets(root: &mut Value, metadata: &Map<MetaMap>, originals: &mut SecretOriginals) {
    let mut paths: Vec<&str> = metadata
        .iter()
        .filter(|(_, mm)| mm.values().any(|meta| has_marker(&meta.markers, "secret")))
        .map(|(path, _)| path.as_str())
        .collect();
    paths.sort_by_key(|path| path.len());
    for path in paths {
        if let Some(map) = object_at_path_mut(root, path) {
            mask_secret_keys(map, &metadata[path], path, originals);
        }
    }
}

/// The object a metadata path names — keys down from the root, and an item
/// of a list where a segment reads `#<index>` — or `None` when the document
/// has no object there.
fn object_at_path_mut<'a>(root: &'a mut Value, path: &str) -> Option<&'a mut Map> {
    let mut current = match root {
        Value::Object(map) => map,
        _ => return None,
    };
    let mut rest = path;
    while !rest.is_empty() {
        let (segment, tail) = next_path_segment(rest);
        rest = tail;
        current = match current.get_mut(segment.as_ref())? {
            Value::Object(map) => map,
            Value::Array(items) => {
                // A list is on a path only through one of its items.
                let (item, tail) = next_path_segment(rest);
                rest = tail;
                let index: usize = item.strip_prefix('#')?.parse().ok()?;
                match items.get_mut(index)? {
                    Value::Object(map) => map,
                    _ => return None,
                }
            }
            _ => return None,
        };
    }
    Some(current)
}

/// The first segment of a metadata path, unescaped, and the path after it
/// (see [`push_key_segment`]).
fn next_path_segment(path: &str) -> (std::borrow::Cow<'_, str>, &str) {
    use std::borrow::Cow;
    let Some(stop) = memchr::memchr2(b'.', b'\\', path.as_bytes()) else {
        return (Cow::Borrowed(path), "");
    };
    if path.as_bytes()[stop] == b'.' {
        return (Cow::Borrowed(&path[..stop]), &path[stop + 1..]);
    }
    // A key with `.` or `\` in it: unescape.
    let mut segment = String::with_capacity(path.len());
    let mut chars = path.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '\\' => {
                if let Some((_, escaped)) = chars.next() {
                    segment.push(escaped);
                }
            }
            '.' => return (Cow::Owned(segment), &path[at + 1..]),
            _ => segment.push(c),
        }
    }
    (Cow::Owned(segment), "")
}

/// [`mask_secret_literals`] for the object or list at `path`.
fn mask_secret_subtree(child: &mut Value, metadata: &Map<MetaMap>, path: &mut String, originals: &mut SecretOriginals) {
    match child {
        Value::Object(_) => mask_secret_literals(child, metadata, path, originals),
        Value::Array(arr) => {
            let list_len = path.len();
            for (idx, item) in arr.iter_mut().enumerate() {
                if let Value::Object(_) = item {
                    push_item_segment(path, idx);
                    mask_secret_literals(item, metadata, path, originals);
                    path.truncate(list_len);
                }
            }
        }
        _ => {}
    }
}

/// The first two segments of a metadata path (`parent.field`), or `None` for
/// a path of one segment. A `.` escaped with `\` is part of its segment.
fn two_segment_head(path: &str) -> Option<&str> {
    let bytes = path.as_bytes();
    let mut dots = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'.' => {
                dots += 1;
                if dots == 2 {
                    return Some(&path[..i]);
                }
            }
            _ => {}
        }
        i += 1;
    }
    if dots == 1 { Some(path) } else { None }
}

fn apply_inheritance(root: &mut Value, metadata: &mut Map<MetaMap>) {
    let root_meta = match metadata.get("") {
        Some(m) => m,
        None => return,
    };

    // Collect inherit targets: child_key → [parent1, parent2, ...]
    //
    // Two surface syntaxes feed this list:
    //   production:inherit:base   → parents come from markers[idx+1..]
    //   production:inherit base   → parser promotes "base" into meta.args
    let mut inherits: Vec<(String, Vec<String>)> = Vec::new();
    for (key, meta) in root_meta {
        if has_marker(&meta.markers, "inherit") {
            let idx = meta.markers.iter().position(|m| m == "inherit").unwrap();
            let mut parents: Vec<String> = meta.markers[idx + 1..].to_vec();
            // Fall back / extend with marker args promoted from a positional value.
            if parents.is_empty() && !meta.args.is_empty() {
                parents = meta.args.clone();
            }
            if !parents.is_empty() {
                inherits.push((key.clone(), parents));
            }
        }
    }
    if inherits.is_empty() {
        return;
    }
    // Root groups marked `:secret`: what is inherited from them stays secret.
    let secret_groups: Vec<String> = root_meta
        .iter()
        .filter(|(_, meta)| has_marker(&meta.markers, "secret"))
        .map(|(key, _)| key.clone())
        .collect();

    // perf: the declarations nested under each `object.field`, found once.
    // Every inherited field used to walk the whole metadata table for them, so
    // 1 500 children of a parent with 1 500 marked fields did not finish in
    // 25 s.
    let mut nested_index: HashMap<String, Vec<String>> = HashMap::new();
    for path in metadata.keys() {
        if let Some(head) = two_segment_head(path) {
            nested_index.entry(head.to_string()).or_default().push(path.clone());
        }
    }

    let Value::Object(map) = root else { return };
    for (child_key, parents) in &inherits {
        // spec: only an object inherits. A list or a scalar under `:inherit`
        // is left as written — it used to be replaced by an empty object.
        if !matches!(map.get(child_key.as_str()), Some(Value::Object(_))) {
            continue;
        }
        // Merge parents left-to-right: first parent is base, each subsequent
        // overrides, and the child overrides all. `source` remembers which
        // object each field came from (`None` = the child itself).
        //
        // safety: every inherited field is a copy, charged to the document's
        // budget — 3 000 children of a 3 000-field parent are nine million
        // fields from a 130 KB file.
        let mut merged: Map = Map::default();
        let mut source: Map<Option<&str>> = Map::default();
        let mut within_budget = true;
        'parents: for parent_name in parents {
            if let Some(Value::Object(p)) = map.get(parent_name.as_str()) {
                for (k, v) in p {
                    // Measured only as far as the budget reaches (see `value_cost_within`).
                    let fits = match value_cost_within(v, expansion_room()) {
                        Some((cost, _)) => charge_expansion(cost.saturating_add(k.len())),
                        None => charge_expansion(usize::MAX),
                    };
                    if !fits {
                        within_budget = false;
                        break 'parents;
                    }
                    merged.insert(k.clone(), v.clone());
                    source.insert(k.clone(), Some(parent_name.as_str()));
                }
            }
        }
        if !within_budget {
            map.insert(child_key.clone(), Value::String(LIMIT_ERR.to_string()));
            continue;
        }
        if let Some(Value::Object(c)) = map.get(child_key.as_str()) {
            for (k, v) in c {
                merged.insert(k.clone(), v.clone());
                source.insert(k.clone(), None);
            }
        }

        // spec: a field keeps the markers of the object its value came from.
        // Inherited fields had none here, so `port:env:default:5 NAME` on the
        // parent reached the child as the literal env-var name. A field the
        // child redefines keeps only the child's own markers.
        //
        // safety: these are copies as well. A parent of 1 200 marked fields
        // gave each child 1 200 declarations outside the budget — 290 MiB
        // for a 160 KB document.
        let mut top = MetaMap::default();
        let mut nested: Vec<(String, MetaMap)> = Vec::new();
        for (k, src) in &source {
            let from = src.unwrap_or(child_key.as_str());
            if let Some(m) = metadata.get(key_segment(from).as_ref()).and_then(|mm| mm.get(k.as_str())) {
                if src.is_some() && !charge_expansion(meta_cost(k, m)) {
                    within_budget = false;
                    break;
                }
                top.insert(k.clone(), m.clone());
            }
            // safety: a group under `:secret` is secret in every field, and a
            // copy of a field is that field. The copies were taken before the
            // group was masked and carried no mark of their own, so
            // `child:inherit base` printed everything `base:secret` hid. An
            // inherited field now says `:secret` itself — first in its chain,
            // where it cannot be read as the argument of another marker — and
            // is masked, resolved and masked again like any key that does.
            if src.is_some_and(|parent| secret_groups.iter().any(|name| name == parent)) {
                let declared = top.entry(k.clone()).or_insert_with(|| Meta {
                    markers: Vec::new(),
                    args: Vec::new(),
                    type_hint: None,
                    constraints: None,
                });
                if !has_marker(&declared.markers, "secret") {
                    if !charge_expansion(2 * VALUE_COST + k.len()) {
                        within_budget = false;
                        break;
                    }
                    declared.markers.insert(0, "secret".to_string());
                }
            }
            if let Some(parent) = src {
                // The inherited subtree is a copy; its nested markers move with it.
                let prefix = format!("{}.{}", key_segment(parent), key_segment(k));
                let Some(paths) = nested_index.get(prefix.as_str()) else { continue };
                for path in paths {
                    let Some(mm) = metadata.get(path.as_str()) else { continue };
                    let rest = &path[prefix.len()..];
                    if !charge_expansion(meta_map_cost(mm).saturating_add(path.len())) {
                        within_budget = false;
                        break;
                    }
                    nested.push((format!("{}.{}{}", key_segment(child_key), key_segment(k), rest), mm.clone()));
                }
            }
        }
        if !within_budget {
            map.insert(child_key.clone(), Value::String(LIMIT_ERR.to_string()));
            continue;
        }
        map.insert(child_key.clone(), Value::Object(merged));
        if top.is_empty() {
            metadata.shift_remove(key_segment(child_key).as_ref());
        } else {
            metadata.insert(key_segment(child_key).into_owned(), top);
        }
        for (path, mm) in nested {
            // A later child may inherit from this one: its copies are indexed too.
            if !metadata.contains_key(path.as_str()) {
                if let Some(head) = two_segment_head(&path) {
                    nested_index.entry(head.to_string()).or_default().push(path.clone());
                }
            }
            metadata.insert(path, mm);
        }
    }
}

/// The value at `path`: a key of `root` spelled exactly so, else a dot-path
/// through nested objects.
///
/// perf: a borrow. `deep_get` cloned the whole subtree for every `{key}`,
/// `:map` source and `:calc` operand, only for the caller to read one number
/// or turn it into text.
fn deep_ref<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    if let Value::Object(map) = root {
        if let Some(val) = map.get(path) {
            return Some(val);
        }
    }
    let mut current = root;
    for part in path.split('.') {
        match current {
            Value::Object(map) => current = map.get(part)?,
            _ => return None,
        }
    }
    Some(current)
}

/// A value read from a file, ready for an object `depth` levels below the
/// root: refused when it would nest past `MAX_RESOLVE_DEPTH`, and under
/// `:secret` masked leaf by leaf.
fn placed_document(mut value: Value, depth: usize, secret: bool) -> Value {
    let (_, nesting) = value_cost(&value);
    if depth.saturating_add(nesting) >= MAX_RESOLVE_DEPTH {
        return Value::String(NESTING_ERR.to_string());
    }
    if secret {
        mask_secret_value(&mut value);
    }
    value
}

/// Whether `s` is an error the engine left in place of a value.
fn is_engine_error(s: &str) -> bool {
    s.as_bytes().first().is_some_and(u8::is_ascii_uppercase) && RUNTIME_ERROR_PREFIXES.iter().any(|p| s.starts_with(p))
}

/// Make a resolved value a secret. A scalar becomes `Value::Secret` of its
/// text; a list or a group keeps its shape and every scalar in it is masked.
/// An absent value (null) stays null — wrapping it made the secret the text
/// "null", which `[required]` read as a present value — an existing secret is
/// kept, and an engine error stays readable: `:secret:watch missing.txt` is
/// a failure to report, not a value to hide.
fn mask_secret_value(v: &mut Value) {
    match v {
        Value::Secret(_) | Value::Null => {}
        Value::Array(items) => items.iter_mut().for_each(mask_secret_value),
        Value::Object(map) => map.values_mut().for_each(mask_secret_value),
        Value::String(s) if is_engine_error(s) => {}
        other => {
            let s = value_to_string(other);
            *other = Value::Secret(s);
        }
    }
}

/// From this many bytes on, an interpolated value is given exactly the room
/// it needs instead of room that doubles.
const EXACT_RESERVE_FROM: usize = 4096;

/// Resolve {placeholder} references in a template string.
/// Supports: {key}, {key.nested}, {key:alias}, {key:include}
///
/// perf: one pass. A reference is `{`, a name of letters, digits, `_` and
/// `.`, then `}` or `:scope}`; the name is read first, so a `{` that opens
/// nothing costs only the characters up to the next one, and the `}` that
/// closes a scope is searched for once. Every `{` used to search the rest of
/// the text for its `}`, then for `:` — a megabyte of `{` took 27 s.
///
/// `None` when the text would pass `cap` bytes.
fn resolve_interpolation(
    tpl: &str,
    root: &Value,
    local_map: &Map,
    includes: &Map,
    cap: usize,
) -> Option<String> {
    let bytes = tpl.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len.min(MAX_ENGINE_SCRATCH_STRING));
    let mut i = 0;
    // The first `}` after the last scope looked at, while it is still ahead.
    let mut close_cache: Option<usize> = None;
    let mut no_more_closes = false;

    while i < len {
        if result.len() >= MAX_ENGINE_SCRATCH_STRING {
            break;
        }
        if bytes[i] != b'{' {
            // Literal text up to the next `{`, whole characters only.
            let end = memchr::memchr(b'{', &bytes[i..]).map_or(len, |rel| i + rel);
            let run = &tpl[i..end];
            let room = MAX_ENGINE_SCRATCH_STRING - result.len();
            if run.len() <= room {
                result.push_str(run);
            } else {
                let mut cut = room;
                while !run.is_char_boundary(cut) {
                    cut += 1;
                }
                result.push_str(&run[..cut]);
            }
            i = end;
            continue;
        }

        let name_start = i + 1;
        let mut name_end = name_start;
        for ch in tpl[name_start..].chars() {
            if ch.is_alphanumeric() || ch == '_' || ch == '.' {
                name_end += ch.len_utf8();
            } else {
                break;
            }
        }
        let close = match bytes.get(name_end) {
            Some(b'}') => Some(name_end),
            Some(b':') if !no_more_closes => {
                if close_cache.map_or(true, |c| c <= name_end) {
                    close_cache = memchr::memchr(b'}', &bytes[name_end + 1..]).map(|rel| name_end + 1 + rel);
                    no_more_closes = close_cache.is_none();
                }
                close_cache
            }
            _ => None,
        };
        let Some(close) = close else {
            // Not a reference: the brace is text.
            result.push('{');
            i += 1;
            continue;
        };

        let ref_name = &tpl[name_start..name_end];
        let inner = &tpl[name_start..close];
        let resolved = if name_end < close {
            let scope = &tpl[name_end + 1..close];
            if scope == "include" {
                // {key:include} — first/only include
                if includes.len() == 1 {
                    includes.values().next().and_then(|first| deep_ref(first, ref_name))
                } else {
                    None
                }
            } else {
                // {key:alias} — look up by alias
                includes.get(scope).and_then(|inc| deep_ref(inc, ref_name))
            }
        } else {
            // {key} — local
            deep_ref(root, ref_name).or_else(|| local_map.get(ref_name))
        };
        if let Some(val) = resolved {
            let s = value_text(val);
            let room = MAX_ENGINE_SCRATCH_STRING.saturating_sub(result.len());
            if room > 0 {
                let take = s.len().min(room);
                let end = s.floor_char_boundary(take);
                if result.len() + end > cap {
                    return None;
                }
                if end >= EXACT_RESERVE_FROM {
                    // perf: growth by doubling held up to three times the
                    // text while it was built and kept up to twice after.
                    let tail = (len - (close + 1)).min(room - end);
                    result.reserve_exact(end + tail);
                }
                result.push_str(&s[..end]);
            }
        } else {
            result.push('{');
            let rem = MAX_ENGINE_SCRATCH_STRING.saturating_sub(result.len() + 1);
            if rem > 0 {
                let end = inner.floor_char_boundary(inner.len().min(rem));
                result.push_str(&inner[..end]);
            }
            if result.len() < MAX_ENGINE_SCRATCH_STRING {
                result.push('}');
            }
        }
        if result.len() > cap {
            return None;
        }
        i = close + 1;
    }
    if result.len() > cap {
        return None;
    }
    if result.capacity() - result.len() >= EXACT_RESERVE_FROM {
        result.shrink_to_fit();
    }
    Some(result)
}

/// Load !include files into a map<alias, Value>.
fn load_includes(
    directives: &[IncludeDirective],
    options: &Options,
) -> Map {
    let mut map = Map::default();
    let base = options.base_path.as_deref().unwrap_or(".");
    let max_depth = options.max_include_depth.unwrap_or(DEFAULT_MAX_INCLUDE_DEPTH);
    if options._include_depth >= max_depth {
        return map;
    }
    for inc in directives {
        let full = match jail_path(base, &inc.path) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if check_file_size(&full).is_err() {
            continue;
        }
        let _open = match include_enter(&full, &inc.path) {
            Ok(g) => g,
            Err(_) => continue,
        };
        if let Ok(text) = read_budgeted(&full, &inc.path) {
            let mut included = parser::parse(&text);
            if included.mode == Mode::Active {
                let mut child_opts = options.clone();
                child_opts._include_depth += 1;
                if let Some(parent) = full.parent() {
                    child_opts.base_path = Some(parent.to_string_lossy().into_owned());
                }
                resolve(&mut included, &child_opts);
            }
            map.insert(inc.alias.clone(), included.root);
        }
    }
    map
}

/// Load !use packages into a map<alias, Value>.
///
/// Looks for `<packages_path>/@scope/name/src/main.synx` on disk.
/// Falls back to `./synx_packages/` when `options.packages_path` is unset.
fn load_packages(
    directives: &[UseDirective],
    options: &Options,
    #[cfg(feature = "wasm")] wasm_runtime: &mut crate::wasm::WasmMarkerRuntime,
) -> Map {
    let mut map = Map::default();
    // safety: a package may `!use` another. The depth was counted but never
    // checked, and nothing noticed a package reaching itself: three packages
    // using one another overflowed the stack.
    if directives.is_empty()
        || options._include_depth >= options.max_include_depth.unwrap_or(DEFAULT_MAX_INCLUDE_DEPTH)
    {
        return map;
    }
    // perf: the packages path is put together only for a document that uses one.
    let pkg_base = options
        .packages_path
        .as_deref()
        .unwrap_or("./synx_packages");
    let base = options.base_path.as_deref().unwrap_or(".");
    let pkg_root = std::path::Path::new(base).join(pkg_base);

    for ud in directives {
        // @scope/name  → package dir is <pkg_root>/@scope/name
        if !is_contained_relative(&ud.package) {
            continue;
        }
        let pkg_dir = pkg_root.join(&ud.package);

        // Check if this is a WASM marker package (type markers in manifest)
        #[cfg(feature = "wasm")]
        {
            if is_marker_package(&pkg_dir) {
                // Load the .wasm file from the package
                let wasm_entry = read_manifest_wasm(&pkg_dir)
                    .unwrap_or_else(|| pkg_dir.join("src").join("main.wasm"));
                let caps = read_manifest_capabilities(&pkg_dir);
                if wasm_entry.is_file() {
                    if let Ok(wasm_bytes) = std::fs::read(&wasm_entry) {
                        match wasm_runtime.load_module(&wasm_bytes, caps) {
                            Ok(_markers) => {}
                            Err(e) => {
                                map.insert(
                                    ud.alias.clone(),
                                    Value::String(format!("WASM_ERR: {}", e)),
                                );
                            }
                        }
                    }
                }
                continue;
            }
        }

        // Read entry point from manifest (synx-pkg.synx), fall back to src/main.synx
        let entry = read_manifest_main(&pkg_dir)
            .unwrap_or_else(|| pkg_dir.join("src").join("main.synx"));

        if !entry.is_file() {
            continue;
        }
        if check_file_size(&entry).is_err() {
            continue;
        }
        let opened = std::fs::canonicalize(&entry).unwrap_or_else(|_| entry.clone());
        let _open = match include_enter(&opened, &ud.package) {
            Ok(g) => g,
            Err(_) => continue,
        };
        if let Ok(text) = read_budgeted(&entry, &ud.package) {
            let mut parsed = parser::parse(&text);
            if parsed.mode == Mode::Active {
                let mut child_opts = options.clone();
                child_opts._include_depth += 1;
                child_opts.base_path = Some(
                    entry.parent().unwrap_or(&pkg_dir).to_string_lossy().into_owned()
                );
                resolve(&mut parsed, &child_opts);
            }
            map.insert(ud.alias.clone(), parsed.root);
        }
    }
    map
}

/// Read synx-pkg.synx manifest and extract the `main` entry point path.
/// Returns the absolute path to the entry file, or None if manifest is missing/invalid.
fn read_manifest_main(pkg_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let manifest = pkg_dir.join("synx-pkg.synx");
    let text = std::fs::read_to_string(&manifest).ok()?;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("main ") {
            let entry_path = rest.trim();
            if is_contained_relative(entry_path) {
                return Some(pkg_dir.join(entry_path));
            }
        }
        // Legacy field name support
        if let Some(rest) = trimmed.strip_prefix("entry ") {
            let entry_path = rest.trim();
            if is_contained_relative(entry_path) {
                return Some(pkg_dir.join(entry_path));
            }
        }
    }
    None
}

/// Check if a package is a WASM marker package.
/// Matches `type markers` in manifest, or `main` pointing to a `.wasm` file.
#[cfg(feature = "wasm")]
fn is_marker_package(pkg_dir: &std::path::Path) -> bool {
    let manifest = pkg_dir.join("synx-pkg.synx");
    if let Ok(text) = std::fs::read_to_string(&manifest) {
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed == "type markers" {
                return true;
            }
            if let Some(rest) = trimmed.strip_prefix("main ") {
                if rest.trim().ends_with(".wasm") {
                    return true;
                }
            }
        }
    }
    false
}

/// Read the WASM entry point from a marker package manifest.
#[cfg(feature = "wasm")]
fn read_manifest_wasm(pkg_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let manifest = pkg_dir.join("synx-pkg.synx");
    let text = std::fs::read_to_string(&manifest).ok()?;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("wasm ") {
            let wasm_path = rest.trim();
            if is_contained_relative(wasm_path) {
                return Some(pkg_dir.join(wasm_path));
            }
        }
        // Also check `main` if it points to a .wasm file
        if let Some(rest) = trimmed.strip_prefix("main ") {
            let path = rest.trim();
            if path.ends_with(".wasm") && is_contained_relative(path) {
                return Some(pkg_dir.join(path));
            }
        }
    }
    None
}

/// Read capability permissions from a marker package manifest.
#[cfg(feature = "wasm")]
fn read_manifest_capabilities(pkg_dir: &std::path::Path) -> crate::wasm::WasmCapabilities {
    let manifest = pkg_dir.join("synx-pkg.synx");
    let text = match std::fs::read_to_string(&manifest) {
        Ok(t) => t,
        Err(_) => return crate::wasm::WasmCapabilities::default(),
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("permissions ") {
            return crate::wasm::WasmCapabilities::from_manifest_line(rest);
        }
    }
    crate::wasm::WasmCapabilities::default()
}

// ─── Type validation ──────────────────────────────────────

/// The constraints every field of one name is held to.
///
/// perf: a reference to the declaration, by its place in the metadata — the
/// table path's index, then the key's index under it. Each declaration with
/// constraints used to be copied into the registry, strings and all: a ninth
/// of the time a service config took to resolve. A copy is still what a
/// field declared twice gets (the two are merged), and what every field of a
/// document that inherits gets, since inheritance rewrites the declarations
/// the indexes point into.
enum FieldRule {
    At(usize, usize),
    Owned(Constraints),
}

impl FieldRule {
    fn constraints<'a>(&'a self, metadata: &'a Map<MetaMap>) -> Option<&'a Constraints> {
        match self {
            FieldRule::Owned(c) => Some(c),
            FieldRule::At(table_at, key_at) => {
                metadata.get_index(*table_at)?.1.get_index(*key_at)?.1.constraints.as_ref()
            }
        }
    }
}

/// Fill the type and constraint registries from one object and its subtree:
/// the object's own declarations first, then each child in document order.
#[allow(clippy::too_many_arguments)]
fn collect_registries(
    value: &Value,
    metadata: &Map<MetaMap>,
    path: &mut String,
    types: &mut Map<String>,
    constraints: &mut Map<FieldRule>,
    copies: bool,
    depth: usize,
) {
    if depth > MAX_RESOLVE_DEPTH {
        return;
    }
    let Value::Object(map) = value else { return };
    if let Some((table_at, _, mm)) = metadata.get_full(path.as_str()) {
        for (key_at, (key, meta)) in mm.iter().enumerate() {
            if let Some(ref hint) = meta.type_hint {
                if !types.contains_key(key.as_str()) {
                    types.insert(key.clone(), hint.clone());
                }
            }
            if let Some(ref c) = meta.constraints {
                match constraints.get_mut(key.as_str()) {
                    // The same field declared again: the two are merged, and
                    // the result is the registry's own.
                    Some(rule) => {
                        let mut merged = match std::mem::replace(rule, FieldRule::At(0, 0)) {
                            FieldRule::Owned(existing) => existing,
                            at => at.constraints(metadata).cloned().unwrap_or_default(),
                        };
                        merge_constraints(&mut merged, c);
                        *rule = FieldRule::Owned(merged);
                    }
                    None => {
                        let rule = if copies { FieldRule::Owned(c.clone()) } else { FieldRule::At(table_at, key_at) };
                        constraints.insert(key.clone(), rule);
                    }
                }
            }
        }
    }
    let base_len = path.len();
    for (key, child) in map {
        if !matches!(child, Value::Object(_) | Value::Array(_)) {
            continue;
        }
        if base_len > 0 {
            path.push('.');
        }
        push_key_segment(path, key);
        match child {
            Value::Object(_) => collect_registries(child, metadata, path, types, constraints, copies, depth + 1),
            Value::Array(items) => {
                let list_len = path.len();
                for (idx, item) in items.iter().enumerate() {
                    if let Value::Object(_) = item {
                        push_item_segment(path, idx);
                        collect_registries(item, metadata, path, types, constraints, copies, depth + 1);
                        path.truncate(list_len);
                    }
                }
            }
            _ => {}
        }
        path.truncate(base_len);
    }
}

/// Merge constraints when the same field is declared multiple times.
/// Strategy is intentionally strict to keep schemas consistent across templates.
fn merge_constraints(base: &mut Constraints, incoming: &Constraints) {
    if incoming.required {
        base.required = true;
    }
    if incoming.readonly {
        base.readonly = true;
    }

    // Stricter numeric bounds win.
    base.min = match (base.min, incoming.min) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (None, Some(b)) => Some(b),
        (a, None) => a,
    };
    base.max = match (base.max, incoming.max) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (None, Some(b)) => Some(b),
        (a, None) => a,
    };

    // Keep first non-empty type/pattern/enum declaration.
    if base.type_name.is_none() {
        base.type_name = incoming.type_name.clone();
    }
    if base.pattern.is_none() {
        base.pattern = incoming.pattern.clone();
    }
    if base.enum_values.is_none() {
        base.enum_values = incoming.enum_values.clone();
    }
}

/// Constraints then type for every field of the tree, in one walk.
///
/// A value that already holds a CONSTRAINT_ERR / TYPE_ERR string from the
/// per-key pass in `apply_markers` is not re-validated against constraints —
/// otherwise the error message itself would be measured (its length checked
/// against min/max) instead of the original value.
fn validate_fields(value: &mut Value, constraints: &Map<FieldRule>, types: &Map<String>, metadata: &Map<MetaMap>) {
    if constraints.is_empty() && types.is_empty() {
        return;
    }
    if let Value::Object(ref mut map) = value {
        for (key, val) in map.iter_mut() {
            if let Some(c) = constraints.get(key.as_str()).and_then(|rule| rule.constraints(metadata)) {
                let already_errored = matches!(
                    val,
                    Value::String(s) if s.starts_with("CONSTRAINT_ERR:") || s.starts_with("TYPE_ERR:")
                );
                if !already_errored {
                    if let Some(err) = constraint_violation(key, val, c) {
                        *val = Value::String(err);
                    }
                }
            }
            if let Some(expected) = types.get(key.as_str()) {
                if !value_matches_type(val, expected) {
                    *val = Value::String(format!(
                        "TYPE_ERR: '{}' expected {} but got {}",
                        key, expected, value_type_name(val)
                    ));
                }
            }
            match val {
                Value::Object(_) => validate_fields(val, constraints, types, metadata),
                Value::Array(arr) => {
                    for item in arr.iter_mut() {
                        if let Value::Object(_) = item {
                            validate_fields(item, constraints, types, metadata);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Check if a value matches an expected type.
fn value_matches_type(value: &Value, expected_type: &str) -> bool {
    match expected_type {
        "int" => matches!(value, Value::Int(_)),
        "float" => matches!(value, Value::Float(_) | Value::Int(_)),
        "bool" => matches!(value, Value::Bool(_)),
        "string" => matches!(value, Value::String(_) | Value::Secret(_)),
        "array" => matches!(value, Value::Array(_)),
        "object" => matches!(value, Value::Object(_)),
        _ => true, // Unknown types are accepted
    }
}

/// Get the human-readable name of a value's type.
fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Bool(_) => "bool",
        Value::String(_) => "string",
        Value::Secret(_) => "secret",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::Null => "null",
    }
}

// ─── CLDR plural rules ───────────────────────────────────

/// Return the CLDR plural category for a given language and integer count.
/// Categories: "zero", "one", "two", "few", "many", "other".
fn plural_category(lang: &str, n: i64) -> &'static str {
    let abs_n = n.unsigned_abs();
    let n10 = abs_n % 10;
    let n100 = abs_n % 100;

    match lang {
        // East Slavic: Russian, Ukrainian, Belarusian
        "ru" | "uk" | "be" => {
            if n10 == 1 && n100 != 11 {
                "one"
            } else if (2..=4).contains(&n10) && !(12..=14).contains(&n100) {
                "few"
            } else {
                "many"
            }
        }
        // West/South Slavic: Polish
        "pl" => {
            if n10 == 1 && n100 != 11 {
                "one"
            } else if (2..=4).contains(&n10) && !(12..=14).contains(&n100) {
                "few"
            } else {
                "many"
            }
        }
        // Czech, Slovak
        "cs" | "sk" => {
            if abs_n == 1 { "one" }
            else if (2..=4).contains(&abs_n) { "few" }
            else { "other" }
        }
        // Arabic
        "ar" => {
            if abs_n == 0 { "zero" }
            else if abs_n == 1 { "one" }
            else if abs_n == 2 { "two" }
            else if (3..=10).contains(&n100) { "few" }
            else if (11..=99).contains(&n100) { "many" }
            else { "other" }
        }
        // French, Portuguese (Brazilian) — 0 and 1 are "one"
        "fr" | "pt" => {
            if abs_n <= 1 { "one" } else { "other" }
        }
        // Japanese, Chinese, Korean, Vietnamese, Thai — no plural forms
        "ja" | "zh" | "ko" | "vi" | "th" => "other",
        // English, German, Spanish, Italian, Dutch, Swedish, Norwegian, Danish, etc.
        _ => {
            if abs_n == 1 { "one" } else { "other" }
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_single_include_key_still_feeds_interpolation() {
        // `{key:include}` reads the only include when there is exactly one; the
        // pre-pass results for the marker must not count as a second one.
        let tmp = std::env::temp_dir().join("synx-include-single");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("common.synx"), "site_name Demo\n").unwrap();
        let mut r = parse("!active\ndb:include common.synx\ngreet {site_name:include}\n");
        let opts = Options { base_path: Some(tmp.to_string_lossy().into_owned()), ..Default::default() };
        resolve(&mut r, &opts);
        assert_eq!(r.root.as_object().unwrap()["greet"], Value::String("Demo".into()));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_clamp_bound_must_be_finite() {
        // `f64::clamp` panics on a NaN bound; a non-finite bound skips the marker.
        let mut r = parse("!active\nv:clamp:nan:5 3\nx:clamp:1:inf 7\ny:clamp:2:9 12");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["v"], Value::Int(3));
        assert_eq!(map["x"], Value::Int(7));
        assert_eq!(map["y"], Value::Int(9));
    }

    #[test]
    fn test_include_of_itself_stops_at_the_depth_cap() {
        // The `:include` pre-pass recursed without the depth cap and overflowed the stack.
        let tmp = std::env::temp_dir().join("synx-include-self");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("self.synx"), "!active\nme:include self.synx\n").unwrap();
        let text = std::fs::read_to_string(tmp.join("self.synx")).unwrap();
        let mut r = parse(&text);
        let opts = Options { base_path: Some(tmp.to_string_lossy().into_owned()), ..Default::default() };
        resolve(&mut r, &opts);
        let json = crate::to_json(&r.root);
        assert!(json.contains("INCLUDE_ERR: circular include: 'self.synx'"), "{}", json);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_double_self_include_is_cut_at_the_cycle() {
        // safety: two self-including keys used to resolve 2^17 times (22 s).
        let tmp = std::env::temp_dir().join("synx-include-bomb");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let text = "!active
a:include bomb.synx
b:include bomb.synx
c:include bomb.synx
";
        std::fs::write(tmp.join("bomb.synx"), text).unwrap();
        let started = std::time::Instant::now();
        let mut r = parse(text);
        let opts = Options { base_path: Some(tmp.to_string_lossy().into_owned()), ..Default::default() };
        resolve(&mut r, &opts);
        assert!(started.elapsed().as_secs() < 5);
        assert!(crate::to_json(&r.root).contains("circular include"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_empty_base_path_keeps_the_jail() {
        // safety: `synx parse --active file.synx` gave base "", which jailed nothing.
        let mut r = parse("!active
x:include ../outside/secret.synx
");
        let opts = Options { base_path: Some(String::new()), ..Default::default() };
        resolve(&mut r, &opts);
        let json = crate::to_json(&r.root);
        assert!(json.contains("INCLUDE_ERR: SECURITY"), "{}", json);
    }

    #[test]
    fn test_unset_secret_env_is_null_and_required_fires() {
        let mut env = std::collections::HashMap::new();
        env.insert("SET_ONE".to_string(), "v".to_string());
        let mut r = parse("!active
a:secret:env NOPE_UNSET
b[required]:secret:env NOPE_UNSET
c:secret:env SET_ONE
");
        resolve(&mut r, &Options { env: Some(env), ..Default::default() });
        let map = r.root.as_object().unwrap();
        assert_eq!(map["a"], Value::Null);
        assert_eq!(map["b"], Value::String("CONSTRAINT_ERR: 'b' is required".into()));
        assert!(matches!(map["c"], Value::Secret(ref s) if s == "v"));
    }

    #[test]
    fn test_required_key_without_value_is_missing() {
        let mut r = parse("!active
name[required, min:1]
");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["name"], Value::String("CONSTRAINT_ERR: 'name' is required".into()));
    }
    use crate::{parse, Options, Value};
    use super::{resolve, read_manifest_main};
    use std::sync::Mutex;

    // The :spam buckets are process-global, and clear_spam_buckets() wipes all of
    // them — so two spam tests running on different threads can clear each other's
    // bucket mid-test. Every spam test takes this lock for its whole body.
    pub(super) static SPAM_TESTS: Mutex<()> = Mutex::new(());

    // The spec's own example: `total` reads `tax`, which is itself a :calc.
    // Resolution follows document order, so this holds on every run — with a
    // HashMap it failed with CALC_ERR about half the time.
    #[test]
    fn test_calc_chain_follows_document_order() {
        for _ in 0..50 {
            let mut r = parse("!active
price 100
tax:calc price * 0.2
total:calc price + tax
");
            resolve(&mut r, &Options::default());
            let map = r.root.as_object().unwrap();
            assert_eq!(map["tax"], Value::Int(20));
            assert_eq!(map["total"], Value::Int(120));
        }
    }

    // A key of the current object shadows a same-named root key, as in the
    // TypeScript engine.
    #[test]
    fn test_calc_local_key_shadows_root() {
        let mut r = parse("!active
limit 5
svc
  limit 100
  price:calc limit * 2
  base:calc limit * 1
");
        resolve(&mut r, &Options::default());
        let svc = r.root.as_object().unwrap()["svc"].as_object().unwrap().clone();
        assert_eq!(svc["price"], Value::Int(200));
    }

    // Keys a word-token lookup cannot see still substitute exactly.
    #[test]
    fn test_calc_special_keys() {
        let mut r = parse("!active
max-hp 10
hp 3
left:calc max-hp - hp
n.m 4
both:calc n.m * hp
");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["left"], Value::Int(7));
        assert_eq!(map["both"], Value::Int(12));
    }

    // :alias to a key resolved by :env elsewhere sees the resolved value no
    // matter how many siblings precede it.
    #[test]
    fn test_alias_resolution_is_deterministic() {
        let mut src = String::from("!active
");
        for i in 0..40 {
            src.push_str(&format!("s{i}
  port:env:default:{} P{i}
", 7000 + i));
        }
        src.push_str("pick:alias s39.port
");
        for _ in 0..20 {
            let mut r = parse(&src);
            resolve(&mut r, &Options::default());
            assert_eq!(r.root.as_object().unwrap()["pick"], Value::Int(7039));
        }
    }

    #[test]
    fn test_ref_simple() {
        let mut r = parse("!active\nbase_rate 50\nquick_rate:ref base_rate");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["quick_rate"], Value::Int(50));
    }

    #[test]
    fn test_ref_calc_shorthand() {
        let mut r = parse("!active\nbase_rate 50\ndouble_rate:ref:calc:*2 base_rate");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["double_rate"], Value::Int(100));
    }

    #[test]
    fn test_inherit() {
        let mut r = parse("!active\n_base\n  weight 10\n  stackable true\nsteel:inherit:_base\n  weight 25\n  material metal");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert!(!map.contains_key("_base"));
        let steel = map["steel"].as_object().unwrap();
        assert_eq!(steel["weight"], Value::Int(25));
        assert_eq!(steel["stackable"], Value::Bool(true));
        assert_eq!(steel["material"], Value::String("metal".into()));
    }

    fn resolve_env(text: &str) -> crate::Map {
        let env: std::collections::HashMap<String, String> = [
            ("SNOW", "1550184245153112174"),
            ("ZIP", "007"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let mut r = parse(text);
        resolve(&mut r, &Options { env: Some(env), ..Default::default() });
        r.root.as_object().unwrap().clone()
    }

    // 3.7.4: `[type:string]` keeps env text a string. Before, the value was cast
    // first and the constraint then rejected its own declared type.
    #[test]
    fn test_env_type_string_constraint_keeps_text() {
        let map = resolve_env(
            "!active\nid[type:string]:env SNOW\nzip[type:string]:env ZIP\nfallback[type:string]:env:default:0042 MISSING\nplain:env ZIP\n",
        );
        assert_eq!(map["id"], Value::String("1550184245153112174".into()));
        assert_eq!(map["zip"], Value::String("007".into()));
        assert_eq!(map["fallback"], Value::String("0042".into()));
        assert_eq!(map["plain"], Value::Int(7));
    }

    #[test]
    fn test_string_length_constraint_counts_characters() {
        let map = resolve_env("!active\nshort[max:4] мир\nlong[max:4] мирок\n");
        assert_eq!(map["short"], Value::String("мир".into()));
        assert_eq!(
            map["long"],
            Value::String("CONSTRAINT_ERR: 'long' value 5 exceeds max 4".into())
        );
    }

    #[test]
    fn test_type_string_constraint_accepts_secret() {
        let map = resolve_env("!active\ntoken[type:string]:secret:env SNOW\n");
        assert_eq!(map["token"], Value::Secret("1550184245153112174".into()));
    }

    #[test]
    fn test_i18n_select_lang() {
        let mut r = parse("!active\ntitle:i18n\n  en Hello\n  ru Привет\n  de Hallo");
        let opts = Options { lang: Some("ru".into()), ..Default::default() };
        resolve(&mut r, &opts);
        let map = r.root.as_object().unwrap();
        assert_eq!(map["title"], Value::String("Привет".into()));
    }

    #[test]
    fn test_i18n_fallback_en() {
        let mut r = parse("!active\ntitle:i18n\n  en Hello\n  ru Привет");
        let opts = Options { lang: Some("fr".into()), ..Default::default() };
        resolve(&mut r, &opts);
        let map = r.root.as_object().unwrap();
        assert_eq!(map["title"], Value::String("Hello".into()));
    }

    #[test]
    fn test_auto_interpolation_simple() {
        let mut r = parse("!active\nname Wario\ngreeting Hello, {name}!");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["greeting"], Value::String("Hello, Wario!".into()));
    }

    #[test]
    fn test_auto_interpolation_nested() {
        let mut r = parse("!active\nserver\n  host localhost\n  port 8080\nurl http://{server.host}:{server.port}/api");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["url"], Value::String("http://localhost:8080/api".into()));
    }

    #[test]
    fn test_template_legacy_still_works() {
        let mut r = parse("!active\nname Wario\ngreeting:template Hello, {name}!");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["greeting"], Value::String("Hello, Wario!".into()));
    }

    // Non-ASCII literal text in a template must survive interpolation — the
    // byte-wise `bytes[i] as char` copy corrupted it (`мир` → `Ð¼Ð¸Ñ`).
    #[test]
    fn test_interpolation_preserves_non_ascii() {
        let mut r = parse("!active\ngreeting Привет\nmsg {greeting} мир\n");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["msg"], Value::String("Привет мир".into()));
    }

    // A `:secret` value must never reach a string sink as plaintext.
    #[test]
    fn test_secret_never_leaks_through_string_sinks() {
        let mut r = parse("!active\ntoken:secret abc123\nmsg hello {token} world\njoined:join\n  - {token}\n  - x\n");
        resolve(&mut r, &Options::default());
        let json = crate::to_json(&r.root);
        assert!(!json.contains("abc123"), "secret leaked into output: {json}");
        let map = r.root.as_object().unwrap();
        assert_eq!(map["msg"], Value::String("hello [SECRET] world".into()));
    }

    // A nested field interpolating a root-level secret must not observe the
    // plaintext — the :secret pre-pass masks literals before resolution.
    #[test]
    fn test_secret_not_leaked_via_nested_interpolation() {
        let mut r = parse("!active\ntoken:secret abc123\nsection\n  msg use {token} now\n");
        resolve(&mut r, &Options::default());
        let json = crate::to_json(&r.root);
        assert!(!json.contains("abc123"), "secret leaked: {json}");
        let section = r.root.as_object().unwrap()["section"].as_object().unwrap();
        assert_eq!(section["msg"], Value::String("use [SECRET] now".into()));
    }

    #[test]
    fn test_unique_dedupes_large_list() {
        let mut src = String::from("!active\nl:unique\n");
        for i in 0..5000 {
            src.push_str(&format!("  - v{}\n", i % 100));
        }
        let mut r = parse(&src);
        resolve(&mut r, &Options::default());
        let arr = r.root.as_object().unwrap()["l"].as_array().unwrap();
        assert_eq!(arr.len(), 100);
    }

    #[test]
    fn test_type_validation() {
        // Test that type validation works: hp(int) defined in _base_unit,
        // then used in other places with correct type
        let mut r = parse(
            "!active\n\
            _base_unit\n  \
              hp(int) 100\n  \
              speed(float) 1.5\n\
            infantry:inherit:_base_unit\n  \
              name Infantry\n  \
              hp 80"
        );
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        
        // _base_unit should be removed (private)
        assert!(!map.contains_key("_base_unit"));
        
        // infantry should exist with correct types
        let infantry = map["infantry"].as_object().unwrap();
        assert_eq!(infantry["hp"], Value::Int(80));  // Correct: int
        assert_eq!(infantry["speed"], Value::Float(1.5));  // Correct: float
    }

    #[test]
    fn test_type_validation_error() {
        // Test that type mismatch is detected and replaced with error
        let mut r = parse(
            "!active\n\
            _base_unit\n  \
              hp(int) 100\n\
            infantry:inherit:_base_unit\n  \
              hp hello"  // Type mismatch: string instead of int
        );
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        
        let infantry = map["infantry"].as_object().unwrap();
        // Should be replaced with error message
        if let Value::String(s) = &infantry["hp"] {
            assert!(s.contains("TYPE_ERR"));
        } else {
            panic!("Expected error string for type mismatch");
        }
    }

    #[test]
    fn test_constraint_validation_inherited_range() {
        let mut r = parse(
            "!active\n\
            _base_unit\n  \
              hp[min:1, max:50000] 1000\n\
            infantry:inherit:_base_unit\n  \
              hp 60000"
        );
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        let infantry = map["infantry"].as_object().unwrap();

        if let Value::String(s) = &infantry["hp"] {
            assert!(s.contains("CONSTRAINT_ERR"));
            assert!(s.contains("exceeds max"));
        } else {
            panic!("Expected constraint error string");
        }
    }

    #[test]
    fn test_constraint_validation_required() {
        let mut r = parse(
            "!active\n\
            _base_unit\n  \
                            description[type:string, required] hello\n\
            scout:inherit:_base_unit\n  \
                            description null"
        );
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        let scout = map["scout"].as_object().unwrap();

        if let Value::String(s) = &scout["description"] {
            assert!(s.contains("CONSTRAINT_ERR"));
            assert!(s.contains("required"));
        } else {
            panic!("Expected required-constraint error string");
        }
    }

    #[test]
    fn test_multi_parent_inherit() {
        let mut r = parse(
            "!active\n\
            _movable\n  \
              speed 10\n  \
              can_move true\n\
            _damageable\n  \
              hp 100\n  \
              armor 5\n\
            tank:inherit:_movable:_damageable\n  \
              name Tank\n  \
              armor 20"
        );
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();

        assert!(!map.contains_key("_movable"));
        assert!(!map.contains_key("_damageable"));

        let tank = map["tank"].as_object().unwrap();
        assert_eq!(tank["speed"], Value::Int(10));        // from _movable
        assert_eq!(tank["can_move"], Value::Bool(true));   // from _movable
        assert_eq!(tank["hp"], Value::Int(100));           // from _damageable
        assert_eq!(tank["armor"], Value::Int(20));         // child overrides _damageable's 5
        assert_eq!(tank["name"], Value::String("Tank".into()));
    }

    #[test]
    fn test_calc_dot_path() {
        let mut r = parse(
            "!active\n\
            stats\n  \
              base_hp 100\n  \
              multiplier 3\n\
            total_hp:calc stats.base_hp * stats.multiplier"
        );
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["total_hp"], Value::Int(300));
    }

    #[test]
    fn test_i18n_plural_en() {
        let mut r = parse(
            "!active\n\
            count 5\n\
            items:i18n:count\n  \
              en\n    \
                one item\n    \
                other items"
        );
        let opts = Options { lang: Some("en".into()), ..Default::default() };
        resolve(&mut r, &opts);
        let map = r.root.as_object().unwrap();
        assert_eq!(map["items"], Value::String("items".into()));
    }

    #[test]
    fn test_i18n_plural_en_one() {
        let mut r = parse(
            "!active\n\
            count 1\n\
            items:i18n:count\n  \
              en\n    \
                one {count} item\n    \
                other {count} items"
        );
        let opts = Options { lang: Some("en".into()), ..Default::default() };
        resolve(&mut r, &opts);
        let map = r.root.as_object().unwrap();
        assert_eq!(map["items"], Value::String("1 item".into()));
    }

    #[test]
    fn test_i18n_plural_ru() {
        let mut r = parse(
            "!active\n\
            count 3\n\
            items:i18n:count\n  \
              ru\n    \
                one предмет\n    \
                few предмета\n    \
                many предметов\n    \
                other предметов"
        );
        let opts = Options { lang: Some("ru".into()), ..Default::default() };
        resolve(&mut r, &opts);
        let map = r.root.as_object().unwrap();
        assert_eq!(map["items"], Value::String("предмета".into()));
    }

    #[test]
    fn test_quoted_null_preserved() {
        let r = parse("status \"null\"\nenabled \"true\"\ncount \"42\"");
        let map = r.root.as_object().unwrap();
        assert_eq!(map["status"], Value::String("null".into()));
        assert_eq!(map["enabled"], Value::String("true".into()));
        assert_eq!(map["count"], Value::String("42".into()));
    }

    #[test]
    fn test_unquoted_null_is_null() {
        let r = parse("status null\nenabled true\ncount 42");
        let map = r.root.as_object().unwrap();
        assert_eq!(map["status"], Value::Null);
        assert_eq!(map["enabled"], Value::Bool(true));
        assert_eq!(map["count"], Value::Int(42));
    }

    #[test]
    fn test_spam_rate_limit_exceeded() {
        let _serialized = SPAM_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        super::clear_spam_buckets();

        let mut r1 = parse("!active\nsecret_token abc\naccess:spam:1:5 secret_token");
        resolve(&mut r1, &Options::default());
        let map1 = r1.root.as_object().unwrap();
        assert_eq!(map1["access"], Value::String("abc".into()));

        let mut r2 = parse("!active\nsecret_token abc\naccess:spam:1:5 secret_token");
        resolve(&mut r2, &Options::default());
        let map2 = r2.root.as_object().unwrap();
        match &map2["access"] {
            Value::String(s) => assert!(s.starts_with("SPAM_ERR:")),
            _ => panic!("Expected SPAM_ERR string"),
        }
    }

    #[test]
    fn test_spam_default_window_sec_is_one() {
        let _serialized = SPAM_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        super::clear_spam_buckets();

        let mut r = parse("!active\na 1\nx:spam:2 a");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["x"], Value::Int(1));
    }

    #[test]
    fn test_deep_nesting_does_not_overflow() {
        // Deep indentation chain: parser caps nesting (see `MAX_PARSE_NESTING_DEPTH` in parser);
        // this test only checks resolve + navigation do not panic and yield a bounded tree.
        let mut synx = String::from("!active\n");
        let mut indent = String::new();
        for i in 0..200 {
            synx.push_str(&format!("{}level_{}\n", indent, i));
            indent.push_str("  ");
        }
        synx.push_str(&format!("{}value deep\n", indent));

        let mut result = parse(&synx);
        resolve(&mut result, &Default::default());
        assert!(matches!(result.root, Value::Object(_)));

        let mut cur = &result.root;
        let mut depth = 0usize;
        loop {
            let Value::Object(map) = cur else { break };
            let key = format!("level_{}", depth);
            match map.get(&key) {
                Some(next) => {
                    cur = next;
                    depth += 1;
                }
                None => break,
            }
        }
        assert!(
            depth >= 100,
            "expected at least 100 chained levels from parse, got {}",
            depth
        );
        assert!(
            depth <= 130,
            "parser nesting cap should keep chain shallow, got {}",
            depth
        );
    }

    #[test]
    fn test_circular_alias_returns_error() {
        let mut r = parse("!active\na:alias b\nb:alias a");
        resolve(&mut r, &Default::default());
        let root = r.root.as_object().unwrap();
        let a_val = root.get("a").unwrap();
        let b_val = root.get("b").unwrap();
        assert!(
            matches!(a_val, Value::String(s) if s.starts_with("ALIAS_ERR:")),
            "expected ALIAS_ERR for 'a', got: {:?}", a_val
        );
        assert!(
            matches!(b_val, Value::String(s) if s.starts_with("ALIAS_ERR:")),
            "expected ALIAS_ERR for 'b', got: {:?}", b_val
        );
    }

    #[test]
    fn test_self_alias_returns_error() {
        let mut r = parse("!active\na:alias a");
        resolve(&mut r, &Default::default());
        let root = r.root.as_object().unwrap();
        let a_val = root.get("a").unwrap();
        assert!(
            matches!(a_val, Value::String(s) if s.starts_with("ALIAS_ERR:")),
            "expected ALIAS_ERR for self-alias, got: {:?}", a_val
        );
    }

    #[test]
    fn test_valid_alias_still_works() {
        let mut r = parse("!active\nbase 42\ncopy:alias base");
        resolve(&mut r, &Default::default());
        let root = r.root.as_object().unwrap();
        assert_eq!(root.get("copy"), Some(&Value::Int(42)));
    }

    #[test]
    fn test_alias_to_string_valued_key_no_false_positive() {
        // 'a' holds a literal string "b". 'b' aliases 'a'.
        // b should resolve to "b" — NOT trigger ALIAS_ERR.
        let mut r = parse("!active\na b\nb:alias a");
        resolve(&mut r, &Default::default());
        let root = r.root.as_object().unwrap();
        assert_eq!(
            root.get("b"),
            Some(&Value::String("b".to_string())),
            "alias to a string-valued key should not produce ALIAS_ERR"
        );
    }

    #[test]
    fn test_prompt_marker() {
        let mut r = parse("!active\nmemory:prompt:Core\n  identity ASAI\n  creator APERTURESyndicate");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        if let Value::String(s) = &map["memory"] {
            assert!(s.starts_with("Core (SYNX):"));
            assert!(s.contains("```synx"));
            assert!(s.contains("identity ASAI"));
        } else {
            panic!("Expected :prompt to produce a string");
        }
    }

    #[test]
    fn test_vision_marker_passthrough() {
        let mut r = parse("!active\nimage:vision Generate a sunset");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["image"], Value::String("Generate a sunset".into()));
    }

    #[test]
    fn test_audio_marker_passthrough() {
        let mut r = parse("!active\nnarration:audio Read this summary aloud");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["narration"], Value::String("Read this summary aloud".into()));
    }

    #[test]
    fn test_use_directive_loads_package() {
        // Set up a temp package for `!use`
        let tmp = std::env::temp_dir().join("synx-use-test-load");
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg_dir = tmp.join("synx_packages/@test/config");
        std::fs::create_dir_all(pkg_dir.join("src")).unwrap();
        std::fs::write(pkg_dir.join("synx-pkg.synx"), "name @test/config\nversion 1.0.0\nmain src/main.synx\n").unwrap();
        std::fs::write(pkg_dir.join("src/main.synx"), "identity APERTURESyndicate\ndefault_port 8080\n").unwrap();

        let mut r = parse("!active\n!use @test/config\napp MyApp");
        let opts = Options {
            base_path: Some(tmp.to_string_lossy().into_owned()),
            ..Default::default()
        };
        resolve(&mut r, &opts);
        let map = r.root.as_object().unwrap();
        assert!(map.contains_key("config"), "package not loaded");
        let pkg = map["config"].as_object().unwrap();
        assert_eq!(pkg["identity"], Value::String("APERTURESyndicate".into()));
        assert_eq!(pkg["default_port"], Value::Int(8080));
        assert_eq!(map["app"], Value::String("MyApp".into()));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_use_directive_with_alias() {
        let tmp = std::env::temp_dir().join("synx-use-test-alias");
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg_dir = tmp.join("synx_packages/@test/config");
        std::fs::create_dir_all(pkg_dir.join("src")).unwrap();
        std::fs::write(pkg_dir.join("synx-pkg.synx"), "name @test/config\nversion 1.0.0\nmain src/main.synx\n").unwrap();
        std::fs::write(pkg_dir.join("src/main.synx"), "identity APERTURESyndicate\ndefault_port 8080\n").unwrap();

        let mut r = parse("!active\n!use @test/config as defaults\napp MyApp");
        let opts = Options {
            base_path: Some(tmp.to_string_lossy().into_owned()),
            ..Default::default()
        };
        resolve(&mut r, &opts);
        let map = r.root.as_object().unwrap();
        assert!(map.contains_key("defaults"), "aliased package not loaded");
        assert!(!map.contains_key("config"), "should use alias, not auto name");
        let pkg = map["defaults"].as_object().unwrap();
        assert_eq!(pkg["identity"], Value::String("APERTURESyndicate".into()));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_use_cannot_leave_the_packages_directory() {
        let tmp = std::env::temp_dir().join("synx-use-test-escape");
        let _ = std::fs::remove_dir_all(&tmp);
        // A SYNX file outside synx_packages that a document must not reach.
        std::fs::create_dir_all(tmp.join("secret/src")).unwrap();
        std::fs::write(tmp.join("secret/src/main.synx"), "token hunter2\n").unwrap();
        // A package whose manifest points its entry outside itself.
        let pkg_dir = tmp.join("synx_packages/@evil/pkg");
        std::fs::create_dir_all(pkg_dir.join("src")).unwrap();
        std::fs::write(pkg_dir.join("synx-pkg.synx"), "name @evil/pkg\nmain ../../../secret/src/main.synx\n").unwrap();
        std::fs::write(pkg_dir.join("src/main.synx"), "own yes\n").unwrap();

        let opts = Options { base_path: Some(tmp.to_string_lossy().into_owned()), ..Default::default() };
        let mut r = parse("!active\n!use @a/../../secret as leak\napp x");
        resolve(&mut r, &opts);
        assert!(!r.root.as_object().unwrap().contains_key("leak"), "package name escaped synx_packages");

        let mut r = parse("!active\n!use @evil/pkg as evil\napp x");
        resolve(&mut r, &opts);
        let evil = r.root.as_object().unwrap()["evil"].as_object().unwrap();
        assert!(!evil.contains_key("token"), "manifest main escaped the package");
        assert_eq!(evil["own"], Value::String("yes".into()));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_use_directive_missing_package_ignored() {
        let mut r = parse("!active\n!use @nonexistent/pkg\napp MyApp");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        // Missing package should be silently skipped
        assert!(!map.contains_key("pkg"));
        assert_eq!(map["app"], Value::String("MyApp".into()));
    }

    #[test]
    fn test_use_reads_manifest_main_field() {
        let tmp = std::env::temp_dir().join("synx-use-test-main");
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg_dir = tmp.join("synx_packages/@test/myapp");
        std::fs::create_dir_all(pkg_dir.join("src")).unwrap();
        std::fs::write(pkg_dir.join("synx-pkg.synx"), "name @test/myapp\nversion 1.0.0\nmain src/main.synx\n").unwrap();
        std::fs::write(pkg_dir.join("src/main.synx"), "app_name MyApp\nversion 2.0.0\n").unwrap();

        let mut r = parse("!active\n!use @test/myapp as myapp\napp Test");
        let opts = Options {
            base_path: Some(tmp.to_string_lossy().into_owned()),
            ..Default::default()
        };
        resolve(&mut r, &opts);
        let map = r.root.as_object().unwrap();
        assert!(map.contains_key("myapp"), "package not loaded via manifest");
        let pkg = map["myapp"].as_object().unwrap();
        assert_eq!(pkg["app_name"], Value::String("MyApp".into()));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_read_manifest_main_function() {
        let tmp = std::env::temp_dir().join("synx-manifest-main-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();
        std::fs::write(tmp.join("synx-pkg.synx"), "name @test/pkg\nversion 1.0.0\nmain src/main.synx\n").unwrap();
        std::fs::write(tmp.join("src/main.synx"), "key value\n").unwrap();

        let result = read_manifest_main(&tmp);
        assert!(result.is_some(), "should read main from synx-pkg.synx");
        let path = result.unwrap();
        assert!(path.ends_with("src/main.synx") || path.ends_with("src\\main.synx"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
mod hardening_tests {
    use super::*;
    use crate::{parse, to_json};

    fn env_of(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn run_in(text: &str, env: &[(&str, &str)], base: Option<&std::path::Path>) -> Map {
        let mut r = parse(text);
        let opts = Options {
            env: Some(env_of(env)),
            base_path: base.map(|p| p.to_string_lossy().into_owned()),
            ..Default::default()
        };
        resolve(&mut r, &opts);
        r.root.as_object().unwrap().clone()
    }

    fn run(text: &str, env: &[(&str, &str)]) -> Map {
        run_in(text, env, None)
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("synx-hardening-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn secret(text: &str) -> Value {
        Value::Secret(text.to_string())
    }

    // S5-02: a list of numbers and text made `sort_by` panic.
    #[test]
    fn sort_orders_numbers_before_text() {
        let m = run("!active\nl:sort\n  - 10\n  - 1a\n  - 9\n  - b\n  - 2\nd:sort:desc\n  - 10\n  - 1a\n  - 9\n", &[]);
        assert_eq!(to_json(&m["l"]), r#"[2,9,10,"1a","b"]"#);
        assert_eq!(to_json(&m["d"]), r#"["1a",10,9]"#);
        let mut doc = String::from("!active\nl:sort\n");
        let mut s: u64 = 0x9e3779b97f4a7c15;
        for _ in 0..600 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            match s % 3 {
                0 => doc.push_str(&format!("  - {}\n", s % 100)),
                1 => doc.push_str(&format!("  - {}a\n", s % 100)),
                _ => doc.push_str(&format!("  - {}.5\n", s % 50)),
            }
        }
        let m = run(&doc, &[]);
        let l = m["l"].as_array().unwrap();
        assert_eq!(l.len(), 600);
        let first_text = l.iter().position(|v| value_as_number(v).is_none()).unwrap();
        assert!(l[first_text..].iter().all(|v| value_as_number(v).is_none()));
        assert!(l[..first_text].windows(2).all(|w| value_as_number(&w[0]) <= value_as_number(&w[1])));
    }

    // S2-05: `:secret` masks what the rest of the chain produced.
    #[test]
    fn secret_is_the_last_step_of_its_chain() {
        let d = dir("secret-chain");
        std::fs::write(d.join("inc.synx"), "user bob\nport 5432\n").unwrap();
        let m = run_in(
            "!active\nonce:secret:once\ndef:secret:default:abc null\nfmt:secret:format:%05d 5\nhosts:secret:env:split H\nmissing:secret:watch nope.txt\nunset:secret:env NOPE\ninc:include:secret inc.synx\n",
            &[("H", "a,b")],
            Some(&d),
        );
        assert!(matches!(m["once"], Value::Secret(ref s) if s.len() == 36), "{:?}", m["once"]);
        assert_eq!(m["def"], secret("abc"));
        assert_eq!(m["fmt"], secret("00005"));
        assert_eq!(m["hosts"], Value::Array(vec![secret("a"), secret("b")]));
        assert_eq!(m["missing"], Value::String("WATCH_ERR: file not found: nope.txt".into()));
        assert_eq!(m["unset"], Value::Null);
        let inc = m["inc"].as_object().unwrap();
        assert_eq!(inc["user"], secret("bob"));
        assert_eq!(inc["port"], secret("5432"));
        let _ = std::fs::remove_dir_all(&d);

        // A group or a list under `:secret` keeps its shape.
        let m = run("!active\ncreds:secret\n  user bob\n  nested\n    pass hunter2\nkeys:secret\n  - k1\n  - 7\n", &[]);
        assert_eq!(to_json(&Value::Object(m.clone())), r#"{"creds":{"nested":{"pass":"[SECRET]"},"user":"[SECRET]"},"keys":["[SECRET]","[SECRET]"]}"#);
        assert_eq!(m["keys"], Value::Array(vec![secret("k1"), secret("7")]));
    }

    // B-017: a key above the secret must not read it before it is masked.
    #[test]
    fn a_secret_is_masked_before_its_turn() {
        let cases = [
            "!active\nleak:alias password\npassword:secret:default:x hunter2\n",
            "!active\nleak:ref password\npassword:secret:replace:q:z hunter2\n",
            "!active\na\n  x {password}\npassword:secret:replace:q:z hunter2\n",
            "!active\ne\n  x {creds.pass}\n  y:alias creds.pass\ncreds:secret\n  pass hunter2\n",
            "!active\npick:map:password\n  - hunter2 leaked\npassword:secret:format:%s hunter2\n",
            "!active\nj:join\n  - {password}\npassword:secret:split:| hunter2\n",
        ];
        for src in cases {
            let m = run(src, &[]);
            let json = to_json(&Value::Object(m));
            assert!(!json.contains("hunter2") && !json.contains("leaked"), "{}\n{}", src, json);
        }
        // The key's own chain still works on the real value.
        let m = run("!active\np:secret:replace:2:9 hunter2\nn:secret:clamp:1:5 9\ng:secret\n  url http://{host}\nhost h1\n", &[]);
        assert_eq!(m["p"], secret("hunter9"));
        assert_eq!(m["n"], secret("5"));
        assert_eq!(m["g"].as_object().unwrap()["url"], secret("http://h1"));
    }

    // A file included under `:secret` is masked wherever it can be read from.
    #[test]
    fn a_secret_include_is_masked_for_interpolation_too() {
        let d = dir("secret-include");
        std::fs::write(d.join("creds.synx"), "password hunter2\nuser bob\n").unwrap();
        let m = run_in(
            "!active\ndb:secret:include creds.synx\nx {password:db}\ny {password:include}\nz:secret:include 5\n",
            &[],
            Some(&d),
        );
        let json = to_json(&Value::Object(m.clone()));
        assert!(!json.contains("hunter2") && !json.contains("bob"), "{}", json);
        assert_eq!(m["x"], Value::String("[SECRET]".into()));
        assert_eq!(m["y"], Value::String("[SECRET]".into()));
        assert_eq!(m["db"].as_object().unwrap()["password"], secret("hunter2"));
        // A value that is not a path is still a value under `:secret`.
        assert_eq!(m["z"], secret("5"));
        let _ = std::fs::remove_dir_all(&d);
    }

    // S1-05: a secret from the environment is its text.
    #[test]
    fn a_secret_from_the_environment_keeps_its_text() {
        let env = [("S1", "00123"), ("S2", "1e5"), ("S3", "+5"), ("S4", "true"), ("S5", "\"quoted\""), ("S6", "null")];
        let m = run(
            "!active\na:secret:env S1\nb:secret:env S2\nc:secret:env S3\nd:secret:env S4\ne:secret:env S5\nf:secret:env S6\ng:secret:env:default:007 NOPE\nh:secret:default:007 null\nlit:secret 007\nplain:env S1\n",
            &env,
        );
        assert_eq!(m["a"], secret("00123"));
        assert_eq!(m["b"], secret("1e5"));
        assert_eq!(m["c"], secret("+5"));
        assert_eq!(m["d"], secret("true"));
        assert_eq!(m["e"], secret("quoted"));
        assert_eq!(m["f"], Value::Null);
        assert_eq!(m["g"], secret("007"));
        assert_eq!(m["h"], secret("007"));
        // A literal is cast by the parser, as before.
        assert_eq!(m["lit"], secret("7"));
        assert_eq!(m["plain"], Value::Int(123));
    }

    // S1-14 / B-015: `inf` and `nan` are text.
    #[test]
    fn non_finite_numbers_from_outside_are_text() {
        let m = run(
            "!active\na:env V1\nb:env V2\nc:env V3\nd:env V4\nl:split Nan, Ina\ns:sum\n  - 1\n  - inf\n  - nan\n",
            &[("V1", "inf"), ("V2", "NaN"), ("V3", "1e999"), ("V4", "1e5")],
        );
        assert_eq!(m["a"], Value::String("inf".into()));
        assert_eq!(m["b"], Value::String("NaN".into()));
        assert_eq!(m["c"], Value::String("1e999".into()));
        assert_eq!(m["d"], Value::Float(100000.0));
        assert_eq!(to_json(&m["l"]), r#"["Nan","Ina"]"#);
        assert_eq!(m["s"], Value::Int(1));
    }

    // B-010: `:round` does not invent an integer.
    #[test]
    fn round_keeps_what_does_not_fit_an_integer() {
        let m = run(
            "!active\nx(float):round nan\ny(float):round inf\nz:round 1000000000000000000000000000000.5\nn:round 2.5\nm:round -2.5\nd:round:2 3.14159\nh:round:400 1.5\n",
            &[],
        );
        assert!(matches!(m["x"], Value::Float(f) if f.is_nan()));
        assert!(matches!(m["y"], Value::Float(f) if f.is_infinite()));
        assert_eq!(m["z"], Value::Float(1e30));
        assert_eq!(m["n"], Value::Int(3));
        assert_eq!(m["m"], Value::Int(-3));
        assert_eq!(m["d"], Value::Float(3.14));
        assert_eq!(m["h"], Value::Float(1.5));
    }

    // S1-11: copies are charged to the document.
    #[test]
    fn copies_stop_at_the_budget() {
        let mut doc = String::from("!active\na0\n  x 1\n");
        for i in 1..=26 {
            doc.push_str(&format!("a{}\n  l:alias a{}\n  r:ref a{}\n", i, i - 1, i - 1));
        }
        let m = run(&doc, &[]);
        assert_eq!(to_json(&m["a2"]), r#"{"l":{"l":{"x":1},"r":{"x":1}},"r":{"l":{"x":1},"r":{"x":1}}}"#);
        assert_eq!(m["a26"].as_object().unwrap()["l"], Value::String(LIMIT_ERR.into()));
        assert_eq!(m["a26"].as_object().unwrap()["r"], Value::String(LIMIT_ERR.into()));

        // `:inherit`: 1 500 children of a 1 500-field parent.
        let mut doc = String::from("!active\n_p\n");
        for i in 0..1500 {
            doc.push_str(&format!("  f{} {}\n", i, i));
        }
        for i in 0..1500 {
            doc.push_str(&format!("c{}:inherit:_p\n  own 1\n", i));
        }
        let m = run(&doc, &[]);
        assert_eq!(m["c0"].as_object().unwrap().len(), 1501);
        assert_eq!(m["c1499"], Value::String(LIMIT_ERR.into()));

        // Text: one large value named from many keys, and the multipliers.
        let mut doc = format!("!active\nbig {}\n", "x".repeat(1024 * 1024));
        for i in 0..100 {
            doc.push_str(&format!("k{} {{big}}\n", i));
        }
        let m = run(&doc, &[]);
        assert_eq!(m["k0"].as_str().unwrap().len(), 1024 * 1024);
        assert_eq!(m["k99"], Value::String(LIMIT_ERR.into()));

        let doc = format!(
            "!active\nok:replace:a:bb aaa\nr:replace:a:{} {}\nj:join:{}\n  - 1\n  - 2\n",
            "b".repeat(40_000),
            "a".repeat(40_000),
            "s".repeat(4096)
        );
        let m = run(&doc, &[]);
        assert_eq!(m["ok"], Value::String("bbbbbb".into()));
        assert_eq!(m["r"], Value::String(LIMIT_ERR.into()));
        assert_eq!(m["j"], Value::String(LIMIT_ERR.into()));
    }

    // A chain of aliases nests one level per key; it stops at the depth cap.
    #[test]
    fn copies_stop_at_the_depth_cap() {
        let mut doc = String::from("!active\na0\n  x 1\n");
        for i in 1..=600 {
            doc.push_str(&format!("a{}\n  c:alias a{}\n", i, i - 1));
        }
        let m = run(&doc, &[]);
        assert!(matches!(m["a100"].as_object().unwrap()["c"], Value::Object(_)));
        assert!(matches!(m["a510"].as_object().unwrap()["c"], Value::Object(_)));
        assert_eq!(m["a511"].as_object().unwrap()["c"], Value::String(NESTING_ERR.into()));
        // What follows copies the error, not five hundred more levels.
        assert!(matches!(m["a600"].as_object().unwrap()["c"], Value::Object(_)));
    }

    // Packages that `!use` each other used to recurse until the stack ended.
    #[test]
    fn packages_using_each_other_end() {
        let d = dir("use-cycle");
        for (a, b, c) in [("a", "b", "c"), ("b", "c", "a"), ("c", "a", "b")] {
            let p = d.join(format!("synx_packages/@x/{}/src", a));
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("main.synx"), format!("!active\n!use @x/{}\n!use @x/{}\nv {}\n", b, c, a)).unwrap();
        }
        let mut r = parse("!active\n!use @x/a\nk 1\n");
        let opts = Options {
            env: Some(HashMap::new()),
            base_path: Some(d.to_string_lossy().into_owned()),
            packages_path: Some(d.join("synx_packages").to_string_lossy().into_owned()),
            ..Default::default()
        };
        resolve(&mut r, &opts);
        let root = r.root.as_object().unwrap();
        assert_eq!(root["a"].as_object().unwrap()["v"], Value::String("a".into()));
        let _ = std::fs::remove_dir_all(&d);
    }

    // The same large file named from many keys is read until the budget ends.
    #[test]
    fn file_reads_have_a_budget() {
        let d = dir("read-budget");
        std::fs::write(d.join("big.synx"), format!("k {}\n", "v".repeat(8 * 1024 * 1024 - 16))).unwrap();
        let mut doc = String::from("!active\n");
        for i in 0..12 {
            doc.push_str(&format!("w{}:watch big.synx\n", i));
        }
        let m = run_in(&doc, &[], Some(&d));
        assert!(matches!(m["w0"], Value::String(ref s) if s.len() > 8_000_000));
        assert_eq!(m["w11"], Value::String(format!("WATCH_ERR: read budget ({} bytes) exceeded", MAX_RESOLVE_READ_BYTES)));
        let _ = std::fs::remove_dir_all(&d);
    }

    // `:once` reads the lock file once and writes it once.
    #[test]
    fn once_keeps_its_values_and_respects_read_only() {
        let d = dir("once");
        let mut doc = String::from("!active\n");
        for i in 0..2000 {
            doc.push_str(&format!("k{}:once\n", i));
        }
        let first = run_in(&doc, &[], Some(&d));
        let lock = std::fs::read_to_string(d.join(".synx.lock")).unwrap();
        assert_eq!(lock.lines().count(), 2000);
        assert!(lock.starts_with(&format!("k0 {}\n", first["k0"].as_str().unwrap())));
        let second = run_in(&doc, &[], Some(&d));
        assert_eq!(first, second);

        // Read-only: recorded values are used, new ones are not written.
        let mut r = parse("!active\nk0:once\nfresh:once\n");
        let opts = Options { base_path: Some(d.to_string_lossy().into_owned()), ..Default::default() };
        resolve_read_only(&mut r, &opts);
        let root = r.root.as_object().unwrap();
        assert_eq!(root["k0"], first["k0"]);
        assert!(matches!(root["fresh"], Value::String(ref s) if s.len() == 36));
        assert_eq!(std::fs::read_to_string(d.join(".synx.lock")).unwrap(), lock);
        let _ = std::fs::remove_dir_all(&d);

        let empty = dir("once-ro");
        let mut r = parse("!active\nid:once\n");
        resolve_read_only(&mut r, &Options { base_path: Some(empty.to_string_lossy().into_owned()), ..Default::default() });
        assert!(!empty.join(".synx.lock").exists());
        let _ = std::fs::remove_dir_all(&empty);
    }

    // B-065: the lock file is shared, and nobody's line is lost.
    #[test]
    fn once_survives_concurrent_writers() {
        let d = dir("once-many");
        let base = d.to_string_lossy().into_owned();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|t| {
                let base = base.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let mut out = Vec::new();
                    for round in 0..25 {
                        // A key of its own, one every thread asks for in this
                        // round, and one every thread asks for every time.
                        let own = format!("own_{}_{}", t, round);
                        let contested = format!("round_{}", round);
                        let mut r = parse(&format!("!active\nshared:once\n{}:once\n{}:once\n", own, contested));
                        resolve(&mut r, &Options { base_path: Some(base.clone()), env: Some(HashMap::new()), ..Default::default() });
                        let root = r.root.as_object().unwrap();
                        for key in [own.as_str(), contested.as_str(), "shared"] {
                            out.push((key.to_string(), root[key].as_str().unwrap().to_string()));
                        }
                    }
                    out
                })
            })
            .collect();
        let given: Vec<(String, String)> = handles.into_iter().flat_map(|h| h.join().unwrap()).collect();
        // What the file says now is what every thread was given.
        let lock = std::fs::read_to_string(d.join(".synx.lock")).unwrap();
        let mut file: HashMap<&str, &str> = HashMap::new();
        for line in lock.lines() {
            let (k, v) = line.split_once(' ').unwrap();
            file.entry(k).or_insert(v);
        }
        assert_eq!(file.len(), 1 + 8 * 25 + 25);
        for (k, v) in &given {
            assert_eq!(file[k.as_str()], v.as_str(), "{}", k);
        }
        let _ = std::fs::remove_dir_all(&d);

        // A file someone edited by hand, with no line break at its end.
        let d = dir("once-open-line");
        std::fs::write(d.join(".synx.lock"), "a 1\nb 2").unwrap();
        let m = run_in("!active\nb:once\nc:once\n", &[], Some(&d));
        assert_eq!(m["b"], Value::String("2".into()));
        let lock = std::fs::read_to_string(d.join(".synx.lock")).unwrap();
        assert_eq!(lock, format!("a 1\nb 2\nc {}\n", m["c"].as_str().unwrap()));
        let _ = std::fs::remove_dir_all(&d);
    }

    // B-027/B-028: a secret is checked by its text.
    #[test]
    fn constraints_read_a_secret_by_its_text() {
        let m = run(
            "!active\na[min:10]:secret:env K\nb[enum:x|live_key]:secret:env K\nc[pattern:^sk_]:secret:env K\nd[max:20, pattern:^live]:secret:env K\n",
            &[("K", "live_key")],
        );
        assert_eq!(m["a"], Value::String("CONSTRAINT_ERR: 'a' value 8 is below min 10".into()));
        assert_eq!(m["b"], secret("live_key"));
        assert_eq!(m["c"], Value::String("CONSTRAINT_ERR: 'c' does not match pattern /^sk_/".into()));
        assert_eq!(m["d"], secret("live_key"));
    }

    // S1-04: the classes a pattern is usually written with are enforced.
    #[test]
    fn patterns_with_classes_are_enforced() {
        let m = run(
            "!active\na[pattern:^\\d+$] abc\nb[pattern:^\\d+$] \"123\"\nc[pattern:^\\w+$] a-b\nd[pattern:(?i)^abc$] ABC\ne[pattern:(?i)^abc$] zzz\nf[pattern:^\\p{L}+$] мир\ng[pattern:\\bfoo\\b] food\n",
            &[],
        );
        assert_eq!(m["a"], Value::String("CONSTRAINT_ERR: 'a' does not match pattern /^\\d+$/".into()));
        assert_eq!(m["b"], Value::String("123".into()));
        assert!(matches!(m["c"], Value::String(ref s) if s.starts_with("CONSTRAINT_ERR")));
        assert_eq!(m["d"], Value::String("ABC".into()));
        assert!(matches!(m["e"], Value::String(ref s) if s.starts_with("CONSTRAINT_ERR")));
        assert_eq!(m["f"], Value::String("мир".into()));
        assert!(matches!(m["g"], Value::String(ref s) if s.starts_with("CONSTRAINT_ERR")));
    }

    // The same failure reads the same on every platform.
    #[test]
    fn read_errors_are_portable() {
        let d = dir("io-errors");
        std::fs::create_dir_all(d.join("sub")).unwrap();
        let m = run_in(
            "!active\na:include nope.synx\nb:include sub\nc:include sub/../nope.synx\nd:include ../outside.synx\ne:watch nope.txt\nf:include a|b.synx\n",
            &[],
            Some(&d),
        );
        assert_eq!(m["a"], Value::String("INCLUDE_ERR: file not found: nope.synx".into()));
        assert_eq!(m["b"], Value::String("INCLUDE_ERR: not a file: sub".into()));
        assert_eq!(m["c"], Value::String("INCLUDE_ERR: SECURITY: path traversal detected: 'sub/../nope.synx'".into()));
        assert_eq!(m["d"], Value::String("INCLUDE_ERR: SECURITY: path traversal detected: '../outside.synx'".into()));
        assert_eq!(m["e"], Value::String("WATCH_ERR: file not found: nope.txt".into()));
        assert_eq!(m["f"], Value::String("INCLUDE_ERR: file not found: a|b.synx".into()));
        let _ = std::fs::remove_dir_all(&d);
    }

    // The table of `:spam` buckets does not grow with the documents parsed.
    #[test]
    fn spam_buckets_are_bounded() {
        let _serialized = tests::SPAM_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        clear_spam_buckets();
        for i in 0..(MAX_SPAM_BUCKETS + 500) {
            assert!(allow_spam_access(&format!("k{}::{}", i, "t".repeat(if i % 2 == 0 { 8 } else { 1000 })), 1_000_000, 3600));
        }
        let buckets = SPAM_BUCKETS.get().unwrap().lock().unwrap();
        assert!(buckets.len() <= MAX_SPAM_BUCKETS);
        assert!(buckets.keys().all(|k| k.len() <= MAX_SPAM_KEY_BYTES));
        drop(buckets);
        clear_spam_buckets();
    }

    /// The substitution `:calc` did before 3.8.1, kept as the definition of
    /// its result: every numeric key, in order, replaced as a whole word.
    fn reference_calc(expr: &str, local: &Map, self_key: &str, root: &Map) -> Option<String> {
        let local_is_root = std::ptr::eq(root, local);
        let mut resolved = expr.to_string();
        let maps = [Some(local), if local_is_root { None } else { Some(root) }];
        for m in maps.into_iter().flatten() {
            for (rk, rv) in m {
                if std::ptr::eq(m, local) && rk == self_key {
                    continue;
                }
                if let Some(n) = value_as_number(rv) {
                    resolved = replace_word(&resolved, rk, &format_number(n));
                    if resolved.len() > MAX_CALC_RESOLVED_LEN {
                        return None;
                    }
                }
            }
        }
        Some(resolved)
    }

    #[test]
    fn calc_substitution_matches_the_sequential_definition() {
        let keys = ["a", "b", "hp", "max-hp", "max", "n.m", "n", "m", "2", "x1", "цена", "a-b", "inf", "NaN", "1.5", "10-3", "k_1", "-"];
        let pieces = ["a", "b", "hp", "max-hp", "max", "n.m", "2", "x1", "цена", "a-b", " ", "+", "-", "*", "(", ")", "1.5", "10", "k_1", "inf", ".", "3", "m", "n"];
        let mut s: u64 = 0x2545f4914f6cdd1d;
        let mut next = move |n: usize| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s % n as u64) as usize
        };
        for round in 0..4000 {
            let mut root = Map::default();
            let mut local = Map::default();
            for map in [&mut root, &mut local] {
                for _ in 0..next(9) {
                    let k = keys[next(keys.len())];
                    let v = match next(6) {
                        0 => Value::String("text".into()),
                        1 => Value::Float([1.5, -2.25, f64::INFINITY, f64::NAN][next(4)]),
                        2 => Value::Int(-3),
                        _ => Value::Int(next(40) as i64),
                    };
                    map.insert(k.to_string(), v);
                }
            }
            let mut expr = String::new();
            for _ in 0..(1 + next(9)) {
                expr.push_str(pieces[next(pieces.len())]);
            }
            let self_key = keys[next(keys.len())];
            // The key being computed holds its expression, as in a document.
            local.insert(self_key.to_string(), Value::String(expr.clone()));
            let mut root_as_local = root.clone();
            root_as_local.insert(self_key.to_string(), Value::String(expr.clone()));
            // Once as an object under the root, once as the root itself.
            let scope = CalcScope::of(&Value::Object(root.clone()));
            let got = substitute_calc_vars(&expr, &local, self_key, Some(&root), &scope, &LocalCalc::default());
            assert_eq!(got, reference_calc(&expr, &local, self_key, &root), "round {} expr {:?} local {:?} root {:?}", round, expr, local, root);
            let root = root_as_local;
            let scope = CalcScope::of(&Value::Object(root.clone()));
            let got = substitute_calc_vars(&expr, &root, self_key, Some(&root), &scope, &LocalCalc::default());
            assert_eq!(got, reference_calc(&expr, &root, self_key, &root), "round {} (root) expr {:?} root {:?}", round, expr, root);
        }
    }

    /// The interpolation of 3.8.0, kept as the definition of the result.
    fn reference_interpolation(tpl: &str, root: &Value, local_map: &Map, includes: &Map) -> String {
        let bytes = tpl.as_bytes();
        let len = bytes.len();
        let mut result = String::new();
        let mut i = 0;
        while i < len {
            if result.len() >= MAX_ENGINE_SCRATCH_STRING {
                break;
            }
            if bytes[i] == b'{' {
                if let Some(close) = tpl[i + 1..].find('}') {
                    let inner = &tpl[i + 1..i + 1 + close];
                    let (ref_name, resolved) = if let Some(colon) = inner.find(':') {
                        let ref_name = &inner[..colon];
                        let scope = &inner[colon + 1..];
                        let found = if scope == "include" {
                            if includes.len() == 1 { deep_ref(includes.values().next().unwrap(), ref_name) } else { None }
                        } else {
                            includes.get(scope).and_then(|inc| deep_ref(inc, ref_name))
                        };
                        (ref_name, found)
                    } else {
                        (inner, deep_ref(root, inner).or_else(|| local_map.get(inner)))
                    };
                    if ref_name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                        match resolved {
                            Some(val) => result.push_str(&value_to_string(val)),
                            None => {
                                result.push('{');
                                result.push_str(inner);
                                result.push('}');
                            }
                        }
                        i += 2 + close;
                        continue;
                    }
                }
            }
            let ch = tpl[i..].chars().next().unwrap();
            result.push(ch);
            i += ch.len_utf8();
        }
        result
    }

    #[test]
    fn interpolation_matches_the_previous_scanner() {
        let root = parse("a 1\nb\n  c text\nname Wario\nключ мир\nempty\n").root;
        let local = parse("a 2\nloc here\n").root.as_object().unwrap().clone();
        let mut includes = Map::default();
        includes.insert("inc".to_string(), parse("a 9\nd\n  e deep\n").root);
        let pieces = [
            "{", "}", "a", ":", "x", ".", "{a}", "{a:inc}", "{b.c}", "{d.e:inc}", "é", " ", "{loc}", "{a:include}", "{ключ}", "{:", "{}", "{{", "}}", "inc", "{name", "c", "_", "{empty}", "{b}", "{a:}", "{a:zz}",
        ];
        let mut s: u64 = 0x9e3779b97f4a7c15;
        let mut next = move |n: usize| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s % n as u64) as usize
        };
        for round in 0..20_000 {
            let mut tpl = String::new();
            for _ in 0..(1 + next(12)) {
                tpl.push_str(pieces[next(pieces.len())]);
            }
            let incs = if round % 3 == 0 { Map::default() } else { includes.clone() };
            let got = resolve_interpolation(&tpl, &root, &local, &incs, usize::MAX);
            assert_eq!(got, Some(reference_interpolation(&tpl, &root, &local, &incs)), "{:?}", tpl);
        }
        // The 4 MiB cap cuts as it did.
        let big = Value::Object([("v".to_string(), Value::String("я".repeat(1_500_000)))].into_iter().collect());
        let tpl = "ж{v}{v}tail";
        assert_eq!(
            resolve_interpolation(tpl, &big, &Map::default(), &Map::default(), usize::MAX).unwrap(),
            reference_interpolation_capped(tpl, &big)
        );
        // And a text that would pass `cap` is refused.
        assert_eq!(resolve_interpolation(tpl, &big, &Map::default(), &Map::default(), 1024), None);
    }

    /// 3.8.0 with its length cap, for one template of one name.
    fn reference_interpolation_capped(tpl: &str, root: &Value) -> String {
        let mut result = String::new();
        let mut rest = tpl;
        while !rest.is_empty() && result.len() < MAX_ENGINE_SCRATCH_STRING {
            if let Some(after) = rest.strip_prefix("{v}") {
                let s = value_to_string(deep_ref(root, "v").unwrap());
                let room = MAX_ENGINE_SCRATCH_STRING.saturating_sub(result.len());
                let end = s.floor_char_boundary(s.len().min(room));
                result.push_str(&s[..end]);
                rest = after;
            } else {
                let ch = rest.chars().next().unwrap();
                result.push(ch);
                rest = &rest[ch.len_utf8()..];
            }
        }
        result
    }

    /// A document for the pre-pass tests: groups, lists of scalars and of
    /// objects, keys with a dot, repeated keys, `:secret` anywhere.
    fn secret_document(next: &mut impl FnMut() -> u64, depth: usize, indent: usize, out: &mut String) {
        let pad = " ".repeat(indent);
        let width = 1 + (next() % 5) as usize;
        for _ in 0..width {
            let name = ["a", "b", "c", "x.y", "d\\e", "pass"][(next() % 6) as usize];
            let secret = if next() % 3 == 0 { ":secret" } else { "" };
            match next() % if depth < 3 { 6 } else { 2 } {
                0 => out.push_str(&format!("{pad}{name}{secret} v{}\n", next() % 9)),
                1 => out.push_str(&format!("{pad}{name}{secret}:default:d\n")),
                2 | 3 => {
                    out.push_str(&format!("{pad}{name}{secret}\n"));
                    secret_document(next, depth + 1, indent + 2, out);
                }
                4 => {
                    out.push_str(&format!("{pad}{name}{secret}\n{pad}  - one\n{pad}  - 2\n"));
                }
                _ => {
                    out.push_str(&format!("{pad}{name}{secret}\n"));
                    for _ in 0..1 + next() % 3 {
                        let head = if next() % 2 == 0 { ":secret" } else { "" };
                        out.push_str(&format!("{pad}  - token{head} t{}\n", next() % 9));
                        secret_document(next, depth + 1, indent + 4, out);
                    }
                }
            }
        }
    }

    // The secret pre-pass follows the declarations to their objects instead
    // of walking the document; both must mask the same values and keep the
    // same originals — after inheritance too, which files copied declarations.
    #[test]
    fn the_secret_pre_pass_finds_what_the_walk_found() {
        let mut seed = 0x243F_6A88_85A3_08D3u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut with_secrets = 0;
        for round in 0..4000 {
            let mut text = String::from("!active\n");
            secret_document(&mut next, 0, 0, &mut text);
            if round % 4 == 0 {
                text.push_str("child:inherit:a\n  own:secret 1\nother:inherit:pass\n  z 2\n");
            }
            let parsed = parser::parse(&text);
            let _scope = ResolveScope::enter(true);
            let mut root = parsed.root;
            let mut metadata = parsed.metadata;
            apply_inheritance(&mut root, &mut metadata);

            let mut by_walk = root.clone();
            let mut walk_originals = SecretOriginals::default();
            mask_secret_literals(&mut by_walk, &metadata, &mut String::new(), &mut walk_originals);
            let mut by_paths = root.clone();
            let mut path_originals = SecretOriginals::default();
            mask_declared_secrets(&mut by_paths, &metadata, &mut path_originals);
            assert_eq!(by_paths, by_walk, "masked differently:\n{}", text);
            assert_eq!(path_originals, walk_originals, "kept different originals:\n{}", text);
            with_secrets += usize::from(!walk_originals.is_empty());
        }
        assert!(with_secrets > 2000, "the generator stopped producing secrets: {}", with_secrets);
    }

    #[test]
    fn a_metadata_path_leads_to_its_object() {
        let mut root = parser::parse("a\n  b\n    c 1\nl\n  - k 1\n    in\n      deep 2\n  - plain\nx.y\n  z 3\ns 5\n").root;
        let at = |root: &mut Value, path: &str| object_at_path_mut(root, path).map(|m| m.keys().cloned().collect::<Vec<_>>());
        assert_eq!(at(&mut root, ""), Some(vec!["a".into(), "l".into(), "x.y".into(), "s".into()]));
        assert_eq!(at(&mut root, "a.b"), Some(vec!["c".to_string()]));
        assert_eq!(at(&mut root, "l.#0"), Some(vec!["k".into(), "in".into()]));
        assert_eq!(at(&mut root, "l.#0.in"), Some(vec!["deep".to_string()]));
        assert_eq!(at(&mut root, "x\\.y"), Some(vec!["z".to_string()]));
        // A scalar, a list itself, an item that is not an object, a missing
        // key, an index past the end: no object there.
        for path in ["s", "l", "l.#1", "l.#7", "l.k", "a.nope", "x.y", "a.b.c", "l.#0.in.deep.more"] {
            assert_eq!(at(&mut root, path), None, "{}", path);
        }
    }

    // `:env` and `:default` cast what they are given without copying it first.
    #[test]
    fn values_from_outside_are_cast_as_before() {
        let doc = "!active\nport:env:default:8080 PORT\nhost:env:default:0.0.0.0 HOST\nflag:env FLAG\nname:env NAME\nratio:env:default:1.5 R\nurl:env:default:http://x:1/y U\ncode(string):env CODE\nkey:secret:env KEY\nempty:env:default NOPE\ngone:env NOPE\nplain:default:7 \"\"\nwide:default:a:b:c \"\"\nbare:default:7\n";
        let m = run(doc, &[("PORT", "7101"), ("FLAG", "true"), ("NAME", "svc"), ("CODE", "007"), ("KEY", "00123")]);
        assert_eq!(m["port"], Value::Int(7101));
        assert_eq!(m["host"], Value::String("0.0.0.0".into()));
        assert_eq!(m["flag"], Value::Bool(true));
        assert_eq!(m["name"], Value::String("svc".into()));
        assert_eq!(m["ratio"], Value::Float(1.5));
        assert_eq!(m["url"], Value::String("http://x:1/y".into()));
        assert_eq!(m["code"], Value::String("007".into()));
        assert_eq!(m["key"], secret("00123"));
        assert_eq!(m["empty"], Value::Null);
        assert_eq!(m["gone"], Value::Null);
        assert_eq!(m["plain"], Value::Int(7));
        assert_eq!(m["wide"], Value::String("a:b:c".into()));
        // A key without a value is an empty group, which `:default` leaves alone.
        assert_eq!(m["bare"], Value::Object(Map::default()));
    }

    // B-017.6 / B-017.7: what is inherited from a group under `:secret` is
    // secret in the child as well — masked before any key can read it, and
    // resolved like the field it is.
    #[test]
    fn a_secret_group_stays_secret_in_its_heirs() {
        let m = run("!active\nchild:inherit base\nbase:secret\n  p hunter2\n", &[]);
        assert_eq!(to_json_of(&m), r#"{"base":{"p":"[SECRET]"},"child":{"p":"[SECRET]"}}"#);
        assert_eq!(m["child"].as_object().unwrap()["p"], secret("hunter2"));

        // Declared before the child, and read through another key.
        let m = run("!active\ncreds:secret\n  pass hunter2\nc:inherit creds\nleak:alias c.pass\ntext {c.pass}\n", &[]);
        let json = to_json_of(&m);
        assert!(!json.contains("hunter2"), "{json}");
        assert_eq!(m["leak"], secret("hunter2"));
        assert_eq!(m["text"], Value::String("[SECRET]".into()));

        // The field's own markers still run, and a nested group is masked whole.
        let doc = "!active\nbase:secret\n  port:env:default:5 PORT\n  inner\n    token abc\nchild:inherit:base\n  own 1\n";
        let m = run(doc, &[("PORT", "7101")]);
        let child = m["child"].as_object().unwrap();
        assert_eq!(child["port"], secret("7101"));
        assert_eq!(child["inner"].as_object().unwrap()["token"], secret("abc"));
        assert_eq!(child["own"], Value::Int(1));

        // A child of the child inherits the mark with the field.
        let m = run("!active\nbase:secret\n  p hunter2\nc1:inherit base\nc2:inherit c1\n", &[]);
        assert!(!to_json_of(&m).contains("hunter2"));
        assert_eq!(m["c2"].as_object().unwrap()["p"], secret("hunter2"));

        // A group that is not secret hands its fields on as they are.
        let m = run("!active\nbase\n  p open\nchild:inherit base\n", &[]);
        assert_eq!(m["child"].as_object().unwrap()["p"], Value::String("open".into()));
    }

    fn to_json_of(map: &Map) -> String {
        crate::to_json(&Value::Object(map.clone()))
    }

    // B-030.6: the cost of a copy is measured only as far as the budget
    // reaches, so a document that has spent it answers each further copy at
    // once instead of walking the value again.
    #[test]
    fn a_copy_is_measured_only_within_the_budget() {
        let doc = parser::parse("g\n  a text\n  b\n    - 1\n    - two\n  c\n    d\n      e 5\ns x\n");
        for value in [&doc.root, &doc.root.as_object().unwrap()["g"], &doc.root.as_object().unwrap()["s"]] {
            let (cost, nesting) = value_cost(value);
            assert_eq!(value_cost_within(value, cost), Some((cost, nesting)));
            assert_eq!(value_cost_within(value, cost + 1000), Some((cost, nesting)));
            assert_eq!(value_cost_within(value, cost - 1), None);
            assert_eq!(value_cost_within(value, 0), None);
        }

        // 3000 fields, 6000 aliases: the budget (64 MiB at 128 a value) is
        // spent by the 175th copy; the other 5825 must not each cost a walk.
        let mut text = String::from("!active\ng\n");
        for i in 0..3000 {
            text.push_str(&format!("  f{} {}\n", i, i));
        }
        for i in 0..6000 {
            text.push_str(&format!("k{}:alias g\n", i));
        }
        let started = std::time::Instant::now();
        let m = run(&text, &[]);
        assert_eq!(m["k0"].as_object().unwrap().len(), 3000);
        assert_eq!(m["k5999"], Value::String(LIMIT_ERR.into()));
        let refused = m.values().filter(|v| **v == Value::String(LIMIT_ERR.into())).count();
        assert!(refused > 5000, "{refused}");
        assert!(started.elapsed() < std::time::Duration::from_secs(20), "{:?}", started.elapsed());
    }

    // The budget is asked before the depth: a copy that fits neither is a
    // `LIMIT_ERR`, as every copy is once the budget is spent.
    #[test]
    fn a_spent_budget_answers_before_the_depth_cap() {
        let _scope = ResolveScope::enter(true);
        let leaf = Value::String("x".into());
        assert_eq!(budgeted_copy(&leaf, MAX_RESOLVE_DEPTH), Value::String(NESTING_ERR.into()));
        assert!(!charge_expansion(usize::MAX));
        assert_eq!(budgeted_copy(&leaf, MAX_RESOLVE_DEPTH), Value::String(LIMIT_ERR.into()));
        assert_eq!(budgeted_copy(&leaf, 0), Value::String(LIMIT_ERR.into()));
    }

    // B-088: the error `:spam` leaves names its target; under `:secret` the
    // target is the secret.
    #[test]
    fn a_spam_error_does_not_quote_a_secret() {
        clear_spam_buckets();
        let doc = "!active\nsx1_spam_secret:spam:1:60:secret hunter2\nsx1_spam_open:spam:1:60 visible\n";
        let first = run(doc, &[]);
        assert_eq!(first["sx1_spam_secret"], secret("hunter2"));
        let second = run(doc, &[]);
        assert_eq!(second["sx1_spam_secret"], Value::String("SPAM_ERR: '[SECRET]' exceeded 1 calls per 60s".into()));
        assert_eq!(second["sx1_spam_open"], Value::String("SPAM_ERR: 'visible' exceeded 1 calls per 60s".into()));
        clear_spam_buckets();
    }

    // The mask of a value is built beside it instead of from a copy of it;
    // the result is the copy, masked.
    #[test]
    fn a_masked_copy_is_the_copy_masked() {
        let doc = parser::parse(
            "a text\nn 5\nf 1.5\nb true\nz null\nerr \"CALC_ERR: x\"\ng\n  k v\n  l\n    - 1\n    - two\n    - in\n      deep x\n  e\nl\n  - a\n  - 2\n",
        );
        let mut values: Vec<Value> = doc.root.as_object().unwrap().values().cloned().collect();
        values.push(doc.root.clone());
        values.push(Value::Secret("kept".into()));
        values.push(Value::String("INCLUDE_ERR: file not found: x".into()));
        for value in values {
            let mut want = value.clone();
            mask_secret_value(&mut want);
            assert_eq!(masked_copy(&value), want, "{:?}", value);
        }
    }

    // A field's constraints are found through its declaration, not copied
    // from it; a field declared twice is still held to both, and a document
    // that inherits is checked as it was.
    #[test]
    fn constraints_by_field_name_hold_without_copies() {
        // One declaration, applied to every field of the name.
        let m = run("!active\nport[type:int, min:1, max:9] 5\na\n  port 50\nb\n  port ok\nl\n  - port 7\n    x 1\n", &[]);
        assert_eq!(m["port"], Value::Int(5));
        assert!(matches!(&m["a"].as_object().unwrap()["port"], Value::String(s) if s.starts_with("CONSTRAINT_ERR:")));
        assert!(matches!(&m["b"].as_object().unwrap()["port"], Value::String(s) if s.starts_with("CONSTRAINT_ERR:")));
        assert_eq!(m["l"].as_array().unwrap()[0].as_object().unwrap()["port"], Value::Int(7));

        // Declared twice: the stricter bounds of the two, the first type.
        let m = run("!active\na\n  n[min:2, max:50] 10\nb\n  n[min:5, max:90] 3\nc\n  n 60\nd\n  n 20\n", &[]);
        assert_eq!(m["a"].as_object().unwrap()["n"], Value::Int(10));
        assert!(matches!(&m["b"].as_object().unwrap()["n"], Value::String(s) if s.starts_with("CONSTRAINT_ERR:")));
        assert!(matches!(&m["c"].as_object().unwrap()["n"], Value::String(s) if s.starts_with("CONSTRAINT_ERR:")));
        assert_eq!(m["d"].as_object().unwrap()["n"], Value::Int(20));

        // With inheritance the registry is taken before the declarations move.
        let m = run("!active\nbase\n  hp[min:1, max:100] 50\nchild:inherit:base\n  hp 500\nother\n  hp 7\n", &[]);
        assert_eq!(m["base"].as_object().unwrap()["hp"], Value::Int(50));
        assert!(matches!(&m["child"].as_object().unwrap()["hp"], Value::String(s) if s.starts_with("CONSTRAINT_ERR:")));
        assert_eq!(m["other"].as_object().unwrap()["hp"], Value::Int(7));
    }

    // B-089: a `.synx.lock` that is a link leads out of the document's
    // folder; `:once` neither reads through it nor writes through it.
    #[test]
    fn once_leaves_a_linked_lock_file_alone() {
        let base = dir("sx1_once_link_base");
        let outside = dir("sx1_once_link_outside").join("target.txt");
        std::fs::write(&outside, "token s3cr3t-outside\n").unwrap();
        let link = base.join(".synx.lock");
        let _ = std::fs::remove_file(&link);
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&outside, &link);
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&outside, &link);
        if made.is_err() {
            // No right to create links on this machine: nothing to test.
            return;
        }
        let m = run_in("!active\ntoken:once\nx;touch:once\n", &[], Some(&base));
        // Not the line of the file the link names, and nothing added to it.
        assert_ne!(m["token"], Value::String("s3cr3t-outside".into()));
        assert!(matches!(&m["token"], Value::String(s) if s.len() == 36), "{:?}", m["token"]);
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "token s3cr3t-outside\n");
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());

        // A folder in the lock file's place is left alone as well.
        let base = dir("sx1_once_dir_base");
        let _ = std::fs::create_dir(base.join(".synx.lock"));
        let m = run_in("!active\nid:once\n", &[], Some(&base));
        assert!(matches!(&m["id"], Value::String(s) if s.len() == 36));
        assert!(base.join(".synx.lock").is_dir());
    }
}
