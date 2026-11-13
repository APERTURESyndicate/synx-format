//! Canonical formatter: `Synx::format`.
//!
//! The output is the same document with two-space indentation, comments
//! removed and — in a static document — keys sorted at every level outside
//! list items. It reads back as the same data, always: the result is parsed
//! and compared with the input before it is returned, and a document the
//! formatter cannot reproduce is returned exactly as it came in.

use crate::parser::{self, ParserOptions};
use crate::value::{Mode, ParseResult, Value};

/// One line that takes part in the document's structure.
struct Line<'a> {
    /// Leading whitespace in bytes, as the parser counts it.
    indent: usize,
    /// The line, trimmed.
    text: &'a str,
    /// Body of the `|` / `|+` block this line opens: raw lines, indent intact.
    body: Vec<&'a str>,
    /// `0` no block, `1` `|`, `2` `|+`.
    block: u8,
    kind: LineKind,
}

/// What the parser takes a structural line for.
///
/// perf: decided once, when the line is scanned. The tree builder used to
/// parse every key line a second time to ask the same three questions.
#[derive(Clone, Copy, PartialEq)]
enum LineKind {
    /// A directive: kept apart from the tree, but seen by look-ahead.
    Directive,
    /// A `- ` line.
    Item,
    /// A key line: whether it has a value, and whether one of its markers
    /// makes it a list without one.
    Key { has_value: bool, opens_list: bool },
    /// A line the parser reads no key from, or a key it refuses.
    Skipped,
}

/// A key line or a list item with what hangs under it.
struct Node<'a> {
    header: &'a str,
    /// Child keys of a group, or the fields of a list item.
    children: Vec<usize>,
    /// Items of a list.
    items: Vec<usize>,
    /// The line that opened the node — its block body is kept there.
    line: usize,
}

struct Frame {
    indent: usize,
    node: usize,
    is_item: bool,
}

fn is_directive(t: &str) -> bool {
    matches!(t, "!active" | "!lock" | "!tool" | "!schema" | "!llm")
        || t.starts_with("!include ")
        || t.starts_with("!use ")
        || t.starts_with("#!mode:")
}

/// The document's lines in the classes the parser gives them, in its order of
/// precedence: a block body first, then `###`, directives, comments.
fn scan(text: &str) -> Vec<Line<'_>> {
    let mut lines: Vec<Line<'_>> = Vec::new();
    // Indent of the open block's key line; its body goes to the last line.
    let mut block: Option<usize> = None;
    let mut in_block_comment = false;
    // Indent of a rejected key: the parser drops what stands under it.
    let mut dropped: Option<usize> = None;
    for raw in text.lines() {
        let (indent, trimmed) = parser::trim_ws_at(raw);
        if trimmed.is_empty() {
            continue;
        }
        if let Some(limit) = dropped {
            if indent > limit {
                continue;
            }
            dropped = None;
        }
        if let Some(opener) = block {
            if indent > opener {
                if let Some(last) = lines.last_mut() {
                    last.body.push(raw);
                }
                continue;
            }
            block = None;
        }
        if trimmed == "###" {
            in_block_comment = !in_block_comment;
            continue;
        }
        if in_block_comment {
            continue;
        }
        if is_directive(trimmed) {
            lines.push(Line { indent, text: trimmed, body: Vec::new(), block: 0, kind: LineKind::Directive });
            continue;
        }
        if trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        let mut block_kind = 0;
        let kind = if trimmed.starts_with("- ") {
            LineKind::Item
        } else {
            match parser::parse_line(trimmed) {
                None => LineKind::Skipped,
                Some(parsed) if parser::is_reserved_js_key(parsed.key) => {
                    dropped = Some(indent);
                    LineKind::Skipped
                }
                Some(parsed) => {
                    block_kind = match parsed.value {
                        "|" => 1,
                        "|+" => 2,
                        _ => 0,
                    };
                    LineKind::Key { has_value: !parsed.value.is_empty(), opens_list: parsed.markers.opens_list() }
                }
            }
        };
        if block_kind != 0 {
            block = Some(indent);
        }
        lines.push(Line { indent, text: trimmed, body: Vec::new(), block: block_kind, kind });
    }
    lines
}

/// The mode the document's directives leave it in: the parser's rule, over
/// the directives in their order.
fn declared_mode(lines: &[Line<'_>]) -> Mode {
    let mut mode = Mode::Static;
    for line in lines.iter().filter(|l| l.kind == LineKind::Directive) {
        if line.text == "!active" {
            mode = Mode::Active;
        } else if line.text.starts_with("#!mode:") {
            let declared = line.text.splitn(2, ':').nth(1).unwrap_or("static").trim();
            mode = if declared == "active" { Mode::Active } else { Mode::Static };
        }
    }
    mode
}

/// Build the tree the parser would build from `lines`: the same stack repair,
/// the same list rules. Node 0 is the root.
fn build<'a>(lines: &[Line<'a>]) -> Vec<Node<'a>> {
    let mut nodes: Vec<Node<'a>> = Vec::with_capacity(lines.len() + 1);
    nodes.push(Node { header: "", children: Vec::new(), items: Vec::new(), line: usize::MAX });
    let mut stack: Vec<Frame> = vec![Frame { indent: 0, node: 0, is_item: false }];
    // Open lists, outermost first: (indent of the key line, node).
    let mut lists: Vec<(usize, usize)> = Vec::new();

    for (n, line) in lines.iter().enumerate() {
        if line.kind == LineKind::Directive {
            continue;
        }
        let indent = line.indent;
        let next = lines.get(n + 1);
        let next_is_item = next.map_or(false, |nx| nx.text.starts_with("- "));
        if line.kind == LineKind::Item {
            let mut accepting = lists.len();
            while accepting > 0 && indent < lists[accepting - 1].0 {
                accepting -= 1;
            }
            if accepting > 0 && indent > lists[accepting - 1].0 {
                lists.truncate(accepting);
            }
            let Some(&(list_indent, list_node)) = lists.last() else { continue };
            if indent <= list_indent {
                continue;
            }
            while stack.len() > 1 && stack[stack.len() - 1].indent >= indent {
                stack.pop();
            }
            let id = nodes.len();
            nodes.push(Node { header: line.text, children: Vec::new(), items: Vec::new(), line: usize::MAX });
            nodes[list_node].items.push(id);
            let nested = next.map_or(false, |nx| nx.indent > indent) && !next_is_item;
            if nested && stack.len() < parser::MAX_PARSE_NESTING_DEPTH {
                stack.push(Frame { indent, node: id, is_item: true });
            }
            continue;
        }

        let mut closed = false;
        while lists.last().map_or(false, |l| indent <= l.0) {
            lists.pop();
            closed = true;
        }
        if closed {
            while stack.len() > 1 && stack[stack.len() - 1].is_item && stack[stack.len() - 1].indent >= indent {
                stack.pop();
            }
        }
        let LineKind::Key { has_value, opens_list } = line.kind else { continue };
        while stack.len() > 1 && stack[stack.len() - 1].indent >= indent {
            stack.pop();
        }
        let id = nodes.len();
        nodes.push(Node { header: line.text, children: Vec::new(), items: Vec::new(), line: n });
        let parent = stack[stack.len() - 1].node;
        nodes[parent].children.push(id);
        if line.block != 0 || has_value {
            continue;
        }
        if opens_list || next_is_item {
            lists.push((indent, id));
        } else if stack.len() < parser::MAX_PARSE_NESTING_DEPTH {
            stack.push(Frame { indent, node: id, is_item: false });
        }
    }
    nodes
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

/// The part of a header keys are ordered by: its first token, which ends at
/// whitespace, `[`, `:` or `(`.
fn sort_token(header: &str) -> &str {
    // perf: a header begins in ASCII nearly always; its token is cut on bytes.
    for (at, &b) in header.as_bytes().iter().enumerate() {
        if b >= 0x80 {
            break;
        }
        if matches!(b, b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b' ' | b'[' | b':' | b'(') {
            return &header[..at];
        }
    }
    header.split(|c: char| c.is_whitespace() || c == '[' || c == ':' || c == '(').next().unwrap_or("")
}

fn sort_key(header: &str) -> String {
    sort_token(header).to_lowercase()
}

/// `sort_key(a).cmp(&sort_key(b))` for two tokens in ASCII, without the two
/// strings: lowercasing ASCII is a byte for a byte.
fn cmp_ascii_tokens(a: &str, b: &str) -> std::cmp::Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    for (x, y) in a.iter().zip(b.iter()) {
        let (x, y) = (x.to_ascii_lowercase(), y.to_ascii_lowercase());
        if x != y {
            return x.cmp(&y);
        }
    }
    a.len().cmp(&b.len())
}

fn push_indent(out: &mut String, n: usize) {
    const SPACES: &str = "                                                                ";
    let mut left = n;
    while left > 0 {
        let take = left.min(SPACES.len());
        out.push_str(&SPACES[..take]);
        left -= take;
    }
}

/// The order the keys of one object are written in.
///
/// perf: keys already in order — every document that has been formatted
/// before — are neither copied nor sorted, and keys in ASCII are compared
/// where they lie, each token cut once. Each key used to be lowercased into
/// a new string, and the list of keys copied, for every object of every
/// document.
fn sorted_order<'n>(nodes: &[Node<'_>], ids: &'n [usize]) -> std::borrow::Cow<'n, [usize]> {
    use std::borrow::Cow;
    let mut all_ascii = true;
    let mut in_order = true;
    let mut previous = "";
    for (n, id) in ids.iter().enumerate() {
        let token = sort_token(nodes[*id].header);
        if !token.is_ascii() {
            all_ascii = false;
            break;
        }
        if n > 0 && in_order && cmp_ascii_tokens(previous, token).is_gt() {
            in_order = false;
        }
        previous = token;
    }
    if !all_ascii {
        let mut order = ids.to_vec();
        // Stable: keys that compare equal (a repeated key) keep document
        // order, so the last one still wins. Each key lowercased once.
        order.sort_by_cached_key(|id| sort_key(nodes[*id].header));
        return Cow::Owned(order);
    }
    if in_order {
        return Cow::Borrowed(ids);
    }
    let mut keyed: Vec<(&str, usize)> = ids.iter().map(|id| (sort_token(nodes[*id].header), *id)).collect();
    // Stable as well.
    keyed.sort_by(|a, b| cmp_ascii_tokens(a.0, b.0));
    Cow::Owned(keyed.into_iter().map(|(_, id)| id).collect())
}

/// Write the keys of one object. `sorted` holds outside list items of a
/// static document; `top` adds the blank line after a top-level block.
fn emit(lines: &[Line<'_>], nodes: &[Node<'_>], ids: &[usize], indent: usize, sorted: bool, top: bool, out: &mut String) {
    let order = if sorted { sorted_order(nodes, ids) } else { std::borrow::Cow::Borrowed(ids) };
    for &id in order.iter() {
        let node = &nodes[id];
        push_indent(out, indent);
        out.push_str(node.header);
        out.push('\n');
        emit(lines, nodes, &node.children, indent + 2, sorted, false, out);
        for item in &node.items {
            let item = &nodes[*item];
            push_indent(out, indent + 2);
            out.push_str(item.header);
            out.push('\n');
            // spec: what sits deeper than the dash is the item. Its fields
            // keep their order — which of them is last decides how an empty
            // key before the next `- ` line reads.
            emit(lines, nodes, &item.children, indent + 4, false, false, out);
        }
        let (body, block): (&[&str], u8) = match lines.get(node.line) {
            Some(line) => (&line.body, line.block),
            None => (&[], 0),
        };
        if block == 2 {
            // `|+` bodies are text: indentation relative to the first line stays.
            let base = body.first().map_or(0, |l| l.len() - l.trim_start().len());
            for raw in body {
                let line = raw.trim_end();
                let own = line.len() - line.trim_start().len();
                push_indent(out, indent + 2);
                out.push_str(&line[ws_prefix_len(line, base.min(own))..]);
                out.push('\n');
            }
        } else {
            for raw in body {
                push_indent(out, indent + 2);
                out.push_str(raw.trim());
                out.push('\n');
            }
        }
        if top && (!node.children.is_empty() || !node.items.is_empty() || !body.is_empty()) {
            out.push('\n');
        }
    }
}

/// The document in canonical form: its directives, a blank line, its tree.
fn render(lines: &[Line<'_>], nodes: &[Node<'_>], sorted: bool, size_hint: usize) -> String {
    let mut out = String::with_capacity(size_hint.max(64));
    let mut wrote_directive = false;
    for line in lines.iter().filter(|l| l.kind == LineKind::Directive) {
        out.push_str(line.text);
        out.push('\n');
        wrote_directive = true;
    }
    if wrote_directive {
        out.push('\n');
    }
    emit(lines, nodes, &nodes[0].children, 0, sorted, true, &mut out);
    // Trim trailing blank lines, ensure single newline at end
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

/// Same data in the same order — the order markers resolve in.
fn same_in_order(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|((ka, va), (kb, vb))| ka == kb && same_in_order(va, vb))
        }
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| same_in_order(p, q)),
        _ => a == b,
    }
}

/// Whether `b` is the document `a`: flags, data, and for an active document
/// the declarations and the order of everything.
fn same_document(a: &ParseResult, b: &ParseResult) -> bool {
    if a.mode != b.mode || a.locked != b.locked || a.tool != b.tool || a.schema != b.schema || a.llm != b.llm {
        return false;
    }
    if a.includes.len() != b.includes.len()
        || a.includes.iter().zip(&b.includes).any(|(x, y)| x.path != y.path || x.alias != y.alias)
        || a.uses.len() != b.uses.len()
        || a.uses.iter().zip(&b.uses).any(|(x, y)| x.package != y.package || x.alias != y.alias)
    {
        return false;
    }
    if a.mode == Mode::Active {
        same_in_order(&a.root, &b.root) && a.metadata == b.metadata
    } else {
        a.root == b.root
    }
}

pub(crate) fn format(input: &str) -> String {
    let text = parser::clamp_synx_text(input);
    let lines = scan(text);
    let nodes = build(&lines);
    // spec: markers resolve in document order, so sorting an active document
    // changes what it means (`total:calc price + tax` ahead of `tax:calc …`).
    let mode = declared_mode(&lines);
    let out = render(&lines, &nodes, mode == Mode::Static, text.len());

    // perf: a document already in canonical form is its own result, and
    // giving the input back is the one answer that needs no proof — it is
    // what every check below falls back to. Formatting a formatted file (a
    // save in an editor, `format --check` in CI) used to parse it twice to
    // learn that nothing had to change.
    if out == input {
        return out;
    }

    // The comparison below must not see a `(random)` hint change between two reads.
    let opts = ParserOptions { directives: true, nondeterministic_hints: false };
    let before = parser::parse_with(text, opts);
    // safety: input past a parser limit is not in `before`; formatting what
    // was read would write the document back without it.
    if before.truncated {
        return input.to_string();
    }
    // The directives stand at the top of `out`, where the parser takes each
    // for what it is, so `out` reads in the mode the scan found. A document
    // the parser read in the other mode is not the one `out` holds.
    if before.mode != mode {
        return input.to_string();
    }

    // safety: `format` rewrites files in place. The formatter reads lines on
    // its own, and wherever it and the parser disagreed data was lost — every
    // list of objects became one object. Now what it wrote is read back, and
    // a document it cannot reproduce is returned untouched.
    let after = parser::parse_with(&out, opts);
    if after.truncated || !same_document(&before, &after) {
        return input.to_string();
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::{parse, Synx};

    fn keeps_data(src: &str) -> String {
        let out = Synx::format(src);
        assert_eq!(parse(&out).root, parse(src).root, "format changed the data:\n{}", out);
        assert_eq!(Synx::format(&out), out, "format is not idempotent:\n{}", out);
        out
    }

    #[test]
    fn format_finishes_past_the_depth_cap() {
        // Past the cap the parser stops nesting; `format` used to loop forever.
        let mut text = String::new();
        for i in 0..140 {
            text.push_str(&"  ".repeat(i));
            text.push_str(&format!("k{}\n", i));
        }
        let out = keeps_data(&text);
        assert!(out.starts_with("k0"));
    }

    // `|+` bodies are text: the formatter must neither sort nor re-indent
    // them, and formatting twice changes nothing.
    #[test]
    fn format_keeps_preserve_indent_block_verbatim() {
        let src = "code |+\n  zeta()\n    alpha()\n  beta\nname x\n";
        let out = keeps_data(src);
        assert_eq!(out, "code |+\n  zeta()\n    alpha()\n  beta\n\nname x\n");
    }

    // Equal keys keep document order (stable sort), so the last one still wins.
    #[test]
    fn format_sort_is_stable_for_repeated_keys() {
        keeps_data("b 1\na first\nc 2\na second\n");
    }

    #[test]
    fn format_sorts_keys_and_normalises_indent() {
        let out = keeps_data("# header\nzeta 1\nserver\n    port 8080\n    host x\nalpha 2\ntags\n    - b\n    - a\n");
        assert_eq!(out, "alpha 2\nserver\n  host x\n  port 8080\n\ntags\n  - b\n  - a\n\nzeta 1\n");
    }

    // The bug that made `format --write` destroy files: the keys of a list
    // item were written under the list's key, ahead of the items.
    #[test]
    fn format_keeps_a_list_of_objects() {
        let src = "servers\n  - host a\n    port 1\n  - host b\n    port 2\n    opts\n      tls true\n";
        let out = keeps_data(src);
        assert_eq!(out, src);
        let servers = parse(&out).root.as_object().unwrap()["servers"].as_array().unwrap().clone();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[1].as_object().unwrap()["port"], crate::Value::Int(2));
    }

    #[test]
    fn format_keeps_lists_nested_in_items() {
        let src = "items\n    - name a\n      tags\n          - x\n          - y\n    - name b\n      k 2\nafter 1\n";
        let out = keeps_data(src);
        assert_eq!(out, "after 1\nitems\n  - name a\n    tags\n      - x\n      - y\n  - name b\n    k 2\n");
    }

    #[test]
    fn format_drops_comments_and_block_comments_only() {
        let src = "a 1\n###\nold secret\n###\nlist\n  # first\n  - x\n  // second\n  - y\nb | # note\n  # kept: body text\n  line\n";
        let out = keeps_data(src);
        assert_eq!(out, "a 1\nb | # note\n  # kept: body text\n  line\n\nlist\n  - x\n  - y\n");
    }

    #[test]
    fn format_handles_an_indented_first_line() {
        let out = keeps_data("    name x\n    port 1\n");
        assert_eq!(out, "name x\nport 1\n");
    }

    // Markers resolve in document order: an active document is not sorted.
    #[test]
    fn format_keeps_the_order_of_an_active_document() {
        let src = "!active\nz_tax 5\na_total:calc z_tax + 1\nitems\n  - host:env H\n    port 1\n";
        let out = Synx::format(src);
        assert_eq!(out, "!active\n\nz_tax 5\na_total:calc z_tax + 1\nitems\n  - host:env H\n    port 1\n");
        assert_eq!(Synx::format(&out), out);
        let late = "!include ./x.synx\nb 1\n!active\na:calc b + 1\n";
        let out = Synx::format(late);
        assert_eq!(out, "!include ./x.synx\n!active\n\nb 1\na:calc b + 1\n");
        assert_eq!(parse(&out).mode, crate::Mode::Active);
    }

    // What the formatter cannot write back as the same data comes back as it was.
    #[test]
    fn format_returns_the_input_when_it_cannot_keep_the_data() {
        // `a` is read before `!active`, so its marker is text; with the
        // directive moved to the top it would become a declaration.
        let src = "a:env X\n!active\nb 1\n";
        assert_eq!(Synx::format(src), src);
        // Lines the parser skips are skipped: the stray `- b` belongs to no list.
        assert_eq!(Synx::format("items\n  - a\n[junk]\n  - b\n"), "items\n  - a\n");
        // A document cut by a parser limit is not rewritten without its tail.
        let mut big = String::from("k |\n");
        for _ in 0..40_000 {
            big.push_str("  0123456789012345678901234567890123456789\n");
        }
        big.push_str("after 1\n");
        assert!(parse(&big).truncated);
        assert_eq!(Synx::format(&big), big);
    }

    /// `format` as it was before the shortcut for a document already in
    /// canonical form: both parses, always, and the mode taken from the parser.
    fn reference_format(input: &str) -> String {
        use super::*;
        let text = parser::clamp_synx_text(input);
        let opts = ParserOptions { directives: true, nondeterministic_hints: false };
        let before = parser::parse_with(text, opts);
        if before.truncated {
            return input.to_string();
        }
        let lines = scan(text);
        let nodes = build(&lines);
        let out = render(&lines, &nodes, before.mode == Mode::Static, text.len());
        let after = parser::parse_with(&out, opts);
        if after.truncated || !same_document(&before, &after) {
            return input.to_string();
        }
        out
    }

    // The shortcut returns what the two parses would have: over documents of
    // every line class the parser knows, raw and already formatted.
    #[test]
    fn format_shortcut_agrees_with_the_checked_path() {
        let templates = [
            "key value", "key", "other 2", "- item", "- k v", "- ", "# comment", "// comment", "###", "!active", "!lock",
            "!include x.synx a", "!use @a/b", "#!mode:static", "#!mode:active", "#!mode:", "text |", "code |+", "body line",
            "__proto__", "constructor x", "Key 2", "KEY 3", "ключ 3", "Ключ 4", "ΟΔΟΣ 5", "list:join", "n:random 70 30",
            "v:env V", "c[min:1] 2", "t(int) 7", "", "[junk]", "a.b 1", "z # note", "q \"x # y\"",
        ];
        let mut seed = 0x6A09_E667_F3BC_C908u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let (mut unchanged, mut changed) = (0, 0);
        for _ in 0..30_000 {
            let mut doc = String::new();
            if next() % 40 == 0 {
                doc.push('\u{feff}');
            }
            for _ in 0..1 + next() % 14 {
                let indent = [0, 0, 2, 2, 4, 6, 1, 3][(next() % 8) as usize];
                doc.push_str(&" ".repeat(indent));
                doc.push_str(templates[(next() % templates.len() as u64) as usize]);
                doc.push_str(if next() % 30 == 0 { "\r\n" } else { "\n" });
            }
            let want = reference_format(&doc);
            let got = Synx::format(&doc);
            assert_eq!(got, want, "format differs from the checked path for:\n{:?}", doc);
            // A formatted document is the shortcut's own case.
            assert_eq!(Synx::format(&got), reference_format(&got), "second format differs for:\n{:?}", doc);
            if got == doc {
                unchanged += 1
            } else {
                changed += 1
            }
        }
        assert!(unchanged > 300 && changed > 20_000, "{} unchanged, {} changed", unchanged, changed);
    }

    // Keys in ASCII are ordered without lowercased copies; the order is the
    // one the copies gave, repeated keys and all.
    #[test]
    fn format_orders_keys_as_their_lowercase_did() {
        let words = ["a", "B", "b", "Ab", "aB", "ab", "Z", "_", "a1", "A-1", "a_", "é", "É", "Σ", "ς", "я", "Я", "k:env", "k[x]", "k(int)", "k v"];
        let mut seed = 0xBB67_AE85_84CA_A73Bu64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for round in 0..20_000 {
            let ascii_only = round % 2 == 0;
            let headers: Vec<&str> = (0..2 + next() % 7)
                .map(|_| words[(next() % words.len() as u64) as usize])
                .filter(|w| !ascii_only || w.is_ascii())
                .collect();
            let nodes: Vec<super::Node<'_>> =
                headers.iter().map(|h| super::Node { header: h, children: Vec::new(), items: Vec::new(), line: usize::MAX }).collect();
            let ids: Vec<usize> = (0..nodes.len()).collect();
            let mut want = ids.clone();
            want.sort_by_cached_key(|id| super::sort_key(nodes[*id].header));
            assert_eq!(super::sorted_order(&nodes, &ids).as_ref(), want.as_slice(), "{:?}", headers);
        }
    }
}
