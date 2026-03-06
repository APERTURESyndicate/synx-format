//! SYNX Parser — converts raw .synx text into a structured value tree
//! with metadata for engine resolution.

use std::collections::HashMap;
use memchr::memchr_iter;
use crate::value::*;
use crate::rng;

/// Parse a SYNX text string into a value tree with metadata.
pub fn parse(text: &str) -> ParseResult {
    let bytes = text.as_bytes();

    // SIMD-accelerated line splitting via memchr
    let mut line_starts: Vec<usize> = Vec::with_capacity(64);
    line_starts.push(0);
    for pos in memchr_iter(b'\n', bytes) {
        line_starts.push(pos + 1);
    }
    let line_count = line_starts.len();

    let mut root = HashMap::new();
    let mut stack: Vec<(i32, StackEntry)> = vec![(-1, StackEntry::Root)];
    let mut mode = Mode::Static;
    let mut locked = false;
    let mut metadata: HashMap<String, MetaMap> = HashMap::new();

    let mut block: Option<BlockState> = None;
    let mut list: Option<ListState> = None;

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
        if trimmed.starts_with("#!mode:") {
            let declared = trimmed.splitn(2, ':').nth(1).unwrap_or("static").trim();
            mode = if declared == "active" { Mode::Active } else { Mode::Static };
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
                let line = trimmed.replace("/n", "\n");
                if !blk.content.is_empty() {
                    blk.content.push('\n');
                }
                blk.content.push_str(&line);
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
                    let val_str = strip_comment(trimmed[2..].trim());
                    let val_str = val_str.replace("/n", "\n");
                    lst.items.push(cast(&val_str));
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
                stack.push((indent, StackEntry::Key(parsed.key)));
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

    ParseResult {
        root: Value::Object(root),
        mode,
        locked,
        metadata,
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
        }
    }

    // Optional [constraints]
    let mut constraints = None;
    if pos < len && bytes[pos] == b'[' {
        if let Some(close) = trimmed[pos..].find(']') {
            let constraint_str = &trimmed[pos + 1..pos + close];
            constraints = Some(parse_constraints(constraint_str));
            pos += close + 1;
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
        markers = chain.split(':').map(|s| s.to_string()).collect();
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
                "enum" => c.enum_values = Some(val.split('|').map(|s| s.to_string()).collect()),
                _ => {}
            }
        }
    }
    c
}

// ─── Value casting ───────────────────────────────────────

fn cast(val: &str) -> Value {
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
}
