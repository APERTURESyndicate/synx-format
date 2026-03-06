# Changelog

All notable changes to this repository are documented in this file.

## [3.1.0] - 2026-03-06

### Added
- Type-cast random generation in parsers:
- `(random)` and `(random:int)` for integer values
- `(random:float)` for float values
- `(random:bool)` for boolean values
- Runtime config manipulation API in JS/TS:
- `Synx.get(obj, keyPath)`
- `Synx.set(obj, keyPath, value)`
- `Synx.add(obj, keyPath, item)`
- `Synx.remove(obj, keyPath, item?)`
- `Synx.isLocked(obj)`
- `!lock` directive support to protect parsed configs from external runtime mutation through the JS/TS API.
- Delimiter keyword support for `slash` in marker processing (`:split` / `:join`).
- Root spelling dictionary config (`cspell.json`) for SYNX-specific terms.

### Changed
- JS and Rust parser type-hint regex now supports `:` in cast names (for example `(random:int)`).
- VS Code extension completion behavior improved:
- marker snippets no longer inject noisy placeholders by default
- `!active` completion after `!` no longer produces `!!active`
- added `!lock` completion and random cast completions
- VS Code diagnostics updated to recognize random type-casts as valid.
- VS Code parser updated to ignore `!lock` directive line as a directive, not a key.
- VS Code syntax grammar updated to highlight `!lock`.
- VS Code extension package version set to `3.1.0`.

### Fixed
- Documentation and runtime behavior aligned for `:join:slash` by adding actual `slash` delimiter support in engines.
- Type diagnostics mismatch for random casts in VS Code extension.

### Documentation
- Guides updated in all supported languages (`_guides`):
- random cast section
- lock mode section
- runtime manipulation examples
- marker compatibility section
- Python access-helper equivalents (`get_path` / `set_path` / `add_path` / `remove_path`) with note that native Python API currently exposes `parse`, `parse_active`, `parse_to_json`
- delimiter keyword lists synchronized in `split` and `join` sections
- Removed "view logo" button lines from GitHub guides while keeping GIF demos.
- VS Code README Full Specification section expanded with links to all language guides and specification files.
- Added extension-scoped changelog: `packages/synx-vscode/CHANGELOG.md`.

### Tooling and Release Scripts
- `publish-npm.bat` improved for safer execution:
- path auto-detection
- optional version bump argument
- better npm auth flow (`npm login` / `NPM_TOKEN`)
- explicit `call npm ...` usage on Windows
- clearer error output
- Added package-local publish helper: `packages/synx-js/publish-npm.bat`.

## [3.0.0] - Original

### Added
- Initial public release of SYNX format and parser/runtime ecosystem.
- Core marker system, constraints, and `!active` processing pipeline.
- Rust core crate and bindings/packages for JS/TS, Python, and VS Code tooling.

---

<div align="center">
  <img src="https://aperturesyndicate.com/branding/logos/asp_128.png" width="128" height="128" />
  <p>Made by <strong>APERTURESyndicate Production</strong></p>
</div>
