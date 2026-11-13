use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

/// An open document and how many times it has been replaced.
struct Document {
    text: Arc<str>,
    generation: u64,
}

struct Backend {
    client: Client,
    documents: Mutex<HashMap<Uri, Document>>,
    /// Workspace root paths (set on initialize)
    workspace_roots: Mutex<Vec<PathBuf>>,
}

/// Locks `mutex` even after a panic elsewhere left it poisoned: the maps
/// behind these locks are replaced whole, never left half-written.
fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs `f`; a panic inside it becomes `fallback`.
///
/// safety: tower-lsp does not catch panics, so one in a request handler ends
/// the server for every open file. A document that trips a bug costs one
/// answer instead.
fn guarded<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}

/// Whether `path` stays inside the directory it is joined onto: relative,
/// with no `..`, root or drive.
fn is_contained(path: &str) -> bool {
    !path.is_empty() && Path::new(path).components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

// ─── Known sets (mirrors VS Code extension logic) ────────────────────────────

const KNOWN_MARKERS: &[&str] = &[
    "random", "calc", "env", "alias", "ref", "inherit", "i18n", "secret", "default",
    "unique", "include", "import", "geo", "template", "split", "join",
    "clamp", "round", "map", "format", "fallback", "once", "version", "watch", "spam",
    "prompt", "vision", "audio", "sum", "sort", "replace",
];

const KNOWN_CONSTRAINTS: &[&str] = &[
    "min", "max", "type", "required", "readonly", "pattern", "enum",
];

const KNOWN_TYPES: &[&str] = &[
    "int", "float", "bool", "string", "random", "random:int", "random:float", "random:bool",
];

const PACKAGES_DIR: &str = "synx_packages";
const MANIFEST_NAME: &str = "synx-pkg.synx";

// ─── Package helpers ─────────────────────────────────────────────────────────

/// Scan synx_packages/ for installed packages, returns list of @scope/name
fn scan_installed_packages(roots: &[PathBuf]) -> Vec<String> {
    let mut packages = Vec::new();
    for root in roots {
        let pkg_dir = root.join(PACKAGES_DIR);
        if !pkg_dir.is_dir() {
            continue;
        }
        // Scan @scope directories
        if let Ok(scopes) = std::fs::read_dir(&pkg_dir) {
            for scope_entry in scopes.flatten() {
                let scope_name = scope_entry.file_name().to_string_lossy().to_string();
                if !scope_name.starts_with('@') || !scope_entry.path().is_dir() {
                    continue;
                }
                if let Ok(pkgs) = std::fs::read_dir(scope_entry.path()) {
                    for pkg_entry in pkgs.flatten() {
                        if pkg_entry.path().is_dir() {
                            let name = format!("{}/{}", scope_name, pkg_entry.file_name().to_string_lossy());
                            packages.push(name);
                        }
                    }
                }
            }
        }
    }
    packages.sort();
    packages.dedup();
    packages
}

/// Resolve a package name to its main .synx file path
fn resolve_package_main(roots: &[PathBuf], package: &str) -> Option<PathBuf> {
    // safety: `!use @a/../../x` and a manifest `main ../../../etc/hosts` sent
    // go-to-definition to any file on the machine.
    if !is_contained(package) {
        return None;
    }
    for root in roots {
        let pkg_dir = root.join(PACKAGES_DIR).join(package);
        if !pkg_dir.is_dir() {
            continue;
        }
        // Try reading manifest to find main field
        let manifest = pkg_dir.join(MANIFEST_NAME);
        if let Ok(text) = std::fs::read_to_string(&manifest) {
            for line in text.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("main ") {
                    let main = rest.trim();
                    if is_contained(main) {
                        let main_path = pkg_dir.join(main);
                        if main_path.exists() {
                            return Some(main_path);
                        }
                    }
                }
                // Legacy: entry field
                if let Some(rest) = trimmed.strip_prefix("entry ") {
                    let entry = rest.trim();
                    if is_contained(entry) {
                        let entry_path = pkg_dir.join(entry);
                        if entry_path.exists() {
                            return Some(entry_path);
                        }
                    }
                }
            }
        }
        // Fallback: src/main.synx
        let fallback = pkg_dir.join("src").join("main.synx");
        if fallback.exists() {
            return Some(fallback);
        }
    }
    None
}

/// Check if a package is installed in any workspace root
fn is_package_installed(roots: &[PathBuf], package: &str) -> bool {
    if !is_contained(package) {
        return false;
    }
    for root in roots {
        let pkg_dir = root.join(PACKAGES_DIR).join(package);
        if pkg_dir.is_dir() {
            return true;
        }
    }
    false
}

/// Extract !use directives from text: returns Vec<(line_number, package_name, alias)>
#[allow(dead_code)]
fn extract_use_directives(text: &str) -> Vec<(usize, String, Option<String>)> {
    let mut directives = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("!use ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if let Some(name) = parts.first() {
                let alias = if parts.len() >= 3 && parts[1] == "as" {
                    Some(parts[2].to_string())
                } else {
                    None
                };
                directives.push((i, name.to_string(), alias));
            }
        }
    }
    directives
}

// ─── Diagnostics ─────────────────────────────────────────────────────────────

/// Markers whose following chain segments are their arguments
/// (`:default:http://localhost:7001`, `:version:>=:1.0.0`, `:format:%.2f`).
const ARG_MARKERS: &[&str] = &[
    "inherit", "default", "map", "round", "clamp", "format", "version", "fallback", "watch",
    "i18n", "include", "import", "spam", "prompt", "replace", "split", "join", "sort", "once",
    "random",
];

/// The head of a key line — `key(type)[constraints]:marker:arg` — as byte
/// ranges of the trimmed line.
struct KeyHead<'a> {
    key: &'a str,
    /// Text inside `(...)` and the byte range of the group, parentheses included.
    type_hint: Option<(&'a str, usize, usize)>,
    /// Text inside `[...]`.
    constraints: Option<&'a str>,
    /// Chain segments after the key, split on `:`.
    markers: Vec<&'a str>,
    /// Bytes the head takes; the value starts after it.
    len: usize,
    value: &'a str,
}

/// Splits a key line the way the parser reads it: the head ends at the first
/// blank outside `[...]` and `(...)`, a constraint list ends at its matching
/// bracket.
///
/// why: the head was cut at the first whitespace and then on every `:`, so
/// `port[type:int]:env` reported an unknown marker `:int]`,
/// `name[required, min:1]` an unknown constraint, and `[pattern:^[A-Z]{2}$]`
/// was cut at its first `]`.
fn split_key_head(trimmed: &str) -> KeyHead<'_> {
    let bytes = trimmed.as_bytes();
    let mut depth = 0usize;
    let mut len = bytes.len();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'[' | b'(' => depth += 1,
            b']' | b')' => depth = depth.saturating_sub(1),
            b' ' | b'\t' if depth == 0 => {
                len = i;
                break;
            }
            _ => {}
        }
    }
    let head = &trimmed[..len];
    let key_end = head.find(|c| c == '(' || c == '[' || c == ':').unwrap_or(head.len());
    let mut rest = key_end;
    let mut type_hint = None;
    let mut constraints = None;
    while rest < head.len() {
        let (open, close) = match head.as_bytes()[rest] {
            b'(' => (b'(', b')'),
            b'[' => (b'[', b']'),
            _ => break,
        };
        let mut nest = 0usize;
        let mut end = None;
        for (i, &b) in head.as_bytes()[rest..].iter().enumerate() {
            if b == open {
                nest += 1;
            } else if b == close {
                nest -= 1;
                if nest == 0 {
                    end = Some(rest + i);
                    break;
                }
            }
        }
        let Some(end) = end else { break };
        let inner = &head[rest + 1..end];
        if open == b'(' {
            type_hint = Some((inner, rest, end + 1));
        } else {
            constraints = Some(inner);
        }
        rest = end + 1;
    }
    let markers = match head[rest..].strip_prefix(':') {
        Some(chain) => chain.split(':').collect(),
        None => Vec::new(),
    };
    KeyHead {
        key: &head[..key_end],
        type_hint,
        constraints,
        markers,
        len,
        value: trimmed[len..].trim_start(),
    }
}

/// Chain segments that name no marker. Whatever follows a marker that takes
/// arguments is its argument, up to the next known marker.
fn unknown_markers<'a>(chain: &[&'a str]) -> Vec<&'a str> {
    let mut is_in_args = false;
    let mut unknown = Vec::new();
    for &segment in chain {
        if KNOWN_MARKERS.contains(&segment) {
            is_in_args = ARG_MARKERS.contains(&segment);
        } else if !is_in_args && !segment.is_empty() && segment.parse::<f64>().is_err() {
            unknown.push(segment);
        }
    }
    unknown
}

/// Names in a constraint list. A `pattern:` value runs to the end of the list,
/// commas and brackets included.
fn constraint_names(list: &str) -> Vec<&str> {
    let mut names = Vec::new();
    for part in list.split(',') {
        let name = part.trim().split(':').next().unwrap_or("").trim();
        if name.is_empty() {
            continue;
        }
        names.push(name);
        if name == "pattern" {
            break;
        }
    }
    names
}

/// UTF-16 length of `text` — the unit of an LSP column.
fn utf16_len(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

/// The byte offset of UTF-16 column `column` in `line`, never inside a
/// character and never past the end.
///
/// safety: the client's column was used as a byte index. `название ` puts
/// the cursor at column 9, byte 9 is inside `е`, and the slice panicked —
/// the language server died on the first Cyrillic line typed.
fn utf16_to_byte(line: &str, column: u32) -> usize {
    let mut units = 0u32;
    for (i, c) in line.char_indices() {
        if units >= column {
            return i;
        }
        units += c.len_utf16() as u32;
    }
    line.len()
}

fn diagnostic(line: u32, from: u32, to: u32, severity: DiagnosticSeverity, message: String) -> Diagnostic {
    Diagnostic {
        range: Range::new(Position::new(line, from), Position::new(line, to)),
        severity: Some(severity),
        message,
        source: Some("synx".into()),
        ..Default::default()
    }
}

fn diagnose(text: &str, workspace_roots: &[PathBuf]) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    // The parser decides the mode: it knows a BOM in front of `!active` and
    // the `#!mode:active` pragma.
    let parsed = synx_core::parse(text);
    let has_active = parsed.mode == synx_core::Mode::Active;
    // Markers of an installed package are not in the built-in list.
    let has_packages = !parsed.uses.is_empty();
    drop(parsed);

    // perf: one hash lookup per key. The scope was a list searched from the
    // start for every key — 100 000 keys took 25 s on each keystroke.
    let mut keys_by_indent: HashMap<usize, HashMap<&str, usize>> = HashMap::new();
    let mut is_in_comment = false;
    let mut block_indent: Option<usize> = None;

    for (i, line) in text.lines().enumerate() {
        let lineno = i as u32;
        let trimmed = line.trim();
        let indent_len = line.len() - line.trim_start().len();

        // The body of a `|` block is text, not keys.
        if let Some(base) = block_indent {
            if trimmed.is_empty() || indent_len > base {
                continue;
            }
            block_indent = None;
        }
        if trimmed == "###" {
            is_in_comment = !is_in_comment;
            continue;
        }
        if is_in_comment {
            continue;
        }

        // spec: a tab is refused in indentation only; inside a value it is data.
        if let Some(col) = line[..indent_len].find('\t') {
            diags.push(diagnostic(
                lineno,
                col as u32,
                col as u32 + 1,
                DiagnosticSeverity::ERROR,
                "Use spaces for indentation, not tabs".into(),
            ));
        }

        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        // Columns are UTF-16 units; the indent is ASCII, so bytes and units agree up to it.
        let indent = indent_len as u32;

        // Check !use directives for missing packages
        if let Some(rest) = trimmed.strip_prefix("!use ") {
            if let Some(pkg_name) = rest.split_whitespace().next() {
                if pkg_name.starts_with('@') && pkg_name.contains('/') && !is_package_installed(workspace_roots, pkg_name) {
                    let start = indent + 5 + utf16_len(&rest[..rest.len() - rest.trim_start().len()]);
                    diags.push(diagnostic(
                        lineno,
                        start,
                        start + utf16_len(pkg_name),
                        DiagnosticSeverity::WARNING,
                        format!("Package '{}' not found in synx_packages/. Run: synx install {}", pkg_name, pkg_name),
                    ));
                }
            }
            continue;
        }

        if trimmed.starts_with('!') {
            continue;
        }

        // A list item is a value; it starts a new scope for the keys under it,
        // so `port` in two items of one list is not a duplicate.
        if trimmed.starts_with("- ") {
            keys_by_indent.retain(|&ind, _| ind <= indent_len);
            continue;
        }
        let body = trimmed;

        if indent_len % 2 != 0 {
            diags.push(diagnostic(
                lineno,
                0,
                indent,
                DiagnosticSeverity::WARNING,
                "Indentation should be a multiple of 2 spaces".into(),
            ));
        }

        if body.starts_with(|c| matches!(c, '-' | '#' | '/' | '!')) {
            continue;
        }

        let head = split_key_head(body);
        if head.value == "|" || head.value == "|+" {
            block_indent = Some(indent_len);
        }
        let head_from = indent;
        let head_to = head_from + utf16_len(&body[..head.len]);

        // A key line closes every deeper scope: siblings under different
        // parents (`db.url`, `redis.url`) are not duplicates.
        keys_by_indent.retain(|&ind, _| ind <= indent_len);
        let scope_keys = keys_by_indent.entry(indent_len).or_default();
        match scope_keys.get(head.key) {
            Some(prev_line) => diags.push(diagnostic(
                lineno,
                indent,
                indent + utf16_len(head.key),
                DiagnosticSeverity::WARNING,
                format!("Duplicate key '{}' (first defined on line {})", head.key, prev_line + 1),
            )),
            None => {
                scope_keys.insert(head.key, i);
            }
        }

        if let Some((type_name, from, to)) = head.type_hint {
            if !KNOWN_TYPES.contains(&type_name) {
                diags.push(diagnostic(
                    lineno,
                    head_from + utf16_len(&body[..from]),
                    head_from + utf16_len(&body[..to]),
                    DiagnosticSeverity::ERROR,
                    format!("Unknown type cast '{}' — allowed: int, float, bool, string, random", type_name),
                ));
            }
        }

        if !has_packages {
            for marker in unknown_markers(&head.markers) {
                diags.push(diagnostic(
                    lineno,
                    head_from,
                    head_to,
                    DiagnosticSeverity::WARNING,
                    format!("Unknown marker ':{}' ", marker),
                ));
            }
        }

        if !head.markers.is_empty() && !has_active {
            diags.push(diagnostic(
                lineno,
                head_from,
                head_to,
                DiagnosticSeverity::INFORMATION,
                "Markers require !active on the first line to take effect".into(),
            ));
        }

        if let Some(list) = head.constraints {
            for name in constraint_names(list) {
                if !KNOWN_CONSTRAINTS.contains(&name) {
                    diags.push(diagnostic(
                        lineno,
                        head_from,
                        head_to,
                        DiagnosticSeverity::WARNING,
                        format!("Unknown constraint '{}'", name),
                    ));
                }
            }
            if !has_active {
                diags.push(diagnostic(
                    lineno,
                    head_from,
                    head_to,
                    DiagnosticSeverity::INFORMATION,
                    "Constraints require !active on the first line".into(),
                ));
            }
        }
    }

    diags
}

// ─── Completion ──────────────────────────────────────────────────────────────

fn build_marker_completions() -> Vec<CompletionItem> {
    KNOWN_MARKERS.iter().map(|&m| {
        CompletionItem {
            label: m.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(format!(":{} marker", m)),
            insert_text: Some(m.to_string()),
            ..Default::default()
        }
    }).collect()
}

fn build_constraint_completions() -> Vec<CompletionItem> {
    KNOWN_CONSTRAINTS.iter().map(|&c| {
        CompletionItem {
            label: c.to_string(),
            kind: Some(CompletionItemKind::PROPERTY),
            detail: Some(format!("[{}] constraint", c)),
            insert_text: Some(c.to_string()),
            ..Default::default()
        }
    }).collect()
}

fn build_directive_completions() -> Vec<CompletionItem> {
    ["!active", "!lock", "!tool", "!schema", "!llm", "!use"].iter().map(|&d| {
        CompletionItem {
            label: d.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Directive".into()),
            insert_text: Some(d.to_string()),
            ..Default::default()
        }
    }).collect()
}

fn build_package_completions(roots: &[PathBuf]) -> Vec<CompletionItem> {
    scan_installed_packages(roots)
        .into_iter()
        .map(|name| CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some("Installed package".into()),
            insert_text: Some(name),
            ..Default::default()
        })
        .collect()
}

// ─── Document Symbols ────────────────────────────────────────────────────────

fn build_symbols(text: &str) -> Vec<DocumentSymbol> {
    let result = synx_core::parse(text);
    match result.root {
        synx_core::Value::Object(map) => {
            let lines: Vec<&str> = text.lines().collect();
            map_to_symbols(&map, &lines, 0, lines.len())
        }
        _ => vec![],
    }
}

/// Line, indent and block end of each direct child key in `lines[from..to]`.
///
/// The children are the lines at the block's shallowest indent; a key's block
/// runs to the next line at or above that indent. One pass per block — the
/// previous lookup scanned the whole file for every key (quadratic) and took
/// the first match anywhere, so `cache.port` pointed at `db.port`. A repeated
/// key resolves to its last declaration, the one the parser keeps.
fn child_key_lines<'a>(lines: &[&'a str], from: usize, to: usize) -> HashMap<&'a str, (usize, usize, usize)> {
    let mut out: HashMap<&'a str, (usize, usize, usize)> = HashMap::new();
    let mut level: Option<usize> = None;
    let mut open: Option<&'a str> = None;
    for i in from..to {
        let line = lines[i];
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") || trimmed.starts_with('!') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let lvl = *level.get_or_insert(indent);
        if indent > lvl {
            continue;
        }
        if let Some(k) = open.take() {
            if let Some(e) = out.get_mut(k) {
                e.2 = i;
            }
        }
        if trimmed.starts_with("- ") {
            continue;
        }
        let key = trimmed
            .split(|c: char| c.is_whitespace() || c == ':' || c == '[' || c == '(')
            .next()
            .unwrap_or("");
        if !key.is_empty() {
            out.insert(key, (i, indent, to));
            open = Some(key);
        }
    }
    out
}

/// At most `max` bytes of `s`, cut on a character boundary.
fn clip(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn map_to_symbols(map: &synx_core::Map, lines: &[&str], from: usize, to: usize) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();
    let children = child_key_lines(lines, from, to);
    let mut sorted_keys: Vec<&String> = map.keys().collect();
    sorted_keys.sort();

    for key in sorted_keys {
        let val = &map[key];
        let (line_num, indent, block_end) = children.get(key.as_str()).copied().unwrap_or((from, 0, from));
        let range = Range::new(
            Position::new(line_num as u32, indent as u32),
            Position::new(line_num as u32, (indent + key.len()) as u32),
        );

        let kind = match val {
            synx_core::Value::Object(_) => SymbolKind::OBJECT,
            synx_core::Value::Array(_) => SymbolKind::ARRAY,
            synx_core::Value::Int(_) | synx_core::Value::Float(_) => SymbolKind::NUMBER,
            synx_core::Value::Bool(_) => SymbolKind::BOOLEAN,
            synx_core::Value::Null => SymbolKind::NULL,
            synx_core::Value::String(_) => SymbolKind::STRING,
            synx_core::Value::Secret(_) => SymbolKind::KEY,
        };

        let detail = match val {
            // `&s[..37]` panicked on a multi-byte character at that offset —
            // a long Cyrillic value took the language server down.
            synx_core::Value::String(s) => {
                if s.len() > 40 { format!("{}...", clip(s, 37)) } else { s.clone() }
            }
            synx_core::Value::Int(n) => n.to_string(),
            synx_core::Value::Float(f) => f.to_string(),
            synx_core::Value::Bool(b) => b.to_string(),
            synx_core::Value::Null => "null".into(),
            synx_core::Value::Array(a) => format!("[{} items]", a.len()),
            synx_core::Value::Object(m) => format!("{{{} keys}}", m.len()),
            synx_core::Value::Secret(_) => "[SECRET]".into(),
        };

        #[allow(deprecated)]
        let mut sym = DocumentSymbol {
            name: key.clone(),
            detail: Some(detail),
            kind,
            range,
            selection_range: range,
            children: None,
            tags: None,
            deprecated: None,
        };

        if let synx_core::Value::Object(inner) = val {
            let nested = map_to_symbols(inner, lines, line_num + 1, block_end.max(line_num + 1));
            if !nested.is_empty() {
                sym.children = Some(nested);
            }
        }

        symbols.push(sym);
    }
    symbols
}

// ─── LanguageServer impl ────────────────────────────────────────────────────

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Capture workspace roots for package resolution
        let mut roots = Vec::new();
        if let Some(folders) = &params.workspace_folders {
            for folder in folders {
                if let Some(path) = folder.uri.to_file_path() {
                    roots.push(path.into_owned());
                }
            }
        }
        if roots.is_empty() {
            #[allow(deprecated)]
            if let Some(uri) = &params.root_uri {
                if let Some(path) = uri.to_file_path() {
                    roots.push(path.into_owned());
                }
            }
        }
        *locked(&self.workspace_roots) = roots;

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![":".into(), "[".into(), "!".into(), " ".into()]),
                    ..Default::default()
                }),
                document_symbol_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "synx-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "synx-lsp initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let (text, generation) = self.store(&uri, params.text_document.text);
        self.publish_diagnostics(uri, text, generation).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().last() {
            let (text, generation) = self.store(&uri, change.text);
            self.publish_diagnostics(uri, text, generation).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        locked(&self.documents).remove(&uri);
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        let Some(text) = self.text_of(&uri) else {
            return Ok(None);
        };
        let roots = locked(&self.workspace_roots).clone();
        Ok(guarded(None, || {
            completions_at(&text, pos, &roots).map(CompletionResponse::Array)
        }))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let Some(text) = self.text_of(&uri) else {
            return Ok(None);
        };

        let symbols = guarded(Vec::new(), || build_symbols(&text));
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let Some(text) = self.text_of(&uri) else {
            return Ok(None);
        };
        let roots = locked(&self.workspace_roots).clone();
        Ok(guarded(None, || {
            let trimmed = text.lines().nth(pos.line as usize).unwrap_or("").trim();

            // Go-to-definition on `!use @scope/name`
            let pkg_name = trimmed.strip_prefix("!use ")?.split_whitespace().next()?;
            let main_path = resolve_package_main(&roots, pkg_name)?;
            let target_uri = Uri::from_file_path(&main_path)?;
            Some(GotoDefinitionResponse::Scalar(Location {
                uri: target_uri,
                range: Range::new(Position::new(0, 0), Position::new(0, 0)),
            }))
        }))
    }
}

/// What to offer at `pos`, from the text before the cursor on its line.
fn completions_at(text: &str, pos: Position, roots: &[PathBuf]) -> Option<Vec<CompletionItem>> {
    let line = text.lines().nth(pos.line as usize).unwrap_or("");
    let before = &line[..utf16_to_byte(line, pos.character)];

    // After `!use ` — suggest installed packages
    if before.trim_start().starts_with("!use ") {
        return Some(build_package_completions(roots));
    }

    if before.contains('[') && !before.contains(']') {
        return Some(build_constraint_completions());
    }

    if before.contains(':') && !before.contains(' ') {
        return Some(build_marker_completions());
    }

    if before.trim_start().starts_with('!') || before.is_empty() {
        return Some(build_directive_completions());
    }

    None
}

impl Backend {
    /// Replaces the text of `uri`; returns it with its generation.
    fn store(&self, uri: &Uri, text: String) -> (Arc<str>, u64) {
        let text: Arc<str> = Arc::from(text);
        let mut docs = locked(&self.documents);
        let generation = docs.get(uri).map_or(0, |doc| doc.generation) + 1;
        docs.insert(uri.clone(), Document { text: text.clone(), generation });
        (text, generation)
    }

    fn text_of(&self, uri: &Uri) -> Option<Arc<str>> {
        locked(&self.documents).get(uri).map(|doc| doc.text.clone())
    }

    fn is_current(&self, uri: &Uri, generation: u64) -> bool {
        locked(&self.documents).get(uri).is_some_and(|doc| doc.generation == generation)
    }

    /// Diagnoses `text` off the async threads and publishes the result unless
    /// the document changed in the meantime.
    ///
    /// perf: every keystroke sends the whole document; the scan ran inside
    /// the handler, so a large file held up completion, and an older scan
    /// finishing late overwrote the diagnostics of a newer one.
    async fn publish_diagnostics(&self, uri: Uri, text: Arc<str>, generation: u64) {
        if !self.is_current(&uri, generation) {
            return;
        }
        let roots = locked(&self.workspace_roots).clone();
        let scan = tokio::task::spawn_blocking(move || guarded(Vec::new(), || diagnose(&text, &roots)));
        let diags = scan.await.unwrap_or_default();
        if self.is_current(&uri, generation) {
            self.client.publish_diagnostics(uri, diags, None).await;
        }
    }
}

// ─── main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend {
        client,
        documents: Mutex::new(HashMap::new()),
        workspace_roots: Mutex::new(Vec::new()),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each nested key points at its own line, and a long multi-byte value is
    // clipped on a character boundary instead of panicking.
    #[test]
    fn symbols_point_at_their_own_lines() {
        let text = "db\n  port 5432\ncache\n  port 6379\nnote ааааааааааааааааааааааааааааааааааааааааааааааа\n";
        let syms = build_symbols(text);
        let cache = syms.iter().find(|s| s.name == "cache").unwrap();
        let port = &cache.children.as_ref().unwrap()[0];
        assert_eq!(port.range.start.line, 3);
        let db = syms.iter().find(|s| s.name == "db").unwrap();
        assert_eq!(db.children.as_ref().unwrap()[0].range.start.line, 1);
        assert!(syms.iter().any(|s| s.name == "note"));
    }

    #[test]
    fn same_key_under_different_parents_is_not_a_duplicate() {
        let text = "database
  url x
redis
  url y
redis2
  url z
  url w
";
        let dups: Vec<_> = diagnose(text, &[]).into_iter().filter(|d| d.message.starts_with("Duplicate key")).collect();
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].range.start.line, 6);
    }

    fn messages(text: &str) -> Vec<String> {
        diagnose(text, &[]).into_iter().map(|d| d.message).collect()
    }

    // The column is in UTF-16 units; as a byte index it lands inside a
    // Cyrillic letter and the slice used to panic, ending the server.
    #[test]
    fn completion_takes_the_column_in_utf16_units() {
        let line = "название Привет ";
        for column in 0..40 {
            let _ = completions_at(line, Position::new(0, column), &[]);
        }
        assert_eq!(utf16_to_byte("abc", 2), 2);
        assert_eq!(utf16_to_byte("имя x", 3), 6);
        assert_eq!(utf16_to_byte("имя x", 4), 7);
        // A column inside a surrogate pair moves to the end of the character.
        assert_eq!(utf16_to_byte("a😀b", 2), 5);
        assert_eq!(utf16_to_byte("a😀b", 3), 5);
        assert_eq!(utf16_to_byte("abc", 99), 3);
        // The text before the cursor is what decides the offer.
        assert!(completions_at("ключ:", Position::new(0, 5), &[]).is_some());
        assert!(completions_at("ключ значение", Position::new(0, 13), &[]).is_none());
        assert!(completions_at("a 1", Position::new(7, 0), &[]).is_some());
    }

    #[test]
    fn odd_key_lines_do_not_panic() {
        for text in ["a)(b 1", "a)(", ")(", "a[b 1", "a]b[ 1", "a(b", "a(((", "x[[[", "к)(л 1", "a:", ":", "[", "(", "a[]:() 1"] {
            let _ = diagnose(text, &[]);
            let _ = build_symbols(text);
        }
    }

    #[test]
    fn valid_config_lines_are_quiet() {
        let text = "!active
port[type:int]:env:default:3001 PORT
db_url[required]:env DATABASE_URL
secret[required]:secret:env JWT_PRIVATE_KEY
workers[type:int, min:1, max:32] 4
api:env:default:http://localhost:7001 API_URL
code[pattern:^[A-Z]{2,3}$] RU
price(float):format:%.2f 10
ok:version:>=:1.0.0 1.2.0
total:sum
tags:sort:desc
text:replace:a:b banana
seed(random:int) x
";
        assert_eq!(messages(text), Vec::<String>::new());
    }

    #[test]
    fn real_mistakes_are_still_reported() {
        let found = messages("!active\na:nope 1\nb[type:int, bogus:1] 2\nc(money) 3\nd:env:nope X\n");
        assert_eq!(
            found,
            vec![
                "Unknown marker ':nope' ",
                "Unknown constraint 'bogus'",
                "Unknown type cast 'money' — allowed: int, float, bool, string, random",
                "Unknown marker ':nope' ",
            ]
        );
    }

    #[test]
    fn block_bodies_comments_and_list_items_are_not_key_lines() {
        let text = "!active
note |
  key:nope looks like a marker
  key:nope looks like a marker
after 1
###
dead:nope 1
dead:nope 1
###
servers
  - name a
    port 1
  - name b
    port 2
links
  - http://example.com/a:b
";
        assert_eq!(messages(text), Vec::<String>::new());
    }

    #[test]
    fn the_parser_decides_whether_the_document_is_active() {
        assert!(messages("\u{feff}!active\nport:env PORT\n").is_empty());
        assert!(messages("#!mode:active\nport:env PORT\n").is_empty());
        assert_eq!(
            messages("port:env PORT\n"),
            vec!["Markers require !active on the first line to take effect"]
        );
    }

    #[test]
    fn a_tab_is_reported_in_indentation_only() {
        assert!(messages("key a\tb\n").is_empty());
        let found = diagnose("a\n\tb 1\n", &[]);
        let tabs: Vec<_> = found.iter().filter(|d| d.message == "Use spaces for indentation, not tabs").collect();
        assert_eq!(tabs.len(), 1);
        assert_eq!((tabs[0].range.start.line, tabs[0].range.start.character), (1, 0));
    }

    #[test]
    fn ranges_are_in_utf16_units() {
        let found = diagnose("!active\nключ:nope 1\nключ 2\n", &[]);
        let marker = found.iter().find(|d| d.message.starts_with("Unknown marker")).unwrap();
        assert_eq!((marker.range.start.character, marker.range.end.character), (0, 9));
        let dup = found.iter().find(|d| d.message.starts_with("Duplicate key")).unwrap();
        assert_eq!((dup.range.start.character, dup.range.end.character), (0, 4));
    }

    // One pass with a hash lookup per key: the scope list searched from its
    // start made 100 000 keys take 25 s on every keystroke.
    #[test]
    fn many_keys_are_diagnosed_in_linear_time() {
        let mut text = String::new();
        for i in 0..200_000 {
            text.push_str(&format!("key_{} {}\n", i, i));
        }
        let started = std::time::Instant::now();
        let found = diagnose(&text, &[]);
        assert!(found.is_empty());
        assert!(started.elapsed().as_secs() < 20, "took {:?}", started.elapsed());
    }

    #[test]
    fn package_lookups_stay_inside_synx_packages() {
        let dir = std::env::temp_dir().join(format!("synx-lsp-pkg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root = dir.join("root");
        let pkg = root.join("synx_packages").join("@a").join("b");
        std::fs::create_dir_all(pkg.join("src")).unwrap();
        std::fs::write(pkg.join("src").join("main.synx"), "k v\n").unwrap();
        std::fs::write(dir.join("outside.synx"), "secret 1\n").unwrap();
        std::fs::create_dir_all(dir.join("victim").join("src")).unwrap();
        std::fs::write(dir.join("victim").join("src").join("main.synx"), "secret 1\n").unwrap();
        let roots = vec![root.clone()];

        assert_eq!(resolve_package_main(&roots, "@a/b"), Some(pkg.join("src").join("main.synx")));
        assert!(is_package_installed(&roots, "@a/b"));
        // A package name that walks out of synx_packages/.
        assert_eq!(resolve_package_main(&roots, "@a/../../../victim"), None);
        assert!(!is_package_installed(&roots, "@a/../../../victim"));
        // A manifest entry that walks out of the package falls back to src/main.synx.
        std::fs::write(pkg.join("synx-pkg.synx"), "name @a/b\nmain ../../../../outside.synx\n").unwrap();
        assert_eq!(resolve_package_main(&roots, "@a/b"), Some(pkg.join("src").join("main.synx")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_panic_in_a_handler_costs_one_answer() {
        let quiet = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let answer: Option<u8> = guarded(None, || panic!("bug"));
        std::panic::set_hook(quiet);
        assert_eq!(answer, None);
    }
}
