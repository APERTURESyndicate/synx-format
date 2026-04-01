# Changelog (extension folder)

VS Code–only release notes used to be maintained here in full; they duplicated the monorepo changelog.

**Source of truth:** [`CHANGELOG.md`](https://github.com/APERTURESyndicate/synx-format/blob/main/CHANGELOG.md) at the **repository root** (Rust core, CLI, JS, bindings, VS Code, etc.).

**Marketplace packages:** run **`build-vscode.bat`** from the monorepo root before `vsce package` — it copies the root `CHANGELOG.md` into this folder so the published extension includes the full history.

### VS Code extension highlights (see root changelog for detail)

| Version | Notes |
|---------|--------|
| **3.6.0** | `!tool` / `!schema` / `!llm` grammar + parser skip; completion for `!llm`. |
| **3.5.2** | `:prompt`, `:vision`, `:audio` in diagnostics and IntelliSense. |
