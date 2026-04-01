# Changelog

All notable changes to this repository are documented in this file.

## [Unreleased]

### Added

- **`docs/dev/`** — contributor-facing stubs: [`docs/dev/plugins-roadmap.md`](docs/dev/plugins-roadmap.md) (registry / `!use` / package layout **TBD**; **no** plugin loader in `synx-core` yet).
- **Publish helper scripts** — `publish-cpp.bat`, `publish-csharp.bat`, `publish-swift.bat`, `publish-kotlin.bat`, `publish-mojo.bat` (see script headers for registry URLs and env vars). **`*.bat`** remains gitignored except **`publish-*.bat`** via `.gitignore` exception.
- **NuGet package ID (C#):** **`APERTURESyndicate.Synx`** — `Synx.Core` was already taken on nuget.org; project folder and `Synx` namespaces unchanged.

### Documentation

- **Install C# everywhere it matters:** root [`README.md`](README.md) — `dotnet add package APERTURESyndicate.Synx`, link [nuget.org/packages/APERTURESyndicate.Synx](https://www.nuget.org/packages/APERTURESyndicate.Synx), pre-publish path (`dotnet add reference` / local `.nupkg` via `publish-csharp.bat`); maintainer notes for **`.\publish-csharp.bat`** (PowerShell), **`NUGET_API_KEY`** without angle brackets (avoids **403**), stale `artifacts/nuget/*.nupkg` cleanup; short **ready to push** checklist (verification scripts, no secrets, bump `Version` in `Synx.Core.csproj`).
- **Indexes:** [`docs/README.md`](docs/README.md), [`docs/SYNX_AT_A_GLANCE.md`](docs/SYNX_AT_A_GLANCE.md), [`docs/repository-layout.md`](docs/repository-layout.md), [`EDITING.md`](EDITING.md), [`parsers/README.md`](parsers/README.md), [`parsers/dotnet/README.md`](parsers/dotnet/README.md) — aligned on NuGet ID and publish script.
- **Guides:** EN/RU C# sections + **DE/ES/JA/ZH** installation blocks — same one-liner and package URL; full detail remains in `parsers/dotnet/README.md`.
- **Spec §5.9 (EN/RU):** C# section updated for **.NET 8**, **`SynxFormat`** API, **`SynxValue`** records, NuGet URL; removed obsolete **`SynxParser` / `packages/synx-csharp`** text; comparison table uses `SynxFormat.Parse(File.ReadAllText(...))`.

## Changes by Module

Quick reference of what was modified in recent versions:

| Version | Components Modified |
|---------|---|
| **3.6.0** | synx-core (`.synxb`, `!tool`, **`!llm`**, `diff`, JSON Schema helpers, hostile-input caps), synx-cli (**`synx schema`**, **`json-validate`**), synx-lsp, VS Code (`!llm`), bindings (**C++**, **Go cgo**, **SwiftPM+`synx-c`**, **Kotlin/JNA+`synx-c`**, **Mojo↔CPython**), Python extra JSON/hex helpers for thin interop, conformance (**11** cases), tree-sitter, fuzz + **`Synx.FuzzReplay`**, **`SYNX_AT_A_GLANCE`**, SYNX-Adapter, docs: **normative spec + CORE-FREEZE (engine frozen 2026-04-01)** |
| **3.5.2** | synx-core (`:prompt` marker, `:vision`/`:audio` metadata, calc modulo-by-zero fix), synx-js (same + `Synx.diff()` + prototype pollution fix + ReDoS guard + `deepGet`/parser hardening), synx-vscode (diagnostics/completion for 3 new markers + `:template` sibling-scope fix), all 6 guides |
| **3.5.1** | synx-core (stack overflow guard, circular alias detection), synx-js (same + SynxError class + browser bundle export), synx-vscode (circular alias diagnostic), security tests |
| **3.5.0** | synx-core (path jail, depth limit, file size limit, calc length limit), synx-js (same), LICENSE (ethical use clause) |
| **3.4.0** | synx-core (`:spam` rate-limit marker), synx-js (`:spam` + strict error sync), VSCode (diagnostics/completion/navigation/preview for `:spam`), guides (all languages), version sync |
| **3.3.0** | synx-core (multi-parent inherit, calc dot-path, i18n plural, quoted strings, :import alias), VSCode (diagnostics/completion/navigation sync), documentation |
| **3.2.3** | synx-core (global [] constraints), documentation, version sync |
| **3.2.2** | synx-core (type validation), documentation |
| **3.2.1** | VSCode (diagnostics, syntax), Python binding, Node.js binding, WASM binding, C FFI, synx-core (serde), CI/CD, documentation |
| **3.2.0** | JS/TS engine, Rust engine, VSCode (completion, parser, syntax), guides (6 languages), documentation |
| **3.1.3** | VSCode extension, JS/TS API, documentation (6 guides), CLI tool, deployment examples |
| **3.1.2** | JS parser, Rust parser, VSCode extension, Node.js binding (napi), all guides |
| **3.1.0** | JS/TS API (runtime manipulation), Rust engine, VSCode extension, all guides |

---

## [3.6.0] — (SYNX Diagen) 2026-03-29

### Added

- **Rust CLI (`synx`):** New `crates/synx-cli` crate providing `synx parse`, `synx validate`, `synx convert`, `synx tool`, `synx compile`, `synx decompile`, `synx diff`, `synx query`, `synx format` commands. Built with clap v4; single static binary. Replaces the legacy Node.js CLI (`packages/synx-js/bin/synx.js` which was stuck at v3.2.0).
- **`diff` module (`synx-core`):** Structural diff between two parsed SYNX objects — `added`, `removed`, `changed`, `unchanged` keys. Public API: `Synx::diff()`, `diff_to_value()`.
- **`diff` in all bindings:** Node (`diff`, `diffJson`), Python (`diff`, `diff_json`), WASM (`diff`, `diff_object`), C FFI (`synx_diff`). Full API parity table updated.
- **`synx query` command:** dot-path query with array index support (e.g. `synx query server.host config.synx`, `synx query items.0 data.synx`).
- **Conformance test suite:** `tests/conformance/` — 11 canonical test cases (scalar types, nesting, arrays, type casting, comments, multiline, mixed structures, strings with spaces, empty values, tool mode, **`!llm` directive**). Rust runner as integration test: `cargo test -p synx-core --test conformance`.
- **Language Server (`synx-lsp`):** New `crates/synx-lsp` crate — LSP server over stdio using `tower-lsp-server`. Supports real-time diagnostics (tabs, odd indentation, unknown markers/constraints/types, duplicate keys, `!active` requirement), completion (markers, constraints, directives), and document symbols (full outline tree). Works in any LSP-capable editor: Neovim, Helix, Zed, Emacs, JetBrains (manual LSP config).
- **GitHub Action:** `.github/actions/synx/action.yml` — composite action to validate `.synx` files in CI using the Rust CLI. Inputs: `files` (glob), `strict` (bool), `version`.
- **Tree-sitter grammar:** `tree-sitter-synx/` subfolder with `grammar.js`, highlight queries (`queries/highlights.scm`), and `package.json`. Provides syntax highlighting for Neovim, Helix, Zed, Emacs, and future GitHub Linguist support.
- **Fuzz targets:** `crates/synx-core/fuzz/` with three `cargo-fuzz` targets (`fuzz_parse`, `fuzz_compile`, `fuzz_format`) exercising parser, binary codec, formatter, calc, and engine with arbitrary inputs. See `fuzz/README.md` for usage.
- **`!tool` directive (synx-core):** LLM tool call format. `!tool` reshapes output to `{ tool: "name", params: { ... } }`. Combined with `!schema`, produces `{ tools: [ { name, params } ] }` for tool definitions. Compatible with `!active` — markers resolve before reshaping. New API: `Synx::parse_tool()`, `reshape_tool_output()`.
- **Node.js binding:** `parseTool(text, options?)` for tool call parsing.
- **Python binding:** `parse_tool(text, env=None, base_path=None)` for tool call parsing.
- **WASM binding:** `parse_tool(text)` / `parse_tool_object(text)` for tool call parsing.
- **C FFI:** `synx_parse_tool()` for tool call parsing. Updated `synx.h` header.
- **C++ SDK (`bindings/cpp`):** `include/synx/synx.hpp` (C++17) — optional `std::string` / `std::vector` wrappers over **`synx-c`** / `synx.h` with **full API parity** (`parse`, `parse_active`, `stringify`, `format`, `parse_tool`, `compile`, `decompile`, `is_synxb`, `diff`). CMake example `synx_cpp_minimal` (`SYNX_C_LIBRARY`). Spec §5.7–5.8 and guides (EN/RU/DE/ES/JA/ZH), `README` binding table, and `parsers/README` updated; **still SYNX 3.6.0** (same engine as Rust).
- **Go binding (`bindings/go`):** cgo module `github.com/APERTURESyndicate/synx-format/bindings/go` — `Parse`, `ParseActive`, `Stringify`, `Format`, `ParseTool`, `Compile`, `Decompile`, `IsSynxb`, `Diff` over **`synx-c`**. Linux/macOS default `-L../../target/release -lsynx_c`; **Windows:** link `synx_c.dll.lib` via `CGO_LDFLAGS` + `CGO_LDFLAGS_ALLOW`, runtime `synx_c.dll` on `PATH`. Tests in `synx_test.go`. Spec §5.10 (EN/RU), guides, `README` parity row, repo layout — **3.6.0**.
- **Mojo (`bindings/mojo`):** `synx/interop.mojo` calls **`synx_native`** via Modular **Python from Mojo** — same engine as **`synx-core`** (not a pure Mojo grammar port). Demo `examples/demo.mojo`. **`synx-python`:** added `parse_active_to_json`, `parse_tool_to_json`, `stringify_json`, `compile_hex`, `decompile_hex`, `is_synxb_hex` for string-only boundaries; `synx-core` dependency enables **`serde`** for `stringify_json`. Spec §5.11 (EN/RU, renumbered following sections), guides (all locales), `README` table — **3.6.0**.
- **Swift (`bindings/swift`):** SwiftPM package **`Synx`** — `SynxEngine` wraps **`synx-c`** (`CSynx` + mirrored `synx.h`), `String`/`Data` API (`parse`, `parseActive`, `stringify`, `format`, `parseTool`, `diff`, `compile`, `decompile`, `isSynxb`). Unit tests under `Tests/SynxTests`. Spec §5.13 (EN/RU), comparison table, `README` parity row, repo layout, guides — **3.6.0**.
- **Kotlin/JVM (`bindings/kotlin`):** **`SynxEngine`** over **`synx-c`** via **JNA** (`com.aperturesyndicate:synx-kotlin`, `publishToMavenLocal`), Gradle **8.11** + Foojay toolchain resolver / **JDK 17**; CI **`kotlin-smoke`** in `bindings-smoke.yml`. Spec §5.12 (EN/RU), parity table, guides, `README` / layout — **3.6.0**.
- **6 new `!tool` tests** covering directive flags, call reshape, schema reshape, empty tool, and `!active` compatibility.
- **Binary format `.synxb` (synx-core):** Compact binary representation of SYNX data. Achieves 40%+ size reduction over text via string interning + deflate compression. Public API: `Synx::compile()`, `Synx::decompile()`, `Synx::is_synxb()`.
- **`binary` module (synx-core):** Wire format: 7-byte header (magic `SYNXB` + version + flags) + 4-byte uncompressed size + deflate-compressed payload. Payload uses string interning table with varint references and zigzag-encoded integers.
- **`--resolved` flag:** When `resolved=true`, `compile()` resolves all `:env`, `:ref`, `:calc` markers and strips metadata/includes — producing a fully-resolved snapshot.
- **Node.js binding:** `compile(text, resolved?)` → `Buffer`, `decompile(buf)` → `string`, `isSynxb(buf)` → `boolean`.
- **Python binding:** `compile(text, resolved=False)` → `bytes`, `decompile(data)` → `str`, `is_synxb(data)` → `bool`.
- **WASM binding:** `compile(text, resolved)` → `Uint8Array`, `decompile(data)` → `string`, `is_synxb(data)` → `boolean`.
- **C FFI:** `synx_compile()`, `synx_decompile()`, `synx_is_synxb()`, `synx_free_bytes()`. Updated `synx.h` header.
- **16 new binary tests** covering round-trip fidelity, all value types, metadata preservation, constraints, size reduction, error paths.
- **Size benchmark** in `benchmarks/rust/` comparing text vs binary on the 110-key production config.
- **`.NET` / C# parser (preview):** `parsers/dotnet/` — SDK-style library `Synx.Core` that parses static SYNX (same structural rules as `synx-core::parser::parse`), emits canonical JSON with **sorted object keys** (matching `synx_core::to_json`). `dotnet test` in `parsers/dotnet/` runs the same `tests/conformance/cases/*.synx` + `.expected.json` pairs as the Rust runner, including `!tool` reshape via `Synx.ParseTool` (no `!active` engine yet — tool cases that only need parse + reshape are covered).
- **Integrations (agents & editors):**
  - **MCP server** [`integrations/mcp/synx-mcp/`](integrations/mcp/synx-mcp/) — stdio MCP with `validate` / `parse` / `format` tools; companion docs in [`integrations/mcp/README.md`](integrations/mcp/README.md) and [`docs/claude.md`](docs/claude.md).
  - **AI adapters** [`integrations/ai/synx-adapter/`](integrations/ai/synx-adapter/) — Python and Node helpers (`pack_for_llm` / `packForLlm`, JSON size estimates); index in [`integrations/ai/README.md`](integrations/ai/README.md).
  - **Sublime Text** — [`integrations/sublime-text/Synx/`](integrations/sublime-text/Synx/).
  - **Neovim** — [`integrations/neovim/synx.nvim/`](integrations/neovim/synx.nvim/).
- **LSP crate docs:** [`crates/synx-lsp/README.md`](crates/synx-lsp/README.md) — install and editor hookup for `synx-lsp`.
- **MCP filesystem tools:** [`integrations/mcp/synx-mcp/`](integrations/mcp/synx-mcp/) — `synx_read_path`, `synx_write_path` (atomic temp + rename), `synx_apply_patch` (ordered unique substring replacements); gated by **`SYNX_MCP_ROOT`** or comma-separated **`SYNX_MCP_ROOTS`** (10 MB cap per file).
- **Docs for Claude / Anthropic:** [`docs/anthropic-system-prompt.txt`](docs/anthropic-system-prompt.txt), [`docs/anthropic-token-notes.md`](docs/anthropic-token-notes.md) (SYNX vs JSON tokenizer sample comparison), [`docs/claude.md`](docs/claude.md).
- **Benchmarks:** Node + Python runners now include **XML** (JSON-derived tree) alongside JSON/YAML/SYNX; Node uses `fast-xml-parser`.
- **`synx-adapter` CLI:** `synx-context` — Python [`synx_adapter.cli_compress`](integrations/ai/synx-adapter/python/synx_adapter/cli_compress.py), Node bin; optional **`--xml` / `--xml-tag`** wraps output in `<synx_data>` (CDATA) so Claude sees XML boundaries while the payload stays SYNX. Adapter [`README.md`](integrations/ai/synx-adapter/README.md) in English.
- **C# `Synx.Core`:** `!active` resolution engine (markers, `[]` constraints metadata, `!include` list, `:calc` / `:env` / `:ref` / `:alias` / `:random` / `:i18n` / interpolation, etc.) via `SynxFormat.ParseActive` / `ParseFullActive`; `SynxValue.Secret`; extra tests in `EngineActiveTests`.
- **C# fuzz replay (`Synx.FuzzReplay`):** [`parsers/dotnet/tools/Synx.FuzzReplay/`](parsers/dotnet/tools/Synx.FuzzReplay/) — replay corpus / `minimized-from-*` through `Parse` + `ToJson` (strict UTF-8, aligned with Rust `fuzz_parse`). Documented in [`crates/synx-core/fuzz/README.md`](crates/synx-core/fuzz/README.md).
- **Python fuzz replay:** [`crates/synx-core/fuzz/scripts/synx_fuzz_replay.py`](crates/synx-core/fuzz/scripts/synx_fuzz_replay.py) — `synx_native` corpus replay (`parse`, `parse_to_json`, `stringify`, `parse_active`, `parse_tool`); same UTF-8 filter as Rust.
- **Fuzz coverage report (llvm-cov):** `crates/synx-core/fuzz/coverage/fuzz_parse/html/` — pruned to remove 0.00% line-coverage rows/pages (unused files), keeping the report focused on executed code.
- **Expanded `fuzz_parse` corpus:** Added an additional **7,177** interesting inputs under `crates/synx-core/fuzz/corpus/fuzz_parse/` from a long fuzzing run.
- **Vendor verification scripts:** [`scripts/verify-release-quality.ps1`](scripts/verify-release-quality.ps1), [`scripts/verify-release-quality.sh`](scripts/verify-release-quality.sh) — `cargo test -p synx-core`, `dotnet test`, FuzzReplay on conformance `.synx`, optional bench build.
- **`docs/SYNX_AT_A_GLANCE.md`:** single-page map (layout, parsers, benchmarks, AI/Claude, quality gates).
- **SYNX-Adapter — long context & Anthropic:** `make_anchor_index` / `inject_section_anchors`, `pack_for_llm` / `packForLlm` anchor options; CLI `--anchor-index`, `--section-anchors`, `--anchor-prefix`; **`synx_adapter.anthropic_tools`** and **`@aperturesyndicate/synx-format-adapter/anthropic`**.
- **`docs/guides/long-context-synx.md`:** SYNX + anchors vs lost-in-the-middle; Anthropic tool-result notes.
- **Claude Artifacts:** [`integrations/artifacts/synx-visualizer/`](integrations/artifacts/synx-visualizer/) — `SynxVisualizer` React snippet.
- **`!llm` directive (synx-core):** Optional top-of-file marker for LLM-oriented envelopes; sets `ParseResult.llm`. Parsed **data tree is unchanged** vs omitting the line (conformance: `011-llm-directive`). **`.synxb`:** header flag bit 6; `Synx::decompile` / canonical `format` emit `!llm` when missing from body.
- **`synx-core` `schema_json`:** `metadata_to_json_schema()`, `value_to_json_value()`, and (with feature `jsonschema`) `validate_with_json_schema` / `validate_serde_json` (draft 2020-12).
- **`synx` CLI:** `synx schema`, `synx json-validate`, `synx validate --self-schema` / `--json-schema` (bundled JSON Schema validation).
- **JS:** `Synx.schema()` no longer emits invalid per-property `required`; `parseData()` exposes `llm` when `!llm` is present.
- **synx-lsp / VS Code:** directive completion + TextMate grammar + editor parser skip for `!llm`.

### Frozen engine (2026-04-01)

The **SYNX 3.6.0** language surface in [`docs/spec/SYNX-3.6-NORMATIVE.md`](docs/spec/SYNX-3.6-NORMATIVE.md) and **`synx-core`** parse → canonical JSON behaviour covered by **`tests/conformance/`** are **frozen**. **PATCH** (`3.6.z`) = spec restoration and compatibility fixes only; non-additive language changes require a new normative version. See [`docs/spec/CORE-FREEZE.md`](docs/spec/CORE-FREEZE.md) and the notice in the root [`README.md`](README.md).

### Changed

- **synx-core (hostile input):** Parser/engine/stringify bounds for long fuzz runs — UTF-8 prefix cap, line-table cap, indentation nesting **128**, multiline block / list / `!include` limits, JSON/stringify/format depth caps, engine scratch string cap; see [`fuzz/README.md`](crates/synx-core/fuzz/README.md). **C#** deep-nesting test aligned with parser limits.
- **synx-core (format patterns):** Clamp `%Nd/%0Nd` integer width and `%.Nf/%.Ne` float precision to prevent `std::fmt` panics like **“Formatting argument out of range”** under hostile/fuzzed inputs.
- **Repository layout:** `_guides/` moved to `docs/guides/`; normative specification moved to `docs/spec/`; Criterion crate path is `benchmarks/rust/` (still package name `synx-bench`); LLM benchmark long-form guide is `benchmarks/llm-tests/GUIDE.md`. Added `docs/README.md` and `docs/repository-layout.md`. Removed root `playground-mini-parser.js` (obsolete subset parser). `packages/synx-js` ships a short `SPECIFICATION.md` pointer; full spec files live under `docs/spec/`.

### Dependencies

- Added `miniz_oxide 0.8` (pure-Rust deflate, zero transitive deps) for payload compression.
- Added `clap 4` for CLI argument parsing (`synx-cli` crate).
- Added `tower-lsp-server 0.23` + `tokio 1` for LSP server (`synx-lsp` crate).
- Added `libfuzzer-sys` for fuzz targets (`crates/synx-core/fuzz/`).
- Added `serde_json` + optional `jsonschema` (with feature `jsonschema`) for schema export/validation in `synx-core` / `synx-cli`.

---

## [3.5.2] — 2026-03-28

### Added

- **`:prompt` marker (Rust + JS):** Formats a subtree into a labeled SYNX code fence for LLM consumption. Usage: `key:prompt:Label` produces a string like `"Label (SYNX):\n\`\`\`synx\n...\n\`\`\`"`. No network calls — purely a serialization transform.
- **`:vision` metadata marker (Rust + JS):** Marks a key as image-generation intent. Pass-through marker preserved for application-layer processing.
- **`:audio` metadata marker (Rust + JS):** Marks a key as audio-generation intent. Pass-through marker preserved for application-layer processing.
- **`Synx.diff(a, b)` (JS):** Static method that compares two parsed SYNX objects and returns a structured diff with `added`, `removed`, `changed` (with `from`/`to`), and `unchanged` fields.
- **`SynxDiff` type (JS):** TypeScript interface for the diff result, exported from the package.
- **VS Code:** IntelliSense autocomplete and documentation for `:prompt`, `:vision`, `:audio` markers.
- **VS Code:** Diagnostics recognize `:prompt`, `:vision`, `:audio` as valid markers.
- **Guides:** All 6 language guides (EN, RU, DE, ES, JA, ZH) updated with new marker documentation and `Synx.diff()` section.

### Internal

- `stringify_value()` helper added to Rust engine for `:prompt` serialization.
- `stringifyValue()` helper added to JS engine for `:prompt` serialization.
- `deepEqual()` helper added to JS for `Synx.diff()` value comparison.
- 3 new Rust engine tests: `test_prompt_marker`, `test_vision_marker_passthrough`, `test_audio_marker_passthrough`.
- Marker count updated from 21 to 24 across all documentation.

### Fixed

- **VS Code: `:template` false-positive warning for sibling keys.** The diagnostic `"Key referenced in template is not defined"` only checked root-level `keyMap` (dot-paths), so `{sibling}` inside a nested `:template` always flagged as missing. Now checks both root path and sibling scope (`parent.sibling`).
- **Python binding CI smoke tests.** `pyo3`'s `extension-module` mode is now feature-gated so binding builds keep extension semantics by default, while `cargo test` in CI can run with `--no-default-features` and avoid unresolved embedded-Python entrypoints.
- **Bindings workflow stability.** `bindings-smoke.yml` now runs the Python smoke job with `--no-default-features`, prints/verifies the built Node native artifact on Windows, and executes the Node smoke test from the binding directory directly instead of going through `npm run`.
- **JS: Prototype pollution via `Synx.set()` / `Synx.add()` / `Synx.remove()`** (Security). Path segments `__proto__`, `constructor`, `prototype` are now rejected with an error.
- **JS: `deepGet()` followed prototype chain** (Security). Lookups like `constructor` or `toString` could reach inherited properties. Now uses `hasOwnProperty` checks at every traversal step.
- **JS: `__proto__` key injection in parser** (Security). A `.synx` file with a key named `__proto__` could corrupt the parsed object's prototype. Such keys are now silently skipped.
- **JS: ReDoS via constraint `[pattern:]`** (Security). User-supplied regex patterns longer than 128 characters are now rejected to prevent catastrophic backtracking.
- **JS: SPAM_BUCKETS memory leak.** Expired bucket entries (empty timestamp arrays) are now removed from the map instead of persisting indefinitely.
- **Rust + JS: `:calc` modulo by zero.** `expr % 0` now returns `CALC_ERR: division by zero` instead of producing `NaN` (JS) or `NaN` (Rust).

---

## [3.5.1] — 2026-03-20

### Fixed

- **engine (Rust + JS):** Prevent stack overflow when parsing objects with more than 512 levels of nesting. Values beyond the limit are replaced with `NESTING_ERR: maximum object nesting depth exceeded`.
- **engine (Rust + JS):** `:alias` markers now detect self-referential and one-hop circular references. Circular aliases produce `ALIAS_ERR: circular alias detected: <key> → <target>` instead of silently returning wrong values.
- **engine (Rust + JS):** Fixed false-positive `ALIAS_ERR` when a plain string value happened to equal the aliasing key's name. Cycle detection now uses metadata to confirm the target key is itself an alias.

### Added

- **JS:** `SynxError` typed class for strict mode. When `{ strict: true }` is passed, SYNX throws a `SynxError` (extends `Error`) with a `.code` field (e.g. `"CALC_ERR"`, `"ALIAS_ERR"`) instead of a generic `Error`.
- **JS:** `ALIAS_ERR` and `NESTING_ERR` added to strict mode error prefix list.
- **VS Code:** Circular alias diagnostic — editor now shows a warning when `:alias` references form a cycle, without needing to run the engine.
- **Tests:** New `security.test.ts` with 31 edge-case and security tests covering deep nesting, circular aliases, empty input, Unicode, `:calc` limits, type casting, CRLF line endings.

### Internal

- `resolve_value` (Rust) takes `depth: usize` parameter and checks against `MAX_RESOLVE_DEPTH = 512`.
- `resolve()` (JS) takes `_resolveDepth` parameter checked against `MAX_RESOLVE_DEPTH = 512`.
- `apply_markers` (Rust) `_path` renamed to `path`, `_metadata` renamed to `metadata` for active use in alias cycle detection.

---

## [3.5.0] - (SYNX Socrate) 2026-03-08

### Security Hardening

All engines (Rust core + JS/TS) now include built-in security protections. No functionality is removed — safe defaults are applied automatically.

- **Path jail (filesystem sandbox)**: `:include`, `:import`, `:watch`, `:fallback` file paths are resolved relative to `basePath` and cannot escape it. Absolute paths and `../` traversal beyond the project root are rejected.
- **Include depth limit**: Nested `:include`/`:import`/`:watch` calls are limited to 16 levels deep (configurable via `maxIncludeDepth` / `max_include_depth`). Prevents infinite recursion and circular include DoS.
- **File size limit**: Included/watched files larger than 10 MB are rejected to prevent memory exhaustion.
- **Calc expression length limit**: `:calc` expressions longer than 4096 characters are rejected to prevent parser abuse.
- **Env isolation**: When `env` option is provided explicitly, only that map is used — no fallthrough to `process.env` / `std::env`.

### Added
- `max_include_depth` option (Rust `Options`) and `maxIncludeDepth` option (JS `SynxOptions`) — configurable max nesting depth for file operations (default: 16).
- `jail_path()` / `jailPath()` internal helpers — canonicalize and validate paths against base directory.
- `check_file_size()` / `checkFileSize()` internal helpers — reject files exceeding 10 MB.

### Changed
- **LICENSE**: Added ethical use notice — SYNX must not be used for unauthorized data exfiltration, credential theft, or causing harm.

---

## [3.4.0] - (SYNX Classic) 2026-03-08

### Added
- **`:spam` marker** (Rust engine + JS engine + VSCode preview parser): `key:spam:MAX_CALLS[:WINDOW_SEC] target` limits how often a target can be resolved inside a time window. If `WINDOW_SEC` is omitted, it defaults to `1`.
- **Rate-limit error surface**: Engines now emit `SPAM_ERR: ...` when the limit is exceeded.
- **VSCode support for `:spam`**: marker recognition in diagnostics, argument validation (`MAX_CALLS`, `WINDOW_SEC`), completion docs/snippet, and go-to-definition support for `:spam` target references.

### Changed
- **Strict runtime mode update (JS)**: `SPAM_ERR` is now treated as a strict-mode runtime error prefix (same fail-fast behavior as `INCLUDE_ERR`, `WATCH_ERR`, `CALC_ERR`, `CONSTRAINT_ERR`).
- **Version sync to `3.4.0`** across core and binding/package manifests.
- **Guide updates**: added `:spam` marker documentation to all language guides.

---

## [3.3.0] - 2026-03-08

### Added
- **Multi-parent `:inherit`** (Rust engine): A block can now inherit from multiple parents via `:inherit:_parent1:_parent2:_parent3`. Parents merge left-to-right (later parents override earlier ones), child fields override all. Enables mixin-style composition for templates.
- **`:calc` dot-path references** (Rust engine): Arithmetic expressions now support nested key references via dot-path syntax (e.g., `total:calc stats.base_hp * stats.multiplier`). Previously only flat root-level keys were resolved.
- **`:i18n` pluralization** (Rust engine): Added CLDR-based plural form selection via `:i18n:COUNT_FIELD` syntax. The language entry contains plural category keys (`one`, `few`, `many`, `other`), and the engine selects the correct form based on the count value. `{count}` placeholder is auto-replaced with the actual number. Supported languages: en, de, es, it, fr, pt, ru, uk, be, pl, cs, sk, ar, ja, zh, ko, vi, th.
- **Quoted string values** (Parser): Wrapping a value in double or single quotes preserves it as a literal string, bypassing auto-casting. `status "null"` → string `"null"` (not null), `enabled "true"` → string `"true"` (not boolean), `count "42"` → string `"42"` (not integer).
- **`:import` marker alias** (Rust engine): `:import` is now a recognized alias for `:include` (key-level file embedding). Recommended to use `:import` to avoid confusion with the `!include` directive (file-level interpolation).
- **Import comparison matrix** (Guide): Added a table in GUIDE.md comparing `!include` (directive) vs `:include`/`:import` (marker) — syntax, placement, behavior, and use cases.
- **7 new tests**: `test_multi_parent_inherit`, `test_calc_dot_path`, `test_i18n_plural_en`, `test_i18n_plural_en_one`, `test_i18n_plural_ru`, `test_quoted_null_preserved`, `test_unquoted_null_is_null`.

### Changed
- **`:inherit` engine rewrite**: `apply_inheritance()` now collects all markers after "inherit" as parent names instead of just one. Backward compatible — single-parent `:inherit:_parent` works unchanged.
- **`:calc` engine enhancement**: Variable substitution now includes a second pass for dot-path identifiers using `deep_get()` traversal after flat key substitution.
- **Guide updates**: Updated `:inherit`, `:calc`, `:i18n`, `:include` sections with new features, examples, and the import matrix table. Added quoted values documentation to Basic Syntax.
- **Version sync to `3.3.0`** across all manifests.

---

## [3.2.3] - 2026-03-08

### Added
- **Global `[]` constraint validation** (Rust engine, active mode): Constraints declared with square brackets now apply consistently across all matching field names in the resolved tree, including inherited fields from `:inherit` templates.
  - Supports global enforcement of `required`, `min`, `max`, `type`, and `enum`
  - Constraint rules are collected into a global registry and recursively applied after marker resolution
  - Violations are surfaced as `CONSTRAINT_ERR: ...` values for visibility in output and downstream checks
- **Constraint merge strategy for repeated field declarations**: When the same field is constrained in multiple places, strict merging is applied (`required`/`readonly` propagate, `min` picks higher bound, `max` picks lower bound).
- **Engine test coverage for global constraints**: Added tests for inherited range validation and required validation:
  - `test_constraint_validation_inherited_range`
  - `test_constraint_validation_required`
- **Guide update near type hints**: Added `Constraint Validation ([]) in Active Mode` section in `_guides/GUIDE.md` with examples for `[required, min:1, max:50000]`, `type`, and `enum`.

### Changed
- **Version sync to `3.2.3`** across core manifests and package manifests used by bindings/extensions.

---

## [3.2.2] - 2026-03-08

### Added
- **Global type validation** (Rust engine, active mode): When you define a field with an explicit type like `hp(int)` or `name(string)`, the engine now validates that **all uses of that field across the entire document match the declared type**. Once a type is registered (e.g., "hp is int"), any value later assigned to that field is checked against the registered type.
  - In `!active` mode, type registry is built from all `key(type)` declarations
  - All field values are validated recursively through the entire value tree
  - Type mismatches are replaced with `TYPE_ERR: 'field' expected TYPE but got ACTUAL` for visibility
  - Benefits: Ensures consistency across blocks (especially with `:inherit`), self-documenting code, early error detection
- **Type validation test coverage**: Added two tests (`test_type_validation`, `test_type_validation_error`) to the synx-core engine test suite to verify correct type matching and error reporting.
- **Type validation documentation** (GUIDE.md): Added "Type Validation (Active Mode)" section under "Type Casting" with examples of valid/invalid type usage and error handling.

---

## [3.2.1] - 2026-03-08

### Added
- **Python binding: `parse_active` now accepts options** — `env` (dict) and `base_path` (str) parameters for `:env` and `:include` marker resolution. Previously `parse_active` used only defaults.
- **Python binding: `stringify`** — converts a Python dict/list back to SYNX format text.
- **Python binding: `format`** — reformats SYNX text into canonical form (sorted keys, normalized indentation).
- **Node.js binding: `stringify`** — converts a JS object back to SYNX format text.
- **Node.js binding: `format`** — reformats SYNX text into canonical form.
- **WASM binding: `parse_object` / `parse_active_object`** — returns JS objects directly via `serde_wasm_bindgen`, eliminating the need for `JSON.parse()` on the consumer side.
- **WASM binding: `stringify`** — converts JSON string to SYNX format text.
- **WASM binding: `format`** — reformats SYNX text into canonical form.
- **C FFI binding: `synx_stringify`** — converts JSON string to SYNX format text. Caller must free with `synx_free`.
- **C FFI binding: `synx_format`** — reformats SYNX text into canonical form. Caller must free with `synx_free`.
- **`serde` feature on `synx-core`** — optional `Serialize`/`Deserialize` derives on `Value` enum (used by WASM and C FFI bindings for JSON round-tripping).
- **Binding API parity table in README** — documented function availability and behavior notes for Rust core, JS package, Python, Node native, WASM, and C FFI.
- **Bindings smoke tests** — lightweight smoke coverage for Python/C/Node/WASM binding surfaces (`parse`, `parse_active`, `stringify`, `format`) without adding runtime overhead to production code paths.
- **CI check matrix** — new GitHub Actions workflow `.github/workflows/bindings-smoke.yml` to run binding-level checks on each push/PR.
- **C header ownership docs** — `bindings/c-header/include/synx.h` now explicitly documents allocation/free contract and NULL-on-error behavior for FFI consumers.

### Fixed
- **VSCode `:inherit` validation** — diagnostics now check that the parent block key exists when using `:inherit:parent_key`. Previously, the error was only shown at parse time, not in the editor.
- **VSCode block comment `###` syntax highlighting** — entire content of block comments is now properly highlighted as comment text. Previously, the first word on a line inside the block could be highlighted as a key instead of comment text.

## [3.2.0] - 2026-03-09

### Added
- **`:ref` marker** (JS + Rust + VSCode): Value reference with marker chaining. Like `:alias` but feeds the resolved value into subsequent markers. Supports shorthand calc: `rate:ref:calc:*2 base_rate` resolves `base_rate` (50), then computes `50 * 2 = 100`.
- **`:inherit` marker** (JS + Rust + VSCode): Block-level field inheritance. Merges all fields from a parent block into the child, with child values taking priority. Use `_` prefix for private template blocks excluded from output: `_base_resource` defines defaults, `steel:inherit:_base_resource` inherits them.
- **`:i18n` marker** (JS + Rust + VSCode): Multilingual values with language selection. Nested keys are language codes (`en`, `ru`, `de`), selected via `options.lang`. Falls back to `en`, then first available value. Syntax: `title:i18n` with child keys per language.
- **Auto-`{}` interpolation** (JS + Rust): In `!active` mode, any string value containing `{key}` is automatically interpolated — no `:template` marker needed. Supports dot-path for nested access (`{server.host}`). The `:template` marker is kept as a recognized no-op for backward compatibility.
- **`!include` directive** (JS + Rust + VSCode): File-level directive `!include ./file.synx [alias]` imports another file's top-level keys for use in `{key:alias}` interpolation. Alias is auto-derived from filename if not provided. Supports `{key:alias}` for named includes and `{key:include}` shorthand when only one file is included.
- **Comment string highlighting** (VSCode extension): Double-quoted `"strings"` and single-quoted `'strings'` inside comments now have distinct colors — orange for `""`, light blue for `''`.

### Fixed
- **Block comment content highlighting** (VSCode extension): Content inside `###` block comments was not highlighted as comments — only the `###` delimiters were colored. Fixed TextMate grammar to apply comment scope to all content between fences.

## [3.1.3] - 2026-03-08

### Added
- **Comment text formatting** (VSCode extension): Markdown-like formatting inside comments — `*italic*` (green), `**bold**` (purple), `***bold+italic***` (gold), `` `code` `` (orange with subtle background). Works in `#`, `//`, and `###` block comments.
- **Deployment example (Docker + Nginx + Redis)**: Added runnable stack example in `examples/docker-stack` with SYNX-driven config and generated Nginx upstream config.
- **CLI tool** (`synx`): New CLI with 4 commands — `synx convert` (export to JSON/YAML/TOML/.env), `synx validate` (strict-mode check for CI/CD), `synx watch` (live reload with `--exec` support), `synx schema` (extract constraints as JSON Schema). Installed globally via `npm install -g @aperturesyndicate/synx-format`.
- **Export formats** (JS/TS API): `Synx.toJSON()`, `Synx.toYAML()`, `Synx.toTOML()`, `Synx.toEnv()` — convert parsed SYNX config to standard formats without external dependencies.
- **File watcher** (JS/TS API): `Synx.watch(filePath, callback, options)` — monitors `.synx` files for changes and delivers hot-reloaded config via callback.
- **Schema export** (JS/TS API): `Synx.schema(text)` — extracts constraint annotations (`[required, min:N, max:N, type:T, enum:A|B, pattern:R]`) as a JSON Schema-compatible object.
- **Deployment guide** (all 6 language guides): Docker, Docker Compose, Nginx, Redis, PostgreSQL, K8s Secrets, Vault, Helm, Terraform, CI/CD validation — added to GUIDE.md and all translations (DE, ES, JA, RU, ZH).


### Changed
- **Syntax highlighting redesign** (VSCode extension): Improved TextMate grammar with semantic scopes for clarity. Parent nodes (with nesting) use `entity.name.section` (bright, bold) to highlight structural branches. Leaf nodes (with values) use `support.type.property-name` (calm, subtle) for actual properties. Markers now `keyword.control.marker.synx` (pink/red). Recursive depth coloring: level 0 `keyword.control` (pink), level 1 `entity.name.tag` (bright cyan), level 2 `entity.name.function` (yellow), level 3+ `variable.parameter` (light cyan). This creates visual hierarchy—structure jumps out, data stays quiet.
- **JS native engine parity**: Large-file native path now forwards `Synx.parse(..., options)` into `parseActive(text, options)`, so `env` and `basePath` behave the same as the pure-JS path.

### Fixed
- **Fail-fast production mode**: Added `strict` option to JS API (`Synx.parse/loadSync/load`) to throw when runtime marker resolution returns `INCLUDE_ERR`, `WATCH_ERR`, `CALC_ERR`, or `CONSTRAINT_ERR` strings.

## [3.1.2] - 2026-03-07

### Fixed
- **`:default` value truncation** (JS parser + VSCode extension): The marker regex `([\w:]+)` only allowed word characters and colons, truncating default values containing dots (IPs like `0.0.0.0`), hyphens (`dev-secret-key`), or operators (`>=`). Changed to `([^\s]+)` to capture the full marker chain up to the next whitespace. This also fixes `:version:>=:18.0` and `:clamp` markers with decimal bounds.
- **`:default` compound values** (all engines — Rust, JS, VSCode): When `:default:VALUE` contained colons (e.g. `0.0.0.0` split as `["0","0","0","0"]` after `:` split), only the first fragment was used as the fallback. Now joins all marker parts after `default` back with `:` to reconstruct the original value.
- **`(string)` type hint ignored by `:default`/`:env`** (Rust + JS engines): A key with `(string)` type hint like `host(string):env:default:0.0.0.0 HOST` would still auto-detect the default value as a number. Now respects the type hint and returns the raw string.
- **VSCode `:env` without `:default` fallback**: The VSCode parser's `:env` handler didn't check for a `:default` sibling marker when the environment variable was missing. Now correctly falls back to the default value.
- **VSCode false "duplicate key" diagnostics in lists**: Keys inside different list-of-objects items (e.g. `category`, `name`, `price` repeated in each `- item`) were flagged as duplicates because scope tracking didn't reset at list item boundaries. Now clears deeper indent scopes when a new list item is encountered.
- **`.gitignore` encoding**: File was UTF-16 (BOM `FF FE`) which git cannot read, causing all ignored files to appear as untracked. Re-encoded to UTF-8 without BOM.

### Added
- **Node native binding: `parseActive` options** (`bindings/node`): `parseActive(text, options?)` now accepts an optional options object with `env` (environment variable overrides) and `basePath` (for `:include` resolution), matching the JS package API.
- **`typeHint` in JS metadata**: The `SynxMeta` interface now includes an optional `typeHint` field, allowing engine markers to respect explicit type casts like `(string)`.
- **Nesting-level key coloring** (VSCode extension): Keys are now colored by indentation depth — each nesting level gets a distinct color (blue → teal → yellow → purple → orange → gold), with separate palettes for dark and light themes.
- **Block comments `###`** (all parsers — Rust, JS, VSCode): Multi-line comments using `###` fences. Everything between an opening `###` and closing `###` is ignored by the parser.
- **Comment text formatting** (VSCode extension): Markdown-like formatting inside comments — `*italic*` (green), `**bold**` (purple), `***bold+italic***` (gold), `` `code` `` (orange with subtle background). Works in `#`, `//`, and `###` block comments.

### Changed
- Added `*.bat` and `.claudeignore` to `.gitignore`.

## [3.1.0] - 2026-03-07

### Fixed
- **Critical**: Removed `/n` → newline replacement from JS parser (`parser.ts`) and Rust core parser (`parser.rs`). The code was corrupting any value containing the two-character sequence `/n` (e.g. URLs like `/newsletter`, `/nginx`, `/node`) in multiline blocks and list items by replacing it with an actual newline character. This behavior was not part of the SYNX spec.
- **Security**: Removed `SynxSecret.valueOf()` from the JS engine. `valueOf()` returned the real secret value, meaning secrets could leak in arithmetic contexts or coercion (e.g. `secretVal + 1`). Only `.reveal()` should expose the underlying value.
- **Dead code**: Removed `SECRET_TAG = Symbol('synx:secret')` from the JS engine. The symbol was set on every `SynxSecret` instance via `Object.defineProperty` but was never read anywhere.
- **Silent coercion**: Fixed `castType` in JS parser — `parseInt(raw, 10) || 0` was silently returning `0` for any invalid or falsy numeric input (e.g. `(int)0` returned `0` but so did `(int)abc`). Changed to explicit `isNaN` check.
- **`:watch` nested key lookup**: `extractFromFileContent` in the JS engine was doing a flat line-by-line scan for `.synx` source files, so `:watch:database.host` always returned `null` for nested keys. Now uses `parseData` + `deepGet` for correct dot-path resolution.
- **`.synx.lock` format in docs**: The English guide showed a JSON block as the lock file format, but the actual implementation writes plain `key value` text (one per line). Corrected in `_guides/GUIDE.md`.
- **`:calc` ordering**: Added inline comment clarifying that `:calc` expressions only see already-resolved numeric siblings — keys that appear later in file order and still hold unresolved marker values are not available. This was a silent failure with no prior indication.

### Changed
- `Synx.format()` canonical formatter added to JS (`packages/synx-js`) and Rust (`crates/synx-core`): sorts keys alphabetically at every level, normalizes indentation to 2 spaces, strips comments, adds blank lines between top-level blocks. Useful for deterministic git diffs and pre-commit hooks.
- All language guides (`_guides`) updated with `format()` usage examples and pre-commit hook script.

## [3.1.0] - 2026-03-06

### Added
- Type-cast random generation in parsers:
- `(random)` and `(random:int)` for integer values
- `(random:float)` for float values
- `(random:bool)` for boolean values
- Runtime config manipulation API in JS/TS:
- `Synx.get(obj, keyPath)`
- `Synx.set(obj, keyPath, value)`
- `Synx.add(obj, keyPath, item)`
- `Synx.remove(obj, keyPath, item?)`
- `Synx.isLocked(obj)`
- `!lock` directive support to protect parsed configs from external runtime mutation through the JS/TS API.
- Delimiter keyword support for `slash` in marker processing (`:split` / `:join`).
- Root spelling dictionary config (`cspell.json`) for SYNX-specific terms.

### Changed
- JS and Rust parser type-hint regex now supports `:` in cast names (for example `(random:int)`).
- VS Code extension completion behavior improved:
- marker snippets no longer inject noisy placeholders by default
- `!active` completion after `!` no longer produces `!!active`
- added `!lock` completion and random cast completions
- VS Code diagnostics updated to recognize random type-casts as valid.
- VS Code parser updated to ignore `!lock` directive line as a directive, not a key.
- VS Code syntax grammar updated to highlight `!lock`.
- VS Code extension package version set to `3.1.0`.

### Fixed
- Documentation and runtime behavior aligned for `:join:slash` by adding actual `slash` delimiter support in engines.
- Type diagnostics mismatch for random casts in VS Code extension.

### Documentation
- Guides updated in all supported languages (`_guides`):
- random cast section
- lock mode section
- runtime manipulation examples
- marker compatibility section
- Python access-helper equivalents (`get_path` / `set_path` / `add_path` / `remove_path`) with note that native Python API currently exposes `parse`, `parse_active`, `parse_to_json`
- delimiter keyword lists synchronized in `split` and `join` sections
- Removed "view logo" button lines from GitHub guides while keeping GIF demos.
- VS Code README Full Specification section expanded with links to all language guides and specification files.
- Added extension-scoped changelog: `packages/synx-vscode/CHANGELOG.md`.

### Tooling and Release Scripts
- `publish-npm.bat` improved for safer execution:
- path auto-detection
- optional version bump argument
- better npm auth flow (`npm login` / `NPM_TOKEN`)
- explicit `call npm ...` usage on Windows
- clearer error output
- Added package-local publish helper: `packages/synx-js/publish-npm.bat`.

## [3.0.0] - Original

### Added
- Initial public release of SYNX format and parser/runtime ecosystem.
- Core marker system, constraints, and `!active` processing pipeline.
- Rust core crate and bindings/packages for JS/TS, Python, and VS Code tooling.

---

<div align="center">
  <img src="https://media.aperturesyndicate.com/asother/as/branding/png/asp_128.png" width="128" height="128" />
  <p>Made by <strong>APERTURESyndicate Production</strong></p>
</div>
