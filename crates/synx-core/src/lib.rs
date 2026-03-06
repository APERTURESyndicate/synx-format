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

pub use value::{Value, Mode, ParseResult, Meta, MetaMap, Options};
pub use parser::parse;
pub use engine::resolve;
pub use calc::safe_calc;

/// Main entry point for the SYNX parser.
pub struct Synx;

impl Synx {
    /// Parse a SYNX string into a key-value map (static mode only).
    pub fn parse(text: &str) -> std::collections::HashMap<String, Value> {
        let result = parse(text);
        match result.root {
            Value::Object(map) => map,
            _ => std::collections::HashMap::new(),
        }
    }

    /// Parse with full engine resolution (!active mode).
    pub fn parse_active(text: &str, opts: &Options) -> std::collections::HashMap<String, Value> {
        let mut result = parse(text);
        if result.mode == Mode::Active {
            resolve(&mut result, opts);
        }
        match result.root {
            Value::Object(map) => map,
            _ => std::collections::HashMap::new(),
        }
    }

    /// Parse and return full result including mode and metadata.
    pub fn parse_full(text: &str) -> ParseResult {
        parse(text)
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
}

fn serialize(value: &Value, indent: usize) -> String {
    match value {
        Value::Object(map) => {
            let mut out = String::new();
            let spaces = " ".repeat(indent);
            // Sort keys for deterministic output
            let mut keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
            keys.sort_unstable();
            for key in keys {
                let val = &map[key];
                match val {
                    Value::Array(arr) => {
                        out.push_str(&spaces);
                        out.push_str(key);
                        out.push('\n');
                        for item in arr {
                            match item {
                                Value::Object(inner) => {
                                    let entries: Vec<_> = inner.iter().collect();
                                    if let Some((k, v)) = entries.first() {
                                        out.push_str(&spaces);
                                        out.push_str("  - ");
                                        out.push_str(k);
                                        out.push(' ');
                                        out.push_str(&format_primitive(v));
                                        out.push('\n');
                                        for (k, v) in entries.iter().skip(1) {
                                            out.push_str(&spaces);
                                            out.push_str("    ");
                                            out.push_str(k);
                                            out.push(' ');
                                            out.push_str(&format_primitive(v));
                                            out.push('\n');
                                        }
                                    }
                                }
                                _ => {
                                    out.push_str(&spaces);
                                    out.push_str("  - ");
                                    out.push_str(&format_primitive(item));
                                    out.push('\n');
                                }
                            }
                        }
                    }
                    Value::Object(_) => {
                        out.push_str(&spaces);
                        out.push_str(key);
                        out.push('\n');
                        out.push_str(&serialize(val, indent + 2));
                    }
                    Value::String(s) if s.contains('\n') => {
                        out.push_str(&spaces);
                        out.push_str(key);
                        out.push_str(" |\n");
                        for line in s.lines() {
                            out.push_str(&spaces);
                            out.push_str("  ");
                            out.push_str(line);
                            out.push('\n');
                        }
                    }
                    _ => {
                        out.push_str(&spaces);
                        out.push_str(key);
                        out.push(' ');
                        out.push_str(&format_primitive(val));
                        out.push('\n');
                    }
                }
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

/// Write a Value as JSON string (for FFI output).
pub fn write_json(out: &mut String, val: &Value) {
    match val {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Int(n) => {
            let mut buf = itoa::Buffer::new();
            out.push_str(buf.format(*n));
        }
        Value::Float(f) => {
            let mut buf = ryu::Buffer::new();
            out.push_str(buf.format(*f));
        }
        Value::String(s) | Value::Secret(s) => {
            out.push('"');
            for ch in s.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if (c as u32) < 0x20 => {
                        out.push_str(&format!("\\u{:04x}", c as u32));
                    }
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        Value::Array(arr) => {
            out.push('[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 { out.push(','); }
                write_json(out, item);
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
                out.push('"');
                for ch in key.chars() {
                    match ch {
                        '"'  => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c if (c as u32) < 0x20 => {
                            out.push_str(&format!("\\u{:04x}", c as u32));
                        }
                        c => out.push(c),
                    }
                }
                out.push_str("\":");
                write_json(out, val);
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

fn fmt_parse(lines: &[&str], start: usize, base: usize) -> (Vec<FmtNode>, usize) {
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
        let is_multiline = t.ends_with(" |") || t == "|";
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
            if node.is_multiline || ct.starts_with("- ") {
                node.list_items.push(ct.to_string());
                i += 1;
            } else if ct.starts_with('#') || ct.starts_with("//") {
                i += 1;
            } else {
                let (subs, ni) = fmt_parse(lines, i, ci);
                node.children.extend(subs);
                i = ni;
            }
        }
        nodes.push(node);
    }
    (nodes, i)
}

fn fmt_sort(nodes: &mut Vec<FmtNode>) {
    nodes.sort_unstable_by(|a, b| {
        let ka = a.header.split(|c: char| c.is_whitespace() || c == '[' || c == ':' || c == '(')
            .next().unwrap_or("").to_lowercase();
        let kb = b.header.split(|c: char| c.is_whitespace() || c == '[' || c == ':' || c == '(')
            .next().unwrap_or("").to_lowercase();
        ka.cmp(&kb)
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
    let lines: Vec<&str> = text.lines().collect();
    let mut directives: Vec<&str> = Vec::new();
    let mut body_start = 0usize;

    for (i, &line) in lines.iter().enumerate() {
        let t = line.trim();
        if t == "!active" || t == "!lock" || t == "#!mode:active" {
            directives.push(t);
            body_start = i + 1;
        } else if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            body_start = i + 1;
        } else {
            break;
        }
    }

    let (mut nodes, _) = fmt_parse(&lines, body_start, 0);
    fmt_sort(&mut nodes);

    let mut out = String::with_capacity(text.len());
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
