<p align="center">
  <a href="https://aperturesyndicate.com/branding/aperturesyndicate.png" target="_blank">
    <img src="https://aperturesyndicate.com/branding/aperturesyndicate.png" alt="APERTURESyndicate" width="400" />
  </a>
</p>

> **🔗 [View logo →](https://aperturesyndicate.com/branding/aperturesyndicate.png)**

<h1 align="center">SYNX v3.0 — The Complete Guide</h1>

<p align="center">
  <strong>Better than JSON. Cheaper than YAML. Built for AI and humans.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-3.0.0-5a6eff?style=for-the-badge" />
  <img src="https://img.shields.io/badge/license-MIT-blue?style=for-the-badge" />
  <img src="https://img.shields.io/badge/format-SYNX-blueviolet?style=for-the-badge" />
  <img src="https://img.shields.io/badge/written_in-Rust-orange?style=for-the-badge" />
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/@aperturesyndicate/synx">npm</a> ·
  <a href="https://pypi.org/project/synx-format/">PyPI</a> ·
  <a href="https://crates.io/crates/synx-core">crates.io</a> ·
  <a href="https://marketplace.visualstudio.com/items?itemName=APERTURESyndicate.synx-vscode">VS Code</a> ·
  <a href="https://github.com/kaiserrberg/synx-format">GitHub</a>
</p>

---

## Table of Contents

- [Philosophy](#-philosophy)
- [See It in Action](#-see-it-in-action)
- [How It Works](#-how-it-works)
- [Security Model (v3.5.0+)](#-security-model-v350)
- [Performance & Benchmarks](#-performance--benchmarks)
- [Installation](#-installation)
- [Grammar Reference](#-grammar-reference)
  - [Basic Syntax](#basic-syntax)
  - [Nesting](#nesting)
  - [Lists](#lists)
  - [Type Casting](#type-casting)
  - [Random Value Generation](#random-value-generation)
  - [Multiline Text](#multiline-text)
  - [Comments](#comments)
- [Active Mode (`!active`)](#-active-mode-active)
- [Lock Mode (`!lock`)](#-lock-mode-lock)
- [Include Directive (`!include`)](#-include-directive-include)
- [Canonical Format (`format`)](#-canonical-format-format)
- [Markers — Full Reference](#-markers--full-reference)
  - [:env](#env--environment-variable)
  - [:default](#default--fallback-value)
  - [:calc](#calc--arithmetic-expression)
  - [:random](#random--random-selection)
  - [:alias](#alias--reference-another-key)
  - [:ref](#ref--reference-with-chaining)
  - [:inherit](#inherit--block-inheritance)
  - [:i18n](#i18n--multilingual-values)
  - [:secret](#secret--hidden-value)
  - [auto-{}](#auto---string-interpolation)
  - [:include / :import](#include--import--import-external-file)
  - [:unique](#unique--deduplicate-list)
  - [:split](#split--string-to-array)
  - [:join](#join--array-to-string)
  - [:geo](#geo--region-based-selection)
  - [:clamp](#clamp--clamp-number-to-range)
  - [:round](#round--round-to-n-decimals)
  - [:map](#map--lookup-table)
  - [:format](#format--number-formatting)
  - [:fallback](#fallback--file-path-fallback)
  - [:once](#once--generate-and-persist)
  - [:version](#version--semantic-version-compare)
  - [:watch](#watch--read-external-file)
  - [:spam](#spam--rate-limit-access)
- [Constraints](#-constraints)
- [Marker Chaining](#-marker-chaining)
- [Code Examples](#-code-examples)
  - [JavaScript / TypeScript](#javascript--typescript)
  - [Python](#python)
  - [Rust](#rust)
- [Editor Support](#-editor-support)
- [Architecture](#-architecture)
- [Specification](#-specification)
- [Links](#-links)

---

## 💡 Philosophy

Configuration is the backbone of every application. Yet the industry standard formats — **JSON** and **YAML** — were never designed for the job:

| Problem | JSON | YAML | SYNX |
|---|:---:|:---:|:---:|
| Requires quotes for strings/keys | ✓ | ✗ | ✗ |
| Trailing commas crash the parser | ✗ | — | ✓ |
| Whitespace-significant indentation | — | ✗ (dangerous) | ✓ (safe, 2-space) |
| Comments | ✗ | ✓ | ✓ |
| Environment variables | ✗ | ✗ | ✓ built-in |
| Computed values | ✗ | ✗ | ✓ built-in |
| AI token cost (110-key config) | ~3,300 chars | ~2,500 chars | **~2,000 chars** |
| Human readability | Low | Medium | **High** |

SYNX was built on three principles:

1. **Minimal syntax** — Key, space, value. That's it. No quotes, no commas, no braces, no colons.
2. **Active by design** — Configs aren't just data, they're logic. Environment variables, math, references, randomness, and validation — all built into the format itself.
3. **Token-efficient** — Every character counts when you're sending configs through LLMs. SYNX uses 30–40% fewer tokens than JSON for the same data.

> **SYNX is not a replacement for JSON. It's what JSON was supposed to be.**

---

## 🎬 See It in Action

### Writing data — clean and simple

Just **key**, **space**, **value**. No quotes, no commas, no braces:

<p align="center">
  <img src="https://aperturesyndicate.com/branding/gifs/synx/synx.gif" alt="Writing static SYNX — key space value" width="720" />
</p>

### `!active` mode — configs with logic

Add `!active` on the first line and your config comes alive — with functions built right into the format:

<p align="center">
  <a href="https://aperturesyndicate.com/branding/gifs/synx/synx2.gif" target="_blank">
    <img src="https://aperturesyndicate.com/branding/gifs/synx/synx2.gif" alt="Writing active SYNX with markers" width="720" />
  </a>
</p>

> **📺 [Watch demo →](https://aperturesyndicate.com/branding/gifs/synx/synx2.gif)**

---

## ⚙ How It Works

The SYNX pipeline has **two stages** — and this separation is key to its performance:

```
┌─────────────┐         ┌─────────────┐         ┌──────────────┐
│  .synx file │ ──────► │   PARSER    │ ──────► │    OUTPUT    │
│  (text)     │         │ (always)    │         │  (JS object) │
└─────────────┘         └──────┬──────┘         └──────────────┘
                               │
                          has !active?
                               │
                          ┌────▼────┐
                          │ ENGINE  │
                          │ (runs   │
                          │ markers)│
                          └─────────┘
```

### Stage 1 — Parser

The **parser** reads raw text and builds a key-value tree. It handles:
- Key-value pairs (first space separates key from value)
- Nesting (2-space indentation)
- Lists (`- item`)
- Type casting (`key(int) value`)
- Comments (`#` and `//`)
- Multiline text (`|`)

The parser records any markers (`:env`, `:calc`, etc.) as **metadata** attached to each key — but does **not** execute them. This means **adding new markers does not slow down parsing**.

### Stage 2 — Engine (only with `!active`)

If the file starts with `!active`, the **engine** walks the parsed tree and resolves each marker. Each marker handler runs only on keys that explicitly use it — so a 110-key config where 3 keys have markers will only execute 3 marker handlers.

**Files without `!active` never touch the engine.** The parser alone handles them and returns instantly.

### Auto-Engine Switching (Node.js)

In Node.js, the library automatically selects the optimal engine:

| File size | Engine | Why |
|---|---|---|
| < 5 KB | Pure TypeScript | Zero startup overhead, no native dependency |
| ≥ 5 KB | Native Rust (NAPI) | Faster on large files (compiled code) |

If the native Rust binding is not installed, it always falls back to pure TypeScript.

---

## Security Model (v3.5.0+)

SYNX keeps full marker functionality while adding runtime guards for file and expression based features.

- **Path jail for file markers**: `:include`, `:import`, `:watch`, `:fallback` are resolved inside `basePath` only. Absolute paths and path traversal (`../`) outside the base are rejected.
- **Depth guard for nested file operations**: include/watch recursion is limited to `16` by default (configurable).
  Rust option: `max_include_depth`
  JS option: `maxIncludeDepth`
- **File size guard**: files larger than `10 MB` are rejected before read.
- **Calc expression guard**: `:calc` expressions longer than `4096` chars are rejected.
- **Engine behavior**: parser still only records metadata; marker handlers execute only in `!active` mode.

Security note:
- SYNX does not execute arbitrary code from config data (no YAML-style object constructors, no `eval`).

---

## 📊 Performance & Benchmarks

All benchmarks are real, run on a standard 110-key SYNX config (2.5 KB):

### Rust (criterion, direct call)

| Benchmark | Time |
|---|---|
| `Synx::parse` (110 keys) | **~39 µs** |
| `parse_to_json` (110 keys) | **~42 µs** |
| `Synx::parse` (4 keys) | **~1.2 µs** |

### Node.js (50,000 iterations)

| Parser | µs/parse | vs JSON | vs YAML |
|---|---:|---:|---:|
| `JSON.parse` (3.3 KB JSON) | 6.08 µs | 1× | — |
| **`synx-js` pure TS** | **39.20 µs** | 6.4× | **2.1× faster** |
| `js-yaml` (2.5 KB YAML) | 82.85 µs | 13.6× | 1× |
| `synx-native parseToJson` | 86.29 µs | 14.2× | ~1× |
| `synx-native parse` | 186.84 µs | 30.7× | — |

### Python (10,000 iterations)

| Parser | µs/parse | vs YAML |
|---|---:|---:|
| `json.loads` (3.3 KB) | 13.04 µs | — |
| **`synx_native.parse`** | **55.44 µs** | **67× faster** |
| `yaml.safe_load` (2.5 KB) | 3,698 µs | 1× |

### Token Cost Comparison (110-key config, GPT-4 tokenizer)

| Format | Characters | Tokens | Cost @ $0.01/1K |
|---|---:|---:|---:|
| JSON | ~3,300 | ~980 | $0.0098 |
| YAML | ~2,500 | ~760 | $0.0076 |
| **SYNX** | **~2,000** | **~580** | **$0.0058** |

> SYNX saves **~40% on AI tokens** compared to JSON, and **~24%** compared to YAML.

---

## 📦 Installation

### Node.js / Browser

```bash
npm install @aperturesyndicate/synx
```

### Python

```bash
pip install synx-format
```

### Rust

```bash
cargo add synx-core
```

### VS Code Extension

Search **"SYNX"** in the Extensions panel, or:

```bash
code --install-extension APERTURESyndicate.synx-vscode
```

### Visual Studio 2022

Download the `.vsix` from [GitHub Releases](https://github.com/kaiserrberg/synx-format/releases) and double-click to install.

---

## 📝 Grammar Reference

### Basic Syntax

The fundamental rule: **key** `(space)` **value**.

The first space character separates the key from the value. Everything after the first space is the value — including additional spaces.

```synx
name John
age 25
phrase I love programming with SYNX!
empty_value
```

**Parsed result:**

```json
{
  "name": "John",
  "age": 25,
  "phrase": "I love programming with SYNX!",
  "empty_value": null
}
```

> Numbers, booleans (`true`/`false`), and `null` are auto-detected. Everything else is a string.

> **Quoted values**: To force a literal string and bypass auto-casting, wrap the value in double or single quotes:
> `status "null"` → `"null"` (string, not null), `enabled "true"` → `"true"` (string, not boolean), `count "42"` → `"42"` (string, not integer).

Parser type detection order (when no explicit `(type)` hint is used):

1. Exact `true`/`false` -> Bool
2. Exact `null` -> Null
3. Integer pattern -> Int
4. Decimal pattern -> Float
5. Otherwise -> String

---

### Nesting

Indentation creates hierarchy — **2 spaces** per level, always:

```synx
server
  host 0.0.0.0
  port 8080
  ssl
    enabled true
    cert /etc/ssl/cert.pem

database
  host localhost
  port 5432
  credentials
    user admin
    password secret123
```

```json
{
  "server": {
    "host": "0.0.0.0",
    "port": 8080,
    "ssl": {
      "enabled": true,
      "cert": "/etc/ssl/cert.pem"
    }
  },
  "database": {
    "host": "localhost",
    "port": 5432,
    "credentials": {
      "user": "admin",
      "password": "secret123"
    }
  }
}
```

---

### Lists

Lines starting with `- ` (dash + space) create arrays:

```synx
fruits
  - Apple
  - Banana
  - Cherry

matrix
  -
    - 1
    - 2
  -
    - 3
    - 4
```

```json
{
  "fruits": ["Apple", "Banana", "Cherry"],
  "matrix": [[1, 2], [3, 4]]
}
```

---

### Type Casting

Force a specific type with `(type)` after the key name:

```synx
zip_code(string) 90210
id(int) 007
ratio(float) 3
enabled(bool) 1
```

```json
{
  "zip_code": "90210",
  "id": 7,
  "ratio": 3.0,
  "enabled": true
}
```

Available casts: `int`, `float`, `bool`, `string`.

#### Type Validation (Active Mode)

In **active mode** (`!active`), types are not just hints — they're **globally enforced**. Once you define a field with a type like `hp(int)`, the system ensures all uses of that field across the entire document are integers:

```synx
!active

_base_unit
  hp(int) 1000
  speed(float) 2.5
  name(string) Basic Unit
  is_available(bool) true

infantry:inherit:_base_unit
  hp 800          # ✓ Valid: integer
  speed 3.0       # ✓ Valid: float
  name Infantry   # ✓ Valid: string

ranger:inherit:_base_unit
  hp 600          # ✓ Valid: integer
  # hp abc        # ✗ Would error: type mismatch (expected int, got string)
```

**Benefits:**
- **Consistency**: Ensures all uses of a field have the same type across blocks
- **Early error detection**: Type mismatches are caught during resolution
- **Self-documenting**: Type hints serve as inline documentation

If a type mismatch is detected, the value is replaced with a `TYPE_ERR` message:
```json
{
  "hp": "TYPE_ERR: 'hp' expected int but got string"
}
```

#### Constraint Validation (`[]`) in Active Mode

Square-bracket constraints are also enforced globally for matching field names. This includes `required`, `min`, `max`, `type`, and `enum`.

```synx
!active

_base_unit
  hp(int)[required, min:1, max:50000] 1000
  tier[type:string, enum:common|elite|boss] common

infantry:inherit:_base_unit
  hp 1200
  tier elite

tank:inherit:_base_unit
  hp 90000   # -> CONSTRAINT_ERR (exceeds max)
  tier rare  # -> CONSTRAINT_ERR (not in enum)
```

Notes:
- `required` fails on `null` or empty strings.
- `min` / `max` apply to numbers, and to string length for string values.
- `type:...` in `[]` works independently from `key(type)` and can be combined with it.

#### Random Value Generation

Generate random values at parse time using `(random)`:

```synx
pin(random) null
flag(random:bool) null
chance(random:float) null
dice(random:int) null
```

```json
{
  "pin": 1847362951,
  "flag": true,
  "chance": 0.7342,
  "dice": 982451653
}
```

Available random casts: `(random)` (int), `(random:int)`, `(random:float)`, `(random:bool)`.

> Values are generated at parse time — each parse produces different values.

---

### Multiline Text

Use the pipe `|` operator for multiline strings:

```synx
description |
  This is a long description
  that spans multiple lines.
  Each line is joined with a newline character.

query |
  SELECT *
  FROM users
  WHERE active = true
  ORDER BY name
```

---

### Comments

Three styles — all are ignored by the parser:

```synx
# This is a hash comment
// This is a slash comment

name John  # inline comment after value
port 8080  // another inline comment

###
This is a block comment.
Everything between ### fences is ignored.
No need to prefix each line.
###
```

Inside comments, the VSCode extension highlights markdown-like formatting:
- `*italic*` — green italic
- `**bold**` — purple bold
- `***bold+italic***` — gold bold italic
- `` `code` `` — orange with background

---

## 🔥 Active Mode (`!active`)

Place `!active` on the **very first line** (or `#!mode:active`) to unlock markers and constraints.

Without `!active`, all markers like `:env`, `:calc`, `:random` are treated as **plain text** in the key name. This is by design — static configs remain fast and predictable.

```synx
!active

# Now markers work!
port:env PORT
boss_hp:calc base_hp * 5
```

---

## 🔐 Lock Mode (`!lock`)

Add `!lock` to prevent external code from modifying config values via `Synx.set()`, `Synx.add()`, `Synx.remove()`. Internal SYNX markers still work normally.

```synx
!active
!lock

max_players 100
server_name MyServer
greeting:random
  - Hello!
  - Welcome!
```

```typescript
const config = Synx.loadSync('./config.synx');

Synx.set(config, 'max_players', 200);
// ❌ throws: "SYNX: Cannot set "max_players" — config is locked (!lock)"

console.log(config.max_players); // ✅ 100 (read is always allowed)
```

Use `Synx.isLocked(config)` to check if a config is locked.

---

## 📎 Include Directive (`!include`)

The `!include` directive imports another `.synx` file's keys for use in `{key:alias}` interpolation. Unlike the `:include` marker (which embeds a file as a child block), `!include` makes the target file's top-level keys available for string interpolation across the current file.

```synx
!active
!include ./db.synx
!include ./cache.synx redis

app_name MyApp
db_url postgresql://{host:db}:{port:db}/{name:db}
cache_url redis://{host:redis}:{port:redis}
```

Contents of `db.synx`:

```synx
host localhost
port 5432
name mydb
```

**Result:**

```json
{
  "app_name": "MyApp",
  "db_url": "postgresql://localhost:5432/mydb",
  "cache_url": "redis://cache-server:6379"
}
```

**Alias rules:**

| Directive | Alias | Access |
|---|---|---|
| `!include ./db.synx` | `db` (auto from filename) | `{host:db}` |
| `!include ./cache.synx redis` | `redis` (explicit) | `{host:redis}` |
| `!include ./config.synx` (only one include) | — | `{host:include}` shorthand |

If the file has only one `!include`, you can use `{key:include}` as a shorthand instead of `{key:alias}`.

---

## 🧹 Canonical Format (`format`)

`Synx.format()` rewrites any `.synx` string into a single, normalized form.

**What it does:**
- **Sorts all keys alphabetically** at every nesting level
- **Normalizes indentation** to exactly 2 spaces per level
- **Strips comments** — canonical form contains only data
- **One blank line** between top-level blocks (objects and lists)
- **Preserves directives** (`!active`, `!lock`) at the top
- **List item order is preserved** — only named keys are sorted

### Why this matters for Git

Without canonical format, two programmers writing the same config produce different files:

```synx
# Programmer A                  # Programmer B
server                          server
    port 8080                     host 0.0.0.0
    host 0.0.0.0                  port 8080
```

`git diff` shows the entire block as changed — even though the data is identical.

After `Synx.format()`, both produce:

```synx
server
  host 0.0.0.0
  port 8080
```

One canonical form. Zero noise in diffs.

### Usage

**JavaScript / TypeScript:**

```typescript
import { Synx } from '@aperturesyndicate/synx';
import * as fs from 'fs';

const raw = fs.readFileSync('config.synx', 'utf-8');
const canonical = Synx.format(raw);
fs.writeFileSync('config.synx', canonical);
```

As a pre-commit hook (add to `.git/hooks/pre-commit`):

```bash
#!/bin/sh
node -e "
const fs = require('fs');
const { Synx } = require('@aperturesyndicate/synx');
process.argv.slice(1).forEach(f => {
  fs.writeFileSync(f, Synx.format(fs.readFileSync(f, 'utf-8')));
});
" $(git diff --cached --name-only --diff-filter=ACM | grep '\.synx$')
```

**Rust:**

```rust
use synx_core::Synx;

let raw = std::fs::read_to_string("config.synx").unwrap();
let canonical = Synx::format(&raw);
std::fs::write("config.synx", canonical).unwrap();
```

---

## 🧩 Markers — Full Reference

SYNX v3.0 ships with **21 markers**. Each marker is a function attached to a key with `:marker` syntax.

> **All markers require `!active` mode.**

---

### `:env` — Environment Variable

Reads a system environment variable at parse time.

```synx
!active

port:env PORT
api_url:env API_BASE_URL
```

**Result** (if `PORT=3000`):

```json
{ "port": 3000, "api_url": null }
```

Combine with `:default` for a fallback:

```synx
!active

port:env:default:8080 PORT
```

If `PORT` is not set → returns `8080`.

---

### `:default` — Fallback Value

Sets a fallback if the value is empty, null, or missing.

```synx
!active

theme:default dark
locale:default en-US
```

Most powerful when chained with `:env`:

```synx
!active

port:env:default:3000 PORT
db_host:env:default:localhost DATABASE_HOST
```

---

### `:calc` — Arithmetic Expression

Evaluates a math expression. References other numeric keys by name, including nested keys via dot-path.

```synx
!active

base_price 100
tax_rate 0.2
tax:calc base_price * tax_rate
total:calc base_price + tax
discount:calc total * 0.1
final:calc total - discount
```

```json
{
  "base_price": 100,
  "tax_rate": 0.2,
  "tax": 20,
  "total": 120,
  "discount": 12,
  "final": 108
}
```

**Dot-path references** — access nested values in calc expressions:

```synx
!active

stats
  base_hp 100
  multiplier 3
  armor 25

total_hp:calc stats.base_hp * stats.multiplier
effective_hp:calc total_hp + stats.armor
```

```json
{
  "stats": { "base_hp": 100, "multiplier": 3, "armor": 25 },
  "total_hp": 300,
  "effective_hp": 325
}
```

Supported operators: `+` `-` `*` `/` `%` `(` `)`.

> **Safe evaluator** — no `eval()`. Only arithmetic operations with numeric literals and key references.

---

### `:random` — Random Selection

Picks one random item from the list below the key.

**Equal probability:**

```synx
!active

greeting:random
  - Hello!
  - Welcome!
  - Hey there!
  - Good day!
```

**Weighted probability** (weights as arguments after `:random`):

```synx
!active

loot:random 70 20 10
  - common
  - rare
  - legendary
```

`common` has 70% chance, `rare` 20%, `legendary` 10%.

---

### `:alias` — Reference Another Key

Copies the resolved value of another key. Change the source once — all aliases follow.

```synx
!active

admin_email alex@example.com
complaints_email:alias admin_email
billing_email:alias admin_email
```

```json
{
  "admin_email": "alex@example.com",
  "complaints_email": "alex@example.com",
  "billing_email": "alex@example.com"
}
```

`:alias` resolves its source first, so you can alias keys that use other markers:

```synx
!active

base_port:env:default:3000 PORT
api_port:alias base_port
metrics_port:alias base_port
```

All three keys will have the same value — the resolved result of `:env:default:3000`.

> **`:alias` vs `:ref`:** Both copy a value, but `:alias` is a terminal operation — no further markers run after it. Use `:ref` when you need to chain markers (e.g., `:ref:calc:*2`).

---

### `:ref` — Reference with Chaining

Like `:alias`, but feeds the resolved value into subsequent markers. This makes it possible to reference a key and then transform the result.

```synx
!active

base_rate 50
quick_rate:ref base_rate
double_rate:ref:calc:*2 base_rate
boosted_rate:ref:calc:+25 base_rate
```

```json
{
  "base_rate": 50,
  "quick_rate": 50,
  "double_rate": 100,
  "boosted_rate": 75
}
```

**Shorthand calc syntax:** `:ref:calc:*2` resolves the reference, then applies the operator. Supported: `+`, `-`, `*`, `/`, `%`.

**Practical example — difficulty scaling:**

```synx
!active

base_hp 100
easy_hp:ref:calc:*0.5 base_hp
normal_hp:ref base_hp
hard_hp:ref:calc:*2 base_hp
nightmare_hp:ref:calc:*4 base_hp
```

```json
{ "easy_hp": 50, "normal_hp": 100, "hard_hp": 200, "nightmare_hp": 400 }
```

> **When to use `:ref` vs `:alias`:** Use `:ref` when you need to chain additional markers after the reference. If you just need a copy of the value, `:alias` is simpler and more explicit.

---

### `:inherit` — Block Inheritance

Merges all fields from one or more parent blocks into a child block. Child values override inherited ones. Use `_` prefix for private template blocks — they are excluded from the final output.

```synx
!active

_base_resource
  weight 10
  stackable true
  category misc

steel:inherit:_base_resource
  weight 25
  material metal

wood:inherit:_base_resource
  material organic
```

```json
{
  "steel": {
    "weight": 25,
    "stackable": true,
    "category": "misc",
    "material": "metal"
  },
  "wood": {
    "weight": 10,
    "stackable": true,
    "category": "misc",
    "material": "organic"
  }
}
```

Note: `_base_resource` is not included in the output because its name starts with `_`.

**Multi-parent inheritance:**

A single block can inherit from multiple parents. Parents merge left-to-right (later parents override earlier ones), and child fields override all.

```synx
!active

_movable
  speed 10
  can_move true

_damageable
  hp 100
  armor 5

_attackable
  damage 15
  range 1

tank:inherit:_movable:_damageable:_attackable
  name Tank
  armor 50
  damage 120
```

```json
{
  "tank": {
    "speed": 10,
    "can_move": true,
    "hp": 100,
    "armor": 50,
    "damage": 120,
    "range": 1,
    "name": "Tank"
  }
}
```

`tank` inherits `speed` and `can_move` from `_movable`, `hp` from `_damageable`, `range` from `_attackable`, and overrides `armor` (50 instead of 5) and `damage` (120 instead of 15).

**Multi-block inheritance chains — game entities:**

```synx
!active

_entity
  visible true
  layer world

_enemy:inherit:_entity
  hostile true
  ai patrol

goblin:inherit:_enemy
  hp 30
  damage 5

dragon:inherit:_enemy
  hp 500
  damage 80
  ai aggressive
```

```json
{
  "goblin": { "visible": true, "layer": "world", "hostile": true, "ai": "patrol", "hp": 30, "damage": 5 },
  "dragon": { "visible": true, "layer": "world", "hostile": true, "ai": "aggressive", "hp": 500, "damage": 80 }
}
```

Inheritance chains work: `_entity` → `_enemy` → `goblin`. Private blocks (`_entity`, `_enemy`) are excluded from the output.

---

### `:i18n` — Multilingual Values

Selects a localized value from nested language keys. Pass `lang` in options to choose the language. Falls back to `en`, then the first available value.

```synx
!active

title:i18n
  en Hello World
  ru Привет мир
  de Hallo Welt
  ja こんにちは世界

description:i18n
  en A great application
  ru Отличное приложение
```

```javascript
const config = Synx.parse(text, { lang: 'ru' });
// config.title → "Привет мир"
// config.description → "Отличное приложение"

const configDe = Synx.parse(text, { lang: 'de' });
// configDe.title → "Hallo Welt"
// configDe.description → "A great application" (fallback to 'en')
```

**Pluralization — `:i18n:COUNT_FIELD`**

When a count field name is specified after `:i18n`, the engine uses CLDR plural rules to select the correct plural form. The language entry must contain plural category keys (`one`, `few`, `many`, `other`, etc.).

```synx
!active

item_count 5

label:i18n:item_count
  en
    one {count} item found
    other {count} items found
  ru
    one {count} предмет найден
    few {count} предмета найдено
    many {count} предметов найдено
    other {count} предметов найдено
```

```javascript
const config = Synx.parse(text, { lang: 'en' });
// config.label → "5 items found"

const configRu = Synx.parse(text, { lang: 'ru' });
// configRu.label → "5 предметов найдено"
```

`{count}` in the plural string is automatically replaced with the actual count value.

**Supported plural categories by language:**

| Language | Categories |
|----------|-----------|
| English, German, Spanish, Italian (`en`, `de`, `es`, `it`) | `one` (1), `other` |
| Russian, Ukrainian, Polish (`ru`, `uk`, `pl`) | `one`, `few`, `many` |
| Czech, Slovak (`cs`, `sk`) | `one`, `few`, `other` |
| French, Portuguese (`fr`, `pt`) | `one` (0–1), `other` |
| Arabic (`ar`) | `zero`, `one`, `two`, `few`, `many`, `other` |
| Japanese, Chinese, Korean (`ja`, `zh`, `ko`) | `other` (no plural forms) |

---

### `:secret` — Hidden Value

Marks a value as secret. Readable by your code, but hidden in logs, `toString()`, and `JSON.stringify()`.

```synx
!active

api_key:secret sk-1234567890abcdef
db_password:secret P@ssw0rd!Super
```

In your code:

```javascript
console.log(config.api_key);          // "[SECRET]"
console.log(config.api_key.reveal()); // "sk-1234567890abcdef"
JSON.stringify(config);               // api_key: "[SECRET]"
```

---

### Auto-`{}` — String Interpolation

In `!active` mode, any string value containing `{key}` is automatically interpolated — no marker needed. Supports dot-path for nested access.

```synx
!active

first_name John
last_name Doe
greeting Hello, {first_name} {last_name}!

server
  host api.example.com
  port 443
api_url https://{server.host}:{server.port}/v1
```

```json
{
  "greeting": "Hello, John Doe!",
  "api_url": "https://api.example.com:443/v1"
}
```

**Cross-file interpolation with `!include`:**

When you use the `!include` directive (see below), you can reference keys from included files:

```synx
!active
!include ./db.synx

conn_string postgresql://{host:db}:{port:db}/{name:db}
```

Syntax: `{key}` for local keys, `{key:alias}` for included file keys, `{key:include}` for the first/only included file.

> **Legacy:** The `:template` marker still works for backward compatibility but is no longer needed — auto-`{}` handles interpolation automatically.

---

### `:include` / `:import` — Import External File

Inserts the contents of another `.synx` file as a child block. Path is relative to the current file. `:import` is an alias for `:include` — they work identically. Use `:import` to avoid confusion with the `!include` directive.

```synx
!active

app_name MyApp
database:import ./db.synx
logging:import ./logging.synx
```

Contents of `db.synx`:

```synx
host localhost
port 5432
name mydb
```

**Result:**

```json
{
  "app_name": "MyApp",
  "database": {
    "host": "localhost",
    "port": 5432,
    "name": "mydb"
  }
}
```

If the included file also has `!active`, its markers are resolved too.

**Import comparison matrix:**

| Feature | `!include` (directive) | `:include` / `:import` (marker) |
|---|---|---|
| **Syntax** | `!include ./file.synx [alias]` | `key:import ./file.synx` |
| **Where** | Top of file, before keys | On any key line |
| **Result** | Makes keys available for `{key:alias}` interpolation | Embeds file contents as a nested object under the key |
| **Use case** | String interpolation across files | Structured config composition |
| **Multiple files** | Yes, each gets an alias | Yes, one per key |

---

### `:unique` — Deduplicate List

Removes duplicate elements from a list:

```synx
!active

tags:unique
  - action
  - rpg
  - action
  - adventure
  - rpg
```

```json
{ "tags": ["action", "rpg", "adventure"] }
```

---

### `:split` — String to Array

Splits a string by a delimiter into an array.

```synx
!active

colors:split red, green, blue
words:split:space hello world foo bar
hosts:split:pipe host1|host2|host3
```

```json
{
  "colors": ["red", "green", "blue"],
  "words": ["hello", "world", "foo", "bar"],
  "hosts": ["host1", "host2", "host3"]
}
```

Delimiter keywords: `space`, `pipe`, `dash`, `dot`, `semi`, `tab`, `slash`. Default: comma.

---

### `:join` — Array to String

Joins list elements into a single string with a delimiter.

Delimiter keywords: `space`, `pipe`, `dash`, `dot`, `semi`, `tab`, `slash`. Default: comma.

```synx
!active

path:join:slash
  - home
  - user
  - documents

csv:join
  - Alice
  - Bob
  - Charlie

tags_line:join:space
  - synx
  - parser
  - config
```

```json
{
  "path": "home/user/documents",
  "csv": "Alice,Bob,Charlie",
  "tags_line": "synx parser config"
}
```

Delimiter keywords: `space`, `pipe`, `dash`, `dot`, `semi`, `tab`, `slash`. Default: comma.

---

### `:geo` — Region-Based Selection

Selects a value based on the user's geographic region.

```synx
!active

currency:geo
  - US USD
  - EU EUR
  - GB GBP
  - JP JPY
```

With `region: "EU"`:

```json
{ "currency": "EUR" }
```

Falls back to the first entry if region is unrecognized.

---

### `:clamp` — Clamp Number to Range

Clamps a numeric value to a `[min, max]` range.

```synx
!active

volume:clamp:0:100 150
opacity:clamp:0.0:1.0 0.7
level:clamp:1:99 0
```

```json
{
  "volume": 100,
  "opacity": 0.7,
  "level": 1
}
```

---

### `:round` — Round to N Decimals

Rounds a number to a specified number of decimal places. Especially useful after `:calc`.

```synx
!active

pi:round:4 3.14159265
price:round:2 19.999
count:round:0 7.7

# Chainable with :calc
revenue 1234.5
profit:calc:round:2 revenue * 0.337
```

```json
{
  "pi": 3.1416,
  "price": 20.0,
  "count": 8,
  "profit": 416.03
}
```

---

### `:map` — Lookup Table

Maps the value of a source key to a human-readable label using a lookup list.

```synx
!active

status_code 2
status_label:map:status_code
  - 0 offline
  - 1 online
  - 2 away
  - 3 busy

error_code 404
error_message:map:error_code
  - 200 OK
  - 404 Not Found
  - 500 Internal Server Error
```

```json
{
  "status_code": 2,
  "status_label": "away",
  "error_code": 404,
  "error_message": "Not Found"
}
```

Returns `null` if no matching entry is found.

---

### `:format` — Number Formatting

Formats a number using a printf-style pattern.

```synx
!active

price:format:%.2f 1234.5
order_id:format:%06d 42
ratio:format:%.4f 0.1
```

```json
{
  "price": "1234.50",
  "order_id": "000042",
  "ratio": "0.1000"
}
```

Supported patterns:
- `%.Nf` — N decimal places (e.g., `%.2f` → `1234.50`)
- `%0Nd` — zero-padded integer (e.g., `%06d` → `000042`)
- `%Nd` — space-padded integer
- `%e` — scientific notation

---

### `:fallback` — File-Path Fallback

If the value (a file path) does not exist on disk, use the fallback path. Also triggers on empty/null values.

```synx
!active

icon:fallback:./assets/default.png ./assets/custom.png
theme:fallback:./themes/default.css ./themes/user.css
```

If `./assets/custom.png` doesn't exist → uses `./assets/default.png`.

---

### `:once` — Generate and Persist

Generates a value **once** on first parse and stores it in a `.synx.lock` sidecar file. Every subsequent parse returns the same locked value.

```synx
!active

session_id:once uuid
app_seed:once random
created_at:once timestamp
```

**First parse** — generates and writes to `.synx.lock`:

```
session_id a3b1f4e2-7c89-4d12-b456-1234abcd5678
app_seed 1847362951
created_at 1741305600000
```

**Every subsequent parse** — reads the same values from `.synx.lock`.

Generation types:
| Type | Output |
|---|---|
| `uuid` (default) | UUID v4 string |
| `random` | Random integer |
| `timestamp` | Unix timestamp (ms) |

---

### `:version` — Semantic Version Compare

Compares the value (a version string) against a required version. Returns a boolean.

```synx
!active

node_version:version:>=:18.0 20.11.0
api_compat:version:==:3.0 3.0.0
legacy_check:version:<:2.0 1.5.3
```

```json
{
  "node_version": true,
  "api_compat": true,
  "legacy_check": true
}
```

Operators: `>=` `<=` `>` `<` `==` `!=`

---

### `:watch` — Read External File

Reads an external file at parse time and uses its content (or a specific key from it) as the value.

```synx
!active

# Read the entire file
raw_config:watch ./data.txt

# Extract a key from a JSON file
app_name:watch:name ./package.json

# Read a nested JSON key
db_host:watch:database.host ./config.json

# Read a key from another SYNX file
api_url:watch:api_url ./settings.synx
```

The file is re-read every time the SYNX document is parsed — enabling live/hot-reload when combined with a file watcher.

---

### `:spam` — Rate-Limit Access

Limits how often a target key/file reference can be resolved inside a time window.

Syntax: `:spam:MAX_CALLS[:WINDOW_SEC]`.
If `WINDOW_SEC` is omitted, it defaults to `1`.

```synx
!active

secret_token abc
access:spam:3:10 secret_token

# WINDOW_SEC defaults to 1
burst_access:spam:5 secret_token
```

When exceeded, engines return `SPAM_ERR: ...`.

---

## 🔒 Constraints

Constraints validate values at parse time. They're defined inside `[brackets]` after the key name.

> **All constraints require `!active` mode.**

| Constraint | Syntax | Description |
|---|---|---|
| `required` | `key[required]` | Key must have a non-empty value |
| `readonly` | `key[readonly]` | Value cannot be changed via API |
| `min:N` | `key[min:3]` | Minimum length (string) or value (number) |
| `max:N` | `key[max:100]` | Maximum length (string) or value (number) |
| `type:T` | `key[type:int]` | Enforced type: `int`, `float`, `bool`, `string` |
| `pattern:R` | `key[pattern:^\d+$]` | Regex pattern validation |
| `enum:A\|B` | `key[enum:light\|dark]` | Allowed values (pipe-separated) |

### Combined example

```synx
!active

app_name[required, min:3, max:30] TotalWario
volume[min:0, max:100, type:int] 75
theme[enum:light|dark|auto] dark
country[pattern:^[A-Z]{2}$] US
api_key[required]:env API_KEY
version[readonly] 3.0.0
password[required, min:8, max:64, type:string] MyP@ssw0rd
```

Constraints can be combined with markers — the constraint runs first, then the marker resolves.

---

## 🔗 Marker Chaining

Markers can be chained using multiple `:marker` segments:

```synx
!active

# :env with :default fallback
port:env:default:8080 PORT

# :calc with :round
profit:calc:round:2 revenue * margin

# :split with :unique
tags:split:unique red, green, red, blue
```

Order matters — markers execute left-to-right within the engine's pipeline.

### ✅ Marker Compatibility

Common combinations that work well:

- `env:default`
- `calc:round`
- `split:unique`
- `split:join` (through an intermediate array)

Important limitations:

- `!active` is required, otherwise markers are not resolved.
- Some markers are type-dependent: `split` expects a string, `join` expects an array, `round`/`clamp` expect numbers.
- Marker arguments are read from the right side of the chain (for example `clamp:min:max`, `round:n`, `map:key`).
- If an earlier marker changes the type, a later marker may no longer apply.

---

## � CLI Tool

> Added in v3.1.3.

Install globally via npm:

```bash
npm install -g @aperturesyndicate/synx
```

### `synx convert` — Export to Other Formats

```bash
# SYNX → JSON
synx convert config.synx --format json

# SYNX → YAML (for Helm, Ansible, K8s)
synx convert config.synx --format yaml > values.yaml

# SYNX → TOML
synx convert config.synx --format toml

# SYNX → .env (for Docker Compose)
synx convert config.synx --format env > .env

# With strict mode (fail on any marker error)
synx convert config.synx --format json --strict
```

### `synx validate` — CI/CD Validation

```bash
synx validate config.synx --strict
# Exits 0 on success, 1 on any INCLUDE_ERR / WATCH_ERR / CALC_ERR / CONSTRAINT_ERR
```

### `synx watch` — Live Reload

```bash
# Print JSON on every change
synx watch config.synx --format json

# Execute a command on every change (e.g. reload Nginx)
synx watch config.synx --exec "nginx -s reload"
```

### `synx schema` — Extract JSON Schema from Constraints

```bash
synx schema config.synx
# Outputs JSON Schema based on [required, min:N, max:N, type:T, enum:A|B, pattern:R]
```

---

## 📤 Export Formats (JS/TS API)

> Added in v3.1.3.

Convert a parsed SYNX object to JSON, YAML, TOML, or .env:

```typescript
import Synx from '@aperturesyndicate/synx';

const config = Synx.loadSync('config.synx');

// JSON
const json = Synx.toJSON(config);        // pretty-printed
const compact = Synx.toJSON(config, false); // compact

// YAML
const yaml = Synx.toYAML(config);

// TOML
const toml = Synx.toTOML(config);

// .env (KEY=VALUE format)
const env = Synx.toEnv(config);           // APP_NAME=TotalWario
const prefixed = Synx.toEnv(config, 'APP'); // APP_APP_NAME=TotalWario
```

---

## 📋 Schema Export

> Added in v3.1.3.

Extract SYNX constraints as a JSON Schema object:

```typescript
const schema = Synx.schema(`
!active
app_name[required, min:3, max:30] TotalWario
volume[min:0, max:100, type:int] 75
theme[enum:light|dark|auto] dark
`);
```

Result:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "properties": {
    "app_name": { "minimum": 3, "maximum": 30, "required": true },
    "volume": { "type": "integer", "minimum": 0, "maximum": 100 },
    "theme": { "enum": ["light", "dark", "auto"] }
  },
  "required": ["app_name"]
}
```

Use in CI to validate configs before deployment:

```bash
synx schema config.synx > schema.json
# Feed into any JSON Schema validator
```

---

## 👁 File Watcher

> Added in v3.1.3.

Watch a `.synx` file and get hot-reloaded config on every change:

```typescript
const handle = Synx.watch('config.synx', (config, error) => {
  if (error) {
    console.error('Config reload failed:', error.message);
    return;
  }
  console.log('Config updated:', config.server.port);
  // Apply new config to your app without restart
}, { strict: true });

// Stop watching
handle.close();
```

---

## 🐳 Deployment Guide

> Added in v3.1.3.

### Docker + Docker Compose

SYNX serves as the **single source of truth** for all service configuration. Services that need their own config format (Nginx, Redis, etc.) receive generated configs at startup.

**Pattern:**

```
┌─────────────────┐     ┌────────────────┐     ┌─────────────────┐
│   config.synx   │────▶│  startup script │────▶│  nginx.conf     │
│  (one file)     │     │  or CLI convert │     │  .env           │
│  :env :default  │     │                 │     │  redis.conf     │
│  :template      │     │                 │     │  app settings   │
└─────────────────┘     └────────────────┘     └─────────────────┘
```

**Step 1 — Write your config:**

```synx
!active

app
  name my-service
  port:env:default:3000 APP_PORT
  host:env:default:0.0.0.0 APP_HOST

database
  host:env:default:postgres DB_HOST
  port:env:default:5432 DB_PORT
  name:env:default:mydb DB_NAME
  user:env:default:app DB_USER
  password:env DB_PASSWORD

redis
  host:env:default:redis REDIS_HOST
  port:env:default:6379 REDIS_PORT
  url:template redis://{redis.host}:{redis.port}/0

nginx
  listen:env:default:8080 NGINX_PORT
  upstream_host:env:default:web APP_HOST
  upstream_port:env:default:3000 APP_PORT
```

**Step 2 — Generate .env for Docker Compose:**

```bash
synx convert config.synx --format env > .env
```

**Step 3 — Use in docker-compose.yml:**

```yaml
services:
  web:
    image: node:20-alpine
    env_file: .env
    ports:
      - "${APP_PORT}:${APP_PORT}"

  redis:
    image: redis:7-alpine

  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_DB: ${DB_NAME}
      POSTGRES_USER: ${DB_USER}
      POSTGRES_PASSWORD: ${DB_PASSWORD}
```

### Nginx Config Generation

Use a template + startup script to generate `nginx.conf` from SYNX:

```javascript
// entrypoint.js — runs before Nginx starts
const Synx = require('@aperturesyndicate/synx');
const fs = require('fs');

const config = Synx.loadSync('/config/app.synx', {
  env: process.env,
  strict: true,
});

const nginxConf = `
server {
  listen ${config.nginx.listen};
  location / {
    proxy_pass http://${config.nginx.upstream_host}:${config.nginx.upstream_port};
  }
}`;

fs.writeFileSync('/etc/nginx/conf.d/default.conf', nginxConf);
```

### Redis Connection

SYNX config feeds Redis connection parameters directly:

```synx
!active

redis
  host:env:default:localhost REDIS_HOST
  port:env:default:6379 REDIS_PORT
  db:default 0
  ttl:default 3600
  password:env REDIS_PASSWORD
  url:template redis://{redis.host}:{redis.port}/{redis.db}
```

```javascript
const config = Synx.loadSync('config.synx', { env: process.env, strict: true });
const redis = new Redis(config.redis.url);
```

### PostgreSQL Connection

```synx
!active

db
  host:env:default:localhost DATABASE_HOST
  port:env:default:5432 DATABASE_PORT
  name:env:default:mydb DATABASE_NAME
  user:env:default:app DATABASE_USER
  password:env DATABASE_PASSWORD
  url:template postgresql://{db.user}:{db.password}@{db.host}:{db.port}/{db.name}
  pool_min:default 5
  pool_max:default 20
```

```javascript
const config = Synx.loadSync('config.synx', { env: process.env, strict: true });
const pool = new Pool({ connectionString: config.db.url });
```

### Kubernetes Secrets

K8s mounts secrets as files in `/run/secrets/`. Use `:watch` to read them:

```synx
!active

db_password:watch /run/secrets/db-password
api_key:watch /run/secrets/api-key
```

Docker Secrets work identically — mounted at `/run/secrets/`.

### HashiCorp Vault

Use Vault Agent to write secrets to files, then read with `:watch`:

```synx
!active

# Vault Agent writes to /vault/secrets/
db_creds:watch:password /vault/secrets/database
api_key:watch:key /vault/secrets/api-key
```

Or inject via environment variables using Vault Agent's `env_template`:

```synx
!active

db_password:env VAULT_DB_PASSWORD
api_key:env VAULT_API_KEY
```

### Helm / Kubernetes

Convert SYNX to YAML for Helm values:

```bash
synx convert config.synx --format yaml > helm/values.yaml
helm upgrade my-release ./chart -f helm/values.yaml
```

### Terraform

Terraform accepts JSON variable files:

```bash
synx convert config.synx --format json > terraform.tfvars.json
terraform apply -var-file=terraform.tfvars.json
```

### CI/CD Pipeline Validation

Add to your CI pipeline to catch config errors before deploy:

```yaml
# GitHub Actions example
- name: Validate SYNX config
  run: npx @aperturesyndicate/synx validate config.synx --strict
```

---

## �💻 Code Examples

### JavaScript / TypeScript

```bash
npm install @aperturesyndicate/synx
```

**Parse a string:**

```typescript
import { Synx } from '@aperturesyndicate/synx';

const config = Synx.parse(`
  app_name TotalWario
  version 3.0.0
  server
    host 0.0.0.0
    port 8080
`);

console.log(config.app_name);     // "TotalWario"
console.log(config.server.port);  // 8080
```

**Parse with `!active` mode:**

```typescript
import { Synx } from '@aperturesyndicate/synx';

const config = Synx.parse(`
  !active
  base_price 100
  tax:calc base_price * 0.2
  total:calc base_price + tax
  api_key:env:default:sk-test API_KEY
  greeting:template Welcome to {app_name}!
  app_name SuperApp
`);

console.log(config.total);     // 120
console.log(config.greeting);  // "Welcome to SuperApp!"
```

**Load a file (sync):**

```typescript
import { Synx } from '@aperturesyndicate/synx';

const config = Synx.loadSync('./config.synx');
console.log(config);
```

**Load a file (async):**

```typescript
import { Synx } from '@aperturesyndicate/synx';

const config = await Synx.load('./config.synx');
console.log(config);
```

**With options:**

```typescript
const config = Synx.loadSync('./config.synx', {
  env: { PORT: '3000', NODE_ENV: 'production' },
  region: 'EU',
  basePath: './configs',
});
```

**Runtime Manipulation (set / add / remove):**

```typescript
import { Synx } from '@aperturesyndicate/synx';

const config = Synx.loadSync('./game.synx');

// Set a value
Synx.set(config, 'max_players', 100);
Synx.set(config, 'server.host', 'localhost');

// Get a value (dot-path supported)
const port = Synx.get(config, 'server.port'); // 8080

// Add to a list
Synx.add(config, 'maps', 'Arena of Doom');
Synx.add(config, 'maps', 'Crystal Caverns');

// Remove from a list
Synx.remove(config, 'maps', 'Arena of Doom');

// Delete a key entirely
Synx.remove(config, 'deprecated_key');

// Check lock status
if (!Synx.isLocked(config)) {
  Synx.set(config, 'motd', 'Welcome!');
}
```

> **Note:** If the `.synx` file has `!lock`, all `set`/`add`/`remove` calls will throw an error.

**Access Methods (JS/TS API):**

- `Synx.get(obj, keyPath)` — read a value by dot-path.
- `Synx.set(obj, keyPath, value)` — set a value by dot-path.
- `Synx.add(obj, keyPath, item)` — append an item to an array field.
- `Synx.remove(obj, keyPath, item?)` — remove an array item or delete a key.
- `Synx.isLocked(obj)` — check if config is locked via `!lock`.

---

### Python

Current `synx_native` exports: `parse`, `parse_active`, `parse_to_json`.

Python equivalents for `get`/`set`/`add`/`remove` can be implemented like this:

```python
def get_path(obj, key_path, default=None):
  cur = obj
  for part in key_path.split('.'):
    if not isinstance(cur, dict) or part not in cur:
      return default
    cur = cur[part]
  return cur

def set_path(obj, key_path, value):
  parts = key_path.split('.')
  cur = obj
  for part in parts[:-1]:
    if part not in cur or not isinstance(cur[part], dict):
      cur[part] = {}
    cur = cur[part]
  cur[parts[-1]] = value

def add_path(obj, key_path, item):
  arr = get_path(obj, key_path)
  if not isinstance(arr, list):
    set_path(obj, key_path, [] if arr is None else [arr])
    arr = get_path(obj, key_path)
  arr.append(item)

def remove_path(obj, key_path, item=None):
  parts = key_path.split('.')
  cur = obj
  for part in parts[:-1]:
    if not isinstance(cur, dict) or part not in cur:
      return
    cur = cur[part]
  last = parts[-1]
  if item is None:
    if isinstance(cur, dict):
      cur.pop(last, None)
    return
  if isinstance(cur, dict) and isinstance(cur.get(last), list):
    try:
      cur[last].remove(item)
    except ValueError:
      pass
```

```bash
pip install synx-format
```

```python
import synx_native

# Parse a string
config = synx_native.parse("""
app_name TotalWario
version 3.0.0
server
  host 0.0.0.0
  port 8080
""")

print(config["app_name"])      # "TotalWario"
print(config["server"]["port"])  # 8080

# Python access helpers usage
set_path(config, "server.port", 9090)
add_path(config, "maps", "Arena of Doom")
remove_path(config, "maps", "Arena of Doom")
print(get_path(config, "server.port"))  # 9090
```

```python
# Parse active mode
config = synx_native.parse("""
!active
base_price 100
tax:calc base_price * 0.2
total:calc base_price + tax
""")

print(config["total"])  # 120
```

```python
# Parse to JSON string
json_str = synx_native.parse_to_json("""
name Alice
age 30
""")
print(json_str)  # {"name":"Alice","age":30}
```

---

### Rust

```bash
cargo add synx-core
```

```rust
use synx_core::Synx;

fn main() {
    // Parse a string
    let config = Synx::parse("
        app_name TotalWario
        version 3.0.0
        server
          host 0.0.0.0
          port 8080
    ");

    println!("{:?}", config.root);

    // Parse to JSON string
    let json = Synx::parse_to_json("
        name Alice
        age 30
    ");
    println!("{}", json); // {"name":"Alice","age":30}
}
```

```rust
use synx_core::{Synx, Options};

fn main() {
    // Active mode with options
    let mut opts = Options::default();
    opts.region = Some("EU".to_string());

    let config = Synx::parse_with_options("
        !active
        currency:geo
          - US USD
          - EU EUR
    ", &opts);

    // currency → "EUR"
}
```

---

## 🛠 Editor Support

### Visual Studio Code

Full language support with 15+ features:

| Feature | Description |
|---|---|
| **Syntax Highlighting** | Keys, values, markers, constraints, comments, types, colors |
| **IntelliSense** | Autocomplete for 21 markers, 7 constraints, type casts |
| **Hover Info** | Documentation on hover for every marker and constraint |
| **Diagnostics** | 15 real-time validation checks with severity levels |
| **Go to Definition** | Ctrl+Click on `:alias`, `:template`, `:calc`, `:include` |
| **Find References** | Find all usages of any key across markers |
| **Document Outline** | Full symbol tree in the Outline panel |
| **Formatting** | Auto-format: 2-space indent, trim whitespace |
| **Color Preview** | Inline color swatches for `#hex` values |
| **Inlay Hints** | Computed `:calc` results shown inline |
| **Live Preview** | Side panel with real-time parsed JSON output |
| **Convert** | SYNX ↔ JSON bidirectional conversion |
| **Freeze** | Resolve all markers into a static `.synx` |

Install: search **"SYNX"** in VS Code Extensions, or `code --install-extension APERTURESyndicate.synx-vscode`.

### Visual Studio 2022

MEF-based extension with:
- Syntax highlighting (classifier)
- IntelliSense (21 markers, 7 constraints)
- Error tagger (diagnostics)
- Outlining (code folding)
- Inlay hints for `:calc`
- Convert / Freeze commands

Install: download `.vsix` from Releases → double-click.

---

## 🏗 Architecture

```
synx-format/
├── crates/
│   └── synx-core/            # Rust core — parser + engine
│       └── src/
│           ├── parser.rs      # Text → Value tree
│           ├── engine.rs      # Marker resolution (21 markers)
│           ├── calc.rs        # Safe math evaluator
│           ├── value.rs       # Value enum, Options, Meta types
│           └── lib.rs         # Public API: Synx::parse()
│
├── bindings/
│   ├── node/                  # NAPI-RS → npm native module
│   └── python/                # PyO3 → PyPI native module
│
├── packages/
│   ├── synx-js/               # Pure TypeScript parser + engine
│   │   └── src/
│   │       ├── index.ts       # Auto-engine: JS ↔ Rust switch
│   │       ├── parser.ts      # 100% JS parser
│   │       ├── engine.ts      # 100% JS engine (21 markers)
│   │       ├── calc.ts        # Safe math evaluator (JS)
│   │       └── types.ts       # TypeScript interfaces
│   │
│   ├── synx-vscode/           # VS Code extension
│   │   └── src/
│   │       ├── extension.ts   # Entry point
│   │       ├── parser.ts      # AST parser with position info
│   │       ├── completion.ts  # IntelliSense (21 markers)
│   │       ├── diagnostics.ts # 15 validation checks
│   │       ├── navigation.ts  # Symbols, GoTo, References
│   │       ├── formatter.ts   # Code formatting
│   │       ├── commands.ts    # Convert, Freeze, Preview
│   │       ├── colors.ts      # Color provider
│   │       └── inlay-hints.ts # :calc result hints
│   │
│   └── synx-visualstudio/     # Visual Studio 2022 extension
│       └── SynxLanguageService/
│           ├── SynxPackage.cs
│           ├── Parser/
│           ├── Classification/
│           ├── Completion/
│           ├── Diagnostics/
│           ├── Formatting/
│           └── Commands/
│
├── benchmarks/                # Criterion + Node + Python benchmarks
│
├── publish-npm.bat            # → npmjs.com
├── publish-pypi.bat           # → pypi.org
├── publish-crates.bat         # → crates.io
├── build-vscode.bat           # → .vsix
└── build-visualstudio.bat     # → .vsix
```

---

## 📖 Specification

The full formal specification of the SYNX format:

- **[SPECIFICATION (English)](https://github.com/kaiserrberg/synx-format/blob/main/SPECIFICATION_EN.md)**
- **[SPECIFICATION (Русский)](https://github.com/kaiserrberg/synx-format/blob/main/SPECIFICATION_RU.md)**

---

## 🔗 Links

| Resource | URL |
|---|---|
| **GitHub** | [github.com/kaiserrberg/synx-format](https://github.com/kaiserrberg/synx-format) |
| **npm** | [npmjs.com/package/@aperturesyndicate/synx](https://www.npmjs.com/package/@aperturesyndicate/synx) |
| **PyPI** | [pypi.org/project/synx-format](https://pypi.org/project/synx-format/) |
| **crates.io** | [crates.io/crates/synx-core](https://crates.io/crates/synx-core) |
| **VS Code** | [marketplace.visualstudio.com](https://marketplace.visualstudio.com/items?itemName=APERTURESyndicate.synx-vscode) |
| **Website** | [aperturesyndicate.com](https://aperturesyndicate.com) |

---

<p align="center">
  <img src="https://aperturesyndicate.com/branding/logos/asp_128.png" width="96" height="96" />
</p>

<p align="center">
  MIT — © <a href="https://github.com/kaiserrberg">APERTURESyndicate</a>
</p>

<p align="center">
  Made by <strong>APERTURESyndicate Production</strong>
</p>
