//! SYNX Parser — converts raw .synx text into a structured value tree
//! with metadata for engine resolution.

use std::collections::HashMap;
use memchr::memchr;
use crate::value::*;
use crate::rng;

// ─── Resource limits (fuzz / hostile input) ─────────────────
// All caps are documented here so callers know parsing is bounded.

/// Maximum UTF-8 bytes accepted per `parse()` (truncate with valid UTF-8 boundary).
pub(crate) const MAX_SYNX_INPUT_BYTES: usize = 16 * 1024 * 1024;

/// Maximum indexed line starts (1 + number of `\n` before truncate). Bounds `line_starts` RAM (~8× on 64-bit).
const MAX_LINE_STARTS: usize = 2_000_000;

/// Indentation-tree depth for nested objects (stack size). Iterative parser — prevents giant parent chains.
const MAX_PARSE_NESTING_DEPTH: usize = 128;

/// Multiline `key |` block body: max accumulated UTF-8 bytes.
const MAX_MULTILINE_BLOCK_BYTES: usize = 1024 * 1024;

/// `- list item` entries per single list.
const MAX_LIST_ITEMS: usize = 1_048_576;

/// `!include` lines per file.
const MAX_INCLUDE_DIRECTIVES: usize = 4096;

/// Max comma-separated parts when parsing `[constraints]` enum values.
const MAX_CONSTRAINT_ENUM_PARTS: usize = 4096;

/// Max `:a:b:c` marker segments on one key line.
const MAX_MARKER_CHAIN_SEGMENTS: usize = 512;

/// Truncate `text` to a UTF-8-safe prefix (used by `parse` and canonical `format`).
pub(crate) fn clamp_synx_text(text: &str) -> &str {
    if text.len() <= MAX_SYNX_INPUT_BYTES {
        return text;
    }
    let slice = &text.as_bytes()[..MAX_SYNX_INPUT_BYTES];
    let end = core::str::from_utf8(slice)
        .map(|s| s.len())
        .unwrap_or_else(|e| e.valid_up_to());
    &text[..end]
}

/// Byte length to parse: full slice, or truncate before the newline that would exceed
/// `MAX_LINE_STARTS` lines (at most `MAX_LINE_STARTS.saturating_sub(1)` `\n` bytes kept).
fn find_parse_end_bytes(bytes: &[u8]) -> usize {
    let max_newlines = MAX_LINE_STARTS.saturating_sub(1);
    let mut seen_newlines = 0usize;
    let mut scan = 0usize;
    while scan < bytes.len() {
        if let Some(rel) = memchr(b'\n', &bytes[scan..]) {
            if seen_newlines >= max_newlines {
                return scan + rel;
            }
            seen_newlines += 1;
            scan += rel + 1;
        } else {
            break;
        }
    }
    bytes.len()
}

/// Parse a SYNX text string into a value tree with metadata.
pub fn parse(text: &str) -> ParseResult {
    let text = clamp_synx_text(text);
    let parse_end = find_parse_end_bytes(text.as_bytes());
    let text = &text[..parse_end];
    let bytes = text.as_bytes();

    let mut line_starts: Vec<usize> = Vec::new();
    line_starts.push(0);
    let mut scan = 0usize;
    while scan < bytes.len() {
        if let Some(rel) = memchr(b'\n', &bytes[scan..]) {
            let pos = scan + rel;
            line_starts.push(pos + 1);
            scan = pos + 1;
        } else {
            break;
        }
    }
    let line_count = line_starts.len();

    let mut root = HashMap::new();
    let mut stack: Vec<(i32, StackEntry)> = vec![(-1, StackEntry::Root)];
    let mut mode = Mode::Static;
    let mut locked = false;
    let mut tool = false;
    let mut schema = false;
    let mut llm = false;
    let mut metadata: HashMap<String, MetaMap> = HashMap::new();
    let mut includes: Vec<IncludeDirective> = Vec::new();

    let mut block: Option<BlockState> = None;
    let mut list: Option<ListState> = None;
    let mut in_block_comment = false;

    let mut i = 0;
    while i < line_count {
        // Extract line without allocating
        let start = line_starts[i];
        let end = if i + 1 < line_count { line_starts[i + 1] - 1 } else { bytes.len() };
        // Handle \r\n
        let end = if end > start && end > 0 && bytes.get(end - 1) == Some(&b'\r') { end - 1 } else { end };
        let raw = &text[start..end];

        let trimmed = raw.trim();

        // Mode declaration
        if trimmed == "!active" {
            mode = Mode::Active;
            i += 1;
            continue;
        }
        if trimmed == "!lock" {
            locked = true;
            i += 1;
            continue;
        }
        if trimmed == "!tool" {
            tool = true;
            i += 1;
            continue;
        }
        if trimmed == "!schema" {
            schema = true;
            i += 1;
            continue;
        }
        if trimmed == "!llm" {
            llm = true;
            i += 1;
            continue;
        }
        if trimmed.starts_with("!include ") {
            if includes.len() < MAX_INCLUDE_DIRECTIVES {
                let rest = trimmed[9..].trim();
                let mut parts = rest.splitn(2, char::is_whitespace);
                let path = parts.next().unwrap_or("").to_string();
                let alias = parts.next().map(|s| s.trim().to_string()).unwrap_or_else(|| {
                    // Auto-derive alias from filename
                    let name = path.rsplit(&['/', '\\'][..]).next().unwrap_or(&path);
                    name.strip_suffix(".synx").or_else(|| name.strip_suffix(".SYNX")).unwrap_or(name).to_string()
                });
                includes.push(IncludeDirective { path, alias });
            }
            i += 1;
            continue;
        }
        if trimmed.starts_with("#!mode:") {
            let declared = trimmed.splitn(2, ':').nth(1).unwrap_or("static").trim();
            mode = if declared == "active" { Mode::Active } else { Mode::Static };
            i += 1;
            continue;
        }

        // Block comment toggle: ###
        if trimmed == "###" {
            in_block_comment = !in_block_comment;
            i += 1;
            continue;
        }
        if in_block_comment {
            i += 1;
            continue;
        }

        // Skip empty / comments
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            i += 1;
            continue;
        }

        let indent = (raw.len() - raw.trim_start().len()) as i32;

        // Continue multiline block
        if let Some(ref mut blk) = block {
            if indent > blk.indent {
                if blk.content.len() < MAX_MULTILINE_BLOCK_BYTES {
                    if !blk.content.is_empty() {
                        blk.content.push('\n');
                    }
                    let room = MAX_MULTILINE_BLOCK_BYTES.saturating_sub(blk.content.len());
                    if room > 0 {
                        let n = trimmed.len().min(room);
                        blk.content.push_str(&trimmed[..n]);
                    }
                }
                i += 1;
                continue;
            } else {
                let content = std::mem::take(&mut blk.content);
                let blk_key = blk.key.clone();
                let blk_stack_idx = blk.stack_idx;
                block = None;
                insert_value(&mut root, &stack, blk_stack_idx, &blk_key, Value::String(content));
            }
        }

        // Continue list items
        if trimmed.starts_with("- ") {
            if let Some(ref mut lst) = list {
                if indent > lst.indent {
                    if lst.items.len() < MAX_LIST_ITEMS {
                        let val_str = strip_comment(trimmed[2..].trim());
                        lst.items.push(cast(&val_str));
                    }
                    i += 1;
                    continue;
                }
            }
        } else if let Some(ref lst) = list {
            if indent <= lst.indent {
                let items = list.take().unwrap();
                let arr = Value::Array(items.items);
                insert_value(&mut root, &stack, items.stack_idx, &items.key, arr);
            }
        }

        // Parse key line
        if let Some(parsed) = parse_line(trimmed) {
            // Pop stack to correct parent
            while stack.len() > 1 && stack.last().unwrap().0 >= indent {
                stack.pop();
            }

            let parent_idx = stack.len() - 1;

            // Save metadata if in active mode
            if mode == Mode::Active
                && (!parsed.markers.is_empty()
                    || parsed.constraints.is_some()
                    || parsed.type_hint.is_some())
            {
                let path = build_path(&stack);
                let meta_map = metadata.entry(path).or_default();
                meta_map.insert(
                    parsed.key.clone(),
                    Meta {
                        markers: parsed.markers.clone(),
                        args: parsed.marker_args.clone(),
                        type_hint: parsed.type_hint.clone(),
                        constraints: parsed.constraints.clone(),
                    },
                );
            }

            let is_block = parsed.value == "|";
            let is_list_marker = parsed.markers.iter().any(|m| {
                matches!(m.as_str(), "random" | "unique" | "geo" | "join")
            });

            if is_block {
                insert_value(
                    &mut root,
                    &stack,
                    parent_idx,
                    &parsed.key,
                    Value::String(String::new()),
                );
                block = Some(BlockState {
                    indent,
                    key: parsed.key,
                    content: String::new(),
                    stack_idx: parent_idx,
                });
            } else if is_list_marker && parsed.value.is_empty() {
                list = Some(ListState {
                    indent,
                    key: parsed.key,
                    items: Vec::new(),
                    stack_idx: parent_idx,
                });
            } else if parsed.value.is_empty() {
                // Peek ahead for list
                let mut peek = i + 1;
                while peek < line_count {
                    let ps = line_starts[peek];
                    let pe = if peek + 1 < line_count {
                        line_starts[peek + 1] - 1
                    } else {
                        bytes.len()
                    };
                    let pe = if pe > ps && bytes.get(pe - 1) == Some(&b'\r') { pe - 1 } else { pe };
                    let pt = text[ps..pe].trim();
                    if !pt.is_empty() {
                        break;
                    }
                    peek += 1;
                }

                if peek < line_count {
                    let ps = line_starts[peek];
                    let pe = if peek + 1 < line_count {
                        line_starts[peek + 1] - 1
                    } else {
                        bytes.len()
                    };
                    let pe = if pe > ps && bytes.get(pe - 1) == Some(&b'\r') { pe - 1 } else { pe };
                    let pt = text[ps..pe].trim();
                    if pt.starts_with("- ") {
                        list = Some(ListState {
                            indent,
                            key: parsed.key,
                            items: Vec::new(),
                            stack_idx: parent_idx,
                        });
                        i += 1;
                        continue;
                    }
                }

                insert_value(
                    &mut root,
                    &stack,
                    parent_idx,
                    &parsed.key,
                    Value::Object(HashMap::new()),
                );
                // Guard against pathological inputs that create extremely deep nesting,
                // which can lead to large allocations (metadata path building, parent navigation, etc).
                // If the cap is hit, we still insert the object but stop increasing nesting.
                if stack.len() < MAX_PARSE_NESTING_DEPTH {
                    stack.push((indent, StackEntry::Key(parsed.key)));
                }
            } else {
                let value = if let Some(ref hint) = parsed.type_hint {
                    cast_typed(&parsed.value, hint)
                } else {
                    cast(&parsed.value)
                };
                insert_value(&mut root, &stack, parent_idx, &parsed.key, value);
            }
        }

        i += 1;
    }

    // Flush pending block
    if let Some(blk) = block {
        insert_value(
            &mut root,
            &stack,
            blk.stack_idx,
            &blk.key,
            Value::String(blk.content),
        );
    }

    // Flush pending list
    if let Some(lst) = list {
        let arr = Value::Array(lst.items);
        insert_value(&mut root, &stack, lst.stack_idx, &lst.key, arr);
    }

    let parsed_root = Value::Object(root);

    // !tool reshaping is deferred — done after engine resolution for !active compatibility.
    // Non-active !tool files are reshaped via Synx::parse_tool() or resolve_tool_output().

    ParseResult {
        root: parsed_root,
        mode,
        locked,
        tool,
        schema,
        llm,
        metadata,
        includes,
    }
}

// ─── !tool output reshaping ──────────────────────────────

/// Reshape parsed tree for `!tool` mode.
///
/// **Call mode** (`!tool` without `!schema`):
///   First top-level key = tool name, its children = params.
///   Output: `{ tool: "name", params: { ... } }`
///
/// **Schema mode** (`!tool` + `!schema`):
///   Each top-level key = tool name, children = param type definitions.
///   Output: `{ tools: [ { name: "tool1", params: { key: "type", ... } }, ... ] }`
pub fn reshape_tool_output(root: &Value, schema: bool) -> Value {
    let map = match root {
        Value::Object(m) => m,
        _ => return root.clone(),
    };

    if schema {
        // Schema mode: list of tool definitions
        let mut tools = Vec::new();
        // Sort for deterministic output
        let mut keys: Vec<&String> = map.keys().collect();
        keys.sort();
        for key in keys {
            let val = &map[key];
            let mut def = HashMap::new();
            def.insert("name".to_string(), Value::String(key.clone()));
            def.insert("params".to_string(), val.clone());
            tools.push(Value::Object(def));
        }
        let mut out = HashMap::new();
        out.insert("tools".to_string(), Value::Array(tools));
        Value::Object(out)
    } else {
        // Call mode: first key = tool name, children = params
        if map.is_empty() {
            let mut out = HashMap::new();
            out.insert("tool".to_string(), Value::Null);
            out.insert("params".to_string(), Value::Object(HashMap::new()));
            return Value::Object(out);
        }

        // Deterministic: pick the first key in source order.
        // Since HashMap doesn't preserve order, sort and take first.
        let mut keys: Vec<&String> = map.keys().collect();
        keys.sort();
        let tool_key = keys[0];
        let tool_value = &map[tool_key];

        let params = match tool_value {
            Value::Object(m) => Value::Object(m.clone()),
            // If tool has a single value (no nested params), wrap it
            _ => Value::Object(HashMap::new()),
        };

        let mut out = HashMap::new();
        out.insert("tool".to_string(), Value::String(tool_key.clone()));
        out.insert("params".to_string(), params);
        Value::Object(out)
    }
}

// ─── Internal types ──────────────────────────────────────

#[derive(Debug)]
enum StackEntry {
    Root,
    Key(String),
}

struct BlockState {
    indent: i32,
    key: String,
    content: String,
    stack_idx: usize,
}

struct ListState {
    indent: i32,
    key: String,
    items: Vec<Value>,
    stack_idx: usize,
}

struct ParsedLine {
    key: String,
    type_hint: Option<String>,
    value: String,
    markers: Vec<String>,
    marker_args: Vec<String>,
    constraints: Option<Constraints>,
}

// ─── Line parser ─────────────────────────────────────────

fn parse_line(trimmed: &str) -> Option<ParsedLine> {
    if trimmed.is_empty()
        || trimmed.starts_with('#')
        || trimmed.starts_with("//")
        || trimmed.starts_with("- ")
    {
        return None;
    }

    let bytes = trimmed.as_bytes();
    let len = bytes.len();

    let first = bytes[0];
    if first == b'[' || first == b':' || first == b'-' || first == b'#' || first == b'/' || first == b'(' {
        return None;
    }

    // Extract key
    let mut pos = 0;
    while pos < len {
        let ch = bytes[pos];
        if ch == b' ' || ch == b'\t' || ch == b'[' || ch == b':' || ch == b'(' {
            break;
        }
        pos += 1;
    }
    let key = trimmed[..pos].to_string();

    // Optional (type)
    let mut type_hint = None;
    if pos < len && bytes[pos] == b'(' {
        let start = pos + 1;
        if let Some(c) = trimmed[start..].find(')') {
            type_hint = Some(trimmed[start..start + c].to_string());
            pos = start + c + 1;
        } else {
            pos += 1;
        }
    }

    // Optional [constraints]
    let mut constraints = None;
    if pos < len && bytes[pos] == b'[' {
        if let Some(close) = trimmed[pos..].find(']') {
            let constraint_str = &trimmed[pos + 1..pos + close];
            constraints = Some(parse_constraints(constraint_str));
            pos += close + 1;
        } else {
            pos += 1;
        }
    }

    // Optional :markers
    let mut markers = Vec::new();
    let mut marker_args = Vec::new();
    if pos < len && bytes[pos] == b':' {
        let marker_start = pos + 1;
        let mut marker_end = marker_start;
        while marker_end < len && bytes[marker_end] != b' ' && bytes[marker_end] != b'\t' {
            marker_end += 1;
        }
        let chain = &trimmed[marker_start..marker_end];
        markers = chain
            .split(':')
            .take(MAX_MARKER_CHAIN_SEGMENTS)
            .map(|s| s.to_string())
            .collect();
        pos = marker_end;
    }

    // Skip whitespace
    while pos < len && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
        pos += 1;
    }

    // Value
    let mut raw_value = if pos < len {
        strip_comment(&trimmed[pos..])
    } else {
        String::new()
    };

    // For :random — parse weight percentages from value
    if markers.contains(&"random".to_string()) && !raw_value.is_empty() {
        let parts: Vec<&str> = raw_value.split_whitespace().collect();
        let nums: Vec<String> = parts
            .iter()
            .filter(|s| s.parse::<f64>().is_ok())
            .map(|s| s.to_string())
            .collect();
        if !nums.is_empty() {
            marker_args = nums;
            raw_value.clear();
        }
    }

    Some(ParsedLine {
        key,
        type_hint,
        value: raw_value,
        markers,
        marker_args,
        constraints,
    })
}

// ─── Constraints parser ──────────────────────────────────

fn parse_constraints(raw: &str) -> Constraints {
    let mut c = Constraints::default();
    for part in raw.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if part == "required" {
            c.required = true;
        } else if part == "readonly" {
            c.readonly = true;
        } else if let Some(colon) = part.find(':') {
            let key = part[..colon].trim();
            let val = part[colon + 1..].trim();
            match key {
                "min" => c.min = val.parse().ok(),
                "max" => c.max = val.parse().ok(),
                "type" => c.type_name = Some(val.to_string()),
                "pattern" => c.pattern = Some(val.to_string()),
                "enum" => {
                    c.enum_values = Some(
                        val.split('|')
                            .take(MAX_CONSTRAINT_ENUM_PARTS)
                            .map(|s| s.to_string())
                            .collect(),
                    );
                }
                _ => {}
            }
        }
    }
    c
}

// ─── Value casting ───────────────────────────────────────

fn cast(val: &str) -> Value {
    // Quoted strings preserve literal value (bypass auto-casting)
    // "null" → String("null"), "true" → String("true"), "123" → String("123")
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
            let bytes = val.as_bytes();
            let len = bytes.len();
            if len == 0 {
                return Value::String(String::new());
            }

            let mut start = 0;
            if bytes[0] == b'-' {
                if len == 1 {
                    return Value::String(val.to_string());
                }
                start = 1;
            }

            if bytes[start] >= b'0' && bytes[start] <= b'9' {
                let mut dot_pos = None;
                let mut all_numeric = true;
                for j in start..len {
                    if bytes[j] == b'.' {
                        if dot_pos.is_some() {
                            all_numeric = false;
                            break;
                        }
                        dot_pos = Some(j);
                    } else if bytes[j] < b'0' || bytes[j] > b'9' {
                        all_numeric = false;
                        break;
                    }
                }
                if all_numeric {
                    if let Some(dp) = dot_pos {
                        if dp > start && dp < len - 1 {
                            if let Ok(f) = val.parse::<f64>() {
                                return Value::Float(f);
                            }
                        }
                    } else if let Ok(n) = val.parse::<i64>() {
                        return Value::Int(n);
                    }
                }
            }

            Value::String(val.to_string())
        }
    }
}

fn cast_typed(val: &str, hint: &str) -> Value {
    match hint {
        "int" => Value::Int(val.parse().unwrap_or(0)),
        "float" => Value::Float(val.parse().unwrap_or(0.0)),
        "bool" => Value::Bool(val.trim() == "true"),
        "string" => Value::String(val.to_string()),
        "random" | "random:int" => Value::Int(rng::random_i64()),
        "random:float" => Value::Float(rng::random_f64_01()),
        "random:bool" => Value::Bool(rng::random_bool()),
        _ => cast(val),
    }
}

fn strip_comment(val: &str) -> String {
    let mut result = val.to_string();
    if let Some(idx) = result.find(" //") {
        result.truncate(idx);
    }
    if let Some(idx) = result.find(" #") {
        result.truncate(idx);
    }
    result.trim_end().to_string()
}

// ─── Tree helpers ────────────────────────────────────────

fn build_path(stack: &[(i32, StackEntry)]) -> String {
    let mut parts = Vec::new();
    for (_, entry) in stack.iter().skip(1) {
        if let StackEntry::Key(ref k) = entry {
            parts.push(k.as_str());
        }
    }
    parts.join(".")
}

fn insert_value(
    root: &mut HashMap<String, Value>,
    stack: &[(i32, StackEntry)],
    parent_idx: usize,
    key: &str,
    value: Value,
) {
    if let Some(target) = navigate_to_parent(root, stack, parent_idx) {
        target.insert(key.to_string(), value);
    }
    // If the path is broken the line is silently skipped — this should not
    // happen under well-formed input; malformed input simply loses the entry
    // rather than inserting it at the wrong nesting level.
}

fn navigate_to_parent<'a>(
    root: &'a mut HashMap<String, Value>,
    stack: &[(i32, StackEntry)],
    target_idx: usize,
) -> Option<&'a mut HashMap<String, Value>> {
    if target_idx == 0 {
        return Some(root);
    }

    let path: Vec<&str> = stack
        .iter()
        .skip(1)
        .take(target_idx)
        .filter_map(|(_, entry)| match entry {
            StackEntry::Key(k) => Some(k.as_str()),
            _ => None,
        })
        .collect();

    // SAFETY: We navigate a tree of nested HashMaps using a raw pointer to
    // work around the borrow-checker's inability to track that successive
    // `get_mut` calls target disjoint subtrees.  The invariants that make
    // this sound are:
    //   1. `root` is a valid, exclusively-owned mutable reference for 'a.
    //   2. We descend strictly downward and never alias: at each step we
    //      replace `current` with a pointer to a child map, discarding the
    //      parent pointer.
    //   3. The returned reference re-borrows from `root`'s lifetime 'a and
    //      is the only mutable reference handed out by this function.
    let mut current = root as *mut HashMap<String, Value>;
    for key in path {
        let child = unsafe { (*current).get_mut(key) };
        match child {
            Some(Value::Object(map)) => current = map as *mut HashMap<String, Value>,
            _ => return None, // Path segment missing or not an Object
        }
    }
    Some(unsafe { &mut *current })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_key_value() {
        let data = parse("name Wario\nage 30\nactive true\nscore 99.5\nempty null");
        let root = data.root.as_object().unwrap();
        assert_eq!(root["name"], Value::String("Wario".into()));
        assert_eq!(root["age"], Value::Int(30));
        assert_eq!(root["active"], Value::Bool(true));
        assert_eq!(root["score"], Value::Float(99.5));
        assert_eq!(root["empty"], Value::Null);
        assert_eq!(data.mode, Mode::Static);
    }

    #[test]
    fn test_nested_objects() {
        let data = parse("server\n  host 0.0.0.0\n  port 8080\n  ssl\n    enabled true");
        let root = data.root.as_object().unwrap();
        let server = root["server"].as_object().unwrap();
        assert_eq!(server["host"], Value::String("0.0.0.0".into()));
        assert_eq!(server["port"], Value::Int(8080));
        let ssl = server["ssl"].as_object().unwrap();
        assert_eq!(ssl["enabled"], Value::Bool(true));
    }

    #[test]
    fn test_lists() {
        let data = parse("inventory\n  - Sword\n  - Shield\n  - Potion");
        let root = data.root.as_object().unwrap();
        let inv = root["inventory"].as_array().unwrap();
        assert_eq!(inv.len(), 3);
        assert_eq!(inv[0], Value::String("Sword".into()));
    }

    #[test]
    fn test_multiline_block() {
        let data = parse("rules |\n  Rule one.\n  Rule two.\n  Rule three.");
        let root = data.root.as_object().unwrap();
        assert_eq!(
            root["rules"],
            Value::String("Rule one.\nRule two.\nRule three.".into())
        );
    }

    #[test]
    fn test_comments() {
        let data = parse("# comment\nname Wario # inline\nage 30 // inline");
        let root = data.root.as_object().unwrap();
        assert_eq!(root["name"], Value::String("Wario".into()));
        assert_eq!(root["age"], Value::Int(30));
    }

    #[test]
    fn test_active_mode() {
        let data = parse("!active\nprice 100\ntax:calc price * 0.2");
        assert_eq!(data.mode, Mode::Active);
        let root = data.root.as_object().unwrap();
        assert_eq!(root["price"], Value::Int(100));
        // Before engine resolution, :calc value is a string
        assert_eq!(root["tax"], Value::String("price * 0.2".into()));
        // Metadata should be saved
        let meta = data.metadata.get("").unwrap();
        assert!(meta.contains_key("tax"));
        assert_eq!(meta["tax"].markers, vec!["calc"]);
    }

    #[test]
    fn test_markers_env_default() {
        let data = parse("!active\nport:env:default:3000 PORT");
        let meta = data.metadata.get("").unwrap();
        assert_eq!(meta["port"].markers, vec!["env", "default", "3000"]);
    }

    #[test]
    fn test_type_hint() {
        let data = parse("zip(string) 90210");
        let root = data.root.as_object().unwrap();
        assert_eq!(root["zip"], Value::String("90210".into()));
    }

    #[test]
    fn test_constraints() {
        let data = parse("!active\nname[min:3, max:30, required] Wario");
        let meta = data.metadata.get("").unwrap();
        let c = meta["name"].constraints.as_ref().unwrap();
        assert_eq!(c.min, Some(3.0));
        assert_eq!(c.max, Some(30.0));
        assert!(c.required);
    }

    #[test]
    fn test_random_weights() {
        let data = parse("!active\ntier:random 90 5 5");
        let meta = data.metadata.get("").unwrap();
        assert_eq!(meta["tier"].markers, vec!["random"]);
        assert_eq!(meta["tier"].args, vec!["90", "5", "5"]);
    }

    #[test]
    fn test_tool_directive_flags() {
        let data = parse("!tool\nweb_search\n  query test\n  lang ru\n");
        assert!(data.tool);
        assert!(!data.schema);
        assert_eq!(data.mode, Mode::Static);
        // Raw parse keeps original tree structure
        let root = data.root.as_object().unwrap();
        let ws = root["web_search"].as_object().unwrap();
        assert_eq!(ws["query"], Value::String("test".into()));
        assert_eq!(ws["lang"], Value::String("ru".into()));
    }

    #[test]
    fn test_tool_schema_flags() {
        let data = parse("!tool\n!schema\nweb_search\n  query string\n");
        assert!(data.tool);
        assert!(data.schema);
    }

    #[test]
    fn test_llm_directive() {
        let data = parse("!llm\ncontext\n  user_profile demo\ntask summarize\n");
        assert!(data.llm);
        assert!(!data.tool);
        let root = data.root.as_object().unwrap();
        assert_eq!(root["task"], Value::String("summarize".into()));
        let ctx = root["context"].as_object().unwrap();
        assert_eq!(ctx["user_profile"], Value::String("demo".into()));
    }

    #[test]
    fn test_parse_caps_nesting_depth() {
        // Pathological input: one key per line, increasing indentation each time,
        // with empty values so every line would normally create a new nested object.
        let mut s = String::new();
        for i in 0..(MAX_PARSE_NESTING_DEPTH as usize + 64) {
            s.push_str(&" ".repeat(i));
            s.push_str(&format!("k{i}\n"));
        }

        let data = parse(&s);
        let mut cur = data.root.as_object().unwrap();
        let mut depth = 0usize;
        // Follow the single-child chain while it stays nested.
        loop {
            if cur.len() != 1 {
                break;
            }
            let (_, v) = cur.iter().next().unwrap();
            match v {
                Value::Object(next) => {
                    depth += 1;
                    cur = next;
                }
                _ => break,
            }
        }

        assert!(depth <= MAX_PARSE_NESTING_DEPTH);
    }

    #[test]
    fn test_tool_call_reshape() {
        let data = parse("!tool\nweb_search\n  query test\n  lang ru\n");
        let shaped = reshape_tool_output(&data.root, false);
        let m = shaped.as_object().unwrap();
        assert_eq!(m["tool"], Value::String("web_search".into()));
        let params = m["params"].as_object().unwrap();
        assert_eq!(params["query"], Value::String("test".into()));
        assert_eq!(params["lang"], Value::String("ru".into()));
    }

    #[test]
    fn test_tool_schema_reshape() {
        let data = parse("!tool\n!schema\nweb_search\n  query string\n  lang string\nmemory_write\n  path string\n  value string\n");
        let shaped = reshape_tool_output(&data.root, true);
        let m = shaped.as_object().unwrap();
        let tools = m["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        // Sorted: memory_write before web_search
        let t0 = tools[0].as_object().unwrap();
        assert_eq!(t0["name"], Value::String("memory_write".into()));
        let p0 = t0["params"].as_object().unwrap();
        assert_eq!(p0["path"], Value::String("string".into()));
        let t1 = tools[1].as_object().unwrap();
        assert_eq!(t1["name"], Value::String("web_search".into()));
    }

    #[test]
    fn test_tool_empty() {
        let data = parse("!tool\n");
        assert!(data.tool);
        let shaped = reshape_tool_output(&data.root, false);
        let m = shaped.as_object().unwrap();
        assert_eq!(m["tool"], Value::Null);
    }

    #[test]
    fn test_tool_with_active() {
        let data = parse("!tool\n!active\nweb_search\n  port:env:default:8080 PORT\n");
        assert!(data.tool);
        assert_eq!(data.mode, Mode::Active);
        // Metadata should be captured for :env:default
        let meta = data.metadata.get("web_search").unwrap();
        assert_eq!(meta["port"].markers, vec!["env", "default", "8080"]);
    }
}
