# Changelog

All notable changes to the `synx-vscode` extension are documented in this file.

## [3.2.0] - 2026-03-07

### Fixed
- Removed `iconThemes` contribution from `package.json`. The extension was prompting users to switch their entire file icon theme on install. The SYNX file icon now uses the `languages[].icon` API (overlay) instead, which works with any existing icon theme without prompting.
- VSCode parser: `resolveActive()` was a no-op. Replaced with `resolveWithNodes()` that correctly applies `:env`, `:default`, `:alias`, `:calc`, `:random`, and `:clamp` markers during preview and hover.
- VSCode parser: Added `#!mode:active` detection alongside `!active` for shebang-style mode declarations.
- VSCode diagnostics: `getParentPath()` was a stub always returning `''`. Implemented via `dotPath.lastIndexOf('.')` using the new `dotPath` field on `SynxNode`. This fixed false-positive "Key not defined" errors for `:alias` references inside nested scopes.
- VSCode diagnostics: `:alias` check now looks in both root scope and sibling scope (parent path + ref), eliminating false positives for nested aliases.
- Added `dotPath` field to all `SynxNode` construction sites in the parser (regular nodes, list-of-objects items, plain list items).

### Changed
- `safeCalc` variable substitution in VSCode parser no longer builds a `new RegExp()` per variable. Uses a char-by-char word-boundary replacement helper instead.

## [3.1.0] - 2026-03-06

### Added
- `!lock` directive completion.
- `!lock` syntax highlighting in TextMate grammar.
- Type-cast completion support for `random`, `random:int`, `random:float`, `random:bool`.
- Marker compatibility and access-method documentation updates in guides.

### Changed
- Completion snippets no longer insert noisy placeholder payloads by default.
- `!active` completion after typing `!` no longer creates `!!active`.
- Diagnostics now recognize random type-casts as valid.
- Parser regex in VSCode extension now supports type hints with `:` (for example `(random:int)`).

### Fixed
- VSCode parser now ignores `!lock` directive line instead of treating it as a key.

## [3.0.0] - Original

### Added
- Initial release of SYNX VSCode extension.
- Syntax highlighting for `.synx` files.
- IntelliSense for markers and constraints.
- Diagnostics, formatting support, and basic navigation helpers.
