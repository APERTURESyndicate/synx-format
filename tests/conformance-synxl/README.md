# SYNXL conformance test suite

Practical conformance contract for **SYNXL format version 1**, as required by
[`docs/spec/SYNXL-1-NORMATIVE.md`](../../docs/spec/SYNXL-1-NORMATIVE.md) §18. Block fields (§9) delegate to an
embedded **SYNX 3.7** parser, so expected values for block bodies are derived from
[`docs/spec/SYNX-3.7-NORMATIVE.md`](../../docs/spec/SYNX-3.7-NORMATIVE.md) and (by inheritance) [`docs/spec/SYNX-3.6-NORMATIVE.md`](../../docs/spec/SYNX-3.6-NORMATIVE.md).

Every case in this suite is derived directly from the normative text of those three documents — not from any
Rust or JS implementation. Two parallel implementations (Rust `synx-core` and a JS port) are expected to match
this suite bit-for-bit; if either one disagrees with a case here, the disagreement is a bug in that
implementation or a genuine spec ambiguity to raise against the normative document, not a reason to change the
fixture without discussion.

> This suite has been revised twice already. Round 1: two questions raised in an earlier "Open questions"
> section were answered by tightening the normative spec itself (§11.2's line-attribution table, §5.2's marker
> rule), and a round of cross-checking between the two implementations surfaced roughly a dozen further
> clarifications (repeated prologue, diagnostic ordering, quote-stripping on unquoted fallback, arity-0,
> `OrphanBlockLine`, etc.). Round 2: running both implementations against the suite found a real bug (JS's
> `String.trim()` was eating the NBSP in `118`) and one genuinely incompatible reading of quoting between the
> two implementations; two fixtures (`080`, `118`) that themselves violated §7.1/§3.3 were corrected, `§7.2`
> gained the "all-null record" rule, and `§3.3` gained a normative note on the indent/trim asymmetry. All of
> that is folded into the spec and into the cases below.

## Directory layout

```
tests/conformance-synxl/
├── README.md
└── cases/
    ├── 001-minimal-document.synxl
    ├── 001-minimal-document.expected.json
    ├── 010-prologue-missing.synxl
    ├── 010-prologue-missing.expected.error
    ├── 090-missing-fields-diagnostic.synxl
    ├── 090-missing-fields-diagnostic.expected.json
    ├── 090-missing-fields-diagnostic.expected.diagnostics
    └── ...
```

Every `<name>.synxl` file has **exactly one** of `<name>.expected.json` (accepted) or `<name>.expected.error`
(hard error, §11.1). A case MAY additionally carry `<name>.expected.diagnostics` when it exercises the
recoverable diagnostics of §11.2 — diagnostics never replace the `.expected.json`, since a diagnostic is by
definition non-fatal and a result is still produced.

All fixture files are UTF-8, no BOM, `LF` line endings only. Two cases (`118-non-breaking-space-not-trimmed`,
`162-nbsp-leading-line-is-block-content-not-record`) deliberately embed a literal U+00A0 (NO-BREAK SPACE) byte
sequence (`C2 A0`) — that is not a mistake, do not "fix" it to an ASCII space.

## File formats

### `*.synxl`

The input document, byte for byte.

### `*.expected.json`

The canonical **array projection** (§12.1): a single line, one JSON array, keys of every object sorted by
Unicode scalar order at every nesting level, no insignificant whitespace, terminated by a single trailing `\n`.
This mirrors the comparison convention of `tests/conformance/` for the base SYNX language.

### `*.expected.error`

Only present for documents that MUST be rejected per §11.1 (hard error, no partial result). Two lines:

1. A condition token, one of:

   | Token | §11.1 condition |
   |---|---|
   | `MissingOrMalformedPrologue` | Missing or malformed prologue (§4.1) — including a valid `!synxl` line that is present but is not the document's first significant line |
   | `UnsupportedFormatVersion` | Unsupported format version (§1.3) |
   | `RepeatedPrologueDifferentVersion` | A later `!synxl` line declares a version different from the first one (§4.1) |
   | `UnknownDirectiveLine` | A line at indent 0 starts with `!` but is neither `!synxl` nor `!fields` (§4.1) |
   | `RecordWithoutFieldList` | Record line with no field list in effect (§4.2) |
   | `MalformedFieldList` | Malformed field list — empty or unparsable declaration (§5) |
   | `DuplicateFieldName` | Duplicate field name (§5.1) |
   | `MarkerInFieldDecl` | A marker run — chain or single — in a field declaration (§5.2) |
   | `NonDeterministicTypeHint` | Non-deterministic type hint (§8.3) |
   | `BlockCombinedWithType` | `block` combined with a type (§5.3) |
   | `FieldListArityZero` | Every field in the list is `[block]`, leaving inline arity 0 (§5.3) |

2. A one-sentence human-readable explanation (informative, not part of the contract — only the token and the
   fact that parsing MUST abort with no partial result are normative).

`MAX_SYNXL_*` limit violations (§13) are in the §11.1 hard-error table but are not exercised here: constructing
a fixture that legitimately crosses `MAX_SYNXL_FIELDS` (4096) or similar is impractical as a checked-in text
fixture and is left to each implementation's own unit tests.

**Note on `!synxl 1` appearing twice with the same version:** this is not a hard error. §4.1 requires a
*repeated* prologue to be accepted and ignored when its version matches the first one (this is what makes
`cat shard1.synxl shard2.synxl` — each shard carrying its own prologue per §15.3 — a valid document). Only a
repeated prologue with a *different* version is a hard error (`RepeatedPrologueDifferentVersion`). See
`016-repeated-prologue-same-version-accepted` (accepted) vs. `015-repeated-prologue-different-version` (hard
error).

### `*.expected.diagnostics`

One line per diagnostic, **in the order** the reference algorithm would record them, of the form:

```
<Kind> <record_index> <line>
```

- `Kind` is one of the §11.2 kinds exercised here: `MissingFields`, `ExtraFields`, `CastFailed`,
  `UnknownBlockKey`, `BlockFieldNotDeclared`, `OrphanBlockLine`. (`ConstraintViolation` requires opt-in
  validating mode and `RecordTruncated` requires an oversized record; neither is exercised by this suite — see
  **Open questions**.)
- `record_index` is 0-based, per §11.2. For `OrphanBlockLine` this is the index of the **following** record
  (§11.2), which may not exist yet at the point the orphan line is encountered.
- `line` is the 1-based source line number, per the normative §11.2 table:
  - `MissingFields`, `ExtraFields`, `CastFailed`, `RecordTruncated` → the record line.
  - `UnknownBlockKey`, `BlockFieldNotDeclared` → the line inside the block where the offending top-level key
    appears (not the record line).
  - `OrphanBlockLine` → the orphan line itself.
  - `ConstraintViolation` → the record line for an inline field, the offending block line for a block field.
- **Order is normative within a record** (§11.2): `RecordTruncated`, then `CastFailed` (in field-list order),
  then `MissingFields`/`ExtraFields`, then block diagnostics (`UnknownBlockKey`/`BlockFieldNotDeclared`, in the
  block's own key-sorted order per §9.3 — *not* document order, and *not* grouped by kind), then
  `ConstraintViolation`. `180-diagnostic-order-across-kinds` and `181-block-diagnostic-order-by-sorted-key`
  exist specifically to pin this down.
- **The all-null record is diagnostic-free by definition** (§7.2): a record line whose trimmed form is exactly
  `;` sets every inline field to `null` regardless of arity and MUST NOT produce `MissingFields` (or any other
  diagnostic) even though a naive split-and-count would see only one empty part. `081`–`083` assert this at
  arities 1–3 by the *absence* of an `.expected.diagnostics` file.

## Comparison rules

1. Parse the `.synxl` file per §4–§10.
2. If the parse MUST abort with a hard error (§11.1), compare the resulting error condition against the token
   in `.expected.error`; nothing else about that case is checked (no partial JSON is produced or compared).
3. Otherwise, serialize the parsed document as the array projection (§12.1) and compare byte-for-byte against
   `.expected.json` (trailing newline stripped).
4. If `.expected.diagnostics` exists, collect the diagnostics recorded during the parse, in order, and compare
   `kind record_index line` per line against it.

## Case index

| File | Spec section(s) | What it tests |
|---|---|---|
| `001-minimal-document` | §4, §7.2, §8.1 | Smallest complete valid document: prologue, one-field list, one record. |
| `002-no-records` | §4, §12.1 | Valid document with a field list but zero records → empty JSON array. |
| `010-prologue-missing` | §4.1 | First significant line is not a `!synxl` prologue → hard error. |
| `011-prologue-wrong-version` | §1.3, §4.1 | `!synxl 2` — unsupported version → hard error. |
| `012-prologue-not-first-line` | §4.1 | A `!fields` line precedes the (otherwise valid) `!synxl 1` line → hard error. |
| `013-prologue-preceded-by-comments` | §4.1, §4.3 | Comments before the prologue are explicitly allowed — positive control. |
| `014-unknown-directive-line-at-indent0` | §4.1 | `!active` at indent 0 (neither prologue nor field list) → hard error. |
| `015-repeated-prologue-different-version` | §4.1 | A second `!synxl 2` later in the document, after `!synxl 1` → hard error. |
| `016-repeated-prologue-same-version-accepted` | §4.1, §15.3 | A second `!synxl 1` (same version) later in the document is accepted and ignored — the shard-concatenation case. |
| `020-record-without-field-list` | §4.2 | Record line with no `!fields` in effect → hard error. |
| `021-field-list-redeclared-mid-file` | §4.2 | A second `!fields` line, with a **different** field set, replaces the first; earlier/later records keep their own schema. |
| `022-field-list-malformed-empty` | §5 | `!fields` with zero declarations → hard error. |
| `030-comments-hash-and-slash` | §4.3 | `#` and `//` line comments at indent 0. |
| `031-comments-block-toggle` | §4.3 | `###` toggles block-comment mode, hiding a record between two `###` lines. |
| `040-duplicate-field-name` | §5.1 | Duplicate field name in one field list → hard error. |
| `041-marker-chain-in-field-decl` | §5.2 | A 2-segment marker chain (`:custom:tag`) on a field declaration → hard error. |
| `042-unknown-constraint-and-flag-ignored` | §5.2 | Unrecognized bare flag and unrecognized `key:value` constraint part are silently ignored; recognized parts still apply. |
| `043-single-marker-in-field-decl` | §5.2 | A lone single marker (`:custom`, no second segment) → hard error, same as a chain. |
| `050-block-combined-with-type-paren` | §5.3 | `(type)` combined with `[block]` → hard error. |
| `051-block-combined-with-type-constraint` | §5.3 | `type:` inside `[...]` combined with `block` → hard error. |
| `052-arity-excludes-block-field` | §5.3, §7.3 | A 3-field list with one `[block]` field has inline arity 2; the block field never occupies a record-line position. |
| `053-field-list-arity-zero` | §5.3 | Every field is `[block]` → inline arity 0 → hard error. |
| `060-record-reserved-prefixes-are-data` | §6 | Record lines starting with `-`, `[`, `:`, `(`, `/`, `@` are ordinary data, not filtered like SYNX key lines. |
| `061-slash-vs-slashslash` | §4.3, §6 | `/var/log/app` is data; `//var` is a comment and produces no record. |
| `070-quoted-semicolon` | §7.1 | A quoted part containing `;` is not split; comparison unquoted part alongside it. |
| `071-quote-unterminated-fallback` | §7.1, §8.1 | An opening quote with no matching close falls back to unquoted (quote char kept as literal content). |
| `072-quote-trailing-garbage-fallback` | §7.1, §8.1 | Non-whitespace after a closing quote (before the next `;`) falls back to unquoted. |
| `073-single-quotes` | §7.1, §7.4 | `'...'` quoting works the same as `"..."`, including an embedded `;`. |
| `074-quote-fallback-symmetric-not-restripped` | §7.1, §8.1 | `'it's'` fails quoting (garbage after the first close), falls back to unquoted — and because it's unquoted, the now-symmetric outer `'...'` is **not** re-stripped by casting; the value is the full 6-character literal `'it's'`. |
| `080-empty-vs-empty-string` | §7.2, §7.1 | Empty part → `null`; `""` → empty string, side by side. Record line is `; ""` at indent 0 with no leading space (a leading space would make it block content, per §7.1 and case `182`). |
| `081-all-null-record-arity-1` | §7.2 | A record line whose trimmed form is exactly `;`, at arity 1, sets the sole field to `null` — the case §7.2 says has no other representation. No `.expected.diagnostics` file. |
| `082-all-null-record-arity-2` | §7.2 | Same all-null rule at arity 2 — both fields `null`. No `.expected.diagnostics` file. |
| `083-all-null-record-arity-3` | §7.2, §7.3 | Same all-null rule at arity 3 — all three fields `null`, and critically **no** `MissingFields` diagnostic, even though a naive split (one empty part vs. arity 3) would produce one. No `.expected.diagnostics` file. |
| `084-all-null-record-with-block-field` | §7.2, §9 | All-null rule at arity 2 (with a third, block-declared field) — the inline fields go `null` while a non-empty block still populates the block field normally. No `.expected.diagnostics` file. |
| `090-missing-fields-diagnostic` | §7.3, §11.2 | Fewer parts than field-list arity → trailing `null`s + `MissingFields` diagnostic on the record line. |
| `091-extra-fields-diagnostic` | §7.3, §11.2 | More parts than arity → surplus dropped + `ExtraFields` diagnostic on the record line. |
| `100-inline-values-not-truncated` | §7.5 | Values containing ` //`, a leading `#`, and a URL fragment `#` are kept whole — SYNX's inline comment stripping does NOT apply to SYNXL inline values. |
| `110-autocast-scalars` | §8.1 | Automatic casting of `true`/`false`/`null`/int/float/string in one record. |
| `111-typed-cast` | §8.2 | Declared `(int)`/`(float)`/`(bool)`/`(string)` typed casting; `(string)` on `"007"` demonstrates it is NOT reinterpreted as an integer the way autocast would. |
| `112-typed-cast-failure` | §8.2, §11.2 | `(int)` on a non-numeric token → field becomes `null` + `CastFailed` diagnostic; record is preserved. |
| `113-type-paren-wins-over-type-constraint` | §8.2 | `n(int)[type:string]` on `42` → `(type)` wins, result is the integer `42`, not the string `"42"`. |
| `114-typed-float-nonfinite-fails` | §8.2, §11.2 | `(float)` on `nan` parses to a non-finite value → counts as a cast failure → `null` + `CastFailed`. |
| `115-typed-bool-cast-fail` | §8.2, §11.2 | `(bool)` on `yes` (not exactly `true`/`false`) → `null` + `CastFailed`. |
| `116-unknown-type-hint-falls-back-to-autocast` | §8.2 | `(weird)`, an unrecognized hint, never fails — falls back to automatic casting (`true` → boolean `true`). |
| `117-inline-value-type-hint-not-interpreted` | §8.3 | A cell literally containing `(random)` is NOT treated as a hint; it is plain data, autocast to the string `"(random)"`. |
| `118-non-breaking-space-not-trimmed` | §7.1 step 4, §3.3 | Field `a` (the *second* inline field, `id(int) ; a`) is `1 ;  hello ` with U+00A0 immediately around `hello` — the NBSP survives because within-field trimming is ASCII space/tab only. U+00A0 cannot lead the record line itself (that would make it structurally indented, §3.3) — see `162` for that side. |
| `119-leading-null-field-no-indent` | §7.1 | `; a ; b` at indent 0 (no leading space) is a valid record line whose first field is `null`. |
| `120-random-hint-forbidden` | §8.3 | `(random)` type hint in a field list → hard error. |
| `121-random-hint-forbidden-variant` | §8.3 | `(random:int)` → hard error (same family as 120). |
| `130-block-list-of-objects` | §9.3, SYNX §8.5 | Chat-style `messages[block]`: a list of `{role, content}` objects, mirroring the §10 worked example's dash-plus-nested-key pattern. |
| `131-block-nested-object` | §9.3, SYNX §8.6 | A block field resolving to a plain nested SYNX object. |
| `132-block-multiline-pipe` | §9.3, SYNX §8.4 | `\|` multiline block inside a record block (trimmed-line-per-line semantics, unchanged from SYNX 3.6). |
| `133-block-multiline-plus` | §9.3, SYNX §8.4.1 | `\|+` indent-preserving multiline block inside a record block, replicating the §10 worked example's base-indent-lock behavior. |
| `134-block-empty-yields-null` | §9.2 | A record with no block at all → every declared block field is `null`. |
| `135-block-unknown-key-diagnostic` | §9.3, §11.2 | A block top-level key matching no declared field → discarded + `UnknownBlockKey`, reported on the offending block line. |
| `136-block-field-not-declared-diagnostic` | §9.3, §11.2 | A block top-level key matching a field that is declared but NOT `[block]` → discarded + `BlockFieldNotDeclared`; the inline value stays authoritative. |
| `140-block-directive-outside-multiline-discarded` | §9.4 | A `!`-prefixed line inside a block, outside any multiline body, is discarded like a comment (not a directive, not data). |
| `141-block-directive-inside-multiline-preserved` | §9.4 | The same kind of `!`-prefixed line, when it is itself a continuation line of a `\|+` body, is preserved verbatim as data. Contrast with 140 — this pair is the critical §9.4 test. |
| `150-blank-lines-inside-block` | §3.5 | A blank line inside a block does not terminate it; both sides of the blank line join the same sub-document. |
| `160-unicode-field-name-and-value` | §5.1, §7, Unicode | Non-ASCII field name and non-ASCII/emoji inline value; verifies Unicode-scalar key sort order (`emoji` < `имя`). |
| `161-unicode-in-multiline-block` | §9.3, SYNX §8.4.1, Unicode | Non-ASCII text inside a `\|+` block body. |
| `162-nbsp-leading-line-is-block-content-not-record` | §3.3 | A line beginning with U+00A0 has non-zero indent (§3.3's `ltrim` uses Unicode whitespace, unlike §7.1's field-trim) — it is NOT a second record but block content of the previous one; the document still has one record, and the block's single delegated key doesn't match the (block-less) field list, producing `UnknownBlockKey`. |
| `170-orphan-block-line-diagnostic` | §11.2 | An indented line right after `!fields`, before any record is open, is discarded with `OrphanBlockLine` attached to the *following* record's index. |
| `180-diagnostic-order-across-kinds` | §11.2 | One record with `CastFailed`, `MissingFields`, and `UnknownBlockKey` all at once — verifies they appear in the normative order, not encounter order. |
| `181-block-diagnostic-order-by-sorted-key` | §9.3, §11.2 | A block with three problematic keys written `zzz`, `label`, `aaa` (deliberately unsorted) — diagnostics must appear in `aaa`, `label`, `zzz` order (lexicographic), not document order. |
| `182-leading-space-before-semicolon-is-block-content` | §7.1, §9.1 | `  ; a ; b` (indented) after a record is block content, not a second record — it delegates to SYNX, produces a key `";"`, which is unknown → `UnknownBlockKey`, and the document still has only one record. |
| `900-spec-worked-example` | §10 (verbatim) | Direct transcription of the normative worked example, including schema evolution — anchor case cross-checked against the JSON the spec itself gives for record 1. |

**Total: 67 cases** (15 hard-error, 12 with diagnostics, 40 plain accept).

## Open questions

Raised here per the task instructions, rather than guessed into the fixtures.

1. **`MAX_SYNXL_*` hard-error limits are not covered.** §13's four limits (`MAX_SYNXL_FIELDS`,
   `MAX_SYNXL_FIELD_NAME_BYTES`, `MAX_SYNXL_FIELD_LISTS`, `MAX_SYNXL_RECORDS`) are listed as hard errors in
   §11.1, but constructing a checked-in text fixture that legitimately crosses any of them (e.g. 4096 fields,
   or a 255-byte-plus field name) is impractical as a small, readable case and was left out of this suite by
   design, not by oversight.

2. **`ConstraintViolation` and `RecordTruncated` diagnostics are not covered.** `ConstraintViolation` only
   fires in the opt-in validating mode (§8.4), which is a mode selection outside the scope of a plain
   parse/compare fixture. `RecordTruncated` requires a record exceeding `MAX_SYNXL_RECORD_BYTES` (16 MiB),
   which is impractical as a checked-in text fixture for the same reason as item 1.

Nothing new and unresolved was found in this revision: the two behaviors called out for review (§7.2's
all-null record, §3.3's indent/trim asymmetry) both turned out to be fully specified by the text — see
`081`–`084` and `118`/`162` respectively — and no fixture in this pass required a documented assumption the way
earlier ones did.

### Closed (resolved by spec revision)

- ~~`OrphanBlockLine`'s reported line was not in the §11.2 "which line" table~~ — closed. §11.2's table now has
  an explicit row: `OrphanBlockLine` → "The orphan line itself," confirming this suite's original inference in
  `170-orphan-block-line-diagnostic` without any change to the fixture.
- ~~§8.3's "literal five-character string" example didn't match its own 8-character `(random)` example~~ —
  closed. §8.3 now reads "the literal eight-character string," matching `117-inline-value-type-hint-not-interpreted`
  without any change to the fixture.
- ~~Line number attribution for block-structural diagnostics~~ — closed. §11.2 has a normative table:
  `UnknownBlockKey`/`BlockFieldNotDeclared` report the offending line inside the block;
  `MissingFields`/`ExtraFields`/`CastFailed`/`RecordTruncated` report the record line. This suite's existing
  fixtures already matched this reading and needed no changes; see `135-block-unknown-key-diagnostic` and
  `136-block-field-not-declared-diagnostic`.
- ~~Whether a single (non-chain) marker on a field declaration is also forbidden~~ — closed, forbidden. §5.2
  explicitly says the rule "covers any marker run in the SYNX §7 sense — a chain (`:a:b`) and a single
  marker (`:custom`) alike." `043-single-marker-in-field-decl` sits alongside the existing
  `041-marker-chain-in-field-decl`; both share the error token `MarkerInFieldDecl` (renamed from
  `MarkerChainInFieldDecl`, which implied only the multi-segment case was covered).

### Fixtures corrected by the coordinator (not authored by this suite's original pass)

Two cases from the previous revision were themselves invalid under the (now-clarified) spec and were fixed
directly rather than left for this pass to catch:

- `080-empty-vs-empty-string.synxl` originally had a leading space before `;`, which — per §7.1's own
  leading-null-field note — makes indent 1 and turns the line into block content, not a record. The leading
  space was removed; the case's intent (`null` vs `""`) is unchanged, and `182` now covers the leading-space
  case explicitly and intentionally.
- `118-non-breaking-space-not-trimmed.synxl` originally led the record line with U+00A0. Per §3.3, `indent`
  uses SYNX's own Unicode-whitespace `ltrim`, which strips U+00A0 — so a leading NBSP does not, in fact, mark
  a value boundary the way an ASCII space does at that position; worse, it also changes the line's indent.
  The field list changed to `id(int) ; a` and the NBSP moved to surround `hello` inside the *second* field
  (`1 ;  hello `), which still exercises the intended rule (§7.1 step 4's ASCII-only trim) without relying on
  position-zero behavior. `162` was added to separately cover the position-zero (structural) side of the same
  asymmetry.
