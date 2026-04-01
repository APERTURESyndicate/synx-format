# synx.nvim (minimal)

Sets **filetype** `synx` for `*.synx`. Combine with:

1. **Tree-sitter** — build `tree-sitter-synx` from the repo (`npx tree-sitter generate` in `tree-sitter-synx/`), then point `nvim-treesitter` at the parser (see [nvim-treesitter local parsers](https://github.com/nvim-treesitter/nvim-treesitter#adding-parsers)).
2. **LSP** — run [`synx-lsp`](../../../crates/synx-lsp/README.md) via `vim.lsp.start` or `nvim-lspconfig` custom config.

## lazy.nvim (example)

```lua
{
  dir = vim.fn.stdpath('config') .. '/../path/to/synx-format/integrations/neovim/synx.nvim',
  lazy = false,
  config = function()
    -- Tree-sitter + LSP are configured separately; this repo only ships ftdetect.
  end,
}
```

Or copy `ftdetect/synx.vim` into your own config’s `ftdetect/` folder.

## Publish

Neovim plugins are usually **Git tags** on a public repo (e.g. `v1.0.0`) and installed via lazy/path/rocks. No store — document the Git URL and tag in your README.
