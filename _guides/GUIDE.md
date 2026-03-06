<p align="center">
  <a href="https://aperturesyndicate.com/branding/aperturesyndicate.png" target="_blank">
    <img src="https://aperturesyndicate.com/branding/aperturesyndicate.png" alt="APERTURESyndicate" width="400" />
  </a>
</p>

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
- [Canonical Format (`format`)](#-canonical-format-format)
- [Markers — Full Reference](#-markers--full-reference)
  - [:env](#env--environment-variable)
  - [:default](#default--fallback-value)
  - [:calc](#calc--arithmetic-expression)
  - [:random](#random--random-selection)
  - [:alias](#alias--reference-another-key)
  - [:secret](#secret--hidden-value)
  - [:template](#template--string-interpolation)
  - [:include](#include--import-external-file)
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

Two styles — both are ignored by the parser:

```synx
# This is a hash comment
// This is a slash comment

name John  # inline comment after value
port 8080  // another inline comment
```

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

SYNX v3.0 ships with **20 markers**. Each marker is a function attached to a key with `:marker` syntax.

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

Evaluates a math expression. References other numeric keys by name.

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

Copies the value of another key without duplication.

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

Change `admin_email` once — all aliases update automatically.

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

### `:template` — String Interpolation

Substitutes `{placeholder}` with values from other keys. Supports dot-path for nested access.

```synx
!active

first_name John
last_name Doe
greeting:template Hello, {first_name} {last_name}!

server
  host api.example.com
  port 443
api_url:template https://{server.host}:{server.port}/v1
```

```json
{
  "greeting": "Hello, John Doe!",
  "api_url": "https://api.example.com:443/v1"
}
```

---

### `:include` — Import External File

Inserts the contents of another `.synx` file. Path is relative to the current file.

```synx
!active

app_name MyApp
database:include ./db.synx
logging:include ./logging.synx
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

```json
{
  "session_id": "a3b1f4e2-7c89-4d12-b456-1234abcd5678",
  "app_seed": "1847362951",
  "created_at": "1741305600000"
}
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

## 💻 Code Examples

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
| **IntelliSense** | Autocomplete for 20 markers, 7 constraints, type casts |
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
- IntelliSense (20 markers, 7 constraints)
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
│           ├── engine.rs      # Marker resolution (20 markers)
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
│   │       ├── engine.ts      # 100% JS engine (20 markers)
│   │       ├── calc.ts        # Safe math evaluator (JS)
│   │       └── types.ts       # TypeScript interfaces
│   │
│   ├── synx-vscode/           # VS Code extension
│   │   └── src/
│   │       ├── extension.ts   # Entry point
│   │       ├── parser.ts      # AST parser with position info
│   │       ├── completion.ts  # IntelliSense (20 markers)
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
