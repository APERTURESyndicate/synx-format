# EDITING — what to re-check after changes

Use this list when you change **language behavior**, **public APIs**, **packaging**, or **tooling**. Skipping these checks is how spec drift and release surprises happen.

---

## Always touch or verify

| If you changed… | Re-check / update |
|-----------------|-------------------|
| **Syntax, semantics, canonical JSON, limits** | `docs/spec/SPECIFICATION_EN.md`, `docs/spec/SPECIFICATION_RU.md`, `docs/spec/SYNX-3.6-NORMATIVE.md` (and errata process in `docs/spec/CORE-FREEZE.md`) |
| **Frozen 3.6 behavior or conformance** | `tests/conformance/cases/*.synx`, matching `*.expected.json`; run `cargo test -p synx-core` |
| **User-visible behavior or versions** | Root `CHANGELOG.md` |
| **Rust parser / engine / stringify / binary** | `crates/synx-core/`; workspace `Cargo.toml` + `crates/synx-core/Cargo.toml` versions |
| **CLI** | `crates/synx-cli/` (`--help` text, exit codes, any new subcommands) |
| **LSP** | `crates/synx-lsp/` (capabilities, messages tied to core behavior) |

---

## Often needed

| If you changed… | Re-check / update |
|-----------------|-------------------|
| **C ABI or shared library** | `bindings/c-header/`, consumers of `synx.h` |
| **C++ header wrapper** | `bindings/cpp/include/synx/synx.hpp`, `bindings/cpp/README.md` (must stay aligned with `synx.h`) |
| **Go cgo binding** | `bindings/go/*.go`, `bindings/go/README.md` (must stay aligned with `synx.h` + link story) |
| **Mojo interop** | `bindings/mojo/synx/interop.mojo`, `bindings/mojo/README.md` (must stay aligned with `synx_native` Python API) |
| **Swift package** | `bindings/swift/Sources/Synx/*.swift`, `bindings/swift/Sources/CSynx/synx.h` (**mirror** of `bindings/c-header/include/synx.h`) |
| **Kotlin/JVM (JNA)** | `bindings/kotlin/src/main/kotlin/**/*.kt`, `build.gradle.kts` — must stay aligned with **`synx.h`** + runtime **`synx_c`** |
| **C# NuGet** | `parsers/dotnet/src/Synx.Core/Synx.Core.csproj` — **`PackageId` `APERTURESyndicate.Synx`**, **`Version`** per release; pack/push: repo root **`publish-csharp.bat`** |
| **Python `synx_native` JSON/hex helpers** | `bindings/python/src/lib.rs` — keep in sync if Mojo/C callers need new string boundaries |
| **Node / npm** | `bindings/node/`, `packages/synx-js/` (pure TS parser), their `package.json` and published API |
| **Python** | `bindings/python/pyproject.toml`, wheel/CI in `.github/workflows/` |
| **WASM** | `bindings/wasm/` |
| **Grammar / editor highlighting** | `tree-sitter-synx/` (`grammar.js`, queries); `integrations/sublime-text/`, Neovim, VS Code grammar if duplicated |
| **.NET parser** | `parsers/dotnet/` |
| **Integrations** | `integrations/vscode/synx-vscode/`, `integrations/mcp/synx-mcp/`, `integrations/visualstudio/`, `integrations/ai/synx-adapter/` — URLs, version pins, docs that claim behavior |
| **CI / release scripts** | `.github/workflows/`, `scripts/verify-release-quality.*` |
| **High-level repo map** | `docs/SYNX_AT_A_GLANCE.md`, `docs/repository-layout.md` (only if layout or names really changed) |
| **Long guides** | `docs/guides/GUIDE*.md` (only sections that mirror the spec or CLI you touched) |

---

## Version and packaging discipline

- **Crates:** root `Cargo.toml` workspace + per-crate `Cargo.toml` (`synx-core`, `synx-cli`, `synx-lsp`, `synx-format`, benches, bindings).
- **npm:** `packages/synx-js/package.json`, `tree-sitter-synx/package.json`, integration `package.json` files under `integrations/`.
- **Python:** `bindings/python/pyproject.toml`, `integrations/ai/synx-adapter/python/pyproject.toml` if applicable.

Align semver / language-version story with `docs/spec/CORE-FREEZE.md` when a change might alter canonical output for existing `.synx`.

---

## Short rule

**If it affects what a valid document means or what JSON comes out — update spec, conformance goldens, and `CHANGELOG.md`, then run the core test suite.**

For questions about what is frozen vs additive, see `docs/spec/CORE-FREEZE.md`.
