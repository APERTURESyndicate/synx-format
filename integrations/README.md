# Integrations — editors, IDEs, AI, apps

IDE extensions, language servers, and anything that **uses** SYNX in a host application (not the core parser library).

Monorepo map (parsers, fuzz, benchmarks, verification): [`../docs/SYNX_AT_A_GLANCE.md`](../docs/SYNX_AT_A_GLANCE.md).

| Path | Role |
|------|------|
| [`vscode/synx-vscode/`](vscode/synx-vscode/) | **Visual Studio Code** extension (syntax, diagnostics, navigation, formatter) |
| [`visualstudio/synx-visualstudio/`](visualstudio/synx-visualstudio/) | **Visual Studio** extension (C# / VS SDK) |
| [`sublime-text/Synx/`](sublime-text/Synx/) | **Sublime Text** — `synx.sublime-syntax` + LSP setup for `synx-lsp` |
| [`neovim/synx.nvim/`](neovim/synx.nvim/) | **Neovim** — `ftdetect` for `*.synx` (+ docs for treesitter + LSP) |
| [`ai/`](ai/) | **SYNX-Adapter** — LangChain / LlamaIndex / JS: контекст в SYNX вместо JSON в промптах ([`synx-adapter`](ai/synx-adapter/)) |
| [`mcp/`](mcp/) | **MCP** — [`synx-mcp`](mcp/synx-mcp/) for Claude Desktop and compatible agents |
| [`../crates/synx-lsp`](../crates/synx-lsp/) | **Language Server** (`synx-lsp`) — see [`README.md`](../crates/synx-lsp/README.md) |
| [`../docs/claude.md`](../docs/claude.md) | **Claude / Anthropic** — MCP config + grounding + prompt helpers |

One **LSP** binary covers diagnostics/completion/symbols in Neovim, Sublime (via LSP package), Helix, Zed, Emacs, JetBrains; **syntax** may use Tree-sitter (`tree-sitter-synx/`) or editor-specific grammars (VS Code, Sublime).

## Publishing (when you’re ready)

| Integration | Typical publishing |
|-------------|-------------------|
| **VS Code** | `vsce publish` (Azure PAT); Marketplace listing under publisher `APERTURESyndicate`. |
| **Visual Studio** | VSIX via Visual Studio Marketplace; sign with publisher account. |
| **Sublime Text** | [Package Control channel](https://packagecontrol.io/docs/submitting_a_package) or your own `repositories.json` + tags. |
| **Neovim** | Public Git repo + **Git tags**; users install with lazy/packer/`packadd`. |
| **MCP (npm)** | `npm publish` for `@aperturesyndicate/synx-format-mcp`; document `npx -y` in `docs/claude.md`. |
| **synx-lsp** | `cargo publish -p synx-lsp` on crates.io; users `cargo install synx-lsp`. |
