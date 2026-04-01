# tree-sitter-synx

[Tree-sitter](https://tree-sitter.github.io/) grammar for the **SYNX** data format.

Queries: **`queries/highlights.scm`** (scopes), **`queries/folds.scm`** (folds, where supported).

Provides syntax highlighting for Neovim, Helix, Zed, Emacs, and — after acceptance into [GitHub Linguist](https://github.com/github-linguist/linguist) — on GitHub itself.

## Quick start

```bash
cd tree-sitter-synx
npm install
npx tree-sitter generate
npx tree-sitter test
```

## Highlight queries

See `queries/highlights.scm`. The grammar tags:

| Node | Highlight group |
|------|----------------|
| `directive` | `@keyword` |
| `comment` | `@comment` |
| `key_name` | `@variable` |
| `type_cast` | `@type` |
| `constraints` | `@attribute` |
| `marker` | `@function` |
| `value` | `@string` |

## Neovim

Add to your tree-sitter config:

```lua
require("nvim-treesitter.configs").setup {
  ensure_installed = { "synx" },
}
```

## Linguist PR (pending)

Once the grammar is stable and tested, a PR to [github-linguist/linguist](https://github.com/github-linguist/linguist) will register `.synx` as a recognized language on GitHub.

## License

MIT — APERTURESyndicate
