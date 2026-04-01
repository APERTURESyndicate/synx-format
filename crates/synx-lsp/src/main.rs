use std::collections::HashMap;
use std::sync::Mutex;

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

struct Backend {
    client: Client,
    documents: Mutex<HashMap<Uri, String>>,
}

// ─── Known sets (mirrors VS Code extension logic) ────────────────────────────

const KNOWN_MARKERS: &[&str] = &[
    "random", "calc", "env", "alias", "ref", "inherit", "i18n", "secret", "default",
    "unique", "include", "import", "geo", "template", "split", "join",
    "clamp", "round", "map", "format", "fallback", "once", "version", "watch", "spam",
    "prompt", "vision", "audio",
];

const KNOWN_CONSTRAINTS: &[&str] = &[
    "min", "max", "type", "required", "readonly", "pattern", "enum",
];

const KNOWN_TYPES: &[&str] = &["int", "float", "bool", "string"];

// ─── Diagnostics ─────────────────────────────────────────────────────────────

fn diagnose(text: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let lines: Vec<&str> = text.lines().collect();

    let has_active = lines.iter().any(|l| l.trim() == "!active");

    let mut keys_by_indent: HashMap<usize, Vec<(String, usize)>> = HashMap::new();

    for (i, line) in lines.iter().enumerate() {
        let lineno = i as u32;

        if line.contains('\t') {
            let col = line.find('\t').unwrap_or(0) as u32;
            diags.push(Diagnostic {
                range: Range::new(Position::new(lineno, col), Position::new(lineno, col + 1)),
                severity: Some(DiagnosticSeverity::ERROR),
                message: "Use spaces for indentation, not tabs".into(),
                source: Some("synx".into()),
                ..Default::default()
            });
        }

        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        if trimmed.starts_with('!') || trimmed.starts_with("- ") {
            continue;
        }

        let indent = line.len() - line.trim_start().len();
        if indent % 2 != 0 && indent > 0 {
            diags.push(Diagnostic {
                range: Range::new(Position::new(lineno, 0), Position::new(lineno, indent as u32)),
                severity: Some(DiagnosticSeverity::WARNING),
                message: "Indentation should be a multiple of 2 spaces".into(),
                source: Some("synx".into()),
                ..Default::default()
            });
        }

        let first_char = trimmed.chars().next().unwrap_or(' ');
        if matches!(first_char, '-' | '#' | '/' | '!') {
            continue;
        }

        let key_part = trimmed.split_whitespace().next().unwrap_or("");
        let base_key = key_part.split(':').next().unwrap_or("")
            .split('[').next().unwrap_or("")
            .split('(').next().unwrap_or("");

        let scope_keys = keys_by_indent.entry(indent).or_default();
        if let Some((_, prev_line)) = scope_keys.iter().find(|(k, _)| k == base_key) {
            diags.push(Diagnostic {
                range: Range::new(Position::new(lineno, indent as u32), Position::new(lineno, (indent + base_key.len()) as u32)),
                severity: Some(DiagnosticSeverity::WARNING),
                message: format!("Duplicate key '{}' (first defined on line {})", base_key, prev_line + 1),
                source: Some("synx".into()),
                ..Default::default()
            });
        } else {
            scope_keys.push((base_key.to_string(), i));
        }

        if key_part.contains('(') {
            if let Some(start) = key_part.find('(') {
                if let Some(end) = key_part.find(')') {
                    let type_name = &key_part[start + 1..end];
                    if !KNOWN_TYPES.contains(&type_name) {
                        diags.push(Diagnostic {
                            range: Range::new(Position::new(lineno, (indent + start) as u32), Position::new(lineno, (indent + end + 1) as u32)),
                            severity: Some(DiagnosticSeverity::ERROR),
                            message: format!("Unknown type cast '{}' — allowed: int, float, bool, string", type_name),
                            source: Some("synx".into()),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        let markers: Vec<&str> = key_part.split(':').skip(1).collect();
        for marker in &markers {
            let marker_name = marker.split(|c: char| !c.is_alphanumeric() && c != '_').next().unwrap_or(marker);
            if !marker_name.is_empty() && !KNOWN_MARKERS.contains(&marker_name) {
                if marker_name.parse::<f64>().is_err() {
                    diags.push(Diagnostic {
                        range: Range::new(Position::new(lineno, indent as u32), Position::new(lineno, (indent + key_part.len()) as u32)),
                        severity: Some(DiagnosticSeverity::WARNING),
                        message: format!("Unknown marker ':{}' ", marker_name),
                        source: Some("synx".into()),
                        ..Default::default()
                    });
                }
            }
        }

        if !markers.is_empty() && !has_active {
            diags.push(Diagnostic {
                range: Range::new(Position::new(lineno, indent as u32), Position::new(lineno, (indent + key_part.len()) as u32)),
                severity: Some(DiagnosticSeverity::INFORMATION),
                message: "Markers require !active on the first line to take effect".into(),
                source: Some("synx".into()),
                ..Default::default()
            });
        }

        if key_part.contains('[') {
            let constraint_str = key_part.split('[').nth(1).unwrap_or("").trim_end_matches(']');
            for part in constraint_str.split(',') {
                let ct = part.trim().split(':').next().unwrap_or("").trim();
                if !ct.is_empty() && !KNOWN_CONSTRAINTS.contains(&ct) {
                    diags.push(Diagnostic {
                        range: Range::new(Position::new(lineno, indent as u32), Position::new(lineno, (indent + key_part.len()) as u32)),
                        severity: Some(DiagnosticSeverity::WARNING),
                        message: format!("Unknown constraint '{}'", ct),
                        source: Some("synx".into()),
                        ..Default::default()
                    });
                }
            }
            if !has_active {
                diags.push(Diagnostic {
                    range: Range::new(Position::new(lineno, indent as u32), Position::new(lineno, (indent + key_part.len()) as u32)),
                    severity: Some(DiagnosticSeverity::INFORMATION),
                    message: "Constraints require !active on the first line".into(),
                    source: Some("synx".into()),
                    ..Default::default()
                });
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
    ["!active", "!lock", "!tool", "!schema", "!llm"].iter().map(|&d| {
        CompletionItem {
            label: d.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Directive".into()),
            insert_text: Some(d.to_string()),
            ..Default::default()
        }
    }).collect()
}

// ─── Document Symbols ────────────────────────────────────────────────────────

fn build_symbols(text: &str) -> Vec<DocumentSymbol> {
    let result = synx_core::parse(text);
    match result.root {
        synx_core::Value::Object(map) => {
            let lines: Vec<&str> = text.lines().collect();
            map_to_symbols(&map, &lines, 0)
        }
        _ => vec![],
    }
}

fn map_to_symbols(
    map: &HashMap<String, synx_core::Value>,
    lines: &[&str],
    base_indent: usize,
) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();
    let mut sorted_keys: Vec<&String> = map.keys().collect();
    sorted_keys.sort();

    for key in sorted_keys {
        let val = &map[key];
        let line_num = find_key_line(lines, key, base_indent);
        let range = Range::new(
            Position::new(line_num, base_indent as u32),
            Position::new(line_num, (base_indent + key.len()) as u32),
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
            synx_core::Value::String(s) => {
                if s.len() > 40 { format!("{}...", &s[..37]) } else { s.clone() }
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
            let children = map_to_symbols(inner, lines, base_indent + 2);
            if !children.is_empty() {
                sym.children = Some(children);
            }
        }

        symbols.push(sym);
    }
    symbols
}

fn find_key_line(lines: &[&str], key: &str, min_indent: usize) -> u32 {
    for (i, line) in lines.iter().enumerate() {
        let indent = line.len() - line.trim_start().len();
        if indent < min_indent { continue; }
        let trimmed = line.trim();
        let line_key = trimmed.split(|c: char| c.is_whitespace() || c == ':' || c == '[' || c == '(')
            .next()
            .unwrap_or("");
        if line_key == key {
            return i as u32;
        }
    }
    0
}

// ─── LanguageServer impl ────────────────────────────────────────────────────

impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![":".into(), "[".into(), "!".into()]),
                    ..Default::default()
                }),
                document_symbol_provider: Some(OneOf::Left(true)),
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
        let uri = params.text_document.uri.clone();
        let text = params.text_document.text.clone();
        self.documents.lock().unwrap().insert(uri.clone(), text.clone());
        self.publish_diagnostics(uri, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        if let Some(change) = params.content_changes.into_iter().last() {
            let text = change.text;
            self.documents.lock().unwrap().insert(uri.clone(), text.clone());
            self.publish_diagnostics(uri, &text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.lock().unwrap().remove(&uri);
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        let text: String = {
            let docs = self.documents.lock().unwrap();
            match docs.get(&uri) {
                Some(t) => t.clone(),
                None => return Ok(None),
            }
        };

        let lines: Vec<&str> = text.lines().collect();
        let line = lines.get(pos.line as usize).unwrap_or(&"");
        let before = &line[..std::cmp::min(pos.character as usize, line.len())];

        if before.contains('[') && !before.contains(']') {
            return Ok(Some(CompletionResponse::Array(build_constraint_completions())));
        }

        if before.contains(':') && !before.contains(' ') {
            return Ok(Some(CompletionResponse::Array(build_marker_completions())));
        }

        if before.trim_start().starts_with('!') || before.is_empty() {
            return Ok(Some(CompletionResponse::Array(build_directive_completions())));
        }

        Ok(None)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let text: String = {
            let docs = self.documents.lock().unwrap();
            match docs.get(&uri) {
                Some(t) => t.clone(),
                None => return Ok(None),
            }
        };

        let symbols = build_symbols(&text);
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }
}

impl Backend {
    async fn publish_diagnostics(&self, uri: Uri, text: &str) {
        let diags = diagnose(text);
        self.client.publish_diagnostics(uri, diags, None).await;
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
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
