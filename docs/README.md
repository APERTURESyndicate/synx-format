> Main SYNX site: https://synx.aperturesyndicate.com/

# SYNX documentation

| | Path |
|---|------|
| **One-page map (start here)** | [`SYNX_AT_A_GLANCE.md`](SYNX_AT_A_GLANCE.md) — layout, tools, AI, benchmarks, verification |
| **User guides** (EN, RU, ZH, ES, JA, DE) | [`guides/`](guides/) — start with [`guides/GUIDE.md`](guides/GUIDE.md) |
| **Normative language spec (SYNX 3.6, RFC-style)** | [`spec/SYNX-3.6-NORMATIVE.md`](spec/SYNX-3.6-NORMATIVE.md) — frozen interoperability baseline |
| **Normative language spec (SYNX 3.7, additive)** | [`spec/SYNX-3.7-NORMATIVE.md`](spec/SYNX-3.7-NORMATIVE.md) — adds `\|+`, the indent-preserving multiline opener; all of 3.6 carries through unchanged |
| **Normative dataset spec (SYNXL 1)** | [`spec/SYNXL-1-NORMATIVE.md`](spec/SYNXL-1-NORMATIVE.md) — `.synxl`, the record-stream format (JSONL/CSV counterpart); own version axis, embeds SYNX 3.7 for block fields |
| **Core freeze policy** | [`spec/CORE-FREEZE.md`](spec/CORE-FREEZE.md) |
| **Human-readable specification** (long guides; verify against normative doc) | [`spec/SPECIFICATION_EN.md`](spec/SPECIFICATION_EN.md) · [`spec/SPECIFICATION_RU.md`](spec/SPECIFICATION_RU.md) · [`spec/SPECIFICATION.md`](spec/SPECIFICATION.md) |
| **How the repo is laid out** | [`repository-layout.md`](repository-layout.md) · [`parsers/`](../parsers/README.md) · [`integrations/`](../integrations/README.md) |
| **C# / .NET (`APERTURESyndicate.Synx` on NuGet)** | [`../parsers/dotnet/README.md`](../parsers/dotnet/README.md) · [nuget.org/packages/APERTURESyndicate.Synx](https://www.nuget.org/packages/APERTURESyndicate.Synx) |
| **C++ over `synx-c`** | [`../bindings/cpp/README.md`](../bindings/cpp/README.md) — `synx/synx.hpp` + link `synx-c` |
| **Go over `synx-c`** | [`../bindings/go/README.md`](../bindings/go/README.md) — cgo (`CGO_ENABLED=1`) |
| **Mojo ↔ `synx_native`** | [`../bindings/mojo/README.md`](../bindings/mojo/README.md) — Python from Mojo |
| **Swift / `synx-c`** | [`../bindings/swift/README.md`](../bindings/swift/README.md) — SwiftPM + `CSynx` |
| **Kotlin/JVM / `synx-c`** | [`../bindings/kotlin/README.md`](../bindings/kotlin/README.md) — JNA **`SynxEngine`** |
| **LLM compatibility benchmarks** (long form) | [`../benchmarks/llm-tests/GUIDE.md`](../benchmarks/llm-tests/GUIDE.md) |
| **Conformance test suite (SYNX)** | [`../tests/conformance/`](../tests/conformance/) — 11 canonical `.synx` + `.expected.json` pairs (includes `!llm`) |
| **Conformance test suite (SYNXL)** | [`../tests/conformance-synxl/`](../tests/conformance-synxl/) — `.synxl` cases with `.expected.json`, `.expected.error`, `.expected.diagnostics` |
| **SYNXL from code** | Rust [`../crates/synx-core/src/synxl.rs`](../crates/synx-core/src/synxl.rs) · CLI [`../crates/synx-cli/src/synxl.rs`](../crates/synx-cli/src/synxl.rs) · TS [`../packages/synx-js/src/synxl.ts`](../packages/synx-js/src/synxl.ts) · Python [`../bindings/python/src/synxl.rs`](../bindings/python/src/synxl.rs); worked examples in [`guides/GUIDE.md`](guides/GUIDE.md#synxl--record-streams-synxl) |
| **SYNXL CLI** | `synx synxl parse` · `synx synxl validate` · `synx synxl convert` · `synx synxl split` |
| **CLI JSON Schema (bundled)** | `synx schema <file.synx>` — emit draft 2020-12 schema from `!active` `[constraints]` · `synx json-validate <instance.json> <schema.json>` · `synx validate <file.synx> --self-schema` or `--json-schema path.json` |
| **Tree-sitter grammar** | [`../tree-sitter-synx/`](../tree-sitter-synx/) — syntax highlighting for Neovim/Helix/Zed/Emacs |
| **Fuzzing** | [`../crates/synx-core/fuzz/`](../crates/synx-core/fuzz/) — `cargo-fuzz` targets |
| **Claude / Anthropic (MCP, prompts, tokens)** | [`claude.md`](claude.md) · [`anthropic-system-prompt.txt`](anthropic-system-prompt.txt) · [`anthropic-token-notes.md`](anthropic-token-notes.md) |
| **Contributor stubs (plugins TBD)** | [`dev/README.md`](dev/README.md) |
| **SYNX-Adapter (LLM prompts)** | [`../integrations/ai/synx-adapter/README.md`](../integrations/ai/synx-adapter/README.md) — LangChain / LlamaIndex / Node |

The project [`README.md`](../README.md) at the repository root is the product overview and quick reference.
