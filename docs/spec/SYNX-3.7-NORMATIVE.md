# SYNX Language Specification — Normative Document

**Protocol / language version:** SYNX **3.7**
**Canonical reference implementation:** Rust crate `synx-core` version 3.7.0 (this repository)
**Media type / file extension (text):** `.synx` (informative; not registered with IANA)
**Related binary container:** `.synxb` format version **1** (orthogonal versioning; see SYNX-3.6 §12)

This document uses [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119) keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** where capitalized.

> **Status.** SYNX 3.7 is an additive revision of SYNX 3.6 ([`SYNX-3.6-NORMATIVE.md`](SYNX-3.6-NORMATIVE.md)). The 3.6 reference remains **frozen** as the canonical interoperability baseline. Every section of 3.6 carries forward unchanged unless explicitly amended below. A 3.6 parser MUST continue to be conformant for any document that does not use a 3.7-only construct.
>
> The single normative change in 3.7 is the addition of §8.4.1 (indent-preserving multiline blocks via the new `|+` opener). Existing 3.6 documents parse identically. The only externally visible behaviour change is that the literal value `|+` — which under 3.6 would have produced a string `"|+"` — now opens a multiline block under 3.7. Documents that intentionally relied on the string form `"|+"` MUST quote it (`key "|+"`) or migrate.

---

## 1. Inheritance from SYNX 3.6

The full text of [`SYNX-3.6-NORMATIVE.md`](SYNX-3.6-NORMATIVE.md) is incorporated by reference. Sections 1 through 13 of 3.6 apply to 3.7 verbatim, with the single amendment in §8.4.1 below.

Implementations claiming SYNX 3.7 conformance MUST:

1. Pass all 3.6 conformance tests, AND
2. Implement §8.4.1 below as specified.

A document is **3.6-portable** if and only if it does not use any 3.7-only construct. Authors who care about cross-version portability should restrict themselves to 3.6 features.

---

## 8.4.1 Indent-preserving multiline string (`|+`)

This section is **new in 3.7**. It augments — but does not replace — §8.4 of 3.6 (`|` multiline blocks).

If the parsed `value` is exactly **`|+`**, an **indent-preserving multiline block** opens for that key.

### Continuation rules

A continuation line is any line whose `indent` is **strictly greater** than the opening line's `indent` (same threshold as 3.6 §8.4). The block ends at the first subsequent line whose `indent` is `≤` the opener's `indent`, OR at end-of-input.

### Base indent

The **base indent** of a `|+` block is the `indent` of its first non-empty continuation line. The base indent MUST be locked at first determination; subsequent continuation lines MUST NOT change it.

If a `|+` block has zero non-empty continuation lines, its base indent is unspecified and the resulting string is empty.

### Line accumulation

For each continuation line:

1. Compute the line's `indent` (§4.2).
2. Let `strip = min(indent, base_indent)`.
3. Slice the raw line bytes from offset `strip` up to (but not including) trailing horizontal whitespace (`U+0020`, `U+0009`) and ASCII line-terminators (`CR`, `LF`).
4. Append the resulting slice to the block's body.
5. Lines after the first are preceded by a single `LF` separator.

The body size limit `MAX_MULTILINE_BLOCK_BYTES` from §3 applies identically to `|+`.

### Properties

The §8.4.1 algorithm preserves the indentation of each continuation line **relative to the base indent**, while stripping the common leading whitespace that exists purely for visual nesting under the parent key. In practice:

* A line indented exactly at the base level appears in the body with zero leading whitespace.
* A line indented `n` characters beyond the base appears in the body with `n` leading whitespace characters.
* A line indented *less* than the base (but still strictly greater than the opener — only possible when the base was locked deeper than necessary) MUST have all `indent` bytes stripped; no padding is added.

### Worked example

Source:

```synx
prompt |+
  Outline:
    - step one
    - step two
      sub-step
  End.
```

Parses to `prompt` =

```
Outline:
  - step one
  - step two
    sub-step
End.
```

### Empty lines inside a `|+` block

Empty (whitespace-only) lines inside a `|+` block follow §4 — they are excluded from `indent`-based block-termination analysis (they cannot end the block) and are joined as zero-width separators. Implementations MAY choose to preserve internal blank lines as empty body lines for `|+`; the reference implementation in this version **does not** preserve them (to keep parity with §8.4 of 3.6). Authors who require blank-line preservation should provide the content via an external source rather than relying on the multiline literal.

### Interaction with markers, type hints, and constraints

The `|+` opener is mutually exclusive with the `|` opener at the lexical level. All other key-line surface — type hints (§7), marker chains (§7), constraints (§7) — applies identically to `|+` as to `|`. The §8.7 `!active` metadata capture treats a `|+` block as a string value of automatic cast (no type hint applied to the body unless one is explicitly given on the key line).

### Conformance

A SYNX 3.7 implementation MUST produce the same body for a `|+` block on every byte-identical input. A SYNX 3.6 implementation, when given a `|+` opener, MUST produce a string value `"|+"` for that key (per §8.3, automatic cast of a literal two-character value). 3.6 and 3.7 parsers therefore diverge on `|+` and on no other input.
