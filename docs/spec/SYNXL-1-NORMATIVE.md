# SYNXL Specification — Normative Document

**Format name:** SYNXL ("SYNX Lines")
**Format version:** **1** (own version axis; see §1.3)
**Embedded language:** SYNX **3.7** ([`SYNX-3.7-NORMATIVE.md`](SYNX-3.7-NORMATIVE.md))
**Canonical reference implementation:** Rust crate `synx-core` version 3.7.x (this repository)
**File extension (text):** `.synxl` (informative; not registered with IANA)

This document uses [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119) keywords **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** where capitalized.

---

## 1. Introduction

### 1.1 Purpose

SYNXL is a **record-stream format** for homogeneous datasets: the SYNX-native counterpart of JSONL and CSV. A SYNXL document declares a **field list** once, then carries an arbitrary number of **records**. Each record projects to a JSON object; a whole document projects to a JSON array of objects (§12).

SYNXL targets two workloads:

1. **AI/LLM datasets** — training samples, few-shot corpora, eval sets, prompt libraries. Long multi-line text stays readable text instead of `\n`-escaped noise, and nested chat structures (`messages` as a list of role/content objects) are expressible directly.
2. **Tabular data** — database exports, analytics extracts. Unlike CSV, the field list carries types and constraints; unlike CSV, an empty field is unambiguously `null`.

### 1.2 Relationship to SYNX

A SYNXL document is **not** a SYNX document: its top-level structure is a sequence of records, which violates SYNX §8.1 ("the root value is always an object"). SYNXL therefore does **not** extend the SYNX language and does **not** require a new SYNX language version.

SYNXL **embeds** SYNX: the block portion of a record (§9) is defined by delegation to the SYNX 3.7 parser. Implementations MUST reuse their existing SYNX parser for that delegation rather than reimplementing tree construction.

Where SYNXL deliberately departs from SYNX line semantics, §17 lists every difference exhaustively. Those departures are normative and intentional.

### 1.3 Versioning

The SYNXL format version is an integer on its **own axis**, independent of the SYNX language version and of the `.synxb` container version. This mirrors the `.synxb` precedent (SYNX 3.6 §12).

This document defines SYNXL version **1**. Every conforming document declares it in the prologue (§4.1). An implementation encountering a version it does not support MUST reject the document with a hard error (§11.1) and MUST NOT attempt a partial parse.

### 1.4 Conformance

An implementation is **SYNXL 1 conforming** for a given input if and only if:

1. It applies the line and indentation rules of §3.
2. It accepts exactly the documents accepted by §4–§10 and rejects the rest with the error classes of §11.
3. The **canonical JSON projection** (§12) of the parsed document is byte-identical to that of the reference implementation, subject to the same resource limits (§13).
4. It reuses a SYNX 3.7 conforming parser for block delegation (§9.3).

The directory `tests/conformance-synxl/cases/` is the **practical conformance contract** (§18).

---

## 2. Terminology

| Term | Meaning |
|---|---|
| **prologue** | The `!synxl <version>` line (§4.1). |
| **field list** | A `!fields` line declaring the ordered fields of subsequent records (§5). |
| **inline field** | A field whose value appears on the record line, delimited by `;` (§7). |
| **block field** | A field declared `[block]`, whose value appears in the record's indented block (§9). |
| **record line** | A structural line at indent 0 that begins a record (§6). |
| **block** | The maximal run of lines at indent > 0 following a record line (§9.1). |
| **record** | A record line plus its block. |
| **diagnostic** | A non-fatal parse observation attached to the result (§11.2). |

---

## 3. Character encoding, lines, and indentation

1. **SYNXL text MUST be interpreted as Unicode encoded in UTF-8.** A byte order mark (U+FEFF) at the start of input MUST be ignored.
2. **Newlines:** input is a sequence of lines separated by `LF` (U+000A). A `CR` (U+000D) immediately preceding `LF` MUST NOT be part of the line's logical content.
3. **Indentation** is computed exactly as in SYNX §4:
   ```text
   indent = length(raw) - length(ltrim(raw))
   ```
   counting leading whitespace as UTF-8 code units, not visual columns.

   Note the asymmetry with §7.1 step 4, which is deliberate and has caught implementers out: `ltrim` here removes **Unicode** whitespace, so a line beginning with U+00A0 has a non-zero indent and is block content — whereas trimming *within* a record line removes only spaces and horizontal tabs, so U+00A0 inside a field is preserved as data. Structure is decided the SYNX way; field content is not.
4. **Indent 0 is structural.** A line with `indent == 0` is a prologue, a field list, a comment, or a record line — never a continuation. A line with `indent > 0` is always part of the current record's block.

   This is the load-bearing invariant of the format: record boundaries are decidable by looking at a single byte, which is what makes streaming, appending, and parallel parsing possible (§15).

5. A line whose trimmed form is empty MUST be ignored for structural purposes and MUST NOT terminate a block.

---

## 4. Document structure

A SYNXL document is:

```abnf
document   = prologue 1*( field-list *record )
prologue   = "!synxl" 1*WSP version LF
version    = 1*DIGIT
```

Interleaved comments (§4.3) and empty lines are permitted anywhere.

### 4.1 Prologue

The **first** non-empty, non-comment line of the document MUST be the prologue. Its trimmed form MUST match `!synxl` followed by at least one space or tab and a decimal integer version. For this specification the version MUST be `1`.

A document whose first significant line is not a valid prologue MUST be rejected with a hard error (§11.1). Absence of a prologue is not recoverable: without it, a `;`-delimited text file cannot be distinguished from a SYNXL document.

**A repeated prologue** later in the document MUST be accepted and ignored if it declares the same version, and MUST produce a hard error if it declares a different one. Concatenating shards (§15.3) is a natural operation and each shard carries its own prologue; concatenating shards of different format versions is not.

**Any other line at indent 0 whose trimmed form begins with `!`** — anything that is neither a prologue nor a field list, including SYNX directives such as `!active` — MUST produce a hard error.

Silently ignoring it would be more dangerous than rejecting it: a typo in a mid-file field list (`!filds …`) would leave the previous field list in effect, and every subsequent record would be parsed against the wrong schema and mis-populated without any signal. Forward compatibility is served by the version axis (§1.3), not by tolerating unknown directives.

### 4.2 Field list placement

At least one field list (§5) MUST appear after the prologue and before the first record line. A record line encountered while no field list is in effect MUST produce a hard error.

A subsequent field list MAY appear at indent 0 at any later point in the document. It **replaces** the field list in effect for all following records and has no effect on records already parsed. This makes a SYNXL file append-safe across schema evolution: a producer that gains a column writes a new `!fields` line and continues appending, without rewriting history.

### 4.3 Comments

At indent 0:

* A line whose trimmed form begins with `#` is a line comment. The `!synxl`/`!fields` forms are matched before this rule and are not comments.
* A line whose trimmed form begins with `//` is a line comment.
* A line whose trimmed form is exactly `###` toggles block-comment mode; all lines until the next such line are ignored.

At indent > 0, comment handling is delegated to SYNX (§9.3) and therefore follows SYNX §5.

Because `#` and `//` are reserved at the start of a record line, a record whose first inline field begins with either sequence MUST quote that field (§7.4).

---

## 5. Field list

```abnf
field-list  = "!fields" 1*WSP field-decl *( ";" field-decl ) LF
field-decl  = *WSP name [ "(" type ")" ] [ "[" constraints "]" ] *WSP
name        = 1*( %x21-7E / %x80-10FFFF )   ; excluding ; [ ( : and whitespace
```

Example:

```synxl
!fields id[type:int, required] ; score[type:float] ; messages[block]
```

### 5.1 Field names

1. A name MUST be non-empty and MUST NOT contain `;`, `[`, `(`, `:`, or whitespace.
2. Names are compared by exact Unicode scalar sequence (case-sensitive).
3. A name length above `MAX_SYNXL_FIELD_NAME_BYTES` (§13) MUST produce a schema error.
4. **Duplicate names within one field list MUST produce a hard error.** Duplicate keys are the single most common source of silent data corruption in tabular formats; SYNXL rejects them rather than picking a winner.

### 5.2 Types and constraints

The optional `(type)` and `[constraints]` productions use the SYNX key-line surface (SYNX §7, §8.8) and MUST be parsed by the implementation's existing SYNX constraint parser. Recognized constraint parts are those of SYNX §8.8 (`required`, `readonly`, `min:`, `max:`, `type:`, `pattern:`, `enum:`) plus the SYNXL-only flag `block` (§5.3). Unrecognized `key:value` parts MUST be ignored, matching SYNX §8.8; unrecognized bare flags MUST also be ignored. This keeps a version-1 parser tolerant of field lists written by later producers, whereas the hard rejections of §5.2 and §8.3 are reserved for syntax whose meaning would otherwise be guessed.

**Markers MUST NOT appear in a field declaration.** This covers any marker run in the SYNX §7 sense — a chain (`:a:b`) and a single marker (`:custom`) alike. A field list containing one MUST produce a hard error. Markers drive the `!active` engine, which has no meaning in a data record; rejecting them in version 1 keeps the syntax available for a future version.

Declared types affect casting (§8). Declared constraints do **not** trigger validation by default (§8.4).

### 5.3 The `block` flag

A field declared with the `block` flag in its constraints is a **block field**.

1. A block field MUST NOT occupy a position on the record line.
2. `block` MUST NOT be combined with `(type)` or with `type:`; the shape of a block value is determined by the embedded SYNX document (§9.3).
3. The **arity** of a field list is the number of fields **without** the `block` flag. Arity is the expected count of inline fields per record (§7.3).
4. **Arity MUST be at least 1.** A field list in which every field carries `block` MUST produce a hard error.

   A zero-arity field list has no representation: its record line would be empty, and §3.5 requires empty lines to be ignored, so records would have no detectable boundary. A dataset whose payload is entirely blocks (a pure chat corpus, say) MUST therefore declare at least one inline field — in practice an identifier, which such datasets need anyway for diagnostics and deduplication.

Declaring block-ness in the field list rather than inferring it per record is deliberate: it makes the expected inline arity a constant, so a malformed record is detected by counting delimiters instead of by guessing intent.

---

## 6. Record lines

A **record line** is a line at indent 0 that is not a prologue, not a field list, and not a comment.

A record line MUST NOT be filtered by the SYNX §7 key-line rules. Specifically, a record line whose first character is `-`, `[`, `:`, `(`, `/`, or `@` is a valid record line and MUST be parsed as data. SYNX discards such lines because they cannot begin a key; in SYNXL the first token is a value, and `-5`, `[unparsed]`, `/var/log/app`, and `@kaiserberg` are all ordinary field values.

The reserved prefixes at indent 0 are exactly: `!` (prologue and field list), `#`, `//`, and `###`. A record whose first inline field would begin with one of these MUST quote it (§7.4).

Note that `/` alone is not reserved — only the two-character sequence `//` is. A record beginning `/var/log/app` is data; a record beginning `//var` is a comment and MUST be quoted if it was meant as data.

---

## 7. Inline fields

### 7.1 Splitting

The record line is split on the ASCII semicolon `;` (U+003B) into **parts**. A `;` inside a quoted part is not a delimiter, so splitting and quote recognition are one pass, not two. Implementations MUST use exactly this algorithm:

Starting at the beginning of the line, and then after each delimiter:

1. Skip spaces and horizontal tabs.
2. If the next character is `"` or `'`, record it as the **opening quote** and scan forward for the next occurrence of the same character. If one is found, and every character between it and the next `;` (or end of line) is a space or horizontal tab, then the part is **quoted**: its value is the text strictly between the quotes (§7.4), and the delimiter search resumes after the following `;`.
3. Otherwise — no opening quote, no matching close, or trailing garbage after the close — the part is **unquoted**: it extends to the next `;` or to end of line, and any quote characters within it are ordinary content.
4. An unquoted part is trimmed of leading and trailing **spaces and horizontal tabs** — not Unicode whitespace generally, matching step 1. A quoted part is not trimmed inside its quotes.

Leading whitespace before the first part is a special case: a record line's first character is at indent 0 by definition (§3.4), so a record beginning with a null field MUST be written `; a ; b` and never `  ; a ; b` — the latter is block content, not a record. Writers MUST NOT indent a record line.

Because SYNXL has no escape sequences (§7.4), a quoted part cannot contain its own quote character. Step 3's fallback to unquoted is what keeps that case deterministic instead of silently swallowing the rest of the line.

### 7.2 Empty parts

A part that is empty after trimming yields JSON `null`.

An empty **string** is written as `""` (§7.4). This resolves the ambiguity CSV never resolved: in SYNXL, "absent" and "empty text" are distinct on the wire.

**The all-null record.** A record line whose trimmed form is exactly `;` sets **every** inline field to `null`, whatever the arity, and produces no diagnostic. Writers MUST emit this form for an all-null record.

Without this rule an all-null record has no representation at arity 1: every part would be empty, so the line itself would be empty, and §3.5 makes empty lines invisible. Deriving the meaning from the split instead would give a different answer per arity — two null parts at arity 2, and a spurious `MissingFields` at arity 3 — so the form is defined directly rather than left to fall out of §7.1.

### 7.3 Arity

Let `N` be the arity of the field list in effect (§5.3) and `P` the number of parts.

| Condition | Behavior |
|---|---|
| `P == N` | Normal. |
| `P < N` | The missing trailing fields are set to `null`, and a `MissingFields` diagnostic is recorded (§11.2). |
| `P > N` | The first `N` parts are used, the surplus is discarded, and an `ExtraFields` diagnostic is recorded. |

Neither case aborts the parse. Both are reported. SYNX §8.10 drops malformed structure silently; SYNXL MUST NOT, because a dataset that silently loses a column is worse than one that fails loudly.

### 7.4 Quoting

If a part, after trimming, both begins and ends with `"` (U+0022) or both begins and ends with `'` (U+0027), and its length is at least 2, the value is the **literal inner text** with no further interpretation: no escape processing, no casting, no comment stripping.

SYNXL has **no escape sequences**, matching SYNX §8.3. Consequently a value containing both a quote character and a `;` cannot be expressed inline; such a value MUST be carried in a block field (§9), where `;` and `"` have no special meaning at all. Writers MUST apply this rule automatically (§14.3).

### 7.5 Inline comments are not stripped

SYNX §8.3 truncates a value at the first occurrence of ` //` or ` #`. **SYNXL MUST NOT apply that rule to inline field values.**

In a dataset, `#` and `//` are content: hashtags, C-style code, Markdown headings, URLs. Truncating at them would corrupt data silently. Comments in SYNXL exist only as whole lines at indent 0 (§4.3).

---

## 8. Casting

### 8.1 Automatic casting

A part that was not quoted (§7.4) and is not empty (§7.2) is cast as in SYNX §8.3 **automatic casting**: `true` / `false` / `null` literals, then integer, then decimal float, then string — with two modifications:

1. Inline comment stripping is not applied (§7.5).
2. **The quote-stripping step of SYNX §8.3 (its step 1) MUST NOT be applied.** Quoting was already resolved by the splitter (§7.1); a part that reached this point unquoted has quote characters as ordinary content. Applying SYNX's rule here would turn the §7.1 step 3 fallback `"a"b"` into `a"b`, which no writer could reproduce — the format has no escapes — and §14.1 round-trip would become unsatisfiable.

### 8.2 Typed casting

If the field declares `(type)` or `type:<name>`, SYNX §8.3 **typed casting** (`cast_typed`) applies instead: `int`, `float`, `bool`, `string`. If a declaration carries both forms, **`(type)` wins**, matching SYNX key-line precedence.

Typed casting **fails** when: an `int` or `float` part does not parse (a non-finite result counts as a failure), or a `bool` part is not exactly `true` or `false`. The hint `string` and unrecognized hints never fail — they fall back to automatic casting, per SYNX §8.3.

If typed casting fails for a part, the field MUST be set to `null` and a `CastFailed` diagnostic recorded. The record is otherwise preserved. A single unparsable cell MUST NOT discard the row.

### 8.3 Non-deterministic hints are forbidden

The SYNX hints `random`, `random:int`, `random:float`, and `random:bool` MUST NOT appear in a SYNXL field list; they MUST produce a hard error. A dataset format whose parse result varies between reads is not interchangeable.

The same requirement applies inside a record: **a type hint appearing within an inline field value MUST NOT be interpreted.** Casting is driven exclusively by the field list (§8.2); a cell reading `(random)` is the literal eight-character string. Honoring a hint carried by data would make the §12 projection non-reproducible and §1.4 unsatisfiable.

### 8.4 Validation is opt-in

Declared constraints (`required`, `min:`, `max:`, `enum:`, `pattern:`) are recorded as field metadata and are **not** enforced by default. An implementation MUST expose a validating mode that checks them and reports violations as diagnostics (§11.2).

Default-off is a performance decision: `pattern:` implies regex evaluation per cell, which would dominate parse time on datasets whose primary consumers do not need validation.

---

## 9. Block fields

### 9.1 Block extent

The **block** of a record is the maximal run of lines following its record line up to (but excluding) the next line at indent 0 that is not empty, or end of input. Empty lines inside a block do not terminate it (§3.5).

### 9.2 Empty blocks

A record with no block yields `null` for every block field. A block field is never required to be present.

### 9.3 Block semantics by delegation

The block's raw text — the original lines with their original indentation, joined by `LF` — MUST be parsed by a **SYNX 3.7 conforming parser** as an independent SYNX document (SYNX §8), producing an object.

Relative indentation inside the block is handled by SYNX itself: SYNX's stack-repair rule (SYNX §8.6) attaches keys at the block's shallowest indent to the root of that sub-document, so no dedent pre-pass is required. `|` and `|+` multiline blocks (SYNX §8.4, §8.4.1) work unchanged, and `|+` base-indent locking is computed relative to the block's own lines.

The resulting object's top-level keys MUST be visited in **lexicographic order by Unicode scalar value**, not in document or hash order, so that the sequence of block diagnostics is reproducible across implementations. (SYNX's root is an unordered map; without this rule `.expected.diagnostics` files would be unstable.)

Each top-level key is matched by exact name against the field list:

| Case | Behavior |
|---|---|
| Key matches a field declared `[block]` | Its value becomes that field's value. |
| Key matches a field **not** declared `[block]` | The value is discarded and a `BlockFieldNotDeclared` diagnostic is recorded. The inline value for that field is authoritative. |
| Key matches no field | The value is discarded and an `UnknownBlockKey` diagnostic is recorded. |

### 9.4 Directives inside a block MUST be ignored

A line inside a block whose trimmed form begins with `!` MUST NOT be interpreted as a SYNX directive: it MUST NOT set mode flags and MUST NOT be recorded as an include or use directive.

This is a security requirement, not a stylistic one. Records commonly originate from untrusted sources (scraped corpora, user submissions, third-party datasets). Honoring `!include` inside a record would turn any dataset row into a file-read primitive against the consuming process (see SYNX §13.3).

Placement determines what happens to such a line:

* **Outside a multiline block** it MUST be discarded, exactly as a comment line would be.
* **Inside a `|` or `|+` multiline block** (SYNX §8.4, §8.4.1) it is ordinary body content and MUST be preserved verbatim. A dataset row may legitimately contain a line beginning with `!`, and the multiline body is data by definition.

Implementations MUST enforce this by disabling directive handling **inside** the embedded parse, where multiline context is known. Enforcement MUST NOT be implemented by pre-filtering the block's raw lines before delegation — that would corrupt multiline bodies — and MUST NOT depend on the caller passing an option.

### 9.5 Active mode

SYNXL has no `!active` mode. Metadata capture (SYNX §8.7) MUST be disabled for block delegation, and a SYNXL parse result carries no SYNX metadata map. Field-level types and constraints live in the field list (§5.2), which is the only schema surface in this format.

---

## 10. Worked example

```synxl
!synxl 1
!fields id[type:int, required] ; score[type:float] ; messages[block]

1 ; 0.91
  messages
    - role system
      content You are a helpful assistant.
    - role user
      content |+
          def f(x):
              return x + 1

2 ; 0.74
  messages
    - role user
      content Привет

# schema evolution: a new column appears mid-file
!fields id[type:int, required] ; score[type:float] ; lang ; messages[block]

3 ; 0.55 ; ru
  messages
    - role user
      content Как дела?
```

Record 1 projects to:

```json
{"id":1,"messages":[{"content":"You are a helpful assistant.","role":"system"},{"content":"def f(x):\n    return x + 1","role":"user"}],"score":0.91}
```

Record 3 projects with `"lang":"ru"`; records 1 and 2 have no `lang` field at all, because they were parsed under the previous field list.

---

## 11. Error model

### 11.1 Hard errors

The following MUST abort the parse and MUST NOT yield a partial result:

| Condition | Reference |
|---|---|
| Missing or malformed prologue | §4.1 |
| Unsupported format version | §1.3 |
| Repeated prologue declaring a different version | §4.1 |
| Unknown `!` line at indent 0 | §4.1 |
| Record line with no field list in effect | §4.2 |
| Malformed field list (empty, unparsable declaration) | §5 |
| Duplicate field name | §5.1 |
| Marker in a field declaration | §5.2 |
| Non-deterministic type hint | §8.3 |
| `block` combined with a type | §5.3 |
| Field list with arity 0 (every field is `block`) | §5.3 |
| `MAX_SYNXL_FIELDS`, `MAX_SYNXL_FIELD_NAME_BYTES`, `MAX_SYNXL_FIELD_LISTS`, or `MAX_SYNXL_RECORDS` exceeded | §13 |

`MAX_SYNXL_RECORD_BYTES` is **not** in this list: an oversized record is truncated and reported as a `RecordTruncated` diagnostic (§13), because one pathological row must not invalidate a multi-gigabyte dataset.

### 11.2 Diagnostics

Everything else is recoverable and MUST be reported as a diagnostic rather than dropped silently. A diagnostic carries: record index (0-based), source line number (1-based), kind, and a human-readable message. Kinds:

`MissingFields`, `ExtraFields`, `CastFailed`, `UnknownBlockKey`, `BlockFieldNotDeclared`, `OrphanBlockLine`, `ConstraintViolation` (validating mode only, §8.4), `RecordTruncated` (§13).

**Which line a diagnostic reports** is normative, since implementations would otherwise diverge on identical input:

| Kind | Line reported |
|---|---|
| `MissingFields`, `ExtraFields`, `CastFailed`, `RecordTruncated` | The record line (§6). |
| `UnknownBlockKey`, `BlockFieldNotDeclared` | The line inside the block on which the offending top-level key appears. |
| `OrphanBlockLine` | The orphan line itself. |
| `ConstraintViolation` | The record line for an inline field; the offending block line for a block field. |

**Order within a record is normative**, for the same reason as §9.3: `RecordTruncated`, then `CastFailed` (in field order), then `MissingFields` / `ExtraFields`, then block diagnostics (in the sorted key order of §9.3), then `ConstraintViolation` (in field order).

A line at indent > 0 appearing where no record is open — for example immediately after a field list — MUST be discarded with an `OrphanBlockLine` diagnostic attached to the following record index.

Diagnostics MUST be exposed on the parse result and MUST be enumerable by a streaming consumer per record. An implementation MUST NOT write diagnostics to standard output or standard error as its only reporting channel.

---

## 12. Canonical JSON projection

### 12.1 Array projection

The document projects to a JSON **array** of objects, one per record, in document order. Each object maps field names to values, following the canonical rules of SYNX §10: object keys sorted lexicographically by Unicode scalar order, arrays in order, string escaping per RFC 8259 style, no insignificant whitespace.

Fields absent from the field list in effect for a given record MUST NOT appear in that record's object. Fields present but unset are the JSON literal `null`.

### 12.2 NDJSON projection

An implementation MUST also provide an **NDJSON projection**: the canonical JSON object of each record, one per line, `LF`-separated, with no enclosing array.

This makes SYNXL ↔ JSONL conversion lossless and mechanical in both directions, which is the price of admission for a format competing with JSONL: existing pipelines must be one command away.

---

## 13. Resource limits

| Limit | Value | Effect |
|---|---|---|
| `MAX_SYNXL_RECORD_BYTES` | 16 777 216 (16 MiB) | Per **record** (line plus block). A record exceeding it is truncated at a valid UTF-8 boundary and a `RecordTruncated` diagnostic is recorded. |
| `MAX_SYNXL_FIELDS` | 4 096 | Fields per field list. Exceeding it is a hard error. |
| `MAX_SYNXL_FIELD_NAME_BYTES` | 255 | Field-name length. Exceeding it is a hard error. |
| `MAX_SYNXL_FIELD_LISTS` | 65 536 | Field lists per document. Exceeding it is a hard error. |
| `MAX_SYNXL_RECORDS` | 16 777 216 | In-memory (whole-document) parse only. The streaming API (§15.1) has no record-count limit. |
| SYNX limits | SYNX §3 | Apply **within** a block, scoped to that block. |

SYNX caps total input at 16 MiB (SYNX §3). **That cap MUST NOT be applied to a SYNXL document.** Datasets are routinely gigabytes; since records are independent, a whole-file byte cap protects nothing that a per-record cap does not, while making the format useless for its primary purpose.

---

## 14. Canonical serialization (writer)

A conforming writer MUST emit:

1. `!synxl 1` as the first line.
2. A `!fields` line with declarations joined by `; ` (semicolon, space).
3. One record per group, inline parts joined by `; `, in field-list order.

### 14.1 Round-trip requirement

For any document `D` accepted without hard error, `parse(write(parse(D)))` MUST produce a JSON projection byte-identical to `parse(D)`.

**Scope.** The requirement is over documents, not over arbitrary in-memory values. A value constructed programmatically may be inexpressible in SYNX and therefore unwritable — SYNX §8.4.1 does not preserve blank lines inside a multiline body, strips per-line trailing whitespace, and locks the base indent to the first continuation line (so leading whitespace on that line is lost). Such values cannot round-trip in **any** conforming implementation, and this section does not require them to. It does require that every value obtained *by parsing* survives the cycle.

### 14.2 Null and empty

An unset field is emitted as an empty part. An empty string is emitted as `""`.

### 14.3 Automatic quoting and promotion

A writer MUST quote an inline value that, after trimming, would otherwise be misread:

* one containing `;`;
* one with significant leading or trailing whitespace;
* one beginning with `#`, `//`, or `!`;
* one that would cast to a different type than intended (for example the string `42`);
* **one whose first character is `"` or `'`** — otherwise §7.1 step 2 would re-read it as quoted and strip a layer. The value is quoted with the *other* quote character.

A writer MUST promote a value to a block field when it contains an `LF`, or when it needs quoting (per the list above) and contains **both** quote characters — the case where no quoting is available, since §7.4 provides no escapes. Multi-line values MUST be written with `|+`, preserving internal indentation.

**Numbers.** A writer MUST emit floats in a form SYNX can read back: SYNX §8.3 recognizes only `-?digits.digits`, so shortest-form output such as `5` or `1e300` would return as an integer or a string. Exponent forms MUST be expanded to full decimal. A non-finite float has no SYNX representation and MUST be written as an empty part, which reads back as `null` — matching the JSON projection, where it is also `null`.

An integral float SHOULD carry `.0`. This is a SHOULD, not a MUST, because a host language with a single numeric type (JavaScript) cannot distinguish `5.0` from `5` in the first place. The resulting documents differ in bytes between implementations, but their §12 projections are identical, which is what §14.1 requires and what conformance (§1.4) is measured on.

**Unrepresentable values.** A writer MAY reject a programmatically constructed value that has no SYNXL rendering — for instance a single-field group whose value would require promotion, since promoting it would leave arity 0 (§5.3.4). Rejecting loudly is required; emitting a document that reads back differently is not permitted. Per §14.1 this cannot arise for values obtained by parsing.

---

## 15. Streaming, appending, and parallelism

### 15.1 Streaming

Because record boundaries are decidable from a single byte (§3.4), an implementation MUST provide a streaming reader that yields records incrementally without materializing the document, and SHOULD provide it as the idiomatic iterator type of its language.

### 15.2 Appending

A producer MAY append records to an existing document by writing at indent 0. When the field list changes, the producer MUST write a new `!fields` line before the first record that uses it (§4.2). No rewrite of existing records is required.

A document truncated mid-record (for example by a crash) is recoverable: a consumer MUST parse all complete records and MUST NOT fail the document.

A truncated trailing record MUST NOT be reported by default. Truncation is not mechanically decidable — a document cut mid-record is byte-indistinguishable from a complete one, and the only available signal ("input does not end with `LF`") fires on ordinary hand-written files. An implementation MAY expose such a warning behind an explicitly opt-in setting.

### 15.3 Shardability

A shard of a SYNXL document is itself a valid SYNXL document only if it begins with a prologue and the field list in effect at the split point. A splitting tool MUST emit both into every shard.

### 15.4 Parallel parsing (informative)

Records share no state. An implementation MAY split input on indent-0 boundaries and parse records in parallel, provided diagnostics and record order are reassembled in document order. Delimiter scanning is a byte search over `;` and `LF` and SHOULD use a vectorized search where available.

---

## 16. Security considerations

1. **Directive injection.** §9.4 is mandatory. A dataset row must never become a file-read or mode-change primitive.
2. **Reserved-prefix confusion.** A producer that fails to quote a leading `#`, `//`, or `!` silently converts a record into a comment or a hard error. Writers MUST apply §14.3.
3. **Resource exhaustion.** §13 limits are mandatory. Streaming consumers MUST bound per-record memory even when total input is unbounded.
4. **Diagnostics are security-relevant.** Per SYNX §13.2, an accepted document does not mean every line became data. A consumer that ignores diagnostics can silently ingest a truncated dataset.
5. **Quoting has no escapes.** §7.4 means a hostile value cannot break out of a quoted part by escaping, but also that quote characters cannot be represented inline; implementations MUST NOT invent an escape mechanism, since divergent escaping across implementations is itself a correctness hazard.

---

## 17. Differences from SYNX, CSV, and JSONL

### 17.1 Departures from SYNX (normative)

| Aspect | SYNX 3.7 | SYNXL 1 |
|---|---|---|
| Root value | Object (§8.1) | Sequence of records (§4) |
| Indent 0 | No special meaning | Structural: record or directive boundary (§3.4) |
| Key-line first-character filter | Lines starting `-` `[` `:` `(` `/` are discarded (§7) | Not applied to record lines (§6) |
| Inline comment stripping | Value truncated at ` //` or ` #` (§8.3) | Not applied to inline values (§7.5) |
| Directives | Recognized anywhere at any indent (§6) | Only `!synxl` / `!fields` at indent 0; ignored inside blocks (§9.4) |
| `!active` metadata | Available (§8.7) | Disabled; schema lives in the field list (§9.5) |
| Input size cap | 16 MiB total (§3) | Per record, not per document (§13) |
| Malformed structure | Silently skipped (§8.10) | Reported as a diagnostic (§11.2) |
| `random` hints | Permitted (§8.3) | Forbidden (§8.3) |

### 17.2 Against CSV

Typed and constrained fields declared once; unambiguous `null` versus empty string; native nesting and multi-line text without escaping; comments; schema evolution mid-file. No quote-doubling rules, and the delimiter is fixed rather than dialect-dependent.

### 17.3 Against JSONL

Field names and structural punctuation are not repeated per record, which is the dominant token cost of JSONL for LLM consumption; multi-line text stays readable rather than `\n`-escaped; types are declared rather than inferred per record. In exchange, SYNXL records are homogeneous by construction: heterogeneous record shapes require separate field lists (§4.2) or separate documents.

---

## 18. Conformance suite

The directory `tests/conformance-synxl/cases/` contains paired files `*.synxl` and `*.expected.json`, where the expectation is the array projection of §12.1. Cases expected to fail carry `*.expected.error` naming the hard-error condition of §11.1. Cases producing diagnostics carry `*.expected.diagnostics` listing kind, record index, and line, in order.

Adding cases is backward compatible for clients. Changing an existing expectation for an unchanged input is a **breaking change** to the format definition and requires a new SYNXL format version.

---

## 19. Document status

**Normative:** Sections 1–16 for SYNXL format version **1**.
**Editor:** Maintainers of `synx-format`; errata against `synx-core` 3.7.x and `tests/conformance-synxl/`.

---

*End of normative specification.*
