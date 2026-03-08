# Changelog

All notable changes to this repository are documented in this file.

## Changes by Module

Quick reference of what was modified in recent versions:

| Version | Components Modified |
|---------|---|
| **3.3.0** | synx-core (multi-parent inherit, calc dot-path, i18n plural, quoted strings, :import alias), VSCode (diagnostics/completion/navigation sync), documentation |
| **3.2.3** | synx-core (global [] constraints), documentation, version sync |
| **3.2.2** | synx-core (type validation), documentation |
| **3.2.1** | VSCode (diagnostics, syntax), Python binding, Node.js binding, WASM binding, C FFI, synx-core (serde), CI/CD, documentation |
| **3.2.0** | JS/TS engine, Rust engine, VSCode (completion, parser, syntax), guides (6 languages), documentation |
| **3.1.3** | VSCode extension, JS/TS API, documentation (6 guides), CLI tool, deployment examples |
| **3.1.2** | JS parser, Rust parser, VSCode extension, Node.js binding (napi), all guides |
| **3.1.0** | JS/TS API (runtime manipulation), Rust engine, VSCode extension, all guides |

---

## [3.3.0] - 2026-03-08

### Added
- **Multi-parent `:inherit`** (Rust engine): A block can now inherit from multiple parents via `:inherit:_parent1:_parent2:_parent3`. Parents merge left-to-right (later parents override earlier ones), child fields override all. Enables mixin-style composition for templates.
- **`:calc` dot-path references** (Rust engine): Arithmetic expressions now support nested key references via dot-path syntax (e.g., `total:calc stats.base_hp * stats.multiplier`). Previously only flat root-level keys were resolved.
- **`:i18n` pluralization** (Rust engine): Added CLDR-based plural form selection via `:i18n:COUNT_FIELD` syntax. The language entry contains plural category keys (`one`, `few`, `many`, `other`), and the engine selects the correct form based on the count value. `{count}` placeholder is auto-replaced with the actual number. Supported languages: en, de, es, it, fr, pt, ru, uk, be, pl, cs, sk, ar, ja, zh, ko, vi, th.
- **Quoted string values** (Parser): Wrapping a value in double or single quotes preserves it as a literal string, bypassing auto-casting. `status "null"` → string `"null"` (not null), `enabled "true"` → string `"true"` (not boolean), `count "42"` → string `"42"` (not integer).
- **`:import` marker alias** (Rust engine): `:import` is now a recognized alias for `:include` (key-level file embedding). Recommended to use `:import` to avoid confusion with the `!include` directive (file-level interpolation).
- **Import comparison matrix** (Guide): Added a table in GUIDE.md comparing `!include` (directive) vs `:include`/`:import` (marker) — syntax, placement, behavior, and use cases.
- **7 new tests**: `test_multi_parent_inherit`, `test_calc_dot_path`, `test_i18n_plural_en`, `test_i18n_plural_en_one`, `test_i18n_plural_ru`, `test_quoted_null_preserved`, `test_unquoted_null_is_null`.

### Changed
- **`:inherit` engine rewrite**: `apply_inheritance()` now collects all markers after "inherit" as parent names instead of just one. Backward compatible — single-parent `:inherit:_parent` works unchanged.
- **`:calc` engine enhancement**: Variable substitution now includes a second pass for dot-path identifiers using `deep_get()` traversal after flat key substitution.
- **Guide updates**: Updated `:inherit`, `:calc`, `:i18n`, `:include` sections with new features, examples, and the import matrix table. Added quoted values documentation to Basic Syntax.
- **Version sync to `3.3.0`** across all manifests.

---

## [3.2.3] - 2026-03-08

### Added
- **Global `[]` constraint validation** (Rust engine, active mode): Constraints declared with square brackets now apply consistently across all matching field names in the resolved tree, including inherited fields from `:inherit` templates.
  - Supports global enforcement of `required`, `min`, `max`, `type`, and `enum`
  - Constraint rules are collected into a global registry and recursively applied after marker resolution
  - Violations are surfaced as `CONSTRAINT_ERR: ...` values for visibility in output and downstream checks
- **Constraint merge strategy for repeated field declarations**: When the same field is constrained in multiple places, strict merging is applied (`required`/`readonly` propagate, `min` picks higher bound, `max` picks lower bound).
- **Engine test coverage for global constraints**: Added tests for inherited range validation and required validation:
  - `test_constraint_validation_inherited_range`
  - `test_constraint_validation_required`
- **Guide update near type hints**: Added `Constraint Validation ([]) in Active Mode` section in `_guides/GUIDE.md` with examples for `[required, min:1, max:50000]`, `type`, and `enum`.

### Changed
- **Version sync to `3.2.3`** across core manifests and package manifests used by bindings/extensions.

---

## [3.2.2] - 2026-03-08

### Added
- **Global type validation** (Rust engine, active mode): When you define a field with an explicit type like `hp(int)` or `name(string)`, the engine now validates that **all uses of that field across the entire document match the declared type**. Once a type is registered (e.g., "hp is int"), any value later assigned to that field is checked against the registered type.
  - In `!active` mode, type registry is built from all `key(type)` declarations
  - All field values are validated recursively through the entire value tree
  - Type mismatches are replaced with `TYPE_ERR: 'field' expected TYPE but got ACTUAL` for visibility
  - Benefits: Ensures consistency across blocks (especially with `:inherit`), self-documenting code, early error detection
- **Type validation test coverage**: Added two tests (`test_type_validation`, `test_type_validation_error`) to the synx-core engine test suite to verify correct type matching and error reporting.
- **Type validation documentation** (GUIDE.md): Added "Type Validation (Active Mode)" section under "Type Casting" with examples of valid/invalid type usage and error handling.

---

## [3.2.1] - 2026-03-08

### Added
- **Python binding: `parse_active` now accepts options** — `env` (dict) and `base_path` (str) parameters for `:env` and `:include` marker resolution. Previously `parse_active` used only defaults.
- **Python binding: `stringify`** — converts a Python dict/list back to SYNX format text.
- **Python binding: `format`** — reformats SYNX text into canonical form (sorted keys, normalized indentation).
- **Node.js binding: `stringify`** — converts a JS object back to SYNX format text.
- **Node.js binding: `format`** — reformats SYNX text into canonical form.
- **WASM binding: `parse_object` / `parse_active_object`** — returns JS objects directly via `serde_wasm_bindgen`, eliminating the need for `JSON.parse()` on the consumer side.
- **WASM binding: `stringify`** — converts JSON string to SYNX format text.
- **WASM binding: `format`** — reformats SYNX text into canonical form.
- **C FFI binding: `synx_stringify`** — converts JSON string to SYNX format text. Caller must free with `synx_free`.
- **C FFI binding: `synx_format`** — reformats SYNX text into canonical form. Caller must free with `synx_free`.
- **`serde` feature on `synx-core`** — optional `Serialize`/`Deserialize` derives on `Value` enum (used by WASM and C FFI bindings for JSON round-tripping).
- **Binding API parity table in README** — documented function availability and behavior notes for Rust core, JS package, Python, Node native, WASM, and C FFI.
- **Bindings smoke tests** — lightweight smoke coverage for Python/C/Node/WASM binding surfaces (`parse`, `parse_active`, `stringify`, `format`) without adding runtime overhead to production code paths.
- **CI check matrix** — new GitHub Actions workflow `.github/workflows/bindings-smoke.yml` to run binding-level checks on each push/PR.
- **C header ownership docs** — `bindings/c-header/include/synx.h` now explicitly documents allocation/free contract and NULL-on-error behavior for FFI consumers.

### Fixed
- **VSCode `:inherit` validation** — diagnostics now check that the parent block key exists when using `:inherit:parent_key`. Previously, the error was only shown at parse time, not in the editor.
- **VSCode block comment `###` syntax highlighting** — entire content of block comments is now properly highlighted as comment text. Previously, the first word on a line inside the block could be highlighted as a key instead of comment text.

## [3.2.0] - 2026-03-09

### Added
- **`:ref` marker** (JS + Rust + VSCode): Value reference with marker chaining. Like `:alias` but feeds the resolved value into subsequent markers. Supports shorthand calc: `rate:ref:calc:*2 base_rate` resolves `base_rate` (50), then computes `50 * 2 = 100`.
- **`:inherit` marker** (JS + Rust + VSCode): Block-level field inheritance. Merges all fields from a parent block into the child, with child values taking priority. Use `_` prefix for private template blocks excluded from output: `_base_resource` defines defaults, `steel:inherit:_base_resource` inherits them.
- **`:i18n` marker** (JS + Rust + VSCode): Multilingual values with language selection. Nested keys are language codes (`en`, `ru`, `de`), selected via `options.lang`. Falls back to `en`, then first available value. Syntax: `title:i18n` with child keys per language.
- **Auto-`{}` interpolation** (JS + Rust): In `!active` mode, any string value containing `{key}` is automatically interpolated — no `:template` marker needed. Supports dot-path for nested access (`{server.host}`). The `:template` marker is kept as a recognized no-op for backward compatibility.
- **`!include` directive** (JS + Rust + VSCode): File-level directive `!include ./file.synx [alias]` imports another file's top-level keys for use in `{key:alias}` interpolation. Alias is auto-derived from filename if not provided. Supports `{key:alias}` for named includes and `{key:include}` shorthand when only one file is included.
- **Comment string highlighting** (VSCode extension): Double-quoted `"strings"` and single-quoted `'strings'` inside comments now have distinct colors — orange for `""`, light blue for `''`.

### Fixed
- **Block comment content highlighting** (VSCode extension): Content inside `###` block comments was not highlighted as comments — only the `###` delimiters were colored. Fixed TextMate grammar to apply comment scope to all content between fences.

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
