<p align="center">
  <img src="https://media.aperturesyndicate.com/asother/as/branding/png/aperturesyndicate.png" alt="APERTURESyndicate" width="360" />
</p>

<p align="center">
  <strong>Built for AI and humans by APERTURESyndicate.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-3.0.0-5a6eff" />
  <img src="https://img.shields.io/badge/license-MIT-blue" />
  <img src="https://img.shields.io/badge/format-SYNX%20v3.0-blueviolet" />
  <img src="https://img.shields.io/badge/IDE-Visual%20Studio%2017%2B-purple" />
</p>

---

## SYNX Language Service for Visual Studio

Full SYNX v3.0 language support for **Visual Studio 2022** (17.0+).

---

## Features

| Feature | Description |
|---|---|
| **Syntax Highlighting** | Keys, values, markers, constraints, comments, types, template placeholders, colors |
| **IntelliSense** | Autocomplete for 12 markers, 7 constraints, type casts, template keys |
| **Diagnostics** | Real-time validation: tabs, indentation, duplicate keys, unknown markers, broken refs |
| **Code Folding** | Collapsible regions for nested SYNX sections |
| **Formatting** | Normalize indentation (2 spaces), trim whitespace, fix tabs |
| **Color Preview** | Inline color detection for `#hex` values (3/4/6/8-digit) |
| **Inlay Hints** | Computed `:calc` results shown inline |
| **Convert** | SYNX → JSON and JSON → SYNX conversion commands |
| **Freeze** | Resolve all `!active` markers into a static `.synx` |

---

## Requirements

- Visual Studio 2022 (version 17.0 or later)
- .NET Framework 4.7.2

---

## Building

```bat
build-visualstudio.bat
```

Or open `SynxLanguageService.sln` in Visual Studio and build from the IDE.

The output `.vsix` will be in `SynxLanguageService\bin\Release\`.

---

## Installation

1. Build the `.vsix` (see above)
2. Double-click the `.vsix` file to install into Visual Studio
3. Restart Visual Studio
4. Open any `.synx` file — highlighting, IntelliSense, and diagnostics activate automatically

---

## Commands

| Command | Description |
|---|---|
| **SYNX: Convert to JSON** | Parse current `.synx` and output resolved JSON |
| **SYNX: Convert from JSON** | Convert current `.json` to `.synx` format |
| **SYNX: Freeze** | Resolve all active markers into static values with `!static` header |
| **SYNX: Format** | Normalize indentation and whitespace |

Access via **Tools → SYNX** menu.

---

## SYNX Format

SYNX is a modern data format — cleaner than JSON, simpler than YAML, with built-in logic via `!active` mode.

```synx
!active

app
  name MyApp
  version 3.0.0
  debug :env DEBUG false

server
  host :env HOST localhost
  port :calc base_port + 1000
  base_port 8000

theme
  primary #5a6eff
  background #1a1a2e
```

Learn more: [SYNX Specification](https://github.com/APERTURESyndicate/synx-format)

---

<p align="center">
  <sub>Built by <strong>APERTURESyndicate</strong></sub>
</p>
