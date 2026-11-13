//! SYNX Parser — converts raw .synx text into a structured value tree
//! with metadata for engine resolution.

use memchr::memchr;
use crate::value::*;
use crate::rng;

// ─── Resource limits (fuzz / hostile input) ─────────────────
// All caps are documented here so callers know parsing is bounded.

/// Maximum UTF-8 bytes accepted per `parse()` (truncate with valid UTF-8 boundary).
pub(crate) const MAX_SYNX_INPUT_BYTES: usize = 16 * 1024 * 1024;

/// Maximum lines considered (1 + number of newlines before truncate).
const MAX_LINE_STARTS: usize = 2_000_000;

/// Indentation-tree depth for nested objects (stack size). Iterative parser — prevents giant parent chains.
pub(crate) const MAX_PARSE_NESTING_DEPTH: usize = 128;

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

/// Max bytes of metadata paths one `!active` document may declare.
///
/// safety: declarations are filed under the dot-path of their object, so
/// every object with a marked key stores its whole ancestry again. A hundred
/// nested 200-byte keys above 70 000 small objects made 1.5 GiB of paths out
/// of a 15 MB file; one 20 KB key above them passed 3 GiB. Four times the
/// input cap is more than any document with keys of ordinary length reaches.
/// The line that would pass the budget, and everything after it, is dropped
/// like input past `MAX_SYNX_INPUT_BYTES`, and `truncated` is set.
const MAX_METADATA_PATH_BYTES: usize = 4 * MAX_SYNX_INPUT_BYTES;

/// Truncate `text` to a UTF-8-safe prefix (used by `parse` and canonical `format`).
///
/// A single leading U+FEFF byte-order mark is dropped first (SYNX §2.1: a BOM at
/// the start MAY be treated as whitespace; §6 requires directives to be matched
/// with a leading BOM ignored). `str::trim` does not treat U+FEFF as whitespace,
/// so without this a BOM-prefixed `!active` produced a `"\u{feff}!active"` key
/// and silently disabled Active mode — the JS parser already strips it, so this
/// also removes a cross-implementation divergence.
pub(crate) fn clamp_synx_text(text: &str) -> &str {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.len() <= MAX_SYNX_INPUT_BYTES {
        return text;
    }
    let slice = &text.as_bytes()[..MAX_SYNX_INPUT_BYTES];
    let end = core::str::from_utf8(slice)
        .map(|s| s.len())
        .unwrap_or_else(|e| e.valid_up_to());
    &text[..end]
}

/// Byte offset of the first character at or past `strip` bytes of leading whitespace.
///
/// Indentation is measured over *Unicode* whitespace (SYNX §4), so a line may
/// carry a multi-byte indent character such as U+00A0. Slicing at the raw byte
/// count would then land inside that character and panic; this walks whole
/// characters instead and never returns a non-boundary offset. For an
/// ASCII-only indent — the overwhelmingly common case — the result is `strip`.
fn strip_boundary(s: &str, strip: usize) -> usize {
    let mut off = 0usize;
    for ch in s.chars() {
        if off >= strip || !ch.is_whitespace() {
            break;
        }
        off += ch.len_utf8();
    }
    off
}

/// Keys that would reach a JS consumer's prototype chain instead of its data.
pub(crate) fn is_reserved_js_key(key: &str) -> bool {
    key == "__proto__" || key == "constructor" || key == "prototype"
}

/// Longest prefix of `s` that fits in `limit` bytes without splitting a character.
fn utf8_prefix_len(s: &str, limit: usize) -> usize {
    if s.len() <= limit {
        return s.len();
    }
    let mut end = limit;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// Byte length to parse: full slice, or truncate before the newline that would exceed
/// `MAX_LINE_STARTS` lines (at most `MAX_LINE_STARTS.saturating_sub(1)` `\n` bytes kept).
fn find_parse_end_bytes(bytes: &[u8]) -> usize {
    let max_newlines = MAX_LINE_STARTS.saturating_sub(1);
    // perf: a text with no more bytes than the cap has lines cannot pass it,
    // and that is every document under 2 MB. The check used to be a `memchr`
    // call per line — a twentieth of the time a config took to parse. Longer
    // texts are counted in one sweep; only one past the cap is walked.
    if bytes.len() <= max_newlines || memchr::memchr_iter(b'\n', bytes).count() <= max_newlines {
        return bytes.len();
    }
    memchr::memchr_iter(b'\n', bytes).nth(max_newlines).unwrap_or(bytes.len())
}

/// Options for a single [`parse_with`] run.
///
/// The default reproduces plain SYNX 3.7 behaviour; the non-default settings
/// exist for *embedded* parses, where the host format owns the directive
/// surface and the SYNX document is untrusted payload.
#[derive(Debug, Clone, Copy)]
pub struct ParserOptions {
    /// Recognise `!`-prefixed directive lines (`!active`, `!lock`, `!tool`,
    /// `!schema`, `!llm`, `!include`, `!use`) and the `#!mode:` pragma.
    ///
    /// SYNXL block delegation sets this to `false` (SYNXL §9.4): a record that
    /// originates from an untrusted dataset must not be able to turn
    /// `!include` into a file-read primitive against the consuming process,
    /// nor flip the document into `!active` mode.
    ///
    /// Neutralisation happens *inside* this parser, after the multiline-block
    /// check, so that `|` / `|+` bodies keep their `!` lines verbatim — a
    /// pre-filter over the raw lines would corrupt those bodies.
    pub directives: bool,

    /// Honour the `random`, `random:int`, `random:float` and `random:bool`
    /// type hints.
    ///
    /// SYNXL block delegation sets this to `false` (SYNXL §8.3): a hint
    /// carried by dataset data would make the §12 projection differ between
    /// two reads of the same bytes, which defeats hashing, deduplication and
    /// the byte-identical projection §1.4 is measured on. Disabled, the hints
    /// fall back to automatic casting like any other unrecognised hint.
    pub nondeterministic_hints: bool,
}

impl Default for ParserOptions {
    fn default() -> Self {
        Self { directives: true, nondeterministic_hints: true }
    }
}

/// Parse a SYNX text string into a value tree with metadata.
pub fn parse(text: &str) -> ParseResult {
    parse_with(text, ParserOptions::default())
}

/// Parse a SYNX text string with explicit [`ParserOptions`].
///
/// `parse(text)` is `parse_with(text, ParserOptions::default())`.
pub fn parse_with(text: &str, opts: ParserOptions) -> ParseResult {
    parse_impl(text, opts, None)
}

/// The 0-based line of `text` that declares each key landing at the root of
/// the parsed document; the first declaration of a key wins.
///
/// SYNXL §11.2 reports a block diagnostic on the line inside the block, and
/// which keys reach the root is the parser's own decision (stack repair, list
/// items, multiline bodies) — re-deriving it from indentation got it wrong.
pub(crate) fn root_key_lines(text: &str, opts: ParserOptions) -> std::collections::HashMap<String, usize> {
    let mut lines = std::collections::HashMap::new();
    parse_impl(text, opts, Some(&mut lines));
    lines
}

fn parse_impl(
    text: &str,
    opts: ParserOptions,
    mut root_lines: Option<&mut std::collections::HashMap<String, usize>>,
) -> ParseResult {
    // spec: a leading U+FEFF is dropped (§2.1) and is not lost content; it
    // used to make every document with a BOM report `truncated`.
    let original_len = text.strip_prefix('\u{feff}').unwrap_or(text).len();
    let text = clamp_synx_text(text);
    let parse_end = find_parse_end_bytes(text.as_bytes());
    let text = &text[..parse_end];
    let bytes = text.as_bytes();
    // Either cap having bitten means input was dropped before parsing began.
    let mut truncated_multiline = text.len() < original_len;

    let mut root = Map::default();
    let mut stack: Vec<(i32, StackEntry)> = vec![(-1, StackEntry::Root)];
    // Parent-navigation cache; cleared on every stack mutation (see NavCache).
    let mut nav_cache = NavCache::default();
    let mut mode = Mode::Static;
    let mut locked = false;
    let mut tool = false;
    let mut schema = false;
    let mut llm = false;
    let mut metadata: Map<MetaMap> = Map::default();
    let mut includes: Vec<IncludeDirective> = Vec::new();
    let mut uses: Vec<UseDirective> = Vec::new();

    let mut block: Option<BlockState> = None;
    // Open lists, outermost first.
    //
    // spec: a list inside a list item ends at the next item of the outer
    // list (§8.5). One `Option` held "the" open list, so a nested `tags`
    // replaced its parent: `- name b` after it matched no list and was
    // dropped without a word, and the keys under it landed in the item before.
    let mut lists: Vec<ListState> = Vec::new();
    let mut in_block_comment = false;
    // Indent of a rejected key whose block is being dropped.
    let mut dropped_below: Option<i32> = None;
    let mut meta_book = MetaBook::default();

    // perf: lines are found on the fly. An index of every line start was built
    // up front before — one more pass and a vector grown to the line count,
    // paid again for every SYNXL block. `find_parse_end_bytes` has already
    // applied the line cap, so walking to the end is walking the capped text.
    let mut pos = 0usize;
    let mut line_no = 0usize;
    while pos <= bytes.len() {
        let this_line = line_no;
        line_no += 1;
        // Extract line without allocating
        let start = pos;
        let nl = memchr(b'\n', &bytes[start..]).map_or(bytes.len(), |rel| start + rel);
        pos = nl + 1;
        // Handle \r\n
        let end = if nl > start && bytes[nl - 1] == b'\r' { nl - 1 } else { nl };
        let raw = &text[start..end];

        let (lead, trimmed) = trim_ws_at(raw);

        // A blank line is never structural and never body: SYNX §8.4.1 does not
        // preserve blank lines inside a multiline body.
        if trimmed.is_empty() {
            continue;
        }

        let indent = lead as i32;

        if let Some(limit) = dropped_below {
            if indent > limit {
                continue;
            }
            dropped_below = None;
        }

        // Continue multiline block.
        //
        // This branch runs *before* every line-class rule below, and that order
        // is load-bearing: a `|` / `|+` body is a raw byte slice (SYNX §8.4.1),
        // so `#`, `//`, `###`, `!include` and `#!mode:` are ordinary content
        // there. Classifying first would delete comment lines out of code
        // samples, let a stray `###` swallow the rest of the record, and turn a
        // dataset row into a directive — which SYNXL §9.4 calls out explicitly.
        if let Some(ref mut blk) = block {
            if indent > blk.indent {
                if blk.content.len() < MAX_MULTILINE_BLOCK_BYTES {
                    if !blk.content.is_empty() {
                        blk.content.push('\n');
                    }
                    let room = MAX_MULTILINE_BLOCK_BYTES.saturating_sub(blk.content.len());
                    if room > 0 {
                        let slice: &str = if blk.preserve_indent {
                            // `|+` (SYNX 3.7): lock the strip prefix to the
                            // indent of the first non-empty continuation line,
                            // then strip exactly that many leading whitespace
                            // bytes from each subsequent line. Anything beyond
                            // the base indent is preserved verbatim.
                            if blk.base_indent < 0 {
                                blk.base_indent = indent;
                            }
                            let strip = blk.base_indent.min(indent) as usize;
                            let raw_trim_end = raw.trim_end();
                            let cut = strip_boundary(raw_trim_end, strip);
                            if cut < raw_trim_end.len() {
                                &raw_trim_end[cut..]
                            } else {
                                ""
                            }
                        } else {
                            trimmed
                        };
                        let n = utf8_prefix_len(slice, room);
                        blk.content.push_str(&slice[..n]);
                        if n < slice.len() {
                            truncated_multiline = true;
                        }
                    }
                } else {
                    truncated_multiline = true;
                }
                continue;
            } else {
                let content = std::mem::take(&mut blk.content);
                let blk_key = blk.key;
                let blk_stack_idx = blk.stack_idx;
                block = None;
                insert_value(&mut root, &stack, blk_stack_idx, blk_key, Value::String(content), &mut nav_cache);
            }
        }

        // Block comment toggle: ###
        if trimmed == "###" {
            in_block_comment = !in_block_comment;
            continue;
        }
        if in_block_comment {
            continue;
        }

        // Directive lines (§6). Every form below starts with `!` or `#!mode:`,
        // so gating the whole block on that prefix is behaviour-preserving and
        // lets an embedded parse switch the class off wholesale (§9.4 of the
        // SYNXL spec). Unknown `!…` lines still fall through to the generic
        // line handling, exactly as in 3.6.
        if opts.directives && (trimmed.starts_with('!') || trimmed.starts_with("#!mode:")) {
            // Mode declaration
            if trimmed == "!active" {
                mode = Mode::Active;
                continue;
            }
            if trimmed == "!lock" {
                locked = true;
                continue;
            }
            if trimmed == "!tool" {
                tool = true;
                continue;
            }
            if trimmed == "!schema" {
                schema = true;
                continue;
            }
            if trimmed == "!llm" {
                llm = true;
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
                continue;
            }
            if trimmed.starts_with("!use ") {
                let rest = trimmed[5..].trim();
                if rest.starts_with('@') {
                    // Parse: !use @scope/name [as alias]
                    let mut parts = rest.splitn(2, " as ");
                    let package = parts.next().unwrap_or("").trim().to_string();
                    let alias = parts.next().map(|s| s.trim().to_string()).unwrap_or_else(|| {
                        // Auto-derive alias from last segment: @scope/name → name
                        package.rsplit('/').next().unwrap_or(&package).to_string()
                    });
                    if !package.is_empty() {
                        uses.push(UseDirective { package, alias });
                    }
                }
                continue;
            }
            if trimmed.starts_with("#!mode:") {
                let declared = trimmed.splitn(2, ':').nth(1).unwrap_or("static").trim();
                mode = if declared == "active" { Mode::Active } else { Mode::Static };
                continue;
            }
        }

        // SYNXL §9.4 — with directives disabled, a `!…` line that reaches this
        // point is *not* multiline body (the block branch above already
        // consumed and preserved those) and MUST be discarded exactly like a
        // comment line: it must neither set a mode flag nor become a key.
        if !opts.directives && trimmed.starts_with('!') {
            continue;
        }

        // Skip comments. Checked after the directive block above so that the
        // `#!mode:` pragma is still recognised.
        if trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }

        // Continue list items
        if trimmed.starts_with("- ") {
            // The list this item belongs to: the innermost open one whose key
            // sits to the left of the dash. Lists nested deeper than the dash
            // end here — but only when a list does take the line; a dash no
            // list accepts is skipped and closes nothing, as it always was.
            let mut accepting = lists.len();
            while accepting > 0 && indent < lists[accepting - 1].indent {
                accepting -= 1;
            }
            if accepting > 0 && indent > lists[accepting - 1].indent {
                lists.truncate(accepting);
            }
            if let Some(lst) = lists.last() {
                if indent > lst.indent {
                    // Pop every frame of the previous item — its own frame and
                    // any object nested inside it. Popping only `ListItem`
                    // frames stopped at a nested `sub` object, so the next
                    // item's keys were looked up through `sub` and dropped.
                    while stack.len() > 1 && stack.last().map_or(false, |(d, _)| *d >= indent) {
                        pop_frame(&mut stack, &mut nav_cache);
                    }

                    let val_str = strip_comment(trim_ws(&trimmed[2..]));

                    // Peek the next line of content — if it is more deeply
                    // indented and not a `- ` continuation, this item is an
                    // object, not a scalar.
                    let mut nested = false;
                    if let Some((ps, pe)) = next_content_line(text, pos) {
                        let pl = &text[ps..pe];
                        let pt = trim_ws(pl);
                        let pi = indent_len(pl) as i32;
                        if pi > indent && !pt.starts_with("- ") {
                            nested = true;
                        }
                    }

                    let list_key = lst.key;
                    let list_stack_idx = lst.stack_idx;
                    let list_slot = lst.slot;

                    // Locate the items array, creating it lazily in the parent map.
                    let mut head_meta: Option<(&str, Meta)> = None;
                    let mut item_frame: Option<usize> = None;
                    if let Some(parent_map) = navigate_cached(&mut root, &stack, list_stack_idx, &mut nav_cache) {
                        // perf: the list is found by position. Looking it up by
                        // key copied and hashed the key for every item.
                        if let Some((_, Value::Array(arr))) = parent_map.get_index_mut(list_slot) {
                            if arr.len() >= MAX_LIST_ITEMS {
                                // The cap dropped an item: say so, like every other cap.
                                truncated_multiline = true;
                                continue;
                            }
                            if nested {
                                let mut item_obj: Map = Map::default();
                                if let Some(mut parsed) = parse_line(val_str) {
                                    let val = if let Some(hint) = parsed.type_hint {
                                        cast_typed_with(parsed.value, hint, opts.nondeterministic_hints)
                                    } else if !parsed.value.is_empty() {
                                        cast(parsed.value)
                                    } else {
                                        Value::Object(Map::default())
                                    };
                                    // Same prototype-pollution guard as the key-line
                                    // branch below: a list item is just as capable of
                                    // carrying `__proto__` into the JSON projection.
                                    if !is_reserved_js_key(parsed.key) {
                                        item_obj.insert(parsed.key.to_string(), val);
                                        if mode == Mode::Active
                                            && (!parsed.markers.is_empty()
                                                || parsed.constraints.is_some()
                                                || parsed.type_hint.is_some())
                                        {
                                            head_meta = Some((
                                                parsed.key,
                                                Meta {
                                                    markers: parsed.markers.to_strings(),
                                                    args: std::mem::take(&mut parsed.marker_args),
                                                    type_hint: parsed.type_hint.map(str::to_string),
                                                    constraints: parsed.constraints.take(),
                                                },
                                            ));
                                        }
                                    }
                                } else {
                                    item_obj.insert("_value".to_string(), cast(val_str));
                                }
                                let item_idx = arr.len();
                                arr.push(Value::Object(item_obj));
                                item_frame = Some(item_idx);
                            } else {
                                arr.push(cast(val_str));
                            }
                        }
                    }
                    if let Some(item_idx) = item_frame {
                        if stack.len() < MAX_PARSE_NESTING_DEPTH {
                            push_frame(&mut stack, &mut nav_cache, (indent, StackEntry::ListItem { list_key, list_slot, item_idx, parent: list_stack_idx }));
                            // spec: the key on the `- ` line is a key line like the
                            // ones under it. Its markers and constraints were read
                            // and thrown away: `- token:secret abc` stayed in the
                            // clear and `- host:env H` kept the variable's name.
                            if let Some((key, meta)) = head_meta {
                                if !meta_book.record(&mut metadata, &stack, &mut nav_cache, key, meta) {
                                    // Over the budget: the item goes with its
                                    // declarations, not without them.
                                    pop_frame(&mut stack, &mut nav_cache);
                                    if let Some(parent_map) = navigate_cached(&mut root, &stack, list_stack_idx, &mut nav_cache) {
                                        if let Some((_, Value::Array(arr))) = parent_map.get_index_mut(list_slot) {
                                            arr.pop();
                                        }
                                    }
                                    truncated_multiline = true;
                                    break;
                                }
                            }
                        }
                    }

                    continue;
                }
            }
        } else {
            // Close every list a non-item line is at-or-below the indent of.
            let mut close = false;
            while lists.last().map_or(false, |lst| indent <= lst.indent) {
                lists.pop();
                close = true;
            }
            if close {
                // Pop any list-item frames at-or-above this indent.
                while stack.len() > 1 {
                    match stack.last() {
                        Some((d, StackEntry::ListItem { .. })) if *d >= indent => { pop_frame(&mut stack, &mut nav_cache); }
                        _ => break,
                    }
                }
            }
        }

        // Parse key line
        if let Some(mut parsed) = parse_line(trimmed) {
            // Reject prototype-polluting keys so downstream consumers (esp. JS
            // applications consuming the JSON output) are not exposed to
            // `__proto__` / `constructor` / `prototype` injection.
            //
            // safety: the lines under such a key go with it. Only the key
            // line was skipped, so its block fell to whatever was open above
            // it — `profile` / `__proto__` / `  role admin` gave `profile`
            // the role — and under `prototype |` each line of the text was
            // read as a line of the document, `!active` included.
            if is_reserved_js_key(parsed.key) {
                dropped_below = Some(indent);
                continue;
            }

            // Pop stack to correct parent
            while stack.len() > 1 && stack.last().unwrap().0 >= indent {
                pop_frame(&mut stack, &mut nav_cache);
            }

            let parent_idx = stack.len() - 1;
            if parent_idx == 0 {
                if let Some(lines) = root_lines.as_deref_mut() {
                    lines.entry(parsed.key.to_string()).or_insert(this_line);
                }
            }

            let declares = mode == Mode::Active
                && (!parsed.markers.is_empty()
                    || parsed.constraints.is_some()
                    || parsed.type_hint.is_some());
            // The budget is checked before the line changes anything, so a
            // document cut here ends on a whole line.
            if declares && !meta_book.has_room(&metadata, &stack, &mut nav_cache) {
                truncated_multiline = true;
                break;
            }

            // spec: a key that replaces an object or list replaces its
            // declarations too. Metadata is filed by path, so the old
            // subtree's markers and constraints used to apply to whatever
            // took its place (`port` / `  port[enum:a]` / `port` left the
            // second, empty `port` failing the first one's enum).
            if mode == Mode::Active && !metadata.is_empty() {
                let replaces_tree = navigate_cached(&mut root, &stack, parent_idx, &mut nav_cache)
                    .and_then(|m| m.get(parsed.key))
                    .map_or(false, |v| matches!(v, Value::Object(_) | Value::Array(_)));
                if replaces_tree {
                    if !nav_cache.meta_path_valid {
                        fill_path(&stack, &mut nav_cache.meta_path);
                        nav_cache.meta_path_valid = true;
                    }
                    let mut prefix = nav_cache.meta_path.clone();
                    if !prefix.is_empty() {
                        prefix.push('.');
                    }
                    crate::engine::push_key_segment(&mut prefix, parsed.key);
                    meta_book.drop_subtree(&mut metadata, &prefix);
                }
            }

            // Save metadata if in active mode
            if declares {
                let meta = Meta {
                    markers: parsed.markers.to_strings(),
                    args: std::mem::take(&mut parsed.marker_args),
                    type_hint: parsed.type_hint.map(str::to_string),
                    constraints: parsed.constraints.take(),
                };
                if !meta_book.record(&mut metadata, &stack, &mut nav_cache, parsed.key, meta) {
                    truncated_multiline = true;
                    break;
                }
            }

            // `|` (3.6 frozen) and `|+` (3.7 addition) both open multiline blocks.
            // `|+` differs only in keeping each continuation line's indent relative
            // to the first non-empty one — see BlockState docs and §8.4.1 of the
            // spec. Old documents with literal value `|+` (extremely unlikely) would
            // change meaning under 3.7; the bump is documented in CHANGELOG.
            let is_block = parsed.value == "|" || parsed.value == "|+";
            let preserve_indent = parsed.value == "|+";
            let is_list_marker = parsed.markers.opens_list();

            if is_block {
                insert_value(
                    &mut root,
                    &stack,
                    parent_idx,
                    &parsed.key,
                    Value::String(String::new()),
                    &mut nav_cache,
                );
                block = Some(BlockState {
                    indent,
                    key: parsed.key,
                    content: String::new(),
                    stack_idx: parent_idx,
                    preserve_indent,
                    base_indent: -1,
                });
            } else if is_list_marker && parsed.value.is_empty() {
                // Insert an empty Array now so callers see the key even
                // if the list ends up empty.
                let slot = insert_value(
                    &mut root,
                    &stack,
                    parent_idx,
                    &parsed.key,
                    Value::Array(Vec::new()),
                    &mut nav_cache,
                );
                lists.push(ListState {
                    indent,
                    key: parsed.key,
                    stack_idx: parent_idx,
                    slot,
                });
            } else if parsed.value.is_empty() {
                // Peek ahead for list
                if let Some((ps, pe)) = next_content_line(text, pos) {
                    let pt = trim_ws(&text[ps..pe]);
                    if pt.starts_with("- ") {
                        let slot = insert_value(
                            &mut root,
                            &stack,
                            parent_idx,
                            &parsed.key,
                            Value::Array(Vec::new()),
                            &mut nav_cache,
                        );
                        lists.push(ListState {
                            indent,
                            key: parsed.key,
                            stack_idx: parent_idx,
                            slot,
                        });
                        continue;
                    }
                }

                let slot = insert_value(
                    &mut root,
                    &stack,
                    parent_idx,
                    &parsed.key,
                    Value::Object(Map::default()),
                    &mut nav_cache,
                );
                // Guard against pathological inputs that create extremely deep nesting,
                // which can lead to large allocations (metadata path building, parent navigation, etc).
                // If the cap is hit, we still insert the object but stop increasing nesting.
                if stack.len() < MAX_PARSE_NESTING_DEPTH {
                    push_frame(&mut stack, &mut nav_cache, (indent, StackEntry::Key { key: parsed.key, slot }));
                }
            } else {
                let value = if let Some(hint) = parsed.type_hint {
                    cast_typed_with(parsed.value, hint, opts.nondeterministic_hints)
                } else {
                    cast(parsed.value)
                };
                insert_value(&mut root, &stack, parent_idx, parsed.key, value, &mut nav_cache);
            }
        }
    }

    // Flush pending block
    if let Some(blk) = block {
        insert_value(
            &mut root,
            &stack,
            blk.stack_idx,
            blk.key,
            Value::String(blk.content),
            &mut nav_cache,
        );
    }

    // List items live directly in the parent map; no flush is required at
    // end-of-input.
    meta_book.finish(&mut metadata);

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
        uses,
        truncated: truncated_multiline,
    }
}

/// The first line at or after byte `from` that is content — not blank and not
/// a comment — as `(start, end)` with a trailing `CR` excluded.
///
/// spec: a comment line is ignored (§5), so it cannot stand between a key and
/// its list or between a `- ` line and the keys of its item. Only blank lines
/// were skipped here: `items` / `# note` / `- a` read as an empty object and
/// lost every item, and a comment after `- name a` turned the item into the
/// text `name a` and sent its keys to the parent.
fn next_content_line(text: &str, from: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut p = from;
    let mut in_block_comment = false;
    while p <= bytes.len() {
        let nl = memchr(b'\n', &bytes[p..]).map_or(bytes.len(), |rel| p + rel);
        let end = if nl > p && bytes[nl - 1] == b'\r' { nl - 1 } else { nl };
        let t = trim_ws(&text[p..end]);
        if t == "###" {
            in_block_comment = !in_block_comment;
        } else if !in_block_comment && !t.is_empty() && !t.starts_with('#') && !t.starts_with("//") {
            return Some((p, end));
        }
        p = nl + 1;
    }
    None
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
            let mut def = Map::default();
            def.insert("name".to_string(), Value::String(key.clone()));
            def.insert("params".to_string(), val.clone());
            tools.push(Value::Object(def));
        }
        let mut out = Map::default();
        out.insert("tools".to_string(), Value::Array(tools));
        Value::Object(out)
    } else {
        // Call mode: first key = tool name, children = params
        if map.is_empty() {
            let mut out = Map::default();
            out.insert("tool".to_string(), Value::Null);
            out.insert("params".to_string(), Value::Object(Map::default()));
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
            _ => Value::Object(Map::default()),
        };

        let mut out = Map::default();
        out.insert("tool".to_string(), Value::String(tool_key.clone()));
        out.insert("params".to_string(), params);
        Value::Object(out)
    }
}

// ─── Internal types ──────────────────────────────────────

#[derive(Debug)]
enum StackEntry<'a> {
    Root,
    /// An open group: its key, and the key's position in the parent object.
    ///
    /// perf: positions never change while a document is read (nothing is
    /// removed, a repeated key keeps its place), so walking down to a parent
    /// is one array index per level. Looking each ancestor up by key hashed
    /// the key again after every push and pop: a long key above many small
    /// groups made a static parse quadratic — 3 MB took 15 s.
    Key { key: &'a str, slot: usize },
    /// We are inside a list item that turned out to be an object
    /// (a `- key value` line followed by deeper-indented sub-keys).
    /// `list_key` is the list's key in its parent map, `list_slot` its
    /// position there; `item_idx` is the position in the list's `Array`.
    ListItem { list_key: &'a str, list_slot: usize, item_idx: usize, parent: usize },
}

struct BlockState<'a> {
    indent: i32,
    key: &'a str,
    content: String,
    stack_idx: usize,
    /// SYNX 3.7 `|+`: keep indent relative to the first continuation line.
    /// `false` is plain `|` (3.6) — every continuation line is fully trimmed.
    preserve_indent: bool,
    /// Indent of the first non-empty continuation line, used as the strip
    /// prefix when `preserve_indent` is true. `-1` means "not yet locked".
    base_indent: i32,
}

struct ListState<'a> {
    indent: i32,
    key: &'a str,
    stack_idx: usize,
    /// Position of the list's key in its parent object.
    slot: usize,
}

/// One key line, borrowed from the source text.
///
/// perf: every field is a slice of the line; owned strings are made only for
/// what is actually stored (the map key, the cast value, active metadata).
pub(crate) struct ParsedLine<'a> {
    pub(crate) key: &'a str,
    pub(crate) type_hint: Option<&'a str>,
    pub(crate) value: &'a str,
    pub(crate) markers: MarkerChain<'a>,
    pub(crate) marker_args: Vec<String>,
    pub(crate) constraints: Option<Constraints>,
}

/// The `:a:b:c` chain of a key line, as written.
///
/// perf: a slice of the line, split when its segments are asked for. Every
/// marked line used to collect a `Vec<&str>` that grew as it went, only to
/// be copied into the `Vec<String>` the metadata keeps.
#[derive(Clone, Copy, Default)]
pub(crate) struct MarkerChain<'a> {
    chain: Option<&'a str>,
    /// How many segments the chain has, up to the cap.
    segments: usize,
    /// Which of the markers the parser itself acts on are in the chain.
    seen: u8,
}

impl<'a> MarkerChain<'a> {
    const RANDOM: u8 = 1;
    const INHERIT: u8 = 2;
    /// `random`, `unique`, `geo` or `join`: a key without a value is a list.
    const LIST: u8 = 4;

    /// The chain between the `:` after the key and the value.
    fn new(chain: &'a str) -> Self {
        let mut out = MarkerChain { chain: Some(chain), segments: 0, seen: 0 };
        for m in out.iter() {
            out.segments += 1;
            out.seen |= match m {
                "random" => Self::RANDOM | Self::LIST,
                "unique" | "geo" | "join" => Self::LIST,
                "inherit" => Self::INHERIT,
                _ => 0,
            };
        }
        out
    }

    /// No `:` followed the key. `key: v` is not empty: its chain is one
    /// marker with no name.
    pub(crate) fn is_empty(&self) -> bool {
        self.chain.is_none()
    }

    pub(crate) fn iter(&self) -> MarkerIter<'a> {
        MarkerIter { rest: self.chain, left: MAX_MARKER_CHAIN_SEGMENTS }
    }

    fn has_random(&self) -> bool {
        self.seen & Self::RANDOM != 0
    }

    fn has_inherit(&self) -> bool {
        self.seen & Self::INHERIT != 0
    }

    /// A marker that makes a key without a value a list.
    pub(crate) fn opens_list(&self) -> bool {
        self.seen & Self::LIST != 0
    }

    /// The chain as the metadata keeps it.
    pub(crate) fn to_strings(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(self.segments);
        out.extend(self.iter().map(str::to_string));
        out
    }
}

/// The segments of a [`MarkerChain`]: what `split(':')` gives, up to the cap.
pub(crate) struct MarkerIter<'a> {
    rest: Option<&'a str>,
    left: usize,
}

impl<'a> Iterator for MarkerIter<'a> {
    type Item = &'a str;

    #[inline]
    fn next(&mut self) -> Option<&'a str> {
        let rest = self.rest?;
        if self.left == 0 {
            self.rest = None;
            return None;
        }
        self.left -= 1;
        match rest.bytes().position(|b| b == b':') {
            Some(colon) => {
                self.rest = Some(&rest[colon + 1..]);
                Some(&rest[..colon])
            }
            None => {
                self.rest = None;
                Some(rest)
            }
        }
    }
}

/// SYNX whitespace inside the ASCII range — exactly what `char::is_whitespace`
/// accepts below U+0080 (U+0009..U+000D and U+0020).
#[inline(always)]
fn is_ascii_ws(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b' ')
}

/// `str::trim` with an ASCII fast path.
///
/// perf: `str::trim` decodes every edge character; config lines almost always
/// begin and end in ASCII, so bytes are checked first and the Unicode-aware
/// trim runs only when a non-ASCII byte sits at an edge.
#[inline]
fn trim_ws(s: &str) -> &str {
    let b = s.as_bytes();
    let mut start = 0;
    while start < b.len() && is_ascii_ws(b[start]) {
        start += 1;
    }
    if start < b.len() && b[start] >= 0x80 {
        return s[start..].trim();
    }
    let mut end = b.len();
    while end > start && is_ascii_ws(b[end - 1]) {
        end -= 1;
    }
    if end > start && b[end - 1] >= 0x80 {
        return s[start..end].trim_end();
    }
    &s[start..end]
}

/// [`trim_ws`] together with [`indent_len`]: the bytes of whitespace that
/// lead the line, and the line without its whitespace.
///
/// perf: every line needs both, and both begin by walking the same indent.
#[inline]
pub(crate) fn trim_ws_at(s: &str) -> (usize, &str) {
    let b = s.as_bytes();
    let mut start = 0;
    while start < b.len() && is_ascii_ws(b[start]) {
        start += 1;
    }
    if start < b.len() && b[start] >= 0x80 {
        let rest = s[start..].trim_start();
        return (s.len() - rest.len(), rest.trim_end());
    }
    let mut end = b.len();
    while end > start && is_ascii_ws(b[end - 1]) {
        end -= 1;
    }
    if end > start && b[end - 1] >= 0x80 {
        return (start, s[start..end].trim_end());
    }
    (start, &s[start..end])
}

/// Byte length of the leading whitespace of `s` (Unicode-aware, ASCII fast path).
#[inline]
fn indent_len(s: &str) -> usize {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && is_ascii_ws(b[i]) {
        i += 1;
    }
    if i < b.len() && b[i] >= 0x80 {
        return s.len() - s.trim_start().len();
    }
    i
}

// ─── Metadata bookkeeping ────────────────────────────────

/// Keeps `ParseResult::metadata` within its budget and makes dropping a
/// replaced subtree's declarations cost what is dropped.
///
/// perf: a key that replaces an object used to `retain` over the whole table
/// — 20 000 redeclared keys beside 20 000 marked objects took 1.2 s, and the
/// time grew with the square. Now the table's paths are kept in order once
/// the first subtree goes (a range of the order is the subtree), a dropped
/// path is emptied in place, and the emptied ones leave together when the
/// parse ends — in an order no different from removing each at once.
#[derive(Default)]
struct MetaBook {
    /// Bytes of the paths ever filed (see `MAX_METADATA_PATH_BYTES`).
    path_bytes: usize,
    /// Every path that has declarations, ordered; built when first needed.
    ordered: Option<std::collections::BTreeSet<String>>,
    /// When each entry of the table was created or last re-created, by table
    /// index; kept alongside `ordered`.
    stamps: Vec<u64>,
    next_stamp: u64,
    /// Emptied entries still in the table.
    dead: usize,
    /// An emptied entry was declared again, so the table is out of order.
    reordered: bool,
}

impl MetaBook {
    /// The table index of the path the current stack files under.
    fn current_index(metadata: &Map<MetaMap>, stack: &[(i32, StackEntry<'_>)], cache: &mut NavCache) -> Option<usize> {
        if !cache.meta_path_valid {
            fill_path(stack, &mut cache.meta_path);
            cache.meta_path_valid = true;
            cache.meta_index = None;
        }
        if cache.meta_index.is_none() {
            cache.meta_index = metadata.get_index_of(cache.meta_path.as_str());
        }
        cache.meta_index
    }

    /// Whether a declaration at the current stack path still fits the budget.
    fn has_room(&self, metadata: &Map<MetaMap>, stack: &[(i32, StackEntry<'_>)], cache: &mut NavCache) -> bool {
        Self::current_index(metadata, stack, cache).is_some()
            || self.path_bytes.saturating_add(cache.meta_path.len()) <= MAX_METADATA_PATH_BYTES
    }

    /// File `meta` for `key` under the current stack path; `false` when the
    /// path is new and past the budget.
    fn record(
        &mut self,
        metadata: &mut Map<MetaMap>,
        stack: &[(i32, StackEntry<'_>)],
        cache: &mut NavCache,
        key: &str,
        meta: Meta,
    ) -> bool {
        // perf: the path — and with it the table entry — changes only with
        // the stack, so both are found after a push or pop, not per marked key.
        let idx = match Self::current_index(metadata, stack, cache) {
            Some(idx) => {
                if self.ordered.is_some() && metadata[idx].is_empty() {
                    // Declared again after its subtree was dropped: it counts
                    // as new, as if the old entry had been removed at once.
                    self.dead -= 1;
                    self.stamps[idx] = self.next_stamp;
                    self.next_stamp += 1;
                    self.reordered = true;
                    if let Some(ordered) = self.ordered.as_mut() {
                        ordered.insert(cache.meta_path.clone());
                    }
                }
                idx
            }
            None => {
                let path = cache.meta_path.as_str();
                if self.path_bytes.saturating_add(path.len()) > MAX_METADATA_PATH_BYTES {
                    return false;
                }
                self.path_bytes += path.len();
                let (idx, _) = metadata.insert_full(path.to_string(), MetaMap::default());
                if let Some(ordered) = self.ordered.as_mut() {
                    ordered.insert(path.to_string());
                    self.stamps.push(self.next_stamp);
                    self.next_stamp += 1;
                }
                cache.meta_index = Some(idx);
                idx
            }
        };
        metadata[idx].insert(key.to_string(), meta);
        true
    }

    /// Drop the declarations filed under `prefix` and under every path below it.
    fn drop_subtree(&mut self, metadata: &mut Map<MetaMap>, prefix: &str) {
        if self.ordered.is_none() {
            self.ordered = Some(
                metadata.iter().filter(|(_, mm)| !mm.is_empty()).map(|(path, _)| path.clone()).collect(),
            );
            self.stamps = (0..metadata.len() as u64).collect();
            self.next_stamp = metadata.len() as u64;
        }
        let ordered = self.ordered.as_mut().expect("set above");
        // `prefix` itself, then `prefix.…`: `/` is the byte after `.`.
        let mut below = String::with_capacity(prefix.len() + 1);
        below.push_str(prefix);
        below.push('.');
        let mut end = String::with_capacity(prefix.len() + 1);
        end.push_str(prefix);
        end.push('/');
        let span = (std::ops::Bound::Included(below.as_str()), std::ops::Bound::Excluded(end.as_str()));
        let mut doomed: Vec<String> = ordered.range::<str, _>(span).cloned().collect();
        if ordered.contains(prefix) {
            doomed.push(prefix.to_string());
        }
        for path in doomed {
            ordered.remove(path.as_str());
            if let Some(mm) = metadata.get_mut(path.as_str()) {
                if !mm.is_empty() {
                    mm.clear();
                    self.dead += 1;
                }
            }
        }
    }

    /// Remove the emptied entries and restore the order of the rest.
    fn finish(self, metadata: &mut Map<MetaMap>) {
        if self.dead == 0 && !self.reordered {
            return;
        }
        if !self.reordered {
            metadata.retain(|_, mm| !mm.is_empty());
            return;
        }
        let mut entries: Vec<(u64, String, MetaMap)> = std::mem::take(metadata)
            .into_iter()
            .zip(self.stamps)
            .filter(|((_, mm), _)| !mm.is_empty())
            .map(|((path, mm), stamp)| (stamp, path, mm))
            .collect();
        entries.sort_by_key(|(stamp, _, _)| *stamp);
        metadata.extend(entries.into_iter().map(|(_, path, mm)| (path, mm)));
    }
}

// ─── Line parser ─────────────────────────────────────────

pub(crate) fn parse_line(trimmed: &str) -> Option<ParsedLine<'_>> {
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
    let key = &trimmed[..pos];

    // Optional (type)
    let mut type_hint = None;
    if pos < len && bytes[pos] == b'(' {
        let start = pos + 1;
        if let Some(c) = memchr(b')', &bytes[start..]) {
            type_hint = Some(&trimmed[start..start + c]);
            pos = start + c + 1;
        } else {
            pos += 1;
        }
    }

    // Optional [constraints] — balanced bracket scan to support patterns like
    // `^[A-Z]{2}$` whose own `]` would otherwise close the constraint block early.
    let mut constraints = None;
    if pos < len && bytes[pos] == b'[' {
        let cstart = pos + 1;
        let mut depth = 1usize;
        let mut scan = cstart;
        while scan < len && depth > 0 {
            match bytes[scan] {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            scan += 1;
        }
        if depth == 0 {
            let constraint_str = &trimmed[cstart..scan];
            constraints = Some(parse_constraints(constraint_str));
            pos = scan + 1; // skip closing `]`
        } else {
            // Unbalanced — fall back to first `]` if any.
            if let Some(rel) = trimmed[cstart..].find(']') {
                let constraint_str = &trimmed[cstart..cstart + rel];
                constraints = Some(parse_constraints(constraint_str));
                pos = cstart + rel + 1;
            } else {
                constraints = Some(parse_constraints(&trimmed[cstart..]));
                pos = len;
            }
        }
    }

    // Optional :markers
    let mut markers = MarkerChain::default();
    let mut marker_args = Vec::new();
    if pos < len && bytes[pos] == b':' {
        let marker_start = pos + 1;
        let mut marker_end = marker_start;
        while marker_end < len && bytes[marker_end] != b' ' && bytes[marker_end] != b'\t' {
            marker_end += 1;
        }
        markers = MarkerChain::new(&trimmed[marker_start..marker_end]);
        pos = marker_end;
    }

    // Skip whitespace
    while pos < len && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
        pos += 1;
    }

    // Value
    let mut raw_value = if pos < len { strip_comment(&trimmed[pos..]) } else { "" };

    if !markers.is_empty() && !raw_value.is_empty() {
        // For :random — parse weight percentages from value
        if markers.has_random() {
            // spec: a weight is `digits[.digits]`. Anything `f64` parses
            // counted, so `a:random inf`, `-5` or `nan` lost its value to a
            // weight list while the TypeScript parser kept it.
            let nums: Vec<String> = raw_value
                .split_whitespace()
                .filter(|s| is_weight(s))
                .map(|s| s.to_string())
                .collect();
            if !nums.is_empty() {
                marker_args = nums;
                raw_value = "";
            }
        }

        // For :inherit — a non-empty value names the parent key, not a scalar.
        // Promote it into marker_args so the line opens a group instead of a leaf.
        if !raw_value.is_empty() && markers.has_inherit() {
            marker_args = vec![raw_value.trim().to_string()];
            raw_value = "";
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

/// `digits[.digits]` — a `:random` weight.
fn is_weight(s: &str) -> bool {
    let (whole, frac) = match s.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (s, None),
    };
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    digits(whole) && frac.map_or(true, digits)
}

/// A `[min:…]` / `[max:…]` bound: a finite number. `inf` and `nan` parse as
/// `f64` but bound nothing sensible (`[max:-inf]` rejected every value).
fn parse_bound(val: &str) -> Option<f64> {
    val.parse::<f64>().ok().filter(|v| v.is_finite())
}

// ─── Constraints parser ──────────────────────────────────

/// Parse the body of a `[…]` constraint block.
///
/// `pub(crate)` so the SYNXL field-list parser can reuse it verbatim, as
/// required by SYNXL §5.2 ("MUST be parsed by the implementation's existing
/// SYNX constraint parser").
pub(crate) fn parse_constraints(raw: &str) -> Constraints {
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
                "min" => c.min = parse_bound(val),
                "max" => c.max = parse_bound(val),
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

/// SYNX §8.3 automatic casting. `pub(crate)` for reuse by SYNXL §8.1.
pub(crate) fn cast(val: &str) -> Value {
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

/// SYNX §8.3 typed casting. `pub(crate)` for reuse by SYNXL §8.2.
///
/// `nondeterministic` switches the `random:*` family off for embedded parses
/// (see [`ParserOptions::nondeterministic_hints`]).
pub(crate) fn cast_typed_with(val: &str, hint: &str, nondeterministic: bool) -> Value {
    match hint {
        "int" => Value::Int(val.parse().unwrap_or(0)),
        "float" => Value::Float(val.parse().unwrap_or(0.0)),
        "bool" => Value::Bool(val.trim() == "true"),
        "string" => Value::String(val.to_string()),
        "random" | "random:int" if nondeterministic => Value::Int(rng::random_i64()),
        "random:float" if nondeterministic => Value::Float(rng::random_f64_01()),
        "random:bool" if nondeterministic => Value::Bool(rng::random_bool()),
        _ => cast(val),
    }
}

/// Cut an inline ` //` or ` #` comment and trailing whitespace off a value.
///
/// perf: a slice of `val`, never an allocation — the value is copied once,
/// by the cast that stores it.
///
/// perf: one scan. A comment opens with `#` or `/`, far rarer in a value
/// than the space before it, so those two bytes are searched for and the
/// space is checked. It used to be three searches per value — for a space,
/// for ` //`, for ` #` — each setting up its own searcher. The earlier of
/// the two openers is where the value ends, which is what cutting at ` //`
/// and then at ` #` within what is left comes to: the two cannot overlap.
fn strip_comment(val: &str) -> &str {
    let bytes = val.as_bytes();
    let mut from = 0;
    while let Some(rel) = memchr::memchr2(b'#', b'/', &bytes[from..]) {
        let at = from + rel;
        if at > 0 && bytes[at - 1] == b' ' && (bytes[at] == b'#' || bytes.get(at + 1) == Some(&b'/')) {
            return val[..at - 1].trim_end();
        }
        from = at + 1;
    }
    // Nothing cut. Lines arrive trimmed, so this is nearly always `val`.
    match bytes.last() {
        Some(&b) if b < 0x80 && !is_ascii_ws(b) => val,
        _ => val.trim_end(),
    }
}

// ─── Tree helpers ────────────────────────────────────────

fn fill_path(stack: &[(i32, StackEntry<'_>)], out: &mut String) {
    // Metadata is keyed by the dot-path the engine walks: object keys, and for
    // a list item the list's key followed by `#<index>` (a key never starts
    // with `#`, that is a comment). Following the frame chain rather than the
    // whole stack keeps an unrelated open object out of it.
    // spec: until 3.7.5 a list item contributed nothing, so its keys were
    // filed under the list's parent and every marker inside a list item was
    // skipped; an index-free path then made all items share one declaration
    // per key, so `port[max:9]` in the third server rewrote the first's
    // `port:env:default:7001`.
    out.clear();
    if stack.len() <= 1 {
        return;
    }
    let mut chain = [0usize; MAX_PARSE_NESTING_DEPTH + 1];
    let n = frame_chain(stack, stack.len() - 1, &mut chain);
    for &idx in &chain[..n] {
        match &stack[idx].1 {
            StackEntry::Key { key, .. } => {
                if !out.is_empty() {
                    out.push('.');
                }
                crate::engine::push_key_segment(out, key);
            }
            StackEntry::ListItem { list_key, item_idx, .. } => {
                if !out.is_empty() {
                    out.push('.');
                }
                crate::engine::push_key_segment(out, list_key);
                crate::engine::push_item_segment(out, *item_idx);
            }
            StackEntry::Root => {}
        }
    }
}

/// One descent from an object into an object it holds, by position.
#[derive(Clone, Copy)]
enum Step {
    /// Into the value of the key at this position.
    Key(usize),
    /// Into an item of the list under a key: `(position of the key, index of the item)`.
    Item(usize, usize),
}

/// The way from the root to the object lines are being inserted into, kept
/// from one line to the next: the stack index of that object and the steps
/// that lead to it.
///
/// perf: finding the parent again for every line walked the stack and hashed
/// each ancestor key — with a multi-megabyte ancestor key an 8 MiB file took
/// tens of seconds. The steps are positions, so following them hashes
/// nothing, and they are rebuilt only when the stack has changed.
///
/// safety: this used to be a cached `*mut Map`. Every line borrows the root
/// anew, and a pointer taken under an earlier borrow is no longer a valid
/// way in once the next one exists, however still the map itself stands.
/// Positions are plain data, and the walk along them is checked code.
#[derive(Default)]
struct NavCache {
    steps_for: Option<usize>,
    steps: Vec<Step>,
    /// Dot-path of the current stack for metadata, valid until the next push/pop.
    meta_path: String,
    meta_path_valid: bool,
    /// Where `meta_path` sits in the metadata table, once looked up. Entries
    /// are never removed while parsing, so an index stays good.
    meta_index: Option<usize>,
}

impl NavCache {
    #[inline]
    fn invalidate(&mut self) {
        self.steps_for = None;
        self.meta_path_valid = false;
        self.meta_index = None;
    }
}

/// Push a stack frame; what the cache holds describes the stack before it.
#[inline]
fn push_frame<'a>(stack: &mut Vec<(i32, StackEntry<'a>)>, cache: &mut NavCache, frame: (i32, StackEntry<'a>)) {
    stack.push(frame);
    cache.invalidate();
}

/// Pop a stack frame (see [`push_frame`]).
#[inline]
fn pop_frame<'a>(stack: &mut Vec<(i32, StackEntry<'a>)>, cache: &mut NavCache) -> Option<(i32, StackEntry<'a>)> {
    let popped = stack.pop();
    cache.invalidate();
    popped
}

/// The object of frame `parent_idx`, or `None` when the stack no longer
/// leads to one (the line is then skipped, §8.10).
fn navigate_cached<'a>(
    root: &'a mut Map,
    stack: &[(i32, StackEntry<'_>)],
    parent_idx: usize,
    cache: &mut NavCache,
) -> Option<&'a mut Map> {
    if cache.steps_for != Some(parent_idx) {
        let mut chain = [0usize; MAX_PARSE_NESTING_DEPTH + 1];
        let n = frame_chain(stack, parent_idx, &mut chain);
        cache.steps.clear();
        for &idx in &chain[..n] {
            match &stack[idx].1 {
                StackEntry::Key { slot, .. } => cache.steps.push(Step::Key(*slot)),
                StackEntry::ListItem { list_slot, item_idx, .. } => cache.steps.push(Step::Item(*list_slot, *item_idx)),
                // Root sits at index 0 only, and the chain stops above it.
                StackEntry::Root => return None,
            }
        }
        cache.steps_for = Some(parent_idx);
    }
    let mut current = root;
    for step in &cache.steps {
        let next = match *step {
            Step::Key(slot) => current.get_index_mut(slot).map(|(_, value)| value),
            Step::Item(slot, item) => match current.get_index_mut(slot) {
                Some((_, Value::Array(items))) => items.get_mut(item),
                _ => None,
            },
        };
        current = match next {
            Some(Value::Object(map)) => map,
            // The path leads to something that is not an object.
            _ => return None,
        };
    }
    Some(current)
}

/// Insert `key` into the object of frame `parent_idx` and return the key's
/// position there (`NO_SLOT` when the parent cannot be reached).
fn insert_value(
    root: &mut Map,
    stack: &[(i32, StackEntry<'_>)],
    parent_idx: usize,
    key: &str,
    value: Value,
    cache: &mut NavCache,
) -> usize {
    if let Some(target) = navigate_cached(root, stack, parent_idx, cache) {
        return target.insert_full(key.to_string(), value).0;
    }
    // If the path is broken the line is silently skipped — this should not
    // happen under well-formed input; malformed input simply loses the entry
    // rather than inserting it at the wrong nesting level.
    NO_SLOT
}

/// The position of a key that was never inserted; no object has it.
const NO_SLOT: usize = usize::MAX;

/// The frames from the root to `target`, root excluded, in descending order,
/// written into `chain`; returns how many there are.
///
/// A `Key` frame's parent is the frame below it. A `ListItem` frame's parent
/// is the frame that holds the list, which need not be the one below: an
/// object opened between the list key and its first item (`x_1` in
/// `items:join` / `x_1` / `- k v`) sits in between without being an ancestor.
/// Walking the stack in order went through it, so the item was looked up in
/// the wrong object and its keys were dropped.
fn frame_chain(stack: &[(i32, StackEntry<'_>)], target: usize, chain: &mut [usize; MAX_PARSE_NESTING_DEPTH + 1]) -> usize {
    let mut n = 0;
    let mut idx = target;
    while idx > 0 && n < chain.len() {
        chain[n] = idx;
        n += 1;
        idx = match stack[idx].1 {
            StackEntry::ListItem { parent, .. } => parent,
            _ => idx - 1,
        };
    }
    chain[..n].reverse();
    n
}

#[cfg(test)]
mod tests {

    #[test]
    fn bom_is_not_truncation() {
        let r = parse("\u{feff}a 1\n");
        assert!(!r.truncated, "a BOM is dropped, not lost content");
        assert_eq!(r.root.as_object().unwrap()["a"], Value::Int(1));
    }
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

    // SYNX 3.7 — `|+` preserves indentation relative to the first non-empty
    // continuation line. Required for embedding indent-sensitive content
    // (code, SYNX examples, ASCII diagrams) inside a multiline value.
    #[test]
    fn test_multiline_block_preserve_indent() {
        let src = "prompt |+\n  Top\n    Indented two\n      Indented four\n    Back to two\n  Top again";
        let data = parse(src);
        let root = data.root.as_object().unwrap();
        assert_eq!(
            root["prompt"],
            Value::String(
                "Top\n  Indented two\n    Indented four\n  Back to two\nTop again".into()
            )
        );
    }

    #[test]
    fn test_multiline_block_preserve_indent_locks_base() {
        // Base indent locks to the first content line (4 spaces), so all
        // subsequent lines have exactly 4 leading spaces stripped.
        let src = "code |+\n    function foo() {\n      return 1;\n    }";
        let data = parse(src);
        let root = data.root.as_object().unwrap();
        assert_eq!(
            root["code"],
            Value::String("function foo() {\n  return 1;\n}".into())
        );
    }

    #[test]
    fn test_multiline_block_preserve_indent_ends_at_opener_indent() {
        let src = "intro |+\n  hello\n    world\nnext plain";
        let data = parse(src);
        let root = data.root.as_object().unwrap();
        assert_eq!(root["intro"], Value::String("hello\n  world".into()));
        assert_eq!(root["next"], Value::String("plain".into()));
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

    // SYNXL §9.4 — an embedded parse must not honour directives: a dataset row
    // must never become a file-read primitive or flip the document's mode.
    #[test]
    fn test_directives_disabled_discards_directive_lines() {
        let src = "!active\n!include /etc/passwd\n!use @scope/pkg\nname Wario\n";
        let data = parse_with(src, ParserOptions { directives: false, nondeterministic_hints: false });
        assert_eq!(data.mode, Mode::Static);
        assert!(data.includes.is_empty());
        assert!(data.uses.is_empty());
        let root = data.root.as_object().unwrap();
        // Discarded like comments — not turned into keys either.
        assert_eq!(root.len(), 1);
        assert_eq!(root["name"], Value::String("Wario".into()));

        // Default options keep 3.7 behaviour intact.
        let data = parse(src);
        assert_eq!(data.mode, Mode::Active);
        assert_eq!(data.includes.len(), 1);
    }

    #[test]
    fn test_directives_disabled_preserves_bang_lines_in_multiline() {
        // Enforcement happens *inside* the parse, so a `!` line that is body
        // content of a `|` / `|+` block survives verbatim.
        let src = "body |+\n  !include /etc/passwd\n  !active\n  tail\n";
        let data = parse_with(src, ParserOptions { directives: false, nondeterministic_hints: false });
        assert_eq!(
            data.root.as_object().unwrap()["body"],
            Value::String("!include /etc/passwd\n!active\ntail".into())
        );
        assert!(data.includes.is_empty());
        assert_eq!(data.mode, Mode::Static);

        let src = "body |\n  !include /etc/passwd\n";
        let data = parse_with(src, ParserOptions { directives: false, nondeterministic_hints: false });
        assert_eq!(
            data.root.as_object().unwrap()["body"],
            Value::String("!include /etc/passwd".into())
        );
    }

    // A leading BOM (Notepad/Windows editors emit one) must not hide the
    // `!active` directive — SYNX §2.1/§6. Regression for the divergence where
    // BOM+!active produced a "\u{feff}!active" key and stayed Static.
    #[test]
    fn test_leading_bom_does_not_disable_active() {
        let data = parse("\u{feff}!active\nsum:calc 2 + 3\n");
        assert_eq!(data.mode, Mode::Active);
        assert!(data.root.as_object().unwrap().get("\u{feff}!active").is_none());
    }

    #[test]
    fn test_leading_bom_stripped_from_first_key() {
        let data = parse("\u{feff}name Wario\n");
        let root = data.root.as_object().unwrap();
        assert_eq!(root["name"], Value::String("Wario".into()));
        assert!(root.get("\u{feff}name").is_none());
    }

    // A multi-megabyte ancestor key with many children used to re-hash the key
    // on every child (O(children × key_len)) — an 8 MiB file took tens of
    // seconds. This must stay well under a second and produce the right tree.
    #[test]
    fn test_long_parent_key_is_linear() {
        let key = "K".repeat(1024 * 1024);
        let mut doc = String::with_capacity(2 * 1024 * 1024);
        doc.push_str(&key);
        doc.push('\n');
        for i in 0..20_000 {
            doc.push_str(&format!("  c{i} {i}\n"));
        }
        let start = std::time::Instant::now();
        let data = parse(&doc);
        assert!(start.elapsed().as_secs() < 5, "long-parent parse took {:?}", start.elapsed());
        let root = data.root.as_object().unwrap();
        let inner = root[&key].as_object().unwrap();
        assert_eq!(inner.len(), 20_000);
        assert_eq!(inner["c0"], Value::Int(0));
        assert_eq!(inner["c19999"], Value::Int(19_999));
    }

    // The navigation cache must not leak a parent pointer across sibling
    // subtrees at the same depth (would misattach keys or alias maps).
    #[test]
    fn test_nav_cache_distinguishes_siblings() {
        let data = parse("a\n  x 1\nb\n  x 2\nc\n  y 3\n  z 4\n");
        let root = data.root.as_object().unwrap();
        assert_eq!(root["a"].as_object().unwrap()["x"], Value::Int(1));
        assert_eq!(root["b"].as_object().unwrap()["x"], Value::Int(2));
        let c = root["c"].as_object().unwrap();
        assert_eq!(c["y"], Value::Int(3));
        assert_eq!(c["z"], Value::Int(4));
        assert_eq!(c.len(), 2);
        assert_eq!(root["a"].as_object().unwrap().len(), 1);
    }

    // A nested object inside one list item must not swallow the next item.
    #[test]
    fn test_list_item_after_nested_object_keeps_its_keys() {
        let data = parse("items
  - name a
    sub
      k 1
  - name b
    v 2
    w 3
after 1
");
        let root = data.root.as_object().unwrap();
        let items = root["items"].as_array().unwrap();
        let b = items[1].as_object().unwrap();
        assert_eq!(b["name"], Value::String("b".into()));
        assert_eq!(b["v"], Value::Int(2));
        assert_eq!(b["w"], Value::Int(3));
        assert_eq!(root["after"], Value::Int(1));
    }

    #[test]
    fn test_objects_keep_document_order() {
        let data = parse("zeta 1
alpha 2
mid
  y 1
  b 2
");
        let root = data.root.as_object().unwrap();
        let keys: Vec<&str> = root.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, ["zeta", "alpha", "mid"]);
        let mid: Vec<&str> = root["mid"].as_object().unwrap().keys().map(|k| k.as_str()).collect();
        assert_eq!(mid, ["y", "b"]);
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

#[cfg(test)]
mod hardening_tests {
    use super::*;

    fn json(text: &str) -> String {
        crate::to_json(&parse(text).root)
    }

    // S1-02: a list inside a list item used to replace the outer list, so the
    // items after it were dropped and their keys landed in the item before.
    #[test]
    fn items_after_a_nested_list_are_kept() {
        let src = "items\n  - name a\n    tags\n      - x\n      - y\n  - name b\n    k 2\n  - name c\nafter 1\n";
        assert_eq!(
            json(src),
            r#"{"after":1,"items":[{"name":"a","tags":["x","y"]},{"k":2,"name":"b"},"name c"]}"#
        );
        // Two levels of nesting, and back out of both.
        let src = "a\n  - n 1\n    b\n      - m 2\n        c\n          - z\n      - m 3\n        w 5\n  - n 4\n    v 6\n";
        assert_eq!(json(src), r#"{"a":[{"b":[{"c":["z"],"m":2},{"m":3,"w":5}],"n":1},{"n":4,"v":6}]}"#);
    }

    // An empty key right before the next `- ` line opens a list of its own
    // (§8.5.2); the outer list must survive it.
    #[test]
    fn an_empty_key_before_the_next_item_keeps_the_outer_list() {
        let src = "list\n  - total 7\n    db\n  - k1 x\n    hp 3\n";
        assert_eq!(json(src), r#"{"list":[{"db":[],"total":7},{"hp":3,"k1":"x"}]}"#);
    }

    // A dash no list accepts is skipped and closes nothing, as before.
    #[test]
    fn a_dash_no_list_accepts_closes_nothing() {
        let src = "a\n  tags\n    - x\n - stray\n    - y\n";
        assert_eq!(json(src), r#"{"a":{"tags":["x","y"]}}"#);
        // A dash at the indent of a nested list's own key is not its item and
        // does not close it either.
        let src = "a\n  - n 1\n    tags\n    - x\n  - m 2\n    z 1\n";
        assert_eq!(json(src), r#"{"a":[{"n":1,"tags":[]},{"m":2,"z":1}]}"#);
    }

    // S1-07: a comment is not content.
    #[test]
    fn a_comment_does_not_stand_between_a_key_and_its_list() {
        assert_eq!(json("items\n  # note\n  - a\n  - b\n"), r#"{"items":["a","b"]}"#);
        assert_eq!(json("items\n  // note\n  - a\n"), r#"{"items":["a"]}"#);
        assert_eq!(json("items\n###\nold - text\n###\n  - a\n"), r#"{"items":["a"]}"#);
        assert_eq!(
            json("items\n  - name a\n    # note\n    port 1\n  - name b\n    port 2\n"),
            r#"{"items":[{"name":"a","port":1},{"name":"b","port":2}]}"#
        );
        // What already worked still does.
        assert_eq!(json("l\n  - a\n  # about a\n  - b\n"), r#"{"l":["a","b"]}"#);
        assert_eq!(json("srv\n  # c\n  host x\n"), r#"{"srv":{"host":"x"}}"#);
        assert_eq!(json("k\n# only a comment\nother 1\n"), r#"{"k":{},"other":1}"#);
    }

    // B-076: what stands under a rejected key goes with it.
    #[test]
    fn the_block_of_a_rejected_key_is_dropped() {
        let src = "user\n  profile\n    name x\n  __proto__\n    role admin\n    name y\n  age 3\n";
        assert_eq!(json(src), r#"{"user":{"age":3,"profile":{"name":"x"}}}"#);
        let src = "limits\n  max 10\n  constructor\n    max 99\n    deep\n      max 100\nuse 1\n";
        assert_eq!(json(src), r#"{"limits":{"max":10},"use":1}"#);
        // Text under one is the text of the dropped key, not lines of the document.
        let r = parse("a 1\nprototype |\n  !active\n  b:env HOME\n  ###\nc 2\n");
        assert_eq!(r.mode, Mode::Static);
        assert_eq!(crate::to_json(&r.root), r#"{"a":1,"c":2}"#);
        // A list under one goes too, and the list above it goes on.
        assert_eq!(json("l\n  - a 1\n    __proto__\n      - x\n      - y\n    b 2\n  - c 3\n    d 4\n"), r#"{"l":[{"a":1,"b":2},{"c":3,"d":4}]}"#);
        // On the dash line the key is dropped; the lines under the dash are the item's.
        assert_eq!(json("l\n  - __proto__ x\n    k 1\n"), r#"{"l":[{"k":1}]}"#);
        // `format` keeps what the parser keeps.
        let text = "user\n  profile\n    name x\n  __proto__\n    role admin\n  age 3\n";
        assert_eq!(crate::to_json(&parse(&crate::Synx::format(text)).root), json(text));
    }

    // S1-09: the key on the `- ` line declares like any other key of the item.
    #[test]
    fn the_key_on_the_dash_line_keeps_its_declarations() {
        let r = parse("!active\nl\n  - host:env H\n    port[min:10] 1\n  - one:env X\n");
        let item = r.metadata.get("l.#0").expect("declarations of the first item");
        assert_eq!(item["host"].markers, vec!["env"]);
        assert_eq!(item["port"].constraints.as_ref().unwrap().min, Some(10.0));
        // An item of one line is text, as it always was.
        let l = r.root.as_object().unwrap()["l"].as_array().unwrap();
        assert_eq!(l[1], Value::String("one:env X".into()));
        // Static documents record nothing.
        assert!(parse("l\n  - host:env H\n    port 1\n").metadata.is_empty());
    }

    // B-011: a weight is `digits[.digits]`.
    #[test]
    fn random_weights_are_plain_numbers() {
        let r = parse("!active\na:random inf\nb:random -5\nc:random 90 10.5\nd:random 1e2\ne:random nan\n");
        let root = r.root.as_object().unwrap();
        assert_eq!(root["a"], Value::String("inf".into()));
        assert_eq!(root["b"], Value::Int(-5));
        assert_eq!(root["c"], Value::Array(Vec::new()));
        assert_eq!(root["d"], Value::String("1e2".into()));
        assert_eq!(root["e"], Value::String("nan".into()));
        let meta = &r.metadata[""];
        assert!(meta["a"].args.is_empty());
        assert_eq!(meta["c"].args, vec!["90", "10.5"]);
    }

    // B-005: `inf` and `nan` bound nothing.
    #[test]
    fn constraint_bounds_are_finite() {
        let r = parse("!active\np[max:-inf] 5\nq[min:inf] 5\nr[min:nan] 5\ns[min:2, max:9] 5\n");
        let meta = &r.metadata[""];
        assert_eq!(meta["p"].constraints.as_ref().unwrap().max, None);
        assert_eq!(meta["q"].constraints.as_ref().unwrap().min, None);
        assert_eq!(meta["r"].constraints.as_ref().unwrap().min, None);
        let s = meta["s"].constraints.as_ref().unwrap();
        assert_eq!((s.min, s.max), (Some(2.0), Some(9.0)));
    }

    // S4-04: the list cap reports what it dropped.
    #[test]
    fn the_list_cap_sets_truncated() {
        let mut doc = String::with_capacity(5 * 1024 * 1024);
        doc.push_str("l\n");
        for _ in 0..(MAX_LIST_ITEMS + 3) {
            doc.push_str(" - 1\n");
        }
        let r = parse(&doc);
        assert_eq!(r.root.as_object().unwrap()["l"].as_array().unwrap().len(), MAX_LIST_ITEMS);
        assert!(r.truncated);
        assert!(!parse("l\n - 1\n - 2\n").truncated);
    }

    // Every object with a marked key stores its whole path; a megabyte of
    // ancestor key above a hundred of them was a hundred megabytes of paths.
    #[test]
    fn metadata_paths_have_a_budget() {
        let key = "K".repeat(1024 * 1024);
        let mut doc = format!("!active\n{}\n", key);
        for i in 0..100 {
            doc.push_str(&format!("  o{}\n    m:env X\n", i));
        }
        doc.push_str("last 1\n");
        let r = parse(&doc);
        assert!(r.truncated);
        let bytes: usize = r.metadata.keys().map(|p| p.len()).sum();
        assert!(bytes <= MAX_METADATA_PATH_BYTES, "{}", bytes);
        assert!(r.metadata.len() >= 60 && r.metadata.len() < 100, "{}", r.metadata.len());
        // The document ends on a whole line: the object whose marked key did
        // not fit is there, empty, and nothing after it.
        let inner = r.root.as_object().unwrap()[&key].as_object().unwrap();
        assert_eq!(inner.len(), r.metadata.len() + 1);
        assert_eq!(inner.get_index(inner.len() - 1).unwrap().1, &Value::Object(Map::default()));
        assert!(!r.root.as_object().unwrap().contains_key("last"));
        // A document of ordinary shape is nowhere near it.
        let mut doc = String::from("!active\n");
        for i in 0..5000 {
            doc.push_str(&format!("svc{}\n  port:env:default:{} P{}\n", i, i, i));
        }
        let r = parse(&doc);
        assert!(!r.truncated);
        assert_eq!(r.metadata.len(), 5000);
    }

    // A key that replaces an object replaces its declarations — and the
    // table ends up exactly as if each had been removed on the spot.
    #[test]
    fn a_redeclared_key_drops_its_subtree_declarations() {
        let r = parse("!active\nx:env A\na\n  p:env B\n  q\n    r:env C\ny:env D\na\n  s:env E\n");
        let paths: Vec<&str> = r.metadata.keys().map(|k| k.as_str()).collect();
        assert_eq!(paths, ["", "a"]);
        assert_eq!(r.metadata["a"].keys().map(|k| k.as_str()).collect::<Vec<_>>(), ["s"]);
        assert_eq!(r.metadata[""].keys().map(|k| k.as_str()).collect::<Vec<_>>(), ["x", "y"]);

        // Declared again after another path: it now comes after it.
        let r = parse("!active\na\n  p:env B\nb\n  q:env C\na\n  s:env E\n");
        assert_eq!(r.metadata.keys().map(|k| k.as_str()).collect::<Vec<_>>(), ["b", "a"]);

        // Not declared again: gone.
        let r = parse("!active\na\n  p:env B\n  l\n    - k:env Z\n      v 1\nb\n  q:env C\na 5\n");
        assert_eq!(r.metadata.keys().map(|k| k.as_str()).collect::<Vec<_>>(), ["b"]);

        // A key that only shares a prefix is another key.
        let r = parse("!active\nab\n  p:env B\na\n  q:env C\na 1\n");
        assert_eq!(r.metadata.keys().map(|k| k.as_str()).collect::<Vec<_>>(), ["ab"]);
    }

    // The frames of the stack find their objects by position, so the keys
    // above them are not hashed again on every line.
    #[test]
    fn positions_survive_repeated_and_replaced_keys() {
        let src = "a\n  x 1\nb\n  y 2\na\n  z 3\nl\n  - p 1\n    q 2\nl\n  - r 3\n    s\n      t 4\n";
        assert_eq!(json(src), r#"{"a":{"z":3},"b":{"y":2},"l":[{"r":3,"s":{"t":4}}]}"#);
        // A list key replaced by a scalar while the list is open takes no more items.
        assert_eq!(json("tags\n  - a\n  tags 5\n  - b\n"), r#"{"tags":5}"#);
    }
}

/// The scanning helpers were rewritten for speed; each is held to the form
/// it replaced over generated inputs.
#[cfg(test)]
mod equivalence_tests {
    use super::*;

    /// Small deterministic generator (xorshift64*), so a failure reproduces.
    struct Gen(u64);

    impl Gen {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn text(&mut self, alphabet: &[&str], max_len: usize) -> String {
            let len = (self.next() % (max_len as u64 + 1)) as usize;
            let mut s = String::new();
            for _ in 0..len {
                s.push_str(alphabet[(self.next() % alphabet.len() as u64) as usize]);
            }
            s
        }
    }

    fn reference_strip_comment(val: &str) -> &str {
        let mut result = val;
        if let Some(idx) = result.find(" //") {
            result = &result[..idx];
        }
        if let Some(idx) = result.find(" #") {
            result = &result[..idx];
        }
        result.trim_end()
    }

    #[test]
    fn strip_comment_cuts_where_two_searches_did() {
        let alphabet = [" ", " ", "#", "/", "/", "a", "b", "\t", "é", "\u{a0}", "\u{3000}", "-", ":"];
        let mut g = Gen(0x9E37_79B9_7F4A_7C15);
        for _ in 0..200_000 {
            let s = g.text(&alphabet, 14);
            assert_eq!(strip_comment(&s), reference_strip_comment(&s), "{:?}", s);
        }
        for s in ["", " ", "#", " #", "a #", "a //", "a /", "a / /", "//x", "a  // b # c", "a # b // c", "http://x/y", "x /#", "a \u{3000}", "ю // я"] {
            assert_eq!(strip_comment(s), reference_strip_comment(s), "{:?}", s);
        }
    }

    #[test]
    fn trim_ws_at_is_trim_and_indent() {
        let alphabet = [" ", " ", "\t", "\r", "\u{b}", "\u{c}", "a", "é", "\u{a0}", "\u{3000}", "\u{feff}", "\u{85}", "-"];
        let mut g = Gen(0xD1B5_4A32_D192_ED03);
        for _ in 0..200_000 {
            let s = g.text(&alphabet, 10);
            let (lead, trimmed) = trim_ws_at(&s);
            assert_eq!(trimmed, s.trim(), "{:?}", s);
            assert_eq!(trimmed, trim_ws(&s), "{:?}", s);
            if !trimmed.is_empty() {
                assert_eq!(lead, s.len() - s.trim_start().len(), "{:?}", s);
                assert_eq!(lead, indent_len(&s), "{:?}", s);
            }
        }
    }

    #[test]
    fn marker_chain_is_the_split_it_replaced() {
        let alphabet = [":", ":", "a", "env", "default", "random", "join", "inherit", "geo", "0", ".", "é"];
        let mut g = Gen(0x94D0_49BB_1331_11EB);
        for _ in 0..50_000 {
            let chain = g.text(&alphabet, 9);
            let line = format!("k:{} v", chain);
            let parsed = parse_line(&line).expect("a key line");
            let want: Vec<&str> = chain.split(':').take(MAX_MARKER_CHAIN_SEGMENTS).collect();
            assert_eq!(parsed.markers.iter().collect::<Vec<_>>(), want, "{:?}", line);
            assert_eq!(parsed.markers.to_strings(), want, "{:?}", line);
            assert!(!parsed.markers.is_empty());
            assert_eq!(parsed.markers.has_random(), want.contains(&"random"));
            assert_eq!(parsed.markers.has_inherit(), want.contains(&"inherit"));
            assert_eq!(parsed.markers.opens_list(), want.iter().any(|m| matches!(*m, "random" | "unique" | "geo" | "join")));
        }
        // No colon, no chain; a bare colon is a chain of one nameless marker.
        assert!(parse_line("k v").unwrap().markers.is_empty());
        assert_eq!(parse_line("k: v").unwrap().markers.to_strings(), [""]);
        // The cap on segments holds for the stored chain as well.
        let long = format!("k:{} v", "m:".repeat(MAX_MARKER_CHAIN_SEGMENTS + 10));
        assert_eq!(parse_line(&long).unwrap().markers.to_strings().len(), MAX_MARKER_CHAIN_SEGMENTS);
    }

    fn reference_parse_end(bytes: &[u8], max_newlines: usize) -> usize {
        let mut seen = 0usize;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                if seen >= max_newlines {
                    return i;
                }
                seen += 1;
            }
        }
        bytes.len()
    }

    #[test]
    fn the_line_cap_cuts_before_the_same_newline() {
        let max = MAX_LINE_STARTS - 1;
        // Under the cap by size alone, exactly at it, one over, and well over.
        for newlines in [0usize, 5, max - 1, max, max + 1, max + 1000] {
            let mut text = String::with_capacity(newlines * 2 + 8);
            for i in 0..newlines {
                text.push_str(if i % 3 == 0 { "a\n" } else { "\n" });
            }
            text.push_str("tail");
            assert_eq!(find_parse_end_bytes(text.as_bytes()), reference_parse_end(text.as_bytes(), max), "{} newlines", newlines);
        }
        // Many bytes, few lines: counted, not cut.
        let wide = format!("k {}\nlast 1\n", "v".repeat(MAX_LINE_STARTS + 10));
        assert_eq!(find_parse_end_bytes(wide.as_bytes()), wide.len());
    }
}
