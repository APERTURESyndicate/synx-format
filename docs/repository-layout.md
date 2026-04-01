# Repository layout



Single monorepo: **parsers & grammars**, **integrations** (extensions / LSP), **bindings**, **docs**, **tests**, **benchmarks**.



```

synx-format/

├── Cargo.toml                 # Rust workspace root

├── parsers/

│   ├── README.md              # Index: Rust core, TS, C#, tree-sitter

│   └── dotnet/                # C# library (project Synx.Core; NuGet ID APERTURESyndicate.Synx) + FuzzReplay

├── crates/

│   ├── synx-core/             # Parser, engine, stringify, diff, .synxb — canonical Rust

│   ├── synx-cli/              # CLI binary (`synx`)

│   └── synx-lsp/              # Language Server (`synx-lsp`) — LSP over stdio

├── bindings/

│   ├── c-header/              # C ABI + `synx.h` (Rust `synx-c`)

│   ├── cpp/                   # C++17 header `synx/synx.hpp` over `synx-c` (same engine as Rust)

│   ├── go/                    # Go cgo module over `synx-c` (JSON / []byte API)

│   ├── mojo/                  # Mojo wrappers → CPython `synx_native` (same engine as Rust)

│   ├── swift/                 # SwiftPM `Synx` + `CSynx` → `synx-c`

│   ├── kotlin/                # JVM: Gradle `synx-kotlin` + JNA → `synx-c`

│   ├── node/                  # N-API package for Node.js

│   ├── python/                # PyO3 / PyPI build

│   └── wasm/                  # WebAssembly build

├── integrations/

│   ├── README.md              # Editors, LSP, MCP, Claude

│   ├── mcp/synx-mcp/          # MCP server (Claude Desktop, agents)

│   ├── ai/synx-adapter/       # LangChain / LlamaIndex / JS — SYNX in prompts vs JSON

│   ├── vscode/synx-vscode/    # VS Code extension

│   ├── sublime-text/Synx/     # Sublime syntax + LSP docs

│   ├── neovim/synx.nvim/      # Neovim ftdetect

│   └── visualstudio/synx-visualstudio/  # Visual Studio extension

├── packages/

│   └── synx-js/               # Pure TypeScript parser + library (@aperturesyndicate/synx-format)

├── scripts/

│   ├── verify-release-quality.ps1  # Rust + .NET + FuzzReplay checks

│   └── verify-release-quality.sh

├── docs/

│   ├── README.md              # Index of documentation

│   └── SYNX_AT_A_GLANCE.md    # One-page repo map

│   ├── repository-layout.md   # This file

│   ├── guides/                # Long-form guides per locale (+ MCP Desktop, token samples)

│   ├── dev/                   # Contributor stubs (e.g. plugins roadmap — not normative)

│   └── spec/                  # Normative specification (EN, RU)

├── tests/

│   └── conformance/           # Canonical test cases (.synx + .expected.json) + runner

├── examples/                  # Sample .synx files and small demos

├── benchmarks/

│   ├── rust/                  # Criterion workspace member (`synx-bench`)

│   ├── llm-tests/             # LLM benchmark suite + GUIDE.md

│   ├── bench_node.js / bench_python.py / config.*

│   └── README.md

├── tree-sitter-synx/          # Tree-sitter grammar + highlights

├── .github/

│   ├── workflows/             # CI (bindings smoke, Python wheels, …)

│   └── actions/synx/          # Composite Action: validate .synx in CI

└── README.md                  # Entry point for contributors and users

```



## What is *not* committed



- `target/`, `node_modules/`, `dist/`, `.venv/` — build artifacts (see root `.gitignore`).

- Local VS / VS Code build outputs under `integrations/visualstudio/**/bin/` and `**/obj/`.

- `*.bat` — local Windows helpers; ignored by `.gitignore` where applicable.



## Naming



| Area | Crate / package name | Role |

|------|----------------------|------|

| Rust library | `synx-core` | Canonical parser + engine |

| Rust CLI | `synx-cli` (binary: `synx`) | `crates/synx-cli/` |

| Rust LSP | `synx-lsp` (binary: `synx-lsp`) | `crates/synx-lsp/` |

| Rust bench | `synx-bench` | `benchmarks/rust/` |

| npm (TS) | `@aperturesyndicate/synx-format` | `packages/synx-js/` |

| VS Code | publisher `APERTURESyndicate`, id `synx-vscode` | `integrations/vscode/synx-vscode/` |


