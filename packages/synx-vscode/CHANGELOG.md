# Changelog

All notable changes to the `synx-vscode` extension are documented in this file.

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
