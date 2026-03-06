<p align="center">
  <img src="https://aperturesyndicate.com/branding/aperturesyndicate.png" alt="APERTURESyndicate" width="360" />
</p>

<p align="center">
  <strong>Better than JSON. Cheaper than YAML. Built for AI and humans.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/visual-studio-marketplace/v/APERTURESyndicate.synx-vscode?label=version&color=5a6eff" />
  <img src="https://img.shields.io/visual-studio-marketplace/i/APERTURESyndicate.synx-vscode?color=5a6eff" />
  <img src="https://img.shields.io/badge/license-MIT-blue" />
  <img src="https://img.shields.io/badge/format-SYNX%20v3.0-blueviolet" />
</p>

---

## See it in action

### Writing data — clean and simple

Just **key**, **space**, **value**. No quotes, no commas, no braces:

<p align="center">
  <img src="https://aperturesyndicate.com/branding/gifs/synx/synx.gif" alt="Writing static SYNX" width="720" />
</p>

### `!active` Mode

Add `!active` on the first line and your config comes alive — with logic built right into the format:

<p align="center">
  <img src="https://aperturesyndicate.com/branding/gifs/synx/synx2.gif" alt="Writing active SYNX with markers" width="720" />
</p>

---

## Features

This extension provides complete SYNX v3.0 language support for Visual Studio Code:

| Feature | Description |
|---|---|
| **Syntax Highlighting** | Keys, values, markers, constraints, comments, types, template placeholders, colors |
| **IntelliSense** | Autocomplete for 12 markers, 7 constraints, type casts, template keys, alias keys |
| **Hover Info** | Documentation on markers, constraints, `!active`, key types and values |
| **Diagnostics** | Real-time validation: tabs, indentation, duplicate keys, unknown markers, broken refs |
| **Go to Definition** | Ctrl+Click on `:alias`, `:template {ref}`, `:calc` variable names, `:include` file paths |
| **Find References** | Find all usages of any key across `:alias`, `:template`, `:calc` |
| **Document Outline** | Full symbol tree in the Outline panel and breadcrumbs |
| **Formatting** | Normalize indentation (2 spaces), trim whitespace, fix tabs |
| **Color Preview** | Inline color swatches for `#hex` values (3/4/6/8-digit) |
| **Inlay Hints** | Computed `:calc` results shown inline as `= 500` |
| **Live Preview** | Side panel with real-time parsed JSON output |
| **Convert** | SYNX → JSON and JSON → SYNX conversion commands |
| **Freeze** | Resolve all `!active` markers into a static `.synx` |
| **Context Menus** | Right-click on `.synx` / `.json` files in Explorer or Editor |

---

## Commands

| Command | Shortcut | Description |
|---|---|---|
| **SYNX: Convert to JSON** | `Ctrl+Shift+P` → type | Parse `.synx` → save `.json` alongside it |
| **SYNX: Convert JSON → SYNX** | `Ctrl+Shift+P` → type | Parse `.json` → save `.synx` alongside it |
| **SYNX: Freeze** | `Ctrl+Shift+P` → type | Resolve all markers → save `.static.synx` |
| **SYNX: Preview** | `Ctrl+Shift+P` → type | Open live side panel with parsed JSON |

All commands also available via **right-click context menu** on `.synx` and `.json` files.

---

## Architecture

The extension is **zero-dependency** — no external runtime, no native modules. Everything runs as pure TypeScript inside VS Code:

```
src/
├── extension.ts      # Entry point — registers all providers
├── parser.ts         # AST-like parser with position info (SynxNode, ParsedDoc)
├── diagnostics.ts    # 15 diagnostic checks with severity levels
├── completion.ts     # IntelliSense (12 markers, 7 constraints, types, hover)
├── navigation.ts     # Document symbols, Go to Definition, Find References
├── formatter.ts      # Formatting provider (2-space indent, trim)
├── commands.ts       # Convert, Freeze, Preview commands
├── colors.ts         # Color provider (#hex inline swatches)
└── inlay-hints.ts    # Inlay hints for :calc results
```

---

## Diagnostics

The extension validates your `.synx` files in real time:

| Check | Severity | Description |
|---|---|---|
| Tab characters | Error | SYNX uses spaces, not tabs |
| Odd indentation | Warning | Indentation should be a multiple of 2 |
| Invalid key start | Error | Keys cannot start with `-`, `#`, `/`, `!` |
| Duplicate keys | Warning | Same key at the same indent level |
| Unknown type cast | Error | Only `int`, `float`, `bool`, `string` allowed |
| Unknown marker | Warning | Not one of the 12 known markers |
| Markers without `!active` | Info | Markers only work in active mode |
| `:alias` broken ref | Error | Referenced key doesn't exist |
| `:calc` unknown var | Warning | Variable in expression not defined |
| `:include` file missing | Error | Included file not found |
| `:template` missing key | Warning | `{placeholder}` key not found |
| Constraints without `!active` | Info | Constraints only work in active mode |
| Unknown constraint | Warning | Not one of the 7 known constraints |
| `min`/`max` non-numeric | Error | Min/max values must be numbers |
| `enum` invalid value | Error | Value not in allowed list |

---

## Performance

SYNX v3.0 uses a unified Rust core with native bindings. Real benchmark results on a 110-key config (2.5 KB):

### Rust (criterion, direct)

| Benchmark | Time |
|---|---|
| `Synx::parse` (110 keys) | **~39 µs** |
| `parse_to_json` (110 keys) | **~42 µs** |
| `Synx::parse` (4 keys) | **~1.2 µs** |

### Node.js (50K iterations)

| Parser | µs/parse |
|---|---|
| `JSON.parse` (3.3 KB) | 6.08 µs |
| `synx-js` pure TS | 39.20 µs |
| `js-yaml` (2.5 KB) | 82.85 µs |
| `synx-native parseToJson` | 86.29 µs |
| `synx-native parse` | 186.84 µs |

### Python (10K iterations)

| Parser | µs/parse |
|---|---|
| `json.loads` (3.3 KB) | 13.04 µs |
| `synx_native.parse` (2.5 KB) | 55.44 µs |
| `yaml.safe_load` (2.5 KB) | 3,698 µs |

> SYNX parses **67× faster** than YAML in Python. In Node.js, the pure TS parser matches Rust direct speed at ~39 µs.

---

## Quick SYNX Syntax Reference

### Basic (always works)

```synx
# Key-value pairs (first space separates key from value)
name John
age 25
phrase I love programming!

# Nesting (2-space indent)
server
  host 0.0.0.0
  port 8080

# Lists
inventory
  - Sword
  - Shield

# Type casting
zip_code(string) 90210
id(int) 007

# Multiline text
description |
  This is a long description
  that spans multiple lines.

# Comments
# hash comment
// slash comment
```

### Markers (require `!active`)

```synx
!active

port:env PORT
port:env:default:8080 PORT
boss_hp:calc base_hp * 5
greeting:random
  - Hello!
  - Welcome!
loot:random 70 20 10
  - common
  - rare
  - legendary
support_email:alias admin_email
api_key:secret sk-1234567890
tags:unique
  - action
  - rpg
  - action
database:include ./db.synx
theme:default dark
greeting:template Hello, {first_name} {last_name}!
colors:split red, green, blue
csv:join
  - a
  - b
  - c
currency:geo
  - US USD
  - EU EUR
```

### Constraints (require `!active`)

```synx
!active

app_name[min:3, max:30] TotalWario
volume[min:1, max:100] 75
api_key[required]:env API_KEY
max_players[type:int] 16
theme[enum:light|dark|auto] dark
country_code[pattern:^[A-Z]{2}$] US
version[readonly] 3.0.0
password[required, min:8, max:64, type:string] MyP@ssw0rd
```

---

## Full Specification

- **Specifications**
- **[SPECIFICATION_EN.md (English)](https://github.com/kaiserrberg/synx-format/blob/main/packages/synx-js/SPECIFICATION_EN.md)**
- **[SPECIFICATION_RU.md (Русский)](https://github.com/kaiserrberg/synx-format/blob/main/packages/synx-js/SPECIFICATION_RU.md)**
- **Guides (All Languages)**
- **[GUIDE.md (English)](https://github.com/kaiserrberg/synx-format/blob/main/_guides/GUIDE.md)**
- **[GUIDE_RU.md (Русский)](https://github.com/kaiserrberg/synx-format/blob/main/_guides/GUIDE_RU.md)**
- **[GUIDE_ZH.md (中文)](https://github.com/kaiserrberg/synx-format/blob/main/_guides/GUIDE_ZH.md)**
- **[GUIDE_ES.md (Español)](https://github.com/kaiserrberg/synx-format/blob/main/_guides/GUIDE_ES.md)**
- **[GUIDE_JA.md (日本語)](https://github.com/kaiserrberg/synx-format/blob/main/_guides/GUIDE_JA.md)**
- **[GUIDE_DE.md (Deutsch)](https://github.com/kaiserrberg/synx-format/blob/main/_guides/GUIDE_DE.md)**

---

## Links

- [GitHub Repository](https://github.com/kaiserrberg/synx-format)
- [npm — @aperturesyndicate/synx](https://www.npmjs.com/package/@aperturesyndicate/synx)
- [PyPI — synx-format](https://pypi.org/project/synx-format/)
- [APERTURESyndicate](https://aperturesyndicate.com)

---

<p align="center">
  MIT — © <a href="https://github.com/kaiserrberg">APERTURESyndicate</a>
</p>

---

<div align="center">
  <img src="https://aperturesyndicate.com/branding/logos/asp_128.png" width="128" height="128" />
  <p>Made by <strong>APERTURESyndicate Production</strong></p>
</div>
