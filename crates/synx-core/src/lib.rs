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

// safety: the crate reads documents it has no reason to trust, and has no
// `unsafe` of its own — the compiler keeps it that way.
#![forbid(unsafe_code)]

mod value;
mod parser;
mod engine;
mod format;
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
pub use engine::{resolve, resolve_read_only, RUNTIME_ERROR_PREFIXES};
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
    /// - Exactly 2 spaces per indentation level
    /// - Keys sorted alphabetically at every nesting level of a static
    ///   document; list items, and the fields inside one, keep their order
    /// - An `!active` document keeps its key order — markers resolve in it
    /// - One blank line between top-level blocks (objects / lists)
    /// - Comment lines stripped
    /// - Directive lines (`!active`, `!lock`, `!include …`) moved to the top
    ///
    /// The result always reads back as the same data: it is parsed and
    /// compared with the input before it is returned, and a document the
    /// formatter cannot reproduce — or one cut by a parser limit — is
    /// returned unchanged.
    pub fn format(text: &str) -> String {
        format::format(text)
    }

    /// Compile a `.synx` string into compact binary `.synxb` format.
    ///
    /// If `resolved` is true, active markers are resolved first (requires
    /// `!active` mode) and metadata is stripped from the output.
    ///
    /// Resolving reads the process environment and the current directory;
    /// a host compiling a document it did not write uses [`Synx::compile_with`].
    pub fn compile(text: &str, resolved: bool) -> Vec<u8> {
        Self::compile_with(text, resolved, &Options::default())
    }

    /// [`Synx::compile`] with the options `resolved` resolves under: `env`
    /// replaces the process environment, `base_path` is the folder files are
    /// read from. Without `resolved` the options are not used.
    pub fn compile_with(text: &str, resolved: bool, opts: &Options) -> Vec<u8> {
        let mut result = parse(text);
        if resolved && result.mode == Mode::Active {
            resolve(&mut result, opts);
        }
        binary::compile(&result, resolved)
    }

    /// Decompile a `.synxb` binary back into a human-readable `.synx` string.
    ///
    /// A file compiled without `resolved` carries its declarations, and they
    /// are written back: `port:env:default:8080 PORT`, `name[required] x`,
    /// `!include` lines, keys in the order the file holds them. Until 3.8.1
    /// the text kept `!active` and dropped every marker, so reading it gave
    /// the variable's *name* as the port and the secret as plain text.
    ///
    /// A resolved file is written as the static document it now is — its
    /// values are final, and `!active` would have `{braces}` in them
    /// interpolated a second time. `!use` lines are not stored in `.synxb`
    /// and cannot be restored.
    pub fn decompile(data: &[u8]) -> Result<String, String> {
        let result = binary::decompile(data)?;
        // safety: a document is an object, and the compiler writes no other
        // root. A file whose root was a string came back as that string —
        // text of its author's choosing, `!active` and `!include` lines
        // included, out of a file flagged static; a list or a scalar came
        // back as a different document. Such a root has no text form.
        if !matches!(result.root, Value::Object(_)) {
            return Err("the root of this .synxb file is not an object; it has no text form".to_string());
        }
        let resolved = binary::is_resolved(data);
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
        if result.mode == Mode::Active && !resolved {
            out.push_str("!active\n");
        }
        if result.locked {
            out.push_str("!lock\n");
        }
        for inc in &result.includes {
            out.push_str("!include ");
            out.push_str(&inc.path);
            out.push(' ');
            out.push_str(&inc.alias);
            out.push('\n');
        }
        if !out.is_empty() {
            out.push('\n');
        }
        // An unresolved active document goes through the declared writer even
        // without declarations: the plain one writes an empty list as
        // `key:join`, a marker the next `resolve` would apply.
        if result.metadata.is_empty() && !(result.mode == Mode::Active && !resolved) {
            out.push_str(&serialize(&result.root, 0));
        } else {
            synxl::write_synx_declared(&mut out, &result.root, &result.metadata);
        }
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
            // Keys sorted, for an output that does not depend on insertion order.
            let mut out = String::new();
            synxl::write_synx_object(&mut out, map, depth_lvl * 2, depth_lvl);
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

/// What a byte is written as inside a JSON string: `0` as itself, `b'u'` as
/// `\u00XX`, anything else as a backslash and that character.
const JSON_ESCAPE: [u8; 256] = {
    let mut table = [0u8; 256];
    let mut b = 0;
    while b < 0x20 {
        table[b] = b'u';
        b += 1;
    }
    table[b'\n' as usize] = b'n';
    table[b'\r' as usize] = b'r';
    table[b'\t' as usize] = b't';
    table[b'"' as usize] = b'"';
    table[b'\\' as usize] = b'\\';
    table
};

/// Nonzero when one of the eight bytes of `word` has to be escaped in a JSON
/// string: a control character, `"` or `\`.
#[inline]
fn json_escape_in(word: u64) -> u64 {
    const ONES: u64 = 0x0101_0101_0101_0101;
    const HIGH: u64 = 0x8080_8080_8080_8080;
    // Each term is nonzero exactly when some byte is below 0x20, equal to
    // `"`, equal to `\`: a byte with its top bit set never counts.
    let control = word.wrapping_sub(ONES * 0x20) & !word;
    let quote = word ^ (ONES * b'"' as u64);
    let backslash = word ^ (ONES * b'\\' as u64);
    (control | (quote.wrapping_sub(ONES) & !quote) | (backslash.wrapping_sub(ONES) & !backslash)) & HIGH
}

/// Write `s` as a JSON string literal.
///
/// perf: unescaped runs are copied whole, and the run is found eight bytes
/// at a time — text rarely needs an escape at all. The loop before this one
/// examined every byte through a five-way match. Every byte that needs
/// escaping is ASCII, so scanning bytes never splits a character.
fn write_json_str(out: &mut String, s: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push('"');
    let bytes = s.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        while let Some(chunk) = bytes.get(i..i + 8) {
            let word = u64::from_le_bytes(chunk.try_into().expect("eight bytes"));
            if json_escape_in(word) != 0 {
                break;
            }
            i += 8;
        }
        // The chunk with an escape in it, or the tail shorter than a chunk.
        let stop = bytes.len().min(i + 8);
        while i < stop {
            let b = bytes[i];
            let escape = JSON_ESCAPE[b as usize];
            if escape != 0 {
                out.push_str(&s[start..i]);
                out.push('\\');
                out.push(escape as char);
                if escape == b'u' {
                    out.push_str("00");
                    out.push(HEX[(b >> 4) as usize] as char);
                    out.push(HEX[(b & 0xf) as usize] as char);
                }
                start = i + 1;
            }
            i += 1;
        }
    }
    out.push_str(&s[start..]);
    out.push('"');
}

/// Write a Value as JSON string (for FFI output).
pub fn write_json(out: &mut String, val: &Value) {
    let mut sorted = Vec::new();
    write_json_depth(out, val, 0, &mut sorted);
}

/// [`write_json`] for each of `values` in turn, `between` written after
/// every value but the last and `after` after each: one sorting buffer for
/// the whole run instead of one per value.
pub(crate) fn write_json_each(out: &mut String, values: &[Value], between: &str, after: &str) {
    let mut sorted = Vec::new();
    for (i, val) in values.iter().enumerate() {
        if i > 0 {
            out.push_str(between);
        }
        write_json_depth(out, val, 0, &mut sorted);
        out.push_str(after);
    }
}

/// `sorted` is scratch space for the keys of the objects being written, one
/// stretch per open object.
///
/// perf: one buffer for the whole document. Every object used to collect its
/// entries into a vector of its own to sort them.
fn write_json_depth<'a>(out: &mut String, val: &'a Value, depth: usize, sorted: &mut Vec<(&'a str, &'a Value)>) {
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
                if i > 0 {
                    out.push(',');
                }
                write_json_depth(out, item, depth + 1, sorted);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            // Sort keys for deterministic, diffable JSON output
            let start = sorted.len();
            sorted.extend(map.iter().map(|(k, v)| (k.as_str(), v)));
            sorted[start..].sort_unstable_by_key(|(k, _)| *k);
            for at in start..start + map.len() {
                let (key, val) = sorted[at];
                if at > start {
                    out.push(',');
                }
                // Escape the key the same way string values are escaped
                write_json_str(out, key);
                out.push(':');
                write_json_depth(out, val, depth + 1, sorted);
            }
            sorted.truncate(start);
            out.push('}');
        }
    }
}

// Convert a Value to a JSON string.
pub fn to_json(val: &Value) -> String {
    let mut out = String::with_capacity(2048);
    write_json(&mut out, val);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // B-078: a resolved `.synxb` can be built from the environment it is given.
    #[test]
    fn compile_with_resolves_under_the_given_options() {
        let doc = "!active\nhome:env:default:none SYNX_CORE_TEST_COMPILE_WITH\n";
        let mut env = HashMap::new();
        env.insert("SYNX_CORE_TEST_COMPILE_WITH".to_string(), "given".to_string());
        let opts = Options { env: Some(env), ..Default::default() };
        let text = Synx::decompile(&Synx::compile_with(doc, true, &opts)).unwrap();
        assert!(text.contains("home given"), "{text}");

        let empty = Options { env: Some(HashMap::new()), ..Default::default() };
        let text = Synx::decompile(&Synx::compile_with(doc, true, &empty)).unwrap();
        assert!(text.contains("home none"), "{text}");

        // Without `resolved` the options change nothing.
        assert_eq!(Synx::compile_with(doc, false, &opts), Synx::compile(doc, false));
    }

    // B-086: a `.synxb` whose root is not an object has no text form.
    #[test]
    fn decompile_refuses_a_root_that_is_not_an_object() {
        let file_of = |root: Value| {
            let mut result = parse("");
            result.root = root;
            binary::compile(&result, false)
        };
        let roots = [
            Value::String("!active\nleak:env PATH\n!include ../../secret.synx s".into()),
            Value::Bool(true),
            Value::Int(7),
            Value::Null,
            Value::Array(vec![Value::Bool(false), Value::Object(Map::default())]),
        ];
        for root in roots {
            let file = file_of(root.clone());
            // The decoder still reads the file; only the text writer refuses.
            assert_eq!(binary::decompile(&file).unwrap().root, root);
            assert!(Synx::decompile(&file).is_err(), "{:?}", root);
        }
        let object = file_of(parse("a 1\n").root);
        assert_eq!(Synx::decompile(&object).unwrap(), "a 1\n");
    }

    /// The string writer as it was: every byte through a match.
    fn reference_json_str(s: &str) -> String {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }

    // The JSON string writer looks at eight bytes at a time; it must escape
    // exactly what the byte-by-byte one did, wherever in a chunk it falls.
    #[test]
    fn json_strings_are_escaped_as_before() {
        let mut seed = 0x3C6E_F372_FE94_F82Bu64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        // Every byte value, in every position of a chunk.
        for byte in 0u8..=0x7f {
            for at in 0..20 {
                let mut s = "a".repeat(at);
                s.push(byte as char);
                s.push_str(&"b".repeat(19 - at));
                let mut out = String::new();
                write_json_str(&mut out, &s);
                assert_eq!(out, reference_json_str(&s), "{:?}", s);
            }
        }
        let alphabet = ["a", "b", " ", "\"", "\\", "\n", "\r", "\t", "\u{0}", "\u{1f}", "\u{7f}", "é", "я", "\u{2028}", "😀", "/", "\u{80}"];
        for _ in 0..100_000 {
            let len = next() % 40;
            let s: String = (0..len).map(|_| alphabet[(next() % alphabet.len() as u64) as usize]).collect();
            let mut out = String::new();
            write_json_str(&mut out, &s);
            assert_eq!(out, reference_json_str(&s), "{:?}", s);
        }
        // The eight-byte test itself, against the definition.
        for _ in 0..200_000 {
            let word = match next() % 3 {
                0 => next(),
                1 => next() | 0x2020_2020_2020_2020,
                _ => (next() & 0x7f7f_7f7f_7f7f_7f7f) | 0x4040_4040_4040_4040,
            };
            let needs = word.to_le_bytes().iter().any(|&b| b < 0x20 || b == b'"' || b == b'\\');
            assert_eq!(json_escape_in(word) != 0, needs, "{:016x}", word);
        }
    }

    // Objects are written with their keys sorted, at every depth, from one
    // shared buffer.
    #[test]
    fn json_keys_are_sorted_at_every_depth() {
        let doc = parse("z 1\nb\n  y 2\n  a\n    q 3\n    c 4\n  m\n    - k 5\n      d 6\n    - 7\na 8\n");
        assert_eq!(to_json(&doc.root), r#"{"a":8,"b":{"a":{"c":4,"q":3},"m":[{"d":6,"k":5},7],"y":2},"z":1}"#);
        assert_eq!(to_json(&parse("").root), "{}");
        let one = parse("only\n  k v\n");
        assert_eq!(to_json(&one.root), r#"{"only":{"k":"v"}}"#);
    }
}
