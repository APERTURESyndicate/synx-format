# Changelog

All notable changes to this repository are documented in this file.

## [3.1.3] - 2026-03-08

### Added
- **Comment text formatting** (VSCode extension): Markdown-like formatting inside comments — `*italic*` (green), `**bold**` (purple), `***bold+italic***` (gold), `` `code` `` (orange with subtle background). Works in `#`, `//`, and `###` block comments.
- **Deployment example (Docker + Nginx + Redis)**: Added runnable stack example in `examples/docker-stack` with SYNX-driven config and generated Nginx upstream config.
- **CLI tool** (`synx`): New CLI with 4 commands — `synx convert` (export to JSON/YAML/TOML/.env), `synx validate` (strict-mode check for CI/CD), `synx watch` (live reload with `--exec` support), `synx schema` (extract constraints as JSON Schema). Installed globally via `npm install -g @aperturesyndicate/synx`.
- **Export formats** (JS/TS API): `Synx.toJSON()`, `Synx.toYAML()`, `Synx.toTOML()`, `Synx.toEnv()` — convert parsed SYNX config to standard formats without external dependencies.
- **File watcher** (JS/TS API): `Synx.watch(filePath, callback, options)` — monitors `.synx` files for changes and delivers hot-reloaded config via callback.
- **Schema export** (JS/TS API): `Synx.schema(text)` — extracts constraint annotations (`[required, min:N, max:N, type:T, enum:A|B, pattern:R]`) as a JSON Schema-compatible object.
- **Deployment guide** (all 6 language guides): Docker, Docker Compose, Nginx, Redis, PostgreSQL, K8s Secrets, Vault, Helm, Terraform, CI/CD validation — added to GUIDE.md and all translations (DE, ES, JA, RU, ZH).


### Changed
- **Syntax highlighting redesign** (VSCode extension): Improved TextMate grammar with semantic scopes for clarity. Parent nodes (with nesting) use `entity.name.section` (bright, bold) to highlight structural branches. Leaf nodes (with values) use `support.type.property-name` (calm, subtle) for actual properties. Markers now `keyword.control.marker.synx` (pink/red). Recursive depth coloring: level 0 `keyword.control` (pink), level 1 `entity.name.tag` (bright cyan), level 2 `entity.name.function` (yellow), level 3+ `variable.parameter` (light cyan). This creates visual hierarchy—structure jumps out, data stays quiet.
- **JS native engine parity**: Large-file native path now forwards `Synx.parse(..., options)` into `parseActive(text, options)`, so `env` and `basePath` behave the same as the pure-JS path.

### Fixed
- **Fail-fast production mode**: Added `strict` option to JS API (`Synx.parse/loadSync/load`) to throw when runtime marker resolution returns `INCLUDE_ERR`, `WATCH_ERR`, `CALC_ERR`, or `CONSTRAINT_ERR` strings.

## [3.1.2] - 2026-03-07

### Fixed
- **`:default` value truncation** (JS parser + VSCode extension): The marker regex `([\w:]+)` only allowed word characters and colons, truncating default values containing dots (IPs like `0.0.0.0`), hyphens (`dev-secret-key`), or operators (`>=`). Changed to `([^\s]+)` to capture the full marker chain up to the next whitespace. This also fixes `:version:>=:18.0` and `:clamp` markers with decimal bounds.
- **`:default` compound values** (all engines — Rust, JS, VSCode): When `:default:VALUE` contained colons (e.g. `0.0.0.0` split as `["0","0","0","0"]` after `:` split), only the first fragment was used as the fallback. Now joins all marker parts after `default` back with `:` to reconstruct the original value.
- **`(string)` type hint ignored by `:default`/`:env`** (Rust + JS engines): A key with `(string)` type hint like `host(string):env:default:0.0.0.0 HOST` would still auto-detect the default value as a number. Now respects the type hint and returns the raw string.
- **VSCode `:env` without `:default` fallback**: The VSCode parser's `:env` handler didn't check for a `:default` sibling marker when the environment variable was missing. Now correctly falls back to the default value.
- **VSCode false "duplicate key" diagnostics in lists**: Keys inside different list-of-objects items (e.g. `category`, `name`, `price` repeated in each `- item`) were flagged as duplicates because scope tracking didn't reset at list item boundaries. Now clears deeper indent scopes when a new list item is encountered.
- **`.gitignore` encoding**: File was UTF-16 (BOM `FF FE`) which git cannot read, causing all ignored files to appear as untracked. Re-encoded to UTF-8 without BOM.

### Added
- **Node native binding: `parseActive` options** (`bindings/node`): `parseActive(text, options?)` now accepts an optional options object with `env` (environment variable overrides) and `basePath` (for `:include` resolution), matching the JS package API.
- **`typeHint` in JS metadata**: The `SynxMeta` interface now includes an optional `typeHint` field, allowing engine markers to respect explicit type casts like `(string)`.
- **Nesting-level key coloring** (VSCode extension): Keys are now colored by indentation depth — each nesting level gets a distinct color (blue → teal → yellow → purple → orange → gold), with separate palettes for dark and light themes.
- **Block comments `###`** (all parsers — Rust, JS, VSCode): Multi-line comments using `###` fences. Everything between an opening `###` and closing `###` is ignored by the parser.
- **Comment text formatting** (VSCode extension): Markdown-like formatting inside comments — `*italic*` (green), `**bold**` (purple), `***bold+italic***` (gold), `` `code` `` (orange with subtle background). Works in `#`, `//`, and `###` block comments.

### Changed
- Added `*.bat` and `.claudeignore` to `.gitignore`.

## [3.1.0] - 2026-03-07

### Fixed
- **Critical**: Removed `/n` → newline replacement from JS parser (`parser.ts`) and Rust core parser (`parser.rs`). The code was corrupting any value containing the two-character sequence `/n` (e.g. URLs like `/newsletter`, `/nginx`, `/node`) in multiline blocks and list items by replacing it with an actual newline character. This behavior was not part of the SYNX spec.
- **Security**: Removed `SynxSecret.valueOf()` from the JS engine. `valueOf()` returned the real secret value, meaning secrets could leak in arithmetic contexts or coercion (e.g. `secretVal + 1`). Only `.reveal()` should expose the underlying value.
- **Dead code**: Removed `SECRET_TAG = Symbol('synx:secret')` from the JS engine. The symbol was set on every `SynxSecret` instance via `Object.defineProperty` but was never read anywhere.
- **Silent coercion**: Fixed `castType` in JS parser — `parseInt(raw, 10) || 0` was silently returning `0` for any invalid or falsy numeric input (e.g. `(int)0` returned `0` but so did `(int)abc`). Changed to explicit `isNaN` check.
- **`:watch` nested key lookup**: `extractFromFileContent` in the JS engine was doing a flat line-by-line scan for `.synx` source files, so `:watch:database.host` always returned `null` for nested keys. Now uses `parseData` + `deepGet` for correct dot-path resolution.
- **`.synx.lock` format in docs**: The English guide showed a JSON block as the lock file format, but the actual implementation writes plain `key value` text (one per line). Corrected in `_guides/GUIDE.md`.
- **`:calc` ordering**: Added inline comment clarifying that `:calc` expressions only see already-resolved numeric siblings — keys that appear later in file order and still hold unresolved marker values are not available. This was a silent failure with no prior indication.

### Changed
- `Synx.format()` canonical formatter added to JS (`packages/synx-js`) and Rust (`crates/synx-core`): sorts keys alphabetically at every level, normalizes indentation to 2 spaces, strips comments, adds blank lines between top-level blocks. Useful for deterministic git diffs and pre-commit hooks.
- All language guides (`_guides`) updated with `format()` usage examples and pre-commit hook script.

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
