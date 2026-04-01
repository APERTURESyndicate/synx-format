# SYNX Language Specification — Normative Document

**Protocol / language version:** SYNX **3.6**  
**Canonical reference implementation:** Rust crate `synx-core` version 3.6.x (this repository)  
**Media type / file extension (text):** `.synx` (informative; not registered with IANA)  
**Related binary container:** `.synxb` format version **1** (orthogonal versioning; see §12)

This document uses [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119) keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** where capitalized.

---

## 1. Introduction

### 1.1 Purpose

SYNX 3.6 is a line-oriented, indentation-based textual notation for tree-structured data (objects, arrays, and scalar values). A conforming implementation produces a logical **value tree** equivalent to a JSON data model (objects with string keys, arrays, null, booleans, numbers, strings), plus optional **metadata**, **directives**, and an optional **binary encoding** of the parsed result.

Informative prose in guides (for example `SPECIFICATION_EN.md`) is **not** normative if it contradicts this document or the reference parser behavior encoded in the conformance suite (§11).

### 1.2 Conformance

An implementation is **SYNX 3.6 conforming** for a given input if and only if:

1. It accepts UTF-8 text (§2).
2. It applies the line and truncation rules (§3).
3. It interprets directives and comments as specified (§4–5).
4. For each non-`!tool` document, the **canonical JSON projection** of the root value (§10) is **byte-identical** to the output of the reference `synx_core::parse` + `synx_core::to_json`, subject to the same truncation and resource limits (§13), **or** the implementation documents a deliberate subset with explicit deviations (non-conforming subset).
5. For `!tool` documents, the same holds for `Synx::parse_tool` (§9).

The repository’s `tests/conformance/` cases are the **practical conformance contract** (§11).

---

## 2. Character encoding and text

1. **SYNX text MUST be interpreted as Unicode encoded in UTF-8.** A byte order mark (U+FEFF) at the start MAY be treated as whitespace by trim operations; implementations SHOULD behave consistently with the reference (trimmed line processing).
2. **Newlines:** The input is a sequence of **lines** separated by `LF` (`U+000A`). If `CR` (`U+000D`) immediately precedes `LF` on a physical line, the `CR` MUST NOT be part of the line’s logical content (strip trailing `CR` before further processing).
3. **Schemata in this document** use ABNF from [RFC 5234](https://www.rfc-editor.org/rfc/rfc5234). `WSP` = space or horizontal tab unless noted.

---

## 3. Input bounds and truncation (normative)

Before parsing, the reference implementation applies the following **MUST** rules. Conforming implementations MUST either match them or declare a strict subset.

| Limit | Value | Effect |
|--------|--------|--------|
| `MAX_SYNX_INPUT_BYTES` | 16 777 216 (16 MiB) | Input is truncated to a valid UTF-8 prefix not exceeding this byte length. |
| `MAX_LINE_STARTS` | 2 000 000 | At most this many lines (count of `LF` + 1) are considered; content after the newline that would exceed line count is dropped. |
| `MAX_PARSE_NESTING_DEPTH` | 128 | Object/group stack depth for subsequent opens is capped; excess depth still creates objects but does not push deeper stack frames (see §8.6). |
| `MAX_MULTILINE_BLOCK_BYTES` | 1 048 576 (1 MiB) | Accumulated body of a multiline value (§8.4) stops growing after this. |
| `MAX_LIST_ITEMS` | 1 048 576 | Per-list item cap. |
| `MAX_INCLUDE_DIRECTIVES` | 4 096 | `!include` directives recorded per file. |
| `MAX_CONSTRAINT_ENUM_PARTS` | 4 096 | Parts in `enum` constraint after split. |
| `MAX_MARKER_CHAIN_SEGMENTS` | 512 | Segments in `:a:b:c` marker chain. |

---

## 4. Lines and trimming

For each line:

1. Let **raw** be the line’s UTF-8 content after §2.2 `CR` handling.
2. Let **trimmed** be **raw** with leading and trailing Unicode whitespace removed (reference uses Rust `str::trim()`).

**Indentation width** used for the tree algorithm is:

```text
indent = length(raw) - length(ltrim(raw))
```

where `ltrim` removes **only leading** Unicode whitespace. Thus both space and tab contribute to indent **as code units**, not as visual columns.

---

## 5. Comments and ignored lines

1. A line whose **trimmed** form is empty MUST be ignored.
2. A line whose **trimmed** form begins with `#` (other than the special `#!mode:` directive, §6) MUST be ignored as a **line comment**.
3. A line whose **trimmed** form begins with `//` MUST be ignored as a line comment.
4. **Block comments:** A line whose **trimmed** form is exactly `###` toggles **block comment mode**. While block comment mode is on, all lines until the next `###` line MUST be ignored (the toggling lines themselves do not nest levels).

---

## 6. Directives (non-data lines)

All directives are recognized on **trimmed** lines. Order matters for mode flags (processed top to bottom).

| Line (trimmed) | Effect |
|----------------|--------|
| `!active` | Set parse **mode** to Active (metadata collection enabled, §8.7). |
| `!lock` | Set **locked** flag on result (informative for engines). |
| `!tool` | Set **tool** flag; JSON reshaping uses §9 when using `parse_tool`. |
| `!schema` | Set **schema** flag; used with `!tool` for schema reshape (§9). |
| `!llm` | Set **llm** flag on `ParseResult`: document is an LLM-oriented envelope (informative for tools). The value tree MUST be identical to parsing the same lines with `!llm` omitted. |
| `!include <path> [<alias>]` | Record include directive; path and optional alias per reference whitespace split (9 bytes prefix `!include `). |
| `#!mode:static` or `#!mode:active` | Set mode to Static or Active (same as `!active` for `active`). |

Directives MUST NOT be treated as key lines. Implementations MUST ignore leading BOM when matching these tokens if the trimmed line still equals the directive after trim.

---

## 7. Lexical structure of a **key line**

A **key line** is a non-empty trimmed line that:

- is not a directive (§6),
- is not entirely in block comment,
- is not a list continuation line starting with `- ` (dash + space) in list context (§8.5),
- does not start with `[`, `:`, `-`, `#`, `/` as the **first UTF-8 scalar** (so `[key`, `:foo`, `-`, `#`, `//` are rejected as key lines),
- **does not** start with `(` (opening paren in first position),

and is tokenized **as implemented** in reference function `parse_line` (informative outline):

- **Key:** maximal prefix of UTF-8 scalars until `SP`, `HTAB`, `[`, `:`, or `(`.
- **Optional `(type)`:** if next char is `(`, consume until first `)` or abort subparse.
- **Optional `[constraints]`:** if next is `[`, consume until first `]` or advance one char.
- **Optional `:markers`:** if next is `:`, consume marker run until `SP`/`HTAB`; split on `:` into segments (cap §3).
- **Value:** rest of line after skipping `SP`/`HTAB`, then §8.3 comment stripping.

If the first character would be `[`, `:`, `-`, `#`, `/`, `(`, the reference returns **no** key line (line skipped for structure purposes). This is **not** an error; parsing continues.

---

## 8. Data model and parsing behavior

### 8.1 Root

The root value is always a JSON **object** (string-keyed map), possibly empty.

### 8.2 Key–value with explicit value

If a key line has a **non-empty** `value`, or a value that is present after markers/casts, the reference inserts a scalar or composite per **casting** (§8.3). Exception: value exactly `|` triggers multiline mode (§8.4).

If `type-cast` is present, **typed casting** (`cast_typed`) applies; otherwise **automatic casting** (`cast`).

### 8.3 Automatic and typed casting

**Automatic `cast` (no type hint):**

1. If the value is surrounded by ASCII quotes `"`…`"` or `'`…`'` with length ≥ 2, the result is a **string** of the inner text (no escape processing inside—literal substring).
2. Else if value is exactly `true` / `false` / `null` (ASCII, case-sensitive), yield boolean or null.
3. Else if value matches **integer** grammar: optional `-` then one or more ASCII digits, no leading zeros restriction beyond what `i64` accepts in reference—yield integer.
4. Else if value matches **decimal float** grammar: optional `-`, digits, single `.`, digits, and parses as `f64`—yield float.
5. Else yield string.

**Typed `cast_typed`:** `int`, `float`, `bool`, `string` coerce; unknown hint falls back to `cast`. Hints `random`, `random:int`, `random:float`, `random:bool` are **non-deterministic** in the reference; conforming docs SHOULD note non-determinism for reproducible interchange.

**Inline comment stripping:** After the value is extracted from the line, the reference removes trailing substrings starting at the **first** occurrence of ` //` or ` #` (space + two slashes, or space + hash), then trims end whitespace.

### 8.4 Multiline string (`|`)

If the parsed `value` is exactly `|`, a **multiline block** opens for that key. Subsequent lines with **strictly greater** `indent` than the opening line append to the string body (trimmed line text, joined with `LF`), until a line with `indent ≤` opener’s indent ends the block. Body size bounded per §3.

### 8.5 Lists

1. **List marker:** If markers contain `random`, `unique`, `geo`, or `join`, and value is empty, the reference opens a **list** under that key at current indent.
2. **Implicit list from group:** If key line has **empty** value and is not `|`, not list-marker form, the reference **peeks** at following non-empty lines; if the first such line starts with `- `, a list opens.
3. **Items:** Lines starting with `- `, with indent strictly greater than list indent, append `cast(strip_comment(item))` to the list until indent returns.
4. **Nested list / `-` outside list:** `- ` at left margin without an open list follows list-closing rules (reference closes open list when indent ≤ list indent).

### 8.6 Groups (empty object / nesting)

If a key line has empty value, is not opening multiline, and does not open a list, the reference inserts an **empty object** under that key and **pushes** `(indent, key)` onto the stack unless stack depth would exceed `MAX_PARSE_NESTING_DEPTH`—in which case the object is still inserted but stack push is skipped (deeper lines may attach to wrong semantic parent; avoidance is implementation quality; reference behavior is as coded).

**Stack repair:** Before inserting each key line, pop stack while `top.indent >= current line indent`.

### 8.7 Active mode and metadata

When **mode** is Active, for each parsed key line that has any of: non-empty markers, non-empty constraints, or type-cast, the reference stores **metadata** in a side table keyed by **dot-path** of ancestor keys (root segments only, not array indices) plus the key name. Metadata content: markers, marker arguments, type hint, constraints structure.

In **Static** mode, the same lines parse to values but **do not** populate the metadata map.

### 8.8 Constraints (informative grammar)

Inside `[`…`]`, comma-separated parts apply:

- `required`, `readonly` flags.
- `min:<n>`, `max:<n>`, `type:<name>`, `pattern:<regex>` (string stored).
- `enum:a|b|c` split with `|`, capped at `MAX_CONSTRAINT_ENUM_PARTS`.

Unknown `key:value` pairs are ignored by the reference constraint parser.

### 8.9 Marker `:random` with numeric weights

If markers include `random` and the value token list contains numeric tokens, the reference may repurpose them as `marker_args` and clear the line value (implementation-specific weight handling for engines).

### 8.10 Malformed parent navigation

If internal navigation cannot resolve the parent object for an insertion, the reference **silently skips** inserting that entry (no global parse error). This is a semantic edge case; authors SHOULD keep indentation consistent.

---

## 9. `!tool` reshaping

When processing with **`parse_tool`**, after ordinary parse:

1. If **schema** flag false (call mode): sort object keys lexicographically; take **first** key as tool name; its object value becomes `params`; output object `{"tool":"<name>","params":{...}}`. If root empty, `tool` is JSON `null` and `params` is `{}`.
2. If **schema** flag true: each top-level key becomes `{ "name": key, "params": child }` in an array `tools`, sorted by key.

**Conformance:** Files in `tests/conformance/` that begin with `!tool` MUST use this path.

---

## 10. Canonical JSON projection

For interchange testing, **canonical JSON** is produced by:

1. Mapping the logical `Value` to JSON types (object keys are Unicode strings; numbers as JSON numbers; strings escaped per RFC 8259 style in reference: `\"`, `\\`, `\n`, `\r`, `\t`, `\uXXXX` for U+0000–U+001F).
2. **Object keys sorted lexicographically** (Unicode scalar comparison as Rust `Ord` on `str`).
3. Arrays preserve order.
4. No insignificant whitespace.

Array serialization and string escaping MUST match `synx_core::write_json` for conformance tests.

---

## 11. Conformance suite

The directory `tests/conformance/cases/` contains paired files `*.synx` and `*.expected.json`. For each pair, a conforming implementation MUST emit the exact `expected.json` bytes for that input under reference truncation rules, using:

- `parse` + `to_json` on root (default), or
- `parse_tool` + `to_json` when the input (after trim start) begins with `!tool`.

Adding cases is **backward compatible** for clients; changing `expected.json` for an unchanged `.synx` is a **breaking change** to the language definition unless tied to a **new language version** document.

---

## 12. Binary format `.synxb` (separate version axis)

The **binary container** version is **1** (magic `SYNXB`, 1-byte version in header). It encodes a `ParseResult` (value tree + flags + metadata + includes). Header flags include, among others: active, locked, has_metadata, resolved, tool, schema, **llm** (bit 6). A full on-the-wire layout lives in `crates/synx-core/src/binary.rs`. Implementations MUST NOT confuse `.synxb` version **1** with **SYNX language 3.6**.

---

## 13. Security considerations

1. **Resource limits** (§3) are mandatory for robust implementations to mitigate denial of service.
2. **Silent drops** (§8.10) mean validators MUST NOT assume “accepted file” equals “all lines became data”.
3. **`!include`** records paths; resolvers MUST sandbox file access in hostile settings.
4. **Non-deterministic** `random` markers affect reproducibility.

---

## 14. Differences from JSON (RFC 8259)

| Aspect | JSON RFC 8259 | SYNX 3.6 |
|--------|------------------|----------|
| Syntax | Token-based | Line + indentation |
| Types | Explicit literals | Inference + optional casts |
| Duplicate keys | undefined | Last wins when same insert path (HashMap); conformance tests avoid ambiguity |
| Number precision | IEEE double for “number” | `i64` / `f64` in reference; edge cases may differ from JSON-only pipelines |
| Root | any value | object at parse output |

---

## 15. Document status

**Normative:** Sections 1–13 for language **SYNX 3.6**.  
**Editor:** Maintainers of `synx-format`; errata against `synx-core` 3.6.x and `tests/conformance/`.

---

*End of normative specification.*
