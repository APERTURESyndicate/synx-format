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

    let base_canonical = match std::fs::canonicalize(base) {
        Ok(p) => p,
        Err(_) => std::path::PathBuf::from(base),
    };
    let full = base_canonical.join(file_path);
    let full_canonical = match std::fs::canonicalize(&full) {
        Ok(p) => p,
        Err(_) => {
            // File may not exist yet — at least verify no ".." escapes.
            let normalized = full.to_string_lossy();
            if normalized.contains("..") {
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
/// directory"). Other kinds fall through to the platform-specific message.
fn fmt_io_err(e: &std::io::Error, ctx: &str) -> String {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::NotFound => format!("file not found: {}", ctx),
        ErrorKind::PermissionDenied => format!("permission denied: {}", ctx),
        _ => e.to_string(),
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
    if result.mode != Mode::Active {
        return;
    }
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
    if let Value::Object(ref root_map) = result.root {
        for (key, _) in root_map.iter() {
            let meta = match metadata.get("").and_then(|mm| mm.get(key)) {
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
            let text = match std::fs::read_to_string(&full) {
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
            preresolved.insert(
                key.clone(),
                Value::Array(vec![Value::String(path.clone()), included.root.clone()]),
            );
            includes_map.entry(key.clone()).or_insert(included.root);
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
    let mut constraint_registry: Map<Constraints> = Map::default();
    collect_registries(&result.root, &metadata, &mut String::new(), &mut type_registry, &mut constraint_registry, 0);

    // ── :inherit pre-pass ──
    apply_inheritance(&mut result.root, &mut metadata);
    // Remove private blocks (keys starting with _)
    if let Value::Object(ref mut root_map) = result.root {
        root_map.retain(|k, _| !k.starts_with('_'));
    }

    // ── :secret pre-pass ──
    // safety: mask literal `:secret` values *before* any resolution so a field
    // that interpolates `{that_key}` can never observe the plaintext. Marker
    // application runs after child recursion (depth-first), so a nested field
    // referencing a root-level secret would otherwise read it unmasked. Only
    // plain values are pre-masked here; a secret that itself needs resolving
    // (`:env:secret`, etc.) is left for its own marker to mask in place.
    mask_secret_literals(&mut result.root, &metadata, &mut String::new());


    // SAFETY: `root_ptr` is a raw pointer to `result.root` used exclusively
    // for *immutable* read access inside marker handlers (:calc, :alias,
    // :map, :watch) that need to look up other keys in the root
    // while also holding a mutable reference to a child object.
    // The invariants that keep this sound:
    //   1. We never write through `root_ptr` — only reads via `&*root_ptr`.
    //   2. Mutable writes go through `map` (the current object), which is
    //      always a distinct subtree from what we read via `root_ptr`.
    //   3. The pointer is valid for the entire duration of `resolve_value`.
    let root_ptr = &mut result.root as *mut Value;
    let calc_scope = CalcScope::of(&result.root);
    let mut path = String::new();
    resolve_value(&mut result.root, root_ptr, options, &metadata, &mut path, &includes_map, &preresolved, &calc_scope, 0);

    // ── Validate field constraints, then types (global, by field name) ──
    // perf: one walk for both; per field the order is the same as two walks.
    validate_fields(&mut result.root, &constraint_registry, &type_registry);
    
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

fn resolve_value(
    value: &mut Value,
    root_ptr: *mut Value,
    options: &Options,
    metadata: &Map<MetaMap>,
    path: &mut String,
    includes: &Map,
    preresolved: &Map,
    calc_scope: &CalcScope,
    depth: usize,
) {
    // Guard: prevent stack overflow from deeply nested objects
    if depth >= MAX_RESOLVE_DEPTH {
        // Safety: recursion only descends into Object variants (see lines below),
        // so value is always an Object here. Non-Object values are safe to skip.
        if let Value::Object(ref mut map) = value {
            for val in map.values_mut() {
                *val = Value::String(
                    "NESTING_ERR: maximum object nesting depth exceeded".to_string()
                );
            }
        }
        return;
    }

    let Value::Object(ref mut map) = value else { return };

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
    for i in 0..map.len() {
        if let Some((key, child)) = map.get_index_mut(i) {
            if matches!(child, Value::Object(_) | Value::Array(_)) {
                if base_len > 0 {
                    path.push('.');
                }
                push_key_segment(path, key);
                match child {
                    Value::Object(_) => {
                        resolve_value(child, root_ptr, options, metadata, path, includes, preresolved, calc_scope, depth + 1);
                    }
                    Value::Array(arr) => {
                        let list_len = path.len();
                        for (idx, item) in arr.iter_mut().enumerate() {
                            if let Value::Object(_) = item {
                                push_item_segment(path, idx);
                                resolve_value(item, root_ptr, options, metadata, path, includes, preresolved, calc_scope, depth + 1);
                                path.truncate(list_len);
                            }
                        }
                    }
                    _ => {}
                }
                path.truncate(base_len);
            }
        }
        let Some(mm) = mm else { continue };
        let Some((k, _)) = map.get_index(i) else { break };
        let Some(meta) = mm.get(k.as_str()) else { continue };
        let key = k.clone();
        apply_markers(map, &key, meta, root_ptr, options, path, metadata, preresolved, calc_scope);
    }

    // Third pass: auto-{} interpolation on all string values
    for i in 0..map.len() {
        let replaced = match map.get_index(i) {
            Some((_, Value::String(s))) if memchr::memchr(b'{', s.as_bytes()).is_some() => {
                let root_ref = unsafe { &*root_ptr };
                let result = resolve_interpolation(s, root_ref, map, includes);
                if result != *s { Some(result) } else { None }
            }
            _ => None,
        };
        if let Some(result) = replaced {
            if let Some((_, v)) = map.get_index_mut(i) {
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

fn apply_markers(
    map: &mut Map,
    key: &str,
    meta: &Meta,
    root_ptr: *mut Value,
    options: &Options,
    path: &str,
    metadata: &Map<MetaMap>,
    preresolved: &Map,
    calc_scope: &CalcScope,
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
            map.insert(
                key.to_string(),
                Value::String("SPAM_ERR: invalid limit, use :spam:MAX[:WINDOW_SEC]".to_string()),
            );
            return;
        }

        let target = map
            .get(key)
            .map(value_to_string)
            .unwrap_or_else(|| key.to_string());
        let bucket_key = format!("{}::{}", key, target);

        if !allow_spam_access(&bucket_key, max_calls, window_sec) {
            map.insert(
                key.to_string(),
                Value::String(format!(
                    "SPAM_ERR: '{}' exceeded {} calls per {}s",
                    target, max_calls, window_sec
                )),
            );
            return;
        }

        if let Some(resolved) = map
            .get(key)
            .and_then(|v| {
                let t = value_to_string(v);
                let root_ref = unsafe { &*root_ptr };
                deep_get(root_ref, &t).or_else(|| map.get(t.as_str()).cloned())
            })
        {
            map.insert(key.to_string(), resolved);
        }
    }

    // ── :include / :import ──
    if (mb & mk::INCLUDE != 0) || (mb & mk::IMPORT != 0) {
        if let Some(Value::String(file_path)) = map.get(key) {
            let max_depth = options.max_include_depth.unwrap_or(DEFAULT_MAX_INCLUDE_DEPTH);
            if options._include_depth >= max_depth {
                map.insert(
                    key.to_string(),
                    Value::String(format!("INCLUDE_ERR: max include depth ({}) exceeded", max_depth)),
                );
                return;
            }
            if path.is_empty() {
                if let Some(Value::Array(pre)) = preresolved.get(key) {
                    if let [Value::String(pre_path), root] = pre.as_slice() {
                        if pre_path == file_path {
                            let root = root.clone();
                            map.insert(key.to_string(), root);
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
                    map.insert(key.to_string(), Value::String(format!("INCLUDE_ERR: {}", e)));
                    return;
                }
            };
            if let Err(e) = check_file_size(&full) {
                map.insert(key.to_string(), Value::String(format!("INCLUDE_ERR: {}", e)));
                return;
            }
            match std::fs::read_to_string(&full) {
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
                    map.insert(key.to_string(), included.root);
                }
                Err(e) => {
                    map.insert(
                        key.to_string(),
                        Value::String(format!("INCLUDE_ERR: {}", fmt_io_err(&e, file_path))),
                    );
                }
            }
        }
        return;
    }

    // ── :env ──
    if mb & mk::ENV != 0 {
        if let Some(Value::String(var_name)) = map.get(key) {
            let env_val = if let Some(ref env_map) = options.env {
                env_map.get(var_name.as_str()).cloned()
            } else {
                std::env::var(var_name).ok()
            };

            let force_string = wants_string(meta);
            let default_idx = markers.iter().position(|m| m == "default");
            if let Some(val) = env_val.filter(|v| !v.is_empty()) {
                let resolved = if force_string {
                    Value::String(val)
                } else {
                    cast_primitive(&val)
                };
                map.insert(key.to_string(), resolved);
            } else if let Some(di) = default_idx {
                if markers.len() > di + 1 {
                    // Join all parts after 'default' back with ':'
                    // to preserve IPs (0.0.0.0) and compound values
                    let fallback = markers[di + 1..].join(":");
                    let resolved = if force_string {
                        Value::String(fallback)
                    } else {
                        cast_primitive(&fallback)
                    };
                    map.insert(key.to_string(), resolved);
                } else {
                    map.insert(key.to_string(), Value::Null);
                }
            } else {
                map.insert(key.to_string(), Value::Null);
            }
        }
    }

    // ── :random ──
    if mb & mk::RANDOM != 0 {
        if let Some(Value::Array(arr)) = map.get(key) {
            if arr.is_empty() {
                map.insert(key.to_string(), Value::Null);
                return;
            }
            let picked = if !meta.args.is_empty() {
                let weights: Vec<f64> = meta.args.iter().filter_map(|s| s.parse().ok()).collect();
                weighted_random(arr, &weights)
            } else {
                arr[rng::random_usize(arr.len())].clone()
            };
            map.insert(key.to_string(), picked);
        }
    }

    // ── :ref ──
    // Like :alias but feeds the resolved value into subsequent markers.
    // Supports :ref:calc shorthand: key:ref:calc:*2 base_rate → resolves base_rate, then applies "VALUE * 2".
    if mb & mk::REF != 0 {
        if let Some(Value::String(target)) = map.get(key) {
            let root_ref = unsafe { &*root_ptr };
            let resolved = deep_get(root_ref, target)
                .or_else(|| map.get(target.as_str()).cloned())
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
                                    map.insert(key.to_string(), v);
                                }
                                Err(e) => {
                                    map.insert(key.to_string(), Value::String(format!("CALC_ERR: {}", e)));
                                }
                            }
                        } else {
                            map.insert(key.to_string(), resolved);
                        }
                    } else {
                        map.insert(key.to_string(), resolved);
                    }
                } else {
                    map.insert(key.to_string(), resolved);
                }
            } else {
                map.insert(key.to_string(), resolved);
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
        if let Some(Value::Object(translations)) = map.get(key) {
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
                let count_val = map.get(cf)
                    .and_then(value_as_number)
                    .or_else(|| {
                        let root_ref = unsafe { &*root_ptr };
                        deep_get(root_ref, cf).and_then(|v| value_as_number(&v))
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
                    map.insert(key.to_string(), Value::String(replaced));
                } else {
                    map.insert(key.to_string(), chosen);
                }
            } else {
                map.insert(key.to_string(), val);
            }
        }
    }

    // ── :calc ──
    if mb & mk::CALC != 0 {
        if let Some(Value::String(expr)) = map.get(key) {
            if expr.len() > MAX_CALC_EXPR_LEN {
                map.insert(
                    key.to_string(),
                    Value::String(format!("CALC_ERR: expression too long ({} chars, max {})", expr.len(), MAX_CALC_EXPR_LEN)),
                );
                return;
            }
            let root_ref = unsafe { &*root_ptr };
            let root_map = match root_ref {
                Value::Object(m) => Some(m),
                _ => None,
            };
            let mut resolved = match substitute_calc_vars(expr, map, key, root_map, calc_scope) {
                Some(r) => r,
                None => {
                    map.insert(
                        key.to_string(),
                        Value::String(format!(
                            "CALC_ERR: resolved expression too long (max {} bytes)",
                            MAX_CALC_RESOLVED_LEN
                        )),
                    );
                    return;
                }
            };

            // Substitute dot-path references (e.g., base.hp, server.port)
            let root_ref2 = unsafe { &*root_ptr };
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
                        if let Some(val) = deep_get(root_ref2, token) {
                            if let Some(n) = value_as_number(&val) {
                                dot_resolved.push_str(&format_number(n));
                                if dot_resolved.len() > MAX_CALC_RESOLVED_LEN {
                                    map.insert(
                                        key.to_string(),
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
                        map.insert(
                            key.to_string(),
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
                        map.insert(
                            key.to_string(),
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
                    map.insert(key.to_string(), v);
                }
                Err(e) => {
                    map.insert(
                        key.to_string(),
                        Value::String(format!("CALC_ERR: {}", e)),
                    );
                }
            }
        }
    }

    // ── :alias ──
    if mb & mk::ALIAS != 0 {
        if let Some(Value::String(target)) = map.get(key) {
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
                map.insert(
                    key.to_string(),
                    Value::String(format!("ALIAS_ERR: self-referential alias: {} → {}", current_path, target)),
                );
            } else {
                // Detect one-hop cycle: a → b → a
                // Only flag as cycle if the target key ALSO has an :alias marker.
                // Without this check, plain string values that happen to match the current
                // key name would produce false-positive ALIAS_ERR results.
                let root_ref = unsafe { &*root_ptr };
                let target_val = deep_get(root_ref, &target);
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
                    map.insert(
                        key.to_string(),
                        Value::String(format!("ALIAS_ERR: circular alias detected: {} → {}", a, b)),
                    );
                } else {
                    let val = target_val.unwrap_or(Value::Null);
                    map.insert(key.to_string(), val);
                }
            }
        }
    }

    // ── :secret ──
    if mb & mk::SECRET != 0 {
        // Idempotent: the :secret pre-pass may already have wrapped a literal
        // secret. `value_to_string` masks a Secret to "[SECRET]", so re-wrapping
        // would replace the stored value with the mask itself.
        match map.get(key) {
            Some(Value::Secret(_)) => {}
            Some(val) => {
                let s = value_to_string(val);
                map.insert(key.to_string(), Value::Secret(s));
            }
            None => {}
        }
    }

    // ── :unique ──
    if mb & mk::UNIQUE != 0 {
        if let Some(Value::Array(arr)) = map.get(key) {
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
            map.insert(key.to_string(), Value::Array(unique));
        }
    }

    // ── :geo ──
    if mb & mk::GEO != 0 {
        if let Some(Value::Array(arr)) = map.get(key) {
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
            map.insert(key.to_string(), result);
        }
    }

    // ── :template (legacy — handled by auto-{} in resolve_value) ──

    // ── :split ──
    if mb & mk::SPLIT != 0 {
        if let Some(Value::String(s)) = map.get(key) {
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
            map.insert(key.to_string(), Value::Array(items));
        }
    }

    // ── :join ──
    if mb & mk::JOIN != 0 {
        if let Some(Value::Array(arr)) = map.get(key) {
            let join_idx = markers.iter().position(|m| m == "join").unwrap();
            let sep = if join_idx + 1 < markers.len() {
                delimiter_from_keyword(&markers[join_idx + 1])
            } else {
                ",".to_string()
            };
            let joined: String = arr
                .iter()
                .map(|v| value_to_string(v))
                .collect::<Vec<_>>()
                .join(&sep);
            map.insert(key.to_string(), Value::String(joined));
        }
    }

    // ── :default (standalone, without :env) ──
    if (mb & mk::DEFAULT != 0) && !(mb & mk::ENV != 0) {
        let is_empty = match map.get(key) {
            Some(Value::Null) | None => true,
            Some(Value::String(s)) if s.is_empty() => true,
            _ => false,
        };
        if is_empty {
            let di = markers.iter().position(|m| m == "default").unwrap();
            if markers.len() > di + 1 {
                let fallback = markers[di + 1..].join(":");
                let resolved = if wants_string(meta) {
                    Value::String(fallback)
                } else {
                    cast_primitive(&fallback)
                };
                map.insert(key.to_string(), resolved);
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
                map.insert(key.to_string(), Value::String(
                    format!("CONSTRAINT_ERR: clamp min ({}) > max ({})", lo, hi),
                ));
            } else if let Some(n) = map.get(key).and_then(value_as_number) {
                let clamped = n.clamp(lo, hi);
                let v = if clamped.fract() == 0.0 && clamped.abs() < i64::MAX as f64 {
                    Value::Int(clamped as i64)
                } else {
                    Value::Float(clamped)
                };
                map.insert(key.to_string(), v);
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
        if let Some(n) = map.get(key).and_then(value_as_number) {
            let factor = 10f64.powi(decimals as i32);
            let rounded = (n * factor).round() / factor;
            let v = if decimals == 0 {
                Value::Int(rounded as i64)
            } else {
                Value::Float(rounded)
            };
            map.insert(key.to_string(), v);
        }
    }

    // ── :map ──
    // Syntax: key:map:source_key\n  - lookup_val result
    // Looks up `source_key` in root, finds matching "lookup_val result" entry in the array.
    if mb & mk::MAP != 0 {
        if let Some(Value::Array(arr)) = map.get(key) {
            let map_idx = markers.iter().position(|m| m == "map").unwrap();
            let source_key = markers.get(map_idx + 1).cloned().unwrap_or_default();
            let lookup_val = if !source_key.is_empty() {
                let root_ref = unsafe { &*root_ptr };
                deep_get(root_ref, &source_key)
                    .or_else(|| map.get(&source_key).cloned())
                    .map(|v| value_to_string(&v))
                    .unwrap_or_default()
            } else {
                // Use the current string value as lookup key
                match map.get(key) {
                    Some(Value::String(s)) => s.clone(),
                    _ => String::new(),
                }
            };

            // Find matching entry: "lookup_val result_text"
            let arr_clone = arr.clone();
            let result = arr_clone.iter().find_map(|item| {
                if let Value::String(s) = item {
                    if let Some(space) = s.find(' ') {
                        if s[..space].trim() == lookup_val {
                            return Some(cast_primitive(s[space + 1..].trim()));
                        }
                    }
                }
                None
            });
            map.insert(key.to_string(), result.unwrap_or(Value::Null));
        }
    }

    // ── :format ──
    // Syntax: key:format:PATTERN value  (printf-style: %.2f, %d, %05d, %e)
    // Converts numeric or string value to a formatted string.
    if mb & mk::FORMAT != 0 {
        let fmt_idx = markers.iter().position(|m| m == "format").unwrap();
        let pattern = markers.get(fmt_idx + 1).cloned().unwrap_or_else(|| "%s".to_string());
        if let Some(current) = map.get(key) {
            let formatted = apply_format_pattern(&pattern, current);
            map.insert(key.to_string(), Value::String(formatted));
        }
    }

    // ── :replace:FROM:TO ──    (since 3.6.2)
    // Literal substring replacement on a string value. `TO` defaults to "" (deletion).
    // `FROM`/`TO` cannot contain ':' because the marker chain is colon-delimited;
    // for those cases use `{interpolation}` instead.
    if mb & mk::REPLACE != 0 {
        if let Some(Value::String(s)) = map.get(key) {
            let idx = markers.iter().position(|m| m == "replace").unwrap();
            let from = markers.get(idx + 1).cloned().unwrap_or_default();
            let to = markers.get(idx + 2).cloned().unwrap_or_default();
            if !from.is_empty() {
                let replaced = s.replace(&from, &to);
                map.insert(key.to_string(), Value::String(replaced));
            }
        }
    }

    // ── :sort  /  :sort:desc ──    (since 3.6.2)
    // Sort an array. Numeric items compare numerically; otherwise lexicographic.
    if mb & mk::SORT != 0 {
        if let Some(Value::Array(arr)) = map.get(key) {
            let idx = markers.iter().position(|m| m == "sort").unwrap();
            let desc = matches!(markers.get(idx + 1).map(|s| s.as_str()), Some("desc"));
            let mut sorted = arr.clone();
            sorted.sort_by(|a, b| {
                match (value_as_number(a), value_as_number(b)) {
                    (Some(an), Some(bn)) => an
                        .partial_cmp(&bn)
                        .unwrap_or(std::cmp::Ordering::Equal),
                    _ => value_to_string(a).cmp(&value_to_string(b)),
                }
            });
            if desc { sorted.reverse(); }
            map.insert(key.to_string(), Value::Array(sorted));
        }
    }

    // ── :sum ──    (since 3.6.2)
    // Sum the numeric items of an array. Non-numeric items are ignored.
    // Returns Int when all summands are integers, Float otherwise.
    if mb & mk::SUM != 0 {
        if let Some(Value::Array(arr)) = map.get(key) {
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
                        if let Ok(f) = s.parse::<f64>() {
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
            map.insert(key.to_string(), result);
        }
    }

    // ── :fallback ──
    // Syntax: key:fallback:DEFAULT_PATH value
    // If the value (treated as a file path) doesn't exist on disk, use the fallback.
    // Falls back to default if value is also null/empty.
    if mb & mk::FALLBACK != 0 {
        let fb_idx = markers.iter().position(|m| m == "fallback").unwrap();
        let default_val = markers.get(fb_idx + 1).cloned().unwrap_or_default();
        let use_fallback = match map.get(key) {
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
            map.insert(key.to_string(), Value::String(default_val));
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

        // Try to read existing value from lock file
        let existing = read_lock_value(&lock_path, key);
        if let Some(locked) = existing {
            map.insert(key.to_string(), Value::String(locked));
        } else {
            let generated = match gen_type {
                "uuid" => rng::generate_uuid(),
                "timestamp" => std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    .to_string(),
                "random" => rng::random_usize(u32::MAX as usize).to_string(),
                _ => rng::generate_uuid(),
            };
            write_lock_value(&lock_path, key, &generated);
            map.insert(key.to_string(), Value::String(generated));
        }
    }

    // ── :version ──
    // Syntax: key:version:OP:REQUIRED value
    // Compares the value (current version) against REQUIRED using OP (>=, <=, >, <, ==, !=).
    // Returns a bool.
    if mb & mk::VERSION != 0 {
        if let Some(Value::String(current_ver)) = map.get(key) {
            let ver_idx = markers.iter().position(|m| m == "version").unwrap();
            let op = markers.get(ver_idx + 1).map(|s| s.as_str()).unwrap_or(">=");
            let required = markers.get(ver_idx + 2).cloned().unwrap_or_default();
            let result = compare_versions(current_ver, op, &required);
            map.insert(key.to_string(), Value::Bool(result));
        }
    }

    // ── :watch ──
    // Syntax: key:watch:KEY_PATH ./file.json  (or ./file.synx)
    // Reads the referenced file at parse time. Optionally extracts a key path (JSON/SYNX).
    if mb & mk::WATCH != 0 {
        if let Some(Value::String(file_path)) = map.get(key) {
            let max_depth = options.max_include_depth.unwrap_or(DEFAULT_MAX_INCLUDE_DEPTH);
            if options._include_depth >= max_depth {
                map.insert(
                    key.to_string(),
                    Value::String(format!("WATCH_ERR: max include depth ({}) exceeded", max_depth)),
                );
                return;
            }
            let base = options.base_path.as_deref().unwrap_or(".");
            let full = match jail_path(base, file_path) {
                Ok(p) => p,
                Err(e) => {
                    map.insert(key.to_string(), Value::String(format!("WATCH_ERR: {}", e)));
                    return;
                }
            };
            if let Err(e) = check_file_size(&full) {
                map.insert(key.to_string(), Value::String(format!("WATCH_ERR: {}", e)));
                return;
            }
            let watch_idx = markers.iter().position(|m| m == "watch").unwrap();
            let key_path = markers.get(watch_idx + 1).cloned();

            match std::fs::read_to_string(&full) {
                Ok(content) => {
                    let value = if let Some(ref kp) = key_path {
                        extract_from_file_content(&content, kp, full.extension().and_then(|e| e.to_str()).unwrap_or("")).unwrap_or(Value::Null)
                    } else {
                        Value::String(content.trim().to_string())
                    };
                    map.insert(key.to_string(), value);
                }
                Err(e) => {
                    map.insert(key.to_string(), Value::String(format!("WATCH_ERR: {}", fmt_io_err(&e, file_path))));
                }
            }
        }
    }

    // ── :prompt ──
    // Syntax: key:prompt:LABEL subtree
    // Converts the resolved subtree (object) into a SYNX-formatted string
    // wrapped in a labeled code fence, ready for LLM prompt embedding.
    if mb & mk::PROMPT != 0 {
        let prompt_idx = markers.iter().position(|m| m == "prompt").unwrap();
        let label = markers.get(prompt_idx + 1).cloned().unwrap_or_else(|| key.to_string());
        if let Some(val) = map.get(key) {
            let synx_text = stringify_value(val, 0);
            let block = format!("{} (SYNX):\n```synx\n{}```", label, synx_text);
            map.insert(key.to_string(), Value::String(block));
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
                let current_value = map.get(key).cloned().unwrap_or(Value::Null);
                match wasm_rt.apply_marker(marker, &current_value, &args) {
                    Ok(result) => {
                        map.insert(key.to_string(), result);
                    }
                    Err(e) => {
                        map.insert(key.to_string(), Value::String(format!("WASM_ERR: {}", e)));
                    }
                }
                break; // Only apply one WASM marker per key
            }
        }
    }

    // ── Constraint validation (always last, after all markers resolved) ──
    if let Some(ref c) = meta.constraints {
        validate_constraints(map, key, c);
    }
}

// ─── Constraint enforcement ───────────────────────────────

fn validate_constraints(map: &mut Map, key: &str, c: &Constraints) {
    match map.get_mut(key) {
        Some(v) => {
            if let Some(err) = constraint_violation(key, v, c) {
                *v = Value::String(err);
            }
        }
        None => {
            if c.required {
                map.insert(key.to_string(), Value::String(
                    format!("CONSTRAINT_ERR: '{}' is required", key),
                ));
            }
        }
    }
}

/// The error a value earns against its constraints, or `None` when it passes.
///
/// perf: reads the value in place — the previous version cloned it first.
fn constraint_violation(key: &str, val: &Value, c: &Constraints) -> Option<String> {
    // required
    if c.required {
        let empty = matches!(val, Value::Null)
            || matches!(val, Value::String(s) if s.is_empty());
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
            Value::String(s) => s,
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
        Value::String(s) if c.min.is_some() || c.max.is_some() => Some(s.chars().count() as f64),
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
            if let Value::String(s) = val {
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

/// Read a persisted value from the .synx.lock file.
fn read_lock_value(lock_path: &std::path::Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(lock_path).ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix(key) {
            if rest.starts_with(' ') {
                return Some(rest.trim_start().to_string());
            }
        }
    }
    None
}

/// Write/update a key value pair in the .synx.lock file.
fn write_lock_value(lock_path: &std::path::Path, key: &str, value: &str) {
    let mut lines: Vec<String> = std::fs::read_to_string(lock_path)
        .unwrap_or_default()
        .lines()
        .map(|l| l.to_string())
        .collect();

    let new_line = format!("{} {}", key, value);
    let mut found = false;
    for line in lines.iter_mut() {
        if line.starts_with(key) && line[key.len()..].starts_with(' ') {
            *line = new_line.clone();
            found = true;
            break;
        }
    }
    if !found {
        lines.push(new_line);
    }
    let _ = std::fs::write(lock_path, lines.join("\n") + "\n");
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

fn allow_spam_access(bucket_key: &str, max_calls: usize, window_sec: u64) -> bool {
    let now = Instant::now();
    let window = Duration::from_secs(window_sec.max(1));

    let buckets = SPAM_BUCKETS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = match buckets.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };

    let calls = guard.entry(bucket_key.to_string()).or_default();
    calls.retain(|ts| now.duration_since(*ts) <= window);

    if calls.len() >= max_calls {
        return false;
    }

    calls.push(now);
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
            } else if let Ok(f) = val.parse::<f64>() {
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
    /// Root keys a single-pass token lookup cannot find (see [`is_calc_special_key`]).
    root_special: Vec<String>,
}

impl CalcScope {
    fn of(root: &Value) -> Self {
        // spec: computed after the pre-passes; resolution replaces values but
        // never adds or removes keys, so the root key set is fixed from here.
        let root_special = match root {
            Value::Object(m) => m.keys().filter(|k| is_calc_special_key(k)).cloned().collect(),
            _ => Vec::new(),
        };
        CalcScope { root_special }
    }
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
/// previous version ran `replace_word` — a full copy of the expression — for
/// every numeric key of the root and of the object, so N calcs over a root of
/// N keys was quadratic. The sequential form survives only for documents that
/// have a special key with a numeric value, where it is the exact semantics.
///
/// spec: until 3.7.5 the root was substituted first, so a root key beat a
/// same-named key of the object — the opposite of the TypeScript engine.
fn substitute_calc_vars(
    expr: &str,
    local: &Map,
    self_key: &str,
    root: Option<&Map>,
    scope: &CalcScope,
) -> Option<String> {
    let local_is_root = root.map_or(false, |r| std::ptr::eq(r, local));
    let numeric = |m: &Map, k: &str| m.get(k).and_then(value_as_number);
    let lookup = |name: &str| -> Option<f64> {
        if name != self_key {
            if let Some(n) = numeric(local, name) {
                return Some(n);
            }
        }
        root.and_then(|r| numeric(r, name))
    };

    let special_in_play = scope
        .root_special
        .iter()
        .any(|k| root.map_or(false, |r| numeric(r, k).is_some()))
        || (!local_is_root
            && local
                .iter()
                .any(|(k, v)| k != self_key && value_as_number(v).is_some() && is_calc_special_key(k)));

    if special_in_play {
        // Exact sequential whole-word replacement, object before root.
        let mut resolved = expr.to_string();
        let maps = [Some(local), if local_is_root { None } else { root }];
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

/// Markers whose value must be resolved (read from env, a file, another key, an
/// expression) before it becomes a secret — those are masked by `:secret` in
/// place, not pre-masked here.
fn needs_resolution_before_secret(markers: &[String]) -> bool {
    const RESOLVING: &[&str] = &[
        "env", "include", "import", "watch", "ref", "alias", "calc", "map", "i18n", "default",
        "fallback", "once", "split", "join", "format", "replace",
    ];
    markers.iter().any(|m| RESOLVING.contains(&m.as_str()))
}

/// Convert every literal `:secret`-marked value to `Value::Secret` before
/// resolution, so no later interpolation can read the plaintext.
fn mask_secret_literals(value: &mut Value, metadata: &Map<MetaMap>, path: &mut String) {
    let map = match value {
        Value::Object(m) => m,
        _ => return,
    };
    if let Some(mm) = metadata.get(path.as_str()) {
        for (key, meta) in mm {
            if has_marker(&meta.markers, "secret") && !needs_resolution_before_secret(&meta.markers) {
                if let Some(v) = map.get_mut(key.as_str()) {
                    if !matches!(v, Value::Secret(_) | Value::Object(_) | Value::Array(_)) {
                        let raw = value_to_string(v);
                        *v = Value::Secret(raw);
                    }
                }
            }
        }
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
        match child {
            Value::Object(_) => mask_secret_literals(child, metadata, path),
            Value::Array(arr) => {
                let list_len = path.len();
                for (idx, item) in arr.iter_mut().enumerate() {
                    if let Value::Object(_) = item {
                        push_item_segment(path, idx);
                        mask_secret_literals(item, metadata, path);
                        path.truncate(list_len);
                    }
                }
            }
            _ => {}
        }
        path.truncate(base_len);
    }
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
        let mut merged: Map = Map::default();
        let mut source: Map<Option<&str>> = Map::default();
        for parent_name in parents {
            if let Some(Value::Object(p)) = map.get(parent_name.as_str()) {
                for (k, v) in p {
                    merged.insert(k.clone(), v.clone());
                    source.insert(k.clone(), Some(parent_name.as_str()));
                }
            }
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
        let mut top = MetaMap::default();
        let mut nested: Vec<(String, MetaMap)> = Vec::new();
        for (k, src) in &source {
            let from = src.unwrap_or(child_key.as_str());
            if let Some(m) = metadata.get(key_segment(from).as_ref()).and_then(|mm| mm.get(k.as_str())) {
                top.insert(k.clone(), m.clone());
            }
            if let Some(parent) = src {
                // The inherited subtree is a copy; its nested markers move with it.
                let prefix = format!("{}.{}", key_segment(parent), key_segment(k));
                for (path, mm) in metadata.iter() {
                    let rest = match path.strip_prefix(prefix.as_str()) {
                        Some(r) if r.is_empty() || r.starts_with('.') => r,
                        _ => continue,
                    };
                    nested.push((format!("{}.{}{}", key_segment(child_key), key_segment(k), rest), mm.clone()));
                }
            }
        }
        map.insert(child_key.clone(), Value::Object(merged));
        if top.is_empty() {
            metadata.shift_remove(key_segment(child_key).as_ref());
        } else {
            metadata.insert(key_segment(child_key).into_owned(), top);
        }
        for (path, mm) in nested {
            metadata.insert(path, mm);
        }
    }
}

fn deep_get(root: &Value, path: &str) -> Option<Value> {
    // Try direct key
    if let Value::Object(map) = root {
        if let Some(val) = map.get(path) {
            return Some(val.clone());
        }
    }
    // Dot-path traversal
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = root;
    for part in parts {
        match current {
            Value::Object(map) => match map.get(part) {
                Some(v) => current = v,
                None => return None,
            },
            _ => return None,
        }
    }
    Some(current.clone())
}

/// Resolve {placeholder} references in a template string.
/// Supports: {key}, {key.nested}, {key:alias}, {key:include}
fn resolve_interpolation(
    tpl: &str,
    root: &Value,
    local_map: &Map,
    includes: &Map,
) -> String {
    let bytes = tpl.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len.min(MAX_ENGINE_SCRATCH_STRING));
    let mut i = 0;

    while i < len {
        if result.len() >= MAX_ENGINE_SCRATCH_STRING {
            break;
        }
        if bytes[i] == b'{' {
            if let Some(close) = tpl[i + 1..].find('}') {
                let inner = &tpl[i + 1..i + 1 + close];
                // Check for scope separator ':'
                if let Some(colon) = inner.find(':') {
                    let ref_name = &inner[..colon];
                    let scope = &inner[colon + 1..];
                    // Valid ref name?
                    if ref_name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                        let resolved = if scope == "include" {
                            // {key:include} — first/only include
                            if includes.len() == 1 {
                                let first = includes.values().next().unwrap();
                                deep_get(first, ref_name)
                            } else {
                                None
                            }
                        } else {
                            // {key:alias} — look up by alias
                            includes.get(scope).and_then(|inc| deep_get(inc, ref_name))
                        };
                        if let Some(val) = resolved {
                            let s = value_to_string(&val);
                            let room = MAX_ENGINE_SCRATCH_STRING.saturating_sub(result.len());
                            if room > 0 {
                                let take = s.len().min(room);
                                let end = s.floor_char_boundary(take);
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
                        i += 2 + close;
                        continue;
                    }
                } else {
                    // {key} — local
                    let ref_name = inner;
                    if ref_name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                        let resolved = deep_get(root, ref_name).or_else(|| {
                            local_map.get(ref_name).cloned()
                        });
                        if let Some(val) = resolved {
                            let s = value_to_string(&val);
                            let room = MAX_ENGINE_SCRATCH_STRING.saturating_sub(result.len());
                            if room > 0 {
                                let take = s.len().min(room);
                                let end = s.floor_char_boundary(take);
                                result.push_str(&s[..end]);
                            }
                        } else {
                            result.push('{');
                            let rem = MAX_ENGINE_SCRATCH_STRING.saturating_sub(result.len() + 1);
                            if rem > 0 {
                                let end = ref_name.floor_char_boundary(ref_name.len().min(rem));
                                result.push_str(&ref_name[..end]);
                            }
                            if result.len() < MAX_ENGINE_SCRATCH_STRING {
                                result.push('}');
                            }
                        }
                        i += 2 + close;
                        continue;
                    }
                }
            }
        }
        // Copy one whole UTF-8 scalar of literal text. `bytes[i] as char`
        // reinterpreted each UTF-8 byte as a code point, turning any non-ASCII
        // literal in the template into mojibake (`мир` → `Ð¼Ð¸Ñ`).
        let ch = tpl[i..].chars().next().unwrap_or('\u{fffd}');
        if result.len() < MAX_ENGINE_SCRATCH_STRING {
            result.push(ch);
        }
        i += ch.len_utf8();
    }
    result
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
        if let Ok(text) = std::fs::read_to_string(&full) {
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
        if let Ok(text) = std::fs::read_to_string(&entry) {
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

/// Fill the type and constraint registries from one object and its subtree:
/// the object's own declarations first, then each child in document order.
fn collect_registries(
    value: &Value,
    metadata: &Map<MetaMap>,
    path: &mut String,
    types: &mut Map<String>,
    constraints: &mut Map<Constraints>,
    depth: usize,
) {
    if depth > MAX_RESOLVE_DEPTH {
        return;
    }
    let Value::Object(map) = value else { return };
    if let Some(mm) = metadata.get(path.as_str()) {
        for (key, meta) in mm {
            if let Some(ref hint) = meta.type_hint {
                if !types.contains_key(key.as_str()) {
                    types.insert(key.clone(), hint.clone());
                }
            }
            if let Some(ref c) = meta.constraints {
                match constraints.get_mut(key.as_str()) {
                    Some(existing) => merge_constraints(existing, c),
                    None => {
                        constraints.insert(key.clone(), c.clone());
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
            Value::Object(_) => collect_registries(child, metadata, path, types, constraints, depth + 1),
            Value::Array(items) => {
                let list_len = path.len();
                for (idx, item) in items.iter().enumerate() {
                    if let Value::Object(_) = item {
                        push_item_segment(path, idx);
                        collect_registries(item, metadata, path, types, constraints, depth + 1);
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
fn validate_fields(value: &mut Value, constraints: &Map<Constraints>, types: &Map<String>) {
    if constraints.is_empty() && types.is_empty() {
        return;
    }
    if let Value::Object(ref mut map) = value {
        for (key, val) in map.iter_mut() {
            if let Some(c) = constraints.get(key.as_str()) {
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
                Value::Object(_) => validate_fields(val, constraints, types),
                Value::Array(arr) => {
                    for item in arr.iter_mut() {
                        if let Value::Object(_) = item {
                            validate_fields(item, constraints, types);
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
        assert!(json.contains("INCLUDE_ERR: max include depth"), "{}", json);
        let _ = std::fs::remove_dir_all(&tmp);
    }
    use crate::{parse, Options, Value};
    use super::{resolve, read_manifest_main};
    use std::sync::Mutex;

    // The :spam buckets are process-global, and clear_spam_buckets() wipes all of
    // them — so two spam tests running on different threads can clear each other's
    // bucket mid-test. Every spam test takes this lock for its whole body.
    static SPAM_TESTS: Mutex<()> = Mutex::new(());

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
