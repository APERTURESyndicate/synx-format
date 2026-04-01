# SYNX at a glance

One-page map of the **synx-format** monorepo: locations, capabilities, quality gates, AI/Claude tooling, benchmarks. For depth, follow links.

---

## What it is

**SYNX** — human-readable structured data (key / value / indent), optional **`!active`** mode with markers (`:calc`, `:env`, `:alias`, …), optional **`!llm`** envelope hint, binary **`.synxb`**, **JSON parity** for interchange. Tagline: fewer tokens than JSON for many LLM payloads.

**3.6.0 frozen core:** normative [`spec/SYNX-3.6-NORMATIVE.md`](spec/SYNX-3.6-NORMATIVE.md) + [`tests/conformance/`](../tests/conformance/) + `synx-core` 3.6.x — see [`spec/CORE-FREEZE.md`](spec/CORE-FREEZE.md).

---

## Where things live

| Area | Path |
|------|------|
| **Canonical parser + engine** | [`crates/synx-core/`](../crates/synx-core/) |
| **Rust CLI** | [`crates/synx-cli/`](../crates/synx-cli/) |
| **LSP** | [`crates/synx-lsp/`](../crates/synx-lsp/) |
| **Fuzz harness (libFuzzer)** | [`crates/synx-core/fuzz/`](../crates/synx-core/fuzz/) |
| **C# parser (.NET 8)** | [`parsers/dotnet/`](../parsers/dotnet/) — project `Synx.Core`, NuGet **`APERTURESyndicate.Synx`** |
| **C# fuzz/corpus replay** | [`parsers/dotnet/tools/Synx.FuzzReplay/`](../parsers/dotnet/tools/Synx.FuzzReplay/) — targets **.NET 8+** (often **net10.0** in-csproj for dev machines without the .NET 8 runtime only) |
| **Python fuzz/corpus replay** | [`crates/synx-core/fuzz/scripts/synx_fuzz_replay.py`](../crates/synx-core/fuzz/scripts/synx_fuzz_replay.py) — `synx_native` (`pip install -e bindings/python`) |
| **TypeScript / npm** | [`packages/synx-js/`](../packages/synx-js/) |
| **Bindings** | [`bindings/node`](../bindings/node), [`bindings/python`](../bindings/python), [`bindings/wasm`](../bindings/wasm), [`bindings/c-header`](../bindings/c-header), [`bindings/cpp`](../bindings/cpp), [`bindings/go`](../bindings/go), [`bindings/mojo`](../bindings/mojo) (Mojo → `synx_native`) |
| **Conformance suite** | [`tests/conformance/`](../tests/conformance/) — `.synx` + `.expected.json` |
| **Benchmarks** | [`benchmarks/`](../benchmarks/) — Rust Criterion, Node, Python |
| **Docs index** | [`docs/README.md`](README.md) |
| **Spec (normative)** | [`docs/spec/SYNX-3.6-NORMATIVE.md`](spec/SYNX-3.6-NORMATIVE.md) · human guides [`SPECIFICATION_EN.md`](spec/SPECIFICATION_EN.md) |
| **Repo tree** | [`docs/repository-layout.md`](repository-layout.md) |
| **Integrations index** | [`integrations/README.md`](../integrations/README.md) |
| **Vendor assurance script** | [`scripts/verify-release-quality.ps1`](../scripts/verify-release-quality.ps1) · [`.sh`](../scripts/verify-release-quality.sh) |

---

## What each parser/runtime does

| Implementation | Parse static | `!active` / engine | `.synxb` | Fuzz entry |
|----------------|-------------|-------------------|---------|------------|
| **Rust `synx-core`** | yes | yes | yes | `cargo-fuzz` (`fuzz_parse`, `fuzz_compile`, `fuzz_format`) |
| **C# `Synx.Core`** | yes | yes (`ParseActive`) | no (yet) | **`Synx.FuzzReplay`** (corpus replay; strict UTF-8 like Rust harness) |
| **JS `@aperturesyndicate/synx-format`** | yes | yes | yes (via core where exposed) | security tests in package |
| **Python `synx-format`** | yes | yes | yes | — |
| **C++ `synx/synx.hpp`** (via `synx-c`) | yes | yes | yes | — |
| **Go** `bindings/go` (cgo, `synx-c`) | yes | yes | yes | — |
| **Mojo** `bindings/mojo` (CPython `synx_native`) | yes | yes | yes | — |
| **Swift** `bindings/swift` (`synx-c`) | yes | yes | yes | — |
| **Kotlin/JVM** `bindings/kotlin` (JNA, `synx-c`) | yes | yes | yes | — |

---

## How to use (fast paths)

```bash
# Rust CLI
cargo install --path crates/synx-cli
synx parse file.synx

# .NET — consume (when listed on nuget.org)
dotnet add package APERTURESyndicate.Synx
# https://www.nuget.org/packages/APERTURESyndicate.Synx

# .NET — develop in repo
cd parsers/dotnet && dotnet test

# Fuzz (nightly)
cd crates/synx-core && cargo +nightly fuzz run fuzz_parse -- -max_total_time=60

# Replay same bytes on C# (after: dotnet build tools/Synx.FuzzReplay)
dotnet run -c Release --project parsers/dotnet/tools/Synx.FuzzReplay -- path/to/corpus/*
```

**Vendor-style full check (Rust tests + .NET tests + corpus replay + optional benches):**

- Windows: `.\scripts\verify-release-quality.ps1`
- Unix: `bash scripts/verify-release-quality.sh`

---

## Quality & safety (why this is “not a bad option”)

1. **Conformance:** shared `tests/conformance/cases/*.synx` — Rust integration test + C# xUnit read the **same files**.
2. **Fuzzing:** `cargo-fuzz` on `synx-core` with **bounded allocations** (input cap, line cap, depth 128, block/list/include limits) — see [`fuzz/README.md`](../crates/synx-core/fuzz/README.md).
3. **Cross-parser replay:** `Synx.FuzzReplay` feeds valid-UTF8 artifacts through **Parse + ToJson** (exercises emit path).
4. **Security tests:** JS/ReDoS/prototype pollution coverage in `packages/synx-js/tests/`.
5. **Engine limits:** calc length, include depth, file size, nesting — see Rust `engine.rs` / parser docs in fuzz README.

---

## Benchmarks (summary)

| Stack | Fixture | See |
|-------|---------|-----|
| **Rust** | ~110-key config | [`benchmarks/README.md`](../benchmarks/README.md) — ~39 µs `parse` order of magnitude |
| **Node** | same | JSON vs SYNX vs YAML vs XML |
| **Python** | same | `json` vs `synx_native` vs PyYAML |

Exact numbers drift by CPU; always **re-run locally** for procurement (`benchmarks/rust/cargo bench`, `node bench_node.js`, etc.).

---

## AI, Claude, MCP

| Piece | Role |
|-------|------|
| [**synx-adapter**](../integrations/ai/synx-adapter/README.md) | Pack context as SYNX for prompts; XML wrapper; **anchor markers**; **Anthropic tool_result** helpers |
| [**long-context guide**](guides/long-context-synx.md) | Lost-in-the-middle mitigations with SYNX |
| [**claude.md**](claude.md) | Claude Desktop + MCP + grounding |
| [**anthropic-system-prompt.txt**](anthropic-system-prompt.txt) | Optional strict system prompt for models |
| [**anthropic-token-notes.md**](anthropic-token-notes.md) | Informal token / format samples |
| [**plugins-roadmap.md**](dev/plugins-roadmap.md) | Registry / plugins (**stub**) |
| [**synx-mcp**](../integrations/mcp/synx-mcp/) | `validate` / `parse` / `format` (+ optional FS tools) |
| [**synx-visualizer**](../integrations/artifacts/synx-visualizer/) | React snippet for Claude Artifacts |

---

## Editors & LSP

VS Code, Visual Studio, Sublime, Neovim, **tree-sitter** grammar — see [`integrations/README.md`](../integrations/README.md) and [`tree-sitter-synx/`](../tree-sitter-synx/).

---

## Version

Current format line in root **README**: **SYNX v3.6** (see [`CHANGELOG.md`](../CHANGELOG.md) for module-level history).

---

*This file is a navigational shortcut; normative syntax is in the specification.*
