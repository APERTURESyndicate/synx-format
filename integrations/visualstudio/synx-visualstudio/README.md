<p align="center">
  <img src="https://media.aperturesyndicate.com/asother/as/branding/png/aperturesyndicate.png" alt="APERTURESyndicate" width="360" />
</p>

<p align="center">
  <strong>Built for AI and humans by APERTURESyndicate.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/visual-studio-marketplace/v/APERTURESyndicate.synx-vscode?label=version&color=5a6eff" />
  <img src="https://img.shields.io/visual-studio-marketplace/i/APERTURESyndicate.synx-vscode?color=5a6eff" />
  <img src="https://img.shields.io/badge/license-MIT-blue" />
  <img src="https://img.shields.io/badge/format-SYNX%20v3.6-blueviolet" />
</p>

## Frozen reference (3.6.0)

As of **April 2026**, **SYNX 3.6.0** is **frozen**: the normative definition is [`docs/spec/SYNX-3.6-NORMATIVE.md`](docs/spec/SYNX-3.6-NORMATIVE.md), and the reference implementation is **`synx-core` 3.6.x** checked by [`tests/conformance/`](tests/conformance/). **PATCH** releases may only restore that contract (bugs, spec alignment); new surface syntax stays **additive** until a new normative version (for example 3.7). Full policy: [`docs/spec/CORE-FREEZE.md`](docs/spec/CORE-FREEZE.md).

---

## Documentation and repo map

- **[docs/SYNX_AT_A_GLANCE.md](docs/SYNX_AT_A_GLANCE.md)** — single-page overview: paths, parsers, fuzz/C#, AI/Claude, benchmarks, verification scripts  
- **[docs/README.md](docs/README.md)** — index of guides, specification, and benchmark docs  
- **[docs/repository-layout.md](docs/repository-layout.md)** — full tree (parsers, integrations, bindings)  
- **[parsers/README.md](parsers/README.md)** — parser implementations (Rust, TS, C#, C++, Go cgo, Swift, Kotlin/JVM, Mojo↔Python, tree-sitter)  
- **[integrations/README.md](integrations/README.md)** — VS Code, Visual Studio, Sublime, Neovim, MCP, LSP  
- **[docs/claude.md](docs/claude.md)** — Claude Desktop (MCP), Anthropic prompt helpers, tokenizer notes  
- **[docs/dev/plugins-roadmap.md](docs/dev/plugins-roadmap.md)** — planned registry / plugins (**stub only**; not shipped in the engine yet)  

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

This extension provides complete SYNX v3.6 language support for Visual Studio Code:

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

## CLI (Rust)

A single native binary for all platforms, built on `synx-core`:

```bash
# Install from source
cargo install --path crates/synx-cli

# Parse → JSON
synx parse config.synx

# Validate (exit 1 on errors)
synx validate config.synx --strict

# Convert JSON → SYNX
synx convert data.json --format synx

# Parse !tool call
synx tool call.synx

# Compile / decompile binary .synxb
synx compile config.synx
synx decompile config.synxb

# Structural diff
synx diff old.synx new.synx

# Query by dot-path (supports array indices)
synx query server.host config.synx

# Canonical formatting
synx format config.synx --write
```

---

## Language Server (LSP)

`synx-lsp` is a standalone Language Server that speaks [LSP](https://microsoft.github.io/language-server-protocol/) over stdio, built on `tower-lsp-server` + `synx-core`. One binary serves **every editor**:

| Editor | Setup |
|--------|-------|
| **Neovim** | `vim.lsp.start({ cmd = { "synx-lsp" }, filetypes = { "synx" } })` |
| **Helix** | Add to `languages.toml`: `[language-server.synx-lsp] command = "synx-lsp"` |
| **Zed** | Settings → Language Servers → add `synx-lsp` binary path |
| **Emacs** | `(lsp-register-client (make-lsp-client :new-connection (lsp-stdio-connection '("synx-lsp"))))` |
| **JetBrains** | Settings → Languages & Frameworks → LSP → add `synx-lsp` |

Capabilities: **real-time diagnostics** (15 checks), **completion** (markers, constraints, directives), **document symbols** (outline tree).

```bash
cargo install --path crates/synx-lsp
```

Full editor matrix: [`crates/synx-lsp/README.md`](crates/synx-lsp/README.md).

---

## Claude & MCP

Use **[`docs/claude.md`](docs/claude.md)** to connect **Claude Desktop** (or any MCP client) to **`integrations/mcp/synx-mcp`** — validate / parse / format `.synx` from the agent without guessing syntax.

---

## GitHub Action

Validate `.synx` files in CI:

```yaml
- uses: ./.github/actions/synx
  with:
    files: 'config/**/*.synx'
    strict: true
```

See [`.github/actions/synx/action.yml`](.github/actions/synx/action.yml) for all inputs.

---

## Tree-sitter

Syntax highlighting for Neovim, Helix, Zed, Emacs — and future GitHub code rendering:

```bash
cd tree-sitter-synx
npm install && npx tree-sitter generate
```

See [`tree-sitter-synx/README.md`](tree-sitter-synx/README.md). Queries live in `tree-sitter-synx/queries/` (`highlights.scm`, `folds.scm`).

---

## Fuzzing

Three `cargo-fuzz` targets exercise the parser, binary codec, and formatter:

```bash
cd crates/synx-core
cargo +nightly fuzz run fuzz_parse -- -max_total_time=60
```

In practice we’ve run `fuzz_parse` at scale (≈ **50M executions** across a large corpus); the parser/engine held up after fixing a single root-cause formatting panic.

See [`crates/synx-core/fuzz/README.md`](crates/synx-core/fuzz/README.md) and the checked-in coverage report at `crates/synx-core/fuzz/coverage/fuzz_parse/html/`.

---

## Architecture (VS Code extension)

Source: [`integrations/vscode/synx-vscode/`](integrations/vscode/synx-vscode/). The extension is **zero-dependency** — no external runtime, no native modules. Everything runs as pure TypeScript inside VS Code:

```
integrations/vscode/synx-vscode/src/
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

SYNX v3.6 uses a unified Rust core with native bindings. Real benchmark results on a 110-key config (2.5 KB):

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

## LLM SYNX Format Compatibility

How well different LLM models understand and work with SYNX format:

### Parsing & Generation Tests

We benchmark how well LLMs can:
- **Parse**: Read SYNX format and convert to JSON
- **Generate**: Create SYNX from English descriptions

Test corpus now has **250 total cases**:
- **125 parsing tests** (`SYNX -> JSON`)
- **125 generation tests** (`Description -> SYNX`)

What is inside the test texts:
- Parsing texts include simple key-value pairs, nested blocks (2-4 levels), arrays, mixed scalar types, null values, comments (`//`, `/* */`), strings with spaces, and configuration-like documents (service/database/deployment shapes).
- Generation prompts include practical tasks: app/service configs, ports, replicas, regions, booleans, arrays of features, and nested objects with explicit required fields.
- Many cases are near-duplicates with controlled value changes (names, numbers, ports, regions) to test consistency instead of single-shot luck.
- Expected outputs are checked structurally: exact JSON equality for parsing tests and required token/key presence for generation tests.

Example compatibility snapshot (illustrative):

```
gemini-2.0-flash
→ Parsing      ████████████████████  100.0% (125/125)
→ Generation   ████████████████████  100.0% (125/125)

claude-opus
  Parsing      ███████████████████░   96.0% (120/125)
  Generation   ██████████████████░░   88.0% (110/125)

claude-sonnet
  Parsing      ██████████████████░░   90.4% (113/125)
  Generation   ████████████████████  100.0% (125/125)

gemini-1.5-pro
  Parsing      ███████████████████░   96.0% (120/125)
  Generation   ██████████████████░░   88.0% (110/125)

gpt-4o
  Parsing      ██████████████████░░   90.4% (113/125)
  Generation   ██████████████████░░   88.0% (110/125)

claude-haiku-4-5
  Parsing      ████████████████░░░░   80.0% (100/125)
  Generation   ███████████████░░░░░   76.0% (95/125)
```

### Failed Test Analysis (Typical LLM Errors)

Analysis of failed cases (usually the remaining 4-12%) shows that errors are mostly caused by cross-format habits from YAML/JSON/TOML, not by SYNX complexity itself.

1. **Syntactic Interference**
Problem: the model hallucinates `:` after keys and adds unnecessary quotes in YAML/JSON style.
Example: `server host localhost` becomes `server: host: "localhost"`.

2. **Indentation Flattening**
Problem: nested SYNX blocks are flattened into one level, which breaks the structure.
Example: `database -> connection -> port` is emitted as sibling top-level keys.

3. **Array Shape Drift**
Problem: arrays are rewritten using another format (`- item`, JSON-like lists with quotes/commas, or mixed syntax).

4. **Type Coercion Bias**
Problem: `true`, `42`, `3.14`, and `~` are sometimes interpreted as strings instead of bool/number/null depending on prompt wording.

5. **Marker/Template Normalization**
Problem: SYNX-specific constructs are "normalized" into familiar syntax and lose their intended semantics.

6. **Over-Helpful Rewriting**
Problem: the model adds wrappers, comments, and readability edits that fail strict structural validation.

### Run Your Own Benchmarks

Test any LLM against SYNX format. See [benchmarks/llm-tests/README.md](benchmarks/llm-tests/README.md) for details:

```bash
cd benchmarks/llm-tests
pip install -r requirements.txt

# Set your API keys (one time setup)
export ANTHROPIC_API_KEY=your_key
export GOOGLE_API_KEY=your_key
export OPENAI_API_KEY=your_key

# Run full benchmark suite
python llm_benchmark.py

# Format and pretty-print results
python format_results.py llm_results.json
```

**See [benchmarks/llm-tests/GUIDE.md](benchmarks/llm-tests/GUIDE.md) for advanced options and detailed results interpretation.**

## Install (v3.6.0)

One-line installs (published names):

```bash
npm install @aperturesyndicate/synx-format
pip install synx-format
cargo add synx-core    # or: cargo add synx-format
cargo install synx-cli --locked   # CLI binary: synx

# C# / .NET 8 — NuGet (package ID is not Synx.Core; that ID is taken on nuget.org)
dotnet add package APERTURESyndicate.Synx
# Browse: https://www.nuget.org/packages/APERTURESyndicate.Synx
```

Until **`APERTURESyndicate.Synx`** appears on nuget.org, consume the library from this repo: `dotnet add reference parsers/dotnet/src/Synx.Core/Synx.Core.csproj` (or run **`.\publish-csharp.bat`** without `NUGET_API_KEY` and add the `.nupkg` under `artifacts/nuget` as a local feed). Details: [`parsers/dotnet/README.md`](parsers/dotnet/README.md).

### Maintainer: publish C# to NuGet

PowerShell: **`.\publish-csharp.bat`** with **`$env:NUGET_API_KEY`** set to the key string only (**no** `<` `>` around it). Clears stale `*.nupkg` in `artifacts/nuget` before pack; pushes only **`APERTURESyndicate.Synx.*.nupkg`**.

### Ready to `git push` or ship?

- Run quality checks when you change core behavior: [`scripts/verify-release-quality.ps1`](scripts/verify-release-quality.ps1) / [`scripts/verify-release-quality.sh`](scripts/verify-release-quality.sh) as appropriate.
- Do **not** commit secrets (`NUGET_API_KEY`, PyPI tokens); `artifacts/` is gitignored.
- For a new **NuGet** version, bump **`Version`** in [`parsers/dotnet/src/Synx.Core/Synx.Core.csproj`](parsers/dotnet/src/Synx.Core/Synx.Core.csproj), then **`.\publish-csharp.bat`**.

## Binding API Parity (v3.6.0)

Unified API surface across runtimes:

| Binding | `parse` | `parse_active` | `parse_tool` | `stringify` | `format` | `compile` | `decompile` | `diff` | Notes |
|---|---|---|---|---|---|---|---|---|---|
| Rust core (`synx-core`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | Full options support via `Options` |
| **CLI (`synx`)** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | `synx parse`, `synx diff`, `synx query`, … |
| JavaScript (npm `@aperturesyndicate/synx-format`) | ✅ | ✅ | — | ✅ | ✅ | — | — | ✅ | Pure TypeScript implementation |
| Python (`synx_native`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | `diff(a, b)` / `diff_json(text_a, text_b)` |
| Node native (`bindings/node`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | `diff(a, b)` / `diffJson(text_a, text_b)` |
| WebAssembly (`bindings/wasm`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | `diff(text_a, text_b)` → JSON |
| C FFI (`bindings/c-header`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | `synx_diff(a, b)` → JSON |
| C++ (`bindings/cpp` + same `synx-c` lib) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | Header `synx/synx.hpp` wraps §C FFI |
| Go (`bindings/go`, cgo + `synx-c`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | JSON/string API; see `bindings/go/README` (Windows: `CGO_LDFLAGS` + `PATH`) |
| Mojo (`bindings/mojo` + CPython `synx_native`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | [`Python.import_module`](https://docs.modular.com/mojo/manual/python/python-from-mojo); pip `synx-format`; `.synxb` via hex helpers |
| Swift (`bindings/swift` + `synx-c`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | SwiftPM `SynxEngine`; link `libsynx_c`; see `bindings/swift/README` |
| Kotlin/JVM (`bindings/kotlin`, JNA + `synx-c`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | `SynxEngine`; `SYNX_LIB_DIR`; see `bindings/kotlin/README` |
| C# (NuGet **`APERTURESyndicate.Synx`**, `parsers/dotnet`) | ✅ | ✅ | ✅ | ✅ (`ToJson`) | — | — | — | — | Managed .NET 8 library; `.synxb` / `diff` not in C# yet; not `synx-c` FFI |

Behavior notes:

- Browser WASM runs without host filesystem/env integration by default.
- C FFI, C++, Go (cgo), Swift (C interop), Kotlin/JVM (JNA), and WASM `stringify` use JSON strings at the boundary for stable cross-language interop.
- This table documents API compatibility only; it does not change parser performance characteristics.

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
# Lists
inventory
  - Sword
  - Shield

# Type casting
zip_code(string) 90210
# Multiline text
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
prompt_block:prompt:AppConfig
  app_name "MyCoolApp"
  version "2.1.0"
banner:vision "sunset landscape, 16:9"
greeting:audio "Welcome to our app"
```

### Constraints (require `!active`)

```synx
!active

volume[min:1, max:100] 75
api_key[required]:env API_KEY
max_players[type:int] 16
country_code[pattern:^[A-Z]{2}$] US
version[readonly] 3.0.0
password[required, min:8, max:64, type:string] MyP@ssw0rd
```

### LLM Tool Use (`!tool`) ⚡NEW

```synx
!tool
web_search
  query latest Rust release
  lang en
  max_results 5
```

→ `{ "tool": "web_search", "params": { "query": "latest Rust release", "lang": "en", "max_results": 5 } }`

Combine with `!schema` for tool definitions, or with `!active` for dynamic parameters. See [GUIDE.md](docs/guides/GUIDE.md#-llm-tool-use-tool) for full documentation.

## 📖 Documentation / Guides

Complete SYNX guides with all 24 markers, benchmarks, code examples, and architecture:

| Language | Guide |
|---|---|
| 🇬🇧 **English** | [GUIDE.md](docs/guides/GUIDE.md) |
| 🇷🇺 **Russian** | [GUIDE_RU.md](docs/guides/GUIDE_RU.md) |
| 🇨🇳 **Chinese** | [GUIDE_ZH.md](docs/guides/GUIDE_ZH.md) |
| 🇪🇸 **Español** | [GUIDE_ES.md](docs/guides/GUIDE_ES.md) |
| 🇯🇵 **Japanese** | [GUIDE_JA.md](docs/guides/GUIDE_JA.md) |
| 🇩🇪 **Deutsch** | [GUIDE_DE.md](docs/guides/GUIDE_DE.md) |

## 🔒 Security

SYNX is designed to be **safe by default** — no code execution, no eval, no network calls from the parser.

### What SYNX does NOT do

| Risk | SYNX | YAML |
|---|---|---|
| Code execution from config | **No** — no `!!python/object`, no eval, no constructors | Yes — `!!python/object/apply` can execute arbitrary code |
| Network/HTTP calls | **No** — parser is offline-only | Depends on loader |
| Shell command injection | **No** — `:calc` uses a safe recursive-descent parser with whitelist operators (`+ - * / %`) | Depends on loader |

### Built-in protections (v3.5.0+)

| Protection | Description |
|---|---|
| **Path jail** | `:include`, `:import`, `:watch`, `:fallback` paths cannot escape the project's base directory. Absolute paths and `../` traversal are blocked. |
| **Include depth limit** | Nested includes are limited to 16 levels (configurable). Prevents infinite recursion. |
| **File size limit** | Included files > 10 MB are rejected. Prevents memory exhaustion. |
| **Calc expression limit** | Expressions longer than 4096 characters are rejected. |
| **Env isolation** | When `env` option is provided, only that map is used — no fallthrough to `process.env`. |

### Configuration

```typescript
// JS/TS
Synx.parse(text, { maxIncludeDepth: 32 }); // default: 16
```

```rust
// Rust
Options { max_include_depth: Some(32), ..Default::default() }
```

## Full Specification

- **[SPECIFICATION_EN.md (English)](https://github.com/kaiserrberg/synx-format/blob/main/docs/spec/SPECIFICATION_EN.md)**
- **[SPECIFICATION_RU.md (Russian)](https://github.com/kaiserrberg/synx-format/blob/main/docs/spec/SPECIFICATION_RU.md)**

How this repo is organized: [docs/repository-layout.md](docs/repository-layout.md).


## Links

- [GitHub Repository](https://github.com/kaiserrberg/synx-format)
- [npm — @aperturesyndicate/synx-format](https://www.npmjs.com/package/@aperturesyndicate/synx-format)
- [PyPI — synx-format](https://pypi.org/project/synx-format/)
- [crates.io — synx-core](https://crates.io/crates/synx-core)
- [APERTURESyndicate](https://aperturesyndicate.com)


<p align="center">
  MIT — © <a href="https://github.com/kaiserrberg">APERTURESyndicate</a>
</p>

---

<div align="center">
  <img src="https://media.aperturesyndicate.com/asother/as/branding/png/asp_128.png" width="128" height="128" />
  <p>Made by <strong>APERTURESyndicate Production</strong></p>
</div>

