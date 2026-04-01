# synx-lsp

Language Server Protocol implementation for **SYNX** (`.synx`). One binary gives **diagnostics**, **completion**, and **document symbols** in any LSP-capable editor.

Built with [`tower-lsp-server`](https://crates.io/crates/tower-lsp-server) + `synx-core`.

## Build

From the repo root:

```bash
cargo build --release -p synx-lsp
# binary: target/release/synx-lsp
```

Or install from path:

```bash
cargo install --path crates/synx-lsp
```

## Run

Communicates over **stdio** (standard LSP):

```bash
synx-lsp
```

The editor must spawn this process and connect stdin/stdout.

## Editor setup

### Neovim (`nvim-lspconfig`)

```lua
vim.lsp.start({
  name = 'synx-lsp',
  cmd = { 'synx-lsp' },
  root_dir = vim.fn.getcwd(),
  filetypes = { 'synx' },
})
```

Or use `vim.api.nvim_create_autocmd('FileType', { pattern = 'synx', callback = ... })` to start once per buffer.

Ensure `filetype` is `synx` (see `integrations/neovim/synx.nvim`).

### Helix

`~/.config/helix/languages.toml`:

```toml
[[language]]
name = "synx"
scope = "source.synx"
file-types = ["synx"]
language-servers = [ "synx-lsp" ]

[language-server.synx-lsp]
command = "synx-lsp"
```

### Zed

Settings → **Language Servers** → add custom server: command `synx-lsp`, args `[]`, languages include `SYNX` / `synx` as your Zed version expects.

### Emacs (`eglot` / `lsp-mode`)

Register a client with command `synx-lsp` and activation on `synx` major mode.

### JetBrains (2024+ LSP)

**Settings → Languages & Frameworks → Language Server** — add `synx-lsp` with file pattern `*.synx`.

### Visual Studio Code

The marketplace extension can stay **self-contained**, or you can switch it to **spawn `synx-lsp`** for parity with other editors (recommended long term). Cursor is VS Code–compatible: install the same extension **or** add an **LSP** block pointing at `synx-lsp`.

### Sublime Text

Install **LSP** (Package Control). Add a client in `LSP.sublime-settings`; see [`integrations/sublime-text/Synx/README.md`](../../integrations/sublime-text/Synx/README.md).

## Capabilities (summary)

- Diagnostics: tabs, odd indentation, duplicate keys, unknown markers/constraints/types, `!active` requirement hints.
- Completion: known markers, constraints, directives.
- Document symbols: outline tree.

## Version

Matches the workspace / crate version (currently **3.6.0**).
