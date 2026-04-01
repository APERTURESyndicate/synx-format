# SYNX — VS Code extension (monorepo slice)

This folder is part of the **[synx-format](https://github.com/APERTURESyndicate/synx-format)** monorepo.

- **Full product README** (badges, CLI, LSP, bindings): see the repository **[`README.md`](https://github.com/APERTURESyndicate/synx-format/blob/main/README.md)** at the repo root.
- **Full changelog** (all components): **[`CHANGELOG.md`](https://github.com/APERTURESyndicate/synx-format/blob/main/CHANGELOG.md)** at the repo root.

**Before packaging a `.vsix` for the Marketplace**, run **`build-vscode.bat`** from the **monorepo root**. It copies the root `README.md` and `CHANGELOG.md` into this directory so `vsce package` ships the same documentation as the main project.

Extension-specific implementation lives under `src/`, `syntaxes/`, and `package.json`.
