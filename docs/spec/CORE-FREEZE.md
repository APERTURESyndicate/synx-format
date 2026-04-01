# SYNX Core Freeze — Policy and Implementation

This document defines how the project enforces **«ядро нельзя ломать»**: after a declared freeze, **published SYNX 3.6 behavior stays stable**, and evolution is **additive** only (new syntax, new optional markers, new tooling) unless the team ships a **new language version** (e.g. SYNX 3.7 or 4.0) with an explicit migration path.

**Freeze in effect:** **2026-04-01** — SYNX language **3.6.0** surface (normative doc + conformance suite + `synx-core` parse → canonical JSON projection) is **frozen**. See root `README.md` and `CHANGELOG.md`.

---

## 1. What counts as «ядро» (the frozen core)

The **core** is the union of:

1. **Normative specification** — `docs/spec/SYNX-3.6-NORMATIVE.md` (and its numbered errata).
2. **Conformance contract** — every pair in `tests/conformance/cases/*.synx` + `*.expected.json`.
3. **Public parsing API behavior** of `synx-core` 3.6.x for:
   - `parse`, `ParseResult` shape used by integrations (including `llm`, `tool`, `schema` flags),
   - `to_json` canonical output rules (sorted keys, escaping),
   - `Synx::parse_tool` / `reshape_tool_output` for `!tool` documents,
   - documented resource limits in §3 of the normative spec.
4. **Binary `.synxb` format version 1** on-disk layout as consumed by `synx_core::binary` (optional: treat as separate freeze tag `SYNXB-v1`).

Anything not specified there (editor themes, CLI UX, non-exported helpers) is **outside** the freeze unless explicitly added to the contract.

### Freeze is not a separate «plugin layer»

**Freeze does not mean** that `synx-core` is never touched again, or that new markers live in a second engine you can unplug. In practice, **parsing, AST, and canonical JSON** are implemented **in `synx-core`** (and related crates). The freeze is a **contract**: for SYNX **3.6**, conforming inputs must keep producing the same canonical output and behavior described in the normative doc and `tests/conformance/`.

**New syntax and markers** are still developed **inside** the same codebase, but they must be **additive** for existing 3.6 documents (see §2 and §5). «Turning off» a feature is usually: **do not use that syntax**, pin an older **crate version**, or introduce explicit **Cargo features** if the project chooses to wire them that way — it is not assumed that the core ships as a stack of unrelated optional layers.

---

## 2. Versioning rules (semver + language version)

| Crate / tag | Rule |
|-------------|------|
| **`synx-core` PATCH** (3.6.z) | Bug fixes that **restore** the normative spec or conformance; performance; no intentional output change for conforming inputs. |
| **`synx-core` MINOR** (3.x) | Prefer **only additive**: new optional syntax that **does not change** parsing of existing valid 3.6 documents. If a change could alter JSON output for an existing `.synx`, require a **new language document** (e.g. `SYNX-3.7-NORMATIVE.md`) and **new major or explicit compatibility policy**. |
| **`synx-core` MAJOR** | Reserved for breaking API **or** breaking language (new normative major). |

**Rule of thumb:** If `tests/conformance/` would need an updated `.expected.json` for an **unchanged** `.synx`, that is a **language breaking** change — not a silent PATCH.

---

## 3. Technical enforcement (CI and repo rules)

1. **Conformance gate** — `cargo test -p synx-core` (includes `conformance_suite`) MUST pass on every merge to the protected branch.
2. **Golden file policy** — Changing any `*.expected.json` requires in the same PR:
   - update to **normative spec** or a clearly labeled **erratum** explaining why the previous expected was wrong, **or**
   - a **new language version** file and migration note.
3. **Regression tests** — For every fixed bug, add a minimal `tests/conformance/cases/` case so the bug cannot return without tripping CI.
4. **Diff discipline** — Optional CI step: fail if `to_json` output changes for frozen corpus without an approved spec bump (could use a dedicated `frozen-3.6/` copy of cases).
5. **Fuzzing** — Keep `cargo fuzz` targets green; no loosening of caps without spec update (§3 limits are part of the contract).

---

## 4. Process (human governance)

1. **Freeze declaration** — Tag a release (e.g. `synx-core-v3.6.0`) and state in `CHANGELOG.md`: *«SYNX 3.6 language frozen»*.
2. **Amendments** — Normative text changes go through a short RFC in-repo (`docs/spec/rfc/` or GitHub Discussion): problem, non-goals, conformance impact.
3. **Deprecations** — New syntax MAY supersede old usage; old syntax MUST keep working until at least one **major** or documented deprecation window (e.g. two minor releases).
4. **Backports** — Security or spec-restoring fixes go to a `release-3.6` branch PATCH bumps; additive features land on `main` with spec additions.

---

## 5. What «additive only» allows

- New directives that are ignored by older parsers (risky for interchange — prefer new **version negotiation** or top-level `synx-version 3.7` key convention).
- New markers / constraints **ignored** in Static mode and only consumed by optional engines — provided **default JSON output** for all current conformance files is unchanged.
- New optional binary flags in a **new** `.synxb` version — keep v1 decoder stable.

---

## 6. What is forbidden after freeze

- Changing sort order, escaping, or number formatting of `to_json` for existing inputs.
- Changing `!tool` reshape rules without a new language version.
- Silently changing truncation limits downward.
- Replacing «silent skip» behavior with hard errors for inputs the spec says are recoverable (without a major).

---

## 7. Summary

**Freeze** = **normative doc + conformance goldens + semver policy + CI**.  
Breaking behavior requires a **new spec version**, not a stealth PATCH.

---

*Maintainers: align `CHANGELOG.md` and release tags when declaring the freeze.*
