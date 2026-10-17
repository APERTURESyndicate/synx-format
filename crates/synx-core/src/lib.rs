//! # SYNX Core — The Active Data Format
//!
//! High-performance SYNX parser with full `!active` engine support.
//! Single Rust crate powering all language bindings.
//!
//! ```rust
//! use synx_core::{Synx, Value};
//!
//! let data = Synx::parse("name Wario\nage 30\nactive true");
//! assert_eq!(data["name"], Value::String("Wario".into()));
//! assert_eq!(data["age"], Value::Int(30));
//! ```

mod value;
mod parser;
mod engine;
mod calc;
pub(crate) mod rng;
pub mod binary;
pub mod diff;
pub mod schema_json;
pub mod synxl;
#[cfg(feature = "wasm")]
pub mod wasm;
#[cfg(feature = "signing")]
pub mod signing;

pub use value::{Value, Map, MapHasher, Mode, ParseResult, Meta, MetaMap, Options, Constraints, IncludeDirective, UseDirective};
pub use schema_json::{metadata_to_json_schema, value_to_json_value};
#[cfg(feature = "jsonschema")]
pub use schema_json::{validate_serde_json, validate_with_json_schema};
pub use parser::{parse, parse_with, reshape_tool_output, ParserOptions};
pub use synxl::{
    parse_lines, parse_lines_with, records_to_json_array, records_to_ndjson, write_document,
    write_lines, Diagnostic, DiagnosticKind, FieldDecl, FieldList, SynxlDocument, SynxlError,
    SynxlErrorKind, SynxlOptions, SynxlReader, SynxlReaderOwned, SynxlRecord, SynxlStreamError,
    SynxlStreamReader,
};
pub use engine::resolve;
pub use calc::safe_calc;
pub use diff::{diff as diff_objects, DiffResult, DiffChange, diff_to_value};

/// Main entry point for the SYNX parser.
pub struct Synx;

impl Synx {
    /// Parse a SYNX string into a key-value map (static mode only).
    pub fn parse(text: &str) -> crate::Map {
        let result = parse(text);
        match result.root {
            Value::Object(map) => map,
            _ => Map::default(),
        }
    }

    /// Parse with full engine resolution (!active mode).
    pub fn parse_active(text: &str, opts: &Options) -> crate::Map {
        let mut result = parse(text);
        if result.mode == Mode::Active {
            resolve(&mut result, opts);
        }
        match result.root {
            Value::Object(map) => map,
            _ => Map::default(),
        }
    }

    /// Parse and return full result including mode and metadata.
    pub fn parse_full(text: &str) -> ParseResult {
        parse(text)
    }

    /// Parse a `!tool` call: returns `{ tool: "name", params: { ... } }`.
    ///
    /// If the text is also `!active`, markers (`:env`, `:default`, etc.)
    /// are resolved before reshaping.
    pub fn parse_tool(text: &str, opts: &Options) -> crate::Map {
        let mut result = parse(text);
        if result.mode == Mode::Active {
            resolve(&mut result, opts);
        }
        let shaped = reshape_tool_output(&result.root, result.schema);
        match shaped {
            Value::Object(map) => map,
            _ => Map::default(),
        }
    }

    /// Stringify a Value back to SYNX format.
    pub fn stringify(value: &Value) -> String {
        serialize(value, 0)
    }

    /// Reformat a .synx string into canonical form:
    /// - Keys sorted alphabetically at every nesting level
    /// - Exactly 2 spaces per indentation level
    /// - One blank line between top-level blocks (objects / lists)
    /// - Comments stripped — canonical form is comment-free
    /// - Directive lines (`!active`, `!lock`) preserved at the top
    ///
    /// The same data always produces byte-for-byte identical output,
    /// making `.synx` files deterministic and noise-free in `git diff`.
    pub fn format(text: &str) -> String {
        fmt_canonical(text)
    }

    /// Compile a `.synx` string into compact binary `.synxb` format.
    ///
    /// If `resolved` is true, active markers are resolved first (requires
    /// `!active` mode) and metadata is stripped from the output.
    pub fn compile(text: &str, resolved: bool) -> Vec<u8> {
        let mut result = parse(text);
        if resolved && result.mode == Mode::Active {
            resolve(&mut result, &Options::default());
        }
        binary::compile(&result, resolved)
    }

    /// Decompile a `.synxb` binary back into a human-readable `.synx` string.
    pub fn decompile(data: &[u8]) -> Result<String, String> {
        let result = binary::decompile(data)?;
        let mut out = String::new();
        if result.tool {
            out.push_str("!tool\n");
        }
        if result.schema {
            out.push_str("!schema\n");
        }
        if result.llm {
            out.push_str("!llm\n");
        }
        if result.mode == Mode::Active {
            out.push_str("!active\n");
        }
        if result.locked {
            out.push_str("!lock\n");
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&serialize(&result.root, 0));
        Ok(out)
    }

    /// Check if data is a `.synxb` binary file.
    pub fn is_synxb(data: &[u8]) -> bool {
        binary::is_synxb(data)
    }

    /// Structural diff between two parsed SYNX objects.
    ///
    /// Returns added / removed / changed / unchanged keys.
    pub fn diff(
        a: &crate::Map,
        b: &crate::Map,
    ) -> DiffResult {
        diff::diff(a, b)
    }
}

/// Nesting depth for `serialize` / stringify (prevents stack blowup on pathological `Value` trees).
const MAX_SERIALIZE_DEPTH: usize = 128;

fn serialize(value: &Value, depth_lvl: usize) -> String {
    if depth_lvl > MAX_SERIALIZE_DEPTH {
        return "[synx:max-depth]\n".to_string();
    }
    match value {
        // spec: written so that parsing reads back the same data — the SYNXL
        // block writer (§14.1 round-trip): strings that would cast or be cut
        // as a comment are quoted or carried as `|+`, multi-line text keeps
        // its relative indent, list objects lead with a key that fits the
        // `- ` line. The previous writer quoted nothing (`"007"` came back as
        // 7, `"true"` as a bool), wrote `|+` bodies as `|` (code lost its
        // indent) and printed objects inside list items as `[Object]`.
        Value::Object(map) => {
            let mut out = String::new();
            // Sort keys for deterministic output
            let mut keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
            keys.sort_unstable();
            for key in keys {
                synxl::write_synx_entry(&mut out, key, &map[key], depth_lvl * 2, depth_lvl);
            }
            out
        }
        _ => format_primitive(value),
    }
}

fn format_primitive(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => {
            let s = f.to_string();
            if s.contains('.') { s } else { format!("{}.0", s) }
        }
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(format_primitive).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(_) => "[Object]".to_string(),
        Value::Secret(_) => "[SECRET]".to_string(),
    }
}

/// Max nesting for JSON emission (matches stringify guard).
const MAX_JSON_DEPTH: usize = 128;

/// Write `s` as a JSON string literal.
///
/// perf: unescaped runs are copied whole; the previous loop pushed one `char`
/// at a time and formatted control characters through `format!`. Every byte
/// that needs escaping is ASCII, so scanning bytes never splits a character.
fn write_json_str(out: &mut String, s: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push('"');
    let bytes = s.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let esc = match b {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            0..=0x1f => "",
            _ => continue,
        };
        out.push_str(&s[start..i]);
        if esc.is_empty() {
            out.push_str("\\u00");
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0xf) as usize] as char);
        } else {
            out.push_str(esc);
        }
        start = i + 1;
    }
    out.push_str(&s[start..]);
    out.push('"');
}

/// Write a Value as JSON string (for FFI output).
pub fn write_json(out: &mut String, val: &Value) {
    write_json_depth(out, val, 0);
}

fn write_json_depth(out: &mut String, val: &Value, depth: usize) {
    if depth > MAX_JSON_DEPTH {
        out.push_str("null");
        return;
    }
    match val {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Int(n) => {
            let mut buf = itoa::Buffer::new();
            out.push_str(buf.format(*n));
        }
        Value::Float(f) => {
            // JSON has no NaN / ±Infinity — emit null so output stays valid.
            if f.is_nan() || f.is_infinite() {
                out.push_str("null");
            } else {
                let mut buf = ryu::Buffer::new();
                out.push_str(buf.format(*f));
            }
        }
        Value::String(s) => write_json_str(out, s),
        Value::Secret(_) => {
            // Secrets MUST never appear in JSON output. Per the README contract
            // ("`:secret` → не попадёт в логи"), we emit a fixed redaction
            // marker. Callers that need the underlying value must use the
            // typed Value API and `Value::as_secret()` directly.
            out.push_str("\"[SECRET]\"");
        }
        Value::Array(arr) => {
            out.push('[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 { out.push(','); }
                write_json_depth(out, item, depth + 1);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            let mut first = true;
            // Sort keys for deterministic, diffable JSON output
            let mut entries: Vec<(&str, &Value)> =
                map.iter().map(|(k, v)| (k.as_str(), v)).collect();
            entries.sort_unstable_by_key(|(k, _)| *k);
            for (key, val) in entries {
                if !first { out.push(','); }
                first = false;
                // Escape the key the same way string values are escaped
                write_json_str(out, key);
                out.push(':');
                write_json_depth(out, val, depth + 1);
            }
            out.push('}');
        }
    }
}

/// Convert a Value to a JSON string.
pub fn to_json(val: &Value) -> String {
    let mut out = String::with_capacity(2048);
    write_json(&mut out, val);
    out
}

// ─── Canonical Formatter ─────────────────────────────────────────────────────

struct FmtNode {
    header: String,
    children: Vec<FmtNode>,
    list_items: Vec<String>,
    is_multiline: bool,
}

fn fmt_indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

const MAX_FMT_PARSE_DEPTH: usize = 128;

fn fmt_parse(lines: &[&str], start: usize, base: usize, depth: usize) -> (Vec<FmtNode>, usize) {
    if depth > MAX_FMT_PARSE_DEPTH {
        // safety: the subtree is dropped, but its lines are consumed —
        // returning `start` left the caller on the same line forever, so a
        // file nested deeper than the cap hung `format`.
        let mut i = start;
        while i < lines.len() {
            let raw = lines[i];
            if !raw.trim().is_empty() && fmt_indent(raw) < base {
                break;
            }
            i += 1;
        }
        return (Vec::new(), i);
    }
    let mut nodes = Vec::new();
    let mut i = start;
    while i < lines.len() {
        let raw = lines[i];
        let t = raw.trim();
        if t.is_empty() { i += 1; continue; }
        let ind = fmt_indent(raw);
        if ind < base { break; }
        if ind > base { i += 1; continue; }
        if t.starts_with("- ") || t.starts_with('#') || t.starts_with("//") { i += 1; continue; }
        // spec: `|+` opens a block too. Read as a header, its body lines were
        // parsed as child keys and sorted — reordering the text of a code
        // sample or prompt. Its lines keep their indent relative to the first.
        let preserve = t.ends_with(" |+") || t == "|+";
        let is_multiline = preserve || t.ends_with(" |") || t == "|";
        let mut block_base: Option<usize> = None;
        let mut node = FmtNode {
            header: t.to_string(),
            children: Vec::new(),
            list_items: Vec::new(),
            is_multiline,
        };
        i += 1;
        while i < lines.len() {
            let cr = lines[i];
            let ct = cr.trim();
            if ct.is_empty() { i += 1; continue; }
            let ci = fmt_indent(cr);
            if ci <= base { break; }
            if preserve {
                let b = *block_base.get_or_insert(ci);
                let body = cr.trim_end();
                let cut = ws_prefix_len(body, b.min(ci));
                node.list_items.push(body[cut..].to_string());
                i += 1;
            } else if node.is_multiline || ct.starts_with("- ") {
                node.list_items.push(ct.to_string());
                i += 1;
            } else if ct.starts_with('#') || ct.starts_with("//") {
                i += 1;
            } else {
                let (subs, ni) = fmt_parse(lines, i, ci, depth + 1);
                node.children.extend(subs);
                i = ni;
            }
        }
        nodes.push(node);
    }
    (nodes, i)
}

/// Byte length of at most `limit` bytes of leading whitespace, ending on a
/// character boundary.
fn ws_prefix_len(s: &str, limit: usize) -> usize {
    let mut off = 0;
    for ch in s.chars() {
        if off >= limit || !ch.is_whitespace() {
            break;
        }
        off += ch.len_utf8();
    }
    off
}

fn fmt_sort(nodes: &mut Vec<FmtNode>) {
    // Stable: keys that compare equal (a repeated key) keep document order,
    // as in the TypeScript formatter. perf: each key is lowercased once.
    nodes.sort_by_cached_key(|n| {
        n.header
            .split(|c: char| c.is_whitespace() || c == '[' || c == ':' || c == '(')
            .next()
            .unwrap_or("")
            .to_lowercase()
    });
    for node in nodes.iter_mut() {
        fmt_sort(&mut node.children);
    }
}

fn fmt_emit(nodes: &[FmtNode], indent: usize, out: &mut String) {
    let sp = " ".repeat(indent);
    let item_sp = " ".repeat(indent + 2);
    for n in nodes {
        out.push_str(&sp);
        out.push_str(&n.header);
        out.push('\n');
        if !n.children.is_empty() {
            fmt_emit(&n.children, indent + 2, out);
        }
        for li in &n.list_items {
            out.push_str(&item_sp);
            out.push_str(li);
            out.push('\n');
        }
        if indent == 0 && (!n.children.is_empty() || !n.list_items.is_empty()) {
            out.push('\n');
        }
    }
}

fn fmt_canonical(text: &str) -> String {
    let text = parser::clamp_synx_text(text);
    let lines: Vec<&str> = text.lines().collect();
    let mut directives: Vec<&str> = Vec::new();
    let mut body_start = 0usize;

    for (i, &line) in lines.iter().enumerate() {
        let t = line.trim();
        if t == "!active"
            || t == "!lock"
            || t == "!tool"
            || t == "!schema"
            || t == "!llm"
            || t == "#!mode:active"
        {
            directives.push(t);
            body_start = i + 1;
        } else if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            body_start = i + 1;
        } else {
            break;
        }
    }

    let (mut nodes, _) = fmt_parse(&lines, body_start, 0, 0);
    fmt_sort(&mut nodes);

    let cap = text.len().min(parser::MAX_SYNX_INPUT_BYTES).max(64);
    let mut out = String::with_capacity(cap);
    if !directives.is_empty() {
        out.push_str(&directives.join("\n"));
        out.push_str("\n\n");
    }
    fmt_emit(&nodes, 0, &mut out);
    // Trim trailing blank lines, ensure single newline at end
    let trimmed = out.trim_end();
    let mut result = trimmed.to_string();
    result.push('\n');
    result
}

#[cfg(test)]
mod format_tests {

    #[test]
    fn format_finishes_past_the_depth_cap() {
        // Past the cap the subtree is dropped; `format` used to loop forever on it.
        let mut text = String::new();
        for i in 0..140 {
            text.push_str(&"  ".repeat(i));
            text.push_str(&format!("k{}\n", i));
        }
        let out = crate::Synx::format(&text);
        assert!(out.starts_with("k0"));
        assert!(out.lines().count() < 140);
    }
    use super::Synx;

    // `|+` bodies are text: the formatter must neither sort nor re-indent
    // them, and formatting twice changes nothing.
    #[test]
    fn format_keeps_preserve_indent_block_verbatim() {
        let src = "code |+\n  zeta()\n    alpha()\n  beta\nname x\n";
        let out = Synx::format(src);
        assert_eq!(out, "code |+\n  zeta()\n    alpha()\n  beta\n\nname x\n");
        assert_eq!(Synx::parse(&out)["code"], Synx::parse(src)["code"]);
        assert_eq!(Synx::format(&out), out);
    }

    // Equal keys keep document order (stable sort), so the last one still wins.
    #[test]
    fn format_sort_is_stable_for_repeated_keys() {
        let src = "b 1\na first\nc 2\na second\n";
        let out = Synx::format(src);
        assert_eq!(Synx::parse(&out)["a"], Synx::parse(src)["a"]);
    }
}
