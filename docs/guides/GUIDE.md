# SYNX 3.7 — Complete Guide

> **Version 3.7** — additive revision of the frozen 3.6 specification (July 2026).  
> 3.6 stays the interoperability baseline; 3.7 adds one construct, `|+`, and changes nothing else.  
> 3.7.0 also ships **SYNXL** (`.synxl`) — a separate record-stream format for datasets, on its own version axis.  
> Reference implementation: [`synx-core`](https://crates.io/crates/synx-core)

---

## Table of Contents

1. [Introduction](#introduction)
2. [Quick Start](#quick-start)
3. [Installation](#installation)
4. [Core Syntax](#core-syntax)
   - [Values & Types](#values--types)
   - [Nesting](#nesting)
   - [Arrays](#arrays)
   - [Multiline Values](#multiline-values)
   - [Indent-Preserving Multiline (`|+`)](#indent-preserving-multiline-)
   - [Comments](#comments)
5. [Directives](#directives)
   - [!active](#active)
   - [!lock](#lock)
   - [!include](#include-directive)
   - [!tool](#tool)
   - [!schema](#schema)
   - [!llm](#llm)
6. [Markers](#markers)
   - [:env](#env)
   - [:calc](#calc)
   - [:alias](#alias)
   - [:secret](#secret)
   - [:default](#default)
   - [:random](#random)
   - [:include / :import](#include--import)
   - [:template](#template)
   - [:split](#split)
   - [:join](#join)
   - [:unique](#unique)
   - [:geo](#geo)
   - [:i18n](#i18n)
   - [:ref](#ref)
   - [:clamp](#clamp)
   - [:round](#round)
   - [:format](#format)
   - [:map](#map)
   - [:once](#once)
   - [:version](#version)
   - [:watch](#watch)
   - [:fallback](#fallback)
   - [:spam](#spam)
   - [:prompt](#prompt)
   - [:vision](#vision)
   - [:audio](#audio)
   - [:inherit](#inherit)
7. [Constraints](#constraints)
   - [type](#type)
   - [required](#required)
   - [min / max](#min--max)
   - [pattern](#pattern)
   - [enum](#enum)
   - [readonly](#readonly)
8. [Marker Chaining](#marker-chaining)
9. [SYNXL — Record Streams (`.synxl`)](#synxl--record-streams-synxl)
10. [Language Bindings](#language-bindings)
11. [Binary Format & Diff](#binary-format--diff)
12. [JSON Schema](#json-schema)
13. [Tools & Editors](#tools--editors)
14. [Packages](#packages)
15. [Security](#security)
16. [Performance](#performance)
17. [FAQ](#faq)

---

## Introduction

SYNX is a structured data format based on indentation. It encodes the same data model as JSON — objects, arrays, scalars — but uses whitespace and a minimal set of rules instead of braces, brackets, colons, and quotes. The result is a format that is fast to write, easy to read, and friendly to both humans and language models.

SYNX is not a configuration-only format. It supports full data exchange, active validation with constraints, AI-oriented directives, and binary encoding.

### Motivation

JSON requires a lot of punctuation. YAML is flexible but has a famous list of surprising edge cases (booleans, multi-document streams, the Norway problem). TOML is opinionated about tables. SYNX takes a different approach:

- No curly braces, square brackets, colons, or commas in the default static mode
- Nesting expressed purely by indentation (2-space canonical)
- Types inferred automatically; explicit casting available when needed
- One mode for data storage, one mode for validated configuration (`!active`)

### Data Model

SYNX values map directly to a JSON-compatible data model:

| SYNX | JSON equivalent | Notes |
|---|---|---|
| `key value` | `"key": "value"` | String scalar |
| `key 42` | `"key": 42` | Integer auto-detected |
| `key 3.14` | `"key": 3.14` | Float auto-detected |
| `key true` | `"key": true` | Boolean: true/false |
| `key ~` | `"key": null` | Null literal |
| `key ""` | `"key": ""` | Empty string |
| Indented block | Nested object | Children create object |
| `- value` | Array element | Dash prefix = array item |

### Two Modes

**Static Mode** (default) — Pure data: key-value pairs, nesting, arrays. No markers, no constraints, no computation. Ideal for data exchange and storage.

**Active Mode** (`!active`) — Enabled by the `!active` directive. Unlocks markers (`:env`, `:calc`, etc.) and constraints (`[type:int, required]`). Designed for configuration files with validation.

### Comparison

| Feature | SYNX | JSON | YAML | TOML |
|---|---|---|---|---|
| Human-readable | Yes | Partial | Yes | Yes |
| No punctuation overhead | Yes | No | Partial | Partial |
| Type inference | Yes | Yes | Yes | Yes |
| Schema / validation | Built-in | External | External | External |
| Env var resolution | Built-in | No | No | No |
| Binary encoding | Yes (.synxb) | No | No | No |
| LLM directive | Yes (!llm) | No | No | No |
| Surprise edge cases | None | None | Many | Few |

---

## Quick Start

Create a file named `hello.synx`:

```synx
name Alice
age 30
active true
score 9.5
bio She writes code

address
  city Berlin
  country Germany
  zip 10115

tags
  - rust
  - systems
  - open-source
```

That is a complete, valid SYNX document. No quotes, no braces, no commas. Indentation (2 spaces) defines structure.

### Parse it

```javascript
// JavaScript / TypeScript
import { Synx } from '@aperturesyndicate/synx-format';

const result = Synx.parse(`
name Alice
age 30
active true
`);

console.log(result.name);    // "Alice"
console.log(result.age);     // 30
console.log(result.active);  // true
```

```python
# Python
import synx_native as synx

result = synx.parse(open("hello.synx").read())
print(result["name"])    # Alice
print(result["age"])     # 30
print(result["active"])  # True
```

```rust
// Rust
use synx_core::Synx;

let input = std::fs::read_to_string("hello.synx")?;
let map = Synx::parse(&input);
println!("{:?}", map["name"]);   // String("Alice")
println!("{:?}", map["age"]);    // Int(30)
```

```csharp
// C#
using Synx;

var result = SynxFormat.Parse(File.ReadAllText("hello.synx"));
Console.WriteLine(result["name"]);    // Alice
Console.WriteLine(result["age"]);     // 30
```

```bash
# CLI
synx parse hello.synx
# → canonical JSON

synx parse hello.synx | jq '.name'
# → "Alice"
```

### Add validation with !active

```synx
!active

port[type:int, min:1024, max:65535] 8080
host[required, type:string] localhost
debug[type:bool] false
workers[type:int, min:1, max:32] 4
database_url[required, type:string]:env DATABASE_URL
```

Now `port` must be an integer between 1024 and 65535. `database_url` is read from the environment variable `DATABASE_URL`. Parsing fails with a clear error if any constraint is violated.

---

## Installation

### CLI (Rust)

```bash
cargo install synx-cli
```

### JavaScript / TypeScript

```bash
npm install @aperturesyndicate/synx-format
```

### Python

```bash
pip install synx-format
```

### C# / .NET

```bash
dotnet add package APERTURESyndicate.Synx
```

### Rust (library)

```toml
[dependencies]
synx-core = "3.7"
```

### C++

Header-only. Copy `synx/synx.hpp` into your project or use CMake FetchContent.

### Go

```bash
go get github.com/APERTURESyndicate/synx-format/go
```

### Swift

```swift
.package(url: "https://github.com/APERTURESyndicate/synx-format", from: "3.7.0")
```

### Kotlin / JVM

```kotlin
implementation("com.aperturesyndicate:synx-engine:3.7.0")
```

### VS Code Extension

```bash
code --install-extension APERTURESyndicate.synx-vscode
```

---

## Core Syntax

### Values & Types

SYNX infers types automatically from the value text:

| Value | Detected type | Notes |
|---|---|---|
| `42` | integer | Digits only |
| `-17` | integer | Negative prefix |
| `3.14` | float | Decimal point |
| `true` / `false` | boolean | Case-sensitive |
| `~` | null | Tilde literal |
| `""` | empty string | Double quotes |
| Everything else | string | No quotes needed |

#### Explicit Type Casting

Force a type with parentheses after the key:

```synx
port(int) 8080
zip_code(string) 90210
flag(bool) true
ratio(float) 2
```

Available casts: `int`, `float`, `bool`, `string`.

### Nesting

Nesting uses 2-space indentation:

```synx
server
  host 0.0.0.0
  port 8080
  ssl
    enabled true
    cert /etc/ssl/cert.pem
```

Result:
```json
{
  "server": {
    "host": "0.0.0.0",
    "port": 8080,
    "ssl": {
      "enabled": true,
      "cert": "/etc/ssl/cert.pem"
    }
  }
}
```

An empty key with no value and no children creates an empty object: `metadata` → `{}`.

Maximum nesting depth: 128 levels.

### Arrays

Arrays use dash-prefixed items:

```synx
colors
  - red
  - green
  - blue

mixed
  - 42
  - hello
  - true
  - ~
```

Arrays can contain objects:

```synx
users
  -
    name Alice
    role admin
  -
    name Bob
    role user
```

Maximum array elements: 1,000,000.

### Multiline Values

Use the pipe operator (`|`) for block strings:

```synx
description |
  This is a long text
  that spans multiple lines.
  Each line is joined with a newline character.
```

Result: `"This is a long text\nthat spans multiple lines.\nEach line is joined with a newline character."`

Maximum block size: 1 MiB.

`|` trims every continuation line, so any indentation *inside* the text is lost. When the indentation is part of the content, use `|+`.

### Indent-Preserving Multiline (`|+`)

**New in 3.7.** `|+` opens a multiline block that keeps each line's indentation **relative to the first continuation line**:

```synx
prompt |+
  Outline:
    - step one
    - step two
      sub-step
  End.
```

Result:

```
Outline:
  - step one
  - step two
    sub-step
End.
```

The indent of the first non-empty continuation line becomes the **base indent** and is locked there. Everything at that depth starts at column 0 in the body; everything deeper keeps the extra spaces. The common leading whitespace that exists only for visual nesting under the parent key is removed, and nothing else is.

Use `|+` whenever the content is indent-sensitive — source code, SYNX examples inside a SYNX file, YAML/Python snippets, AI prompt scaffolds:

```synx
!active

snippet |+
  def factorial(n):
      if n <= 1:
          return 1
      return n * factorial(n - 1)
```

Notes:

- Everything else on the key line — type hints, constraints, marker chains — works exactly as with `|`.
- The 1 MiB block limit applies identically.
- Interior blank lines are **not** preserved in this version (same behaviour as `|`).
- Compatibility: a 3.6 parser reads `key |+` as the two-character string `"|+"`. That is the only input on which 3.6 and 3.7 parsers disagree. If you need a document to stay 3.6-portable, avoid `|+`; if you need the literal string, quote it (`key "|+"`).

### Comments

Three comment styles:

```synx
# Hash comment (single line)
// Slash comment (single line)

### Block comment
This is a multi-line comment.
Everything between the ### markers is ignored.
###
```

---

## Directives

Directives are top-level lines that begin with `!` or `#!`. They control the document's mode and metadata. A directive must appear before any key-value lines.

| Directive | Description |
|---|---|
| `!active` | Enable markers, constraints, and validation |
| `!lock` | Mark document as read-only (metadata flag) |
| `!include` | Include another SYNX file at the document root level |
| `!tool` | Reshape output as a tool-call envelope |
| `!schema` | Export a JSON Schema Draft 2020-12 |
| `!llm` | Annotate document for LLM consumption (metadata-only) |

### !active

Switches the document from static to active mode. Must appear on the very first non-comment line.

```synx
!active

db_host:env DATABASE_HOST
max_conn[type:int, min:1, max:1000] 100
app_name[required, type:string] MyApp
```

What it enables:
- **Markers** — `:env`, `:calc`, `:alias`, and all others
- **Constraints** — `[type:int, required, min:0]` syntax
- **Validation** — parsing fails with a structured error when a constraint is violated

`!active` applies to the entire document. There is no per-block activation. If you need mixed static/active sections, use separate files with `:include`.

Alternative hashbang form: `#!mode:active`

> **Warning:** `!active` must be the first content line (after optional comments). Placing it elsewhere makes it a string value, not a directive.

### !lock

Sets the `locked` flag on the parse result. The data tree is unchanged — it is purely advisory metadata for tooling.

```synx
!lock

name Alice
role admin
```

Tooling that respects `!lock` should prevent edits, overrides, or patch operations.

### !include (directive)

Includes another SYNX file and merges its contents at the root level. An optional alias assigns the included tree to a named key.

```synx
!include ./db.synx
!include ./auth.synx auth_config

name MyApp
```

- Path is relative to the current file's directory
- Maximum include depth: 16
- Circular includes produce an error
- Path traversal (`..`) is blocked by path-jail security

> **Tip:** `!include` is a directive (top-level). The `:include` marker does the same at value level: `db:include ./db.synx`.

### !tool

Reshapes the output into a tool-call envelope. The first key becomes the tool name, its children become parameters.

```synx
!tool

web_search
  query rust async
  lang en
  results 10
```

Output:
```json
{
  "tool": "web_search",
  "params": {
    "query": "rust async",
    "lang": "en",
    "results": 10
  }
}
```

### !schema

Causes the parser to emit a JSON Schema Draft 2020-12 object derived from the document's keys and constraints.

```synx
!schema

name[required, type:string]
port[type:int, min:1, max:65535]
```

### !llm

Metadata-only directive for LLM tooling. Does not change the parsed data tree.

```synx
!llm

persona Assistant
role You are a helpful coding assistant
tone professional and concise
context You have access to the user's codebase
```

---

## Markers

Markers are value-level directives that transform or resolve a value at parse time. They require `!active` mode. A marker is attached to the key with a colon (no space before it).

**General syntax:** `key:marker_name argument`

Markers can be chained: `key:marker1:marker2 value`

Type casts and constraints precede markers: `key(type)[constraints]:marker value`

### All Markers Summary

| Marker | Description |
|---|---|
| `:env` | Read a value from an environment variable |
| `:calc` | Evaluate a mathematical expression |
| `:alias` | Reference another key in the same document |
| `:secret` | Wrap a value as a secret (serializes to [SECRET]) |
| `:default` | Provide a fallback value |
| `:random` | Pick a random item from a child list |
| `:include` | Include and merge another SYNX file |
| `:import` | Alias for `:include` |
| `:template` | Interpolate a string template with {key} placeholders |
| `:split` | Split a string into an array |
| `:join` | Join an array into a string |
| `:unique` | Deduplicate an array |
| `:geo` | Select value by geographic region |
| `:i18n` | Look up a translation from a locale bundle |
| `:ref` | Reference another key, chain with other markers |
| `:clamp` | Clamp a numeric value to a min:max range |
| `:round` | Round a numeric value to N decimal places |
| `:format` | Format a value using printf-style pattern |
| `:map` | Map child values through a lookup table |
| `:once` | Generate a value once and cache it |
| `:version` | Compare a value against a version constraint |
| `:watch` | Read external file at parse time |
| `:fallback` | Provide a fallback file path |
| `:spam` | Rate-limit access to a value |
| `:prompt` | Attach an LLM prompt label |
| `:vision` | Reference an image path for vision models |
| `:audio` | Reference an audio file for audio models |
| `:inherit` | Inherit fields from parent objects (mixin) |

---

### :env

Read a value from an environment variable at parse time.

**Syntax:** `key:env VAR_NAME`

```synx
!active

db_host:env DATABASE_HOST
db_port:env DATABASE_PORT
secret_key:env APP_SECRET
```

If the variable is not set, the value resolves to `null`. Use `:default` chaining for a fallback:

```synx
!active
db_host:env:default:localhost DATABASE_HOST
port:env:default:8080 PORT
```

---

### :calc

Evaluate a mathematical expression using other key values.

**Syntax:** `key:calc expression`

Supports: `+`, `-`, `*`, `/`, `**`, `%`, `()`. Max expression length: 4096 characters.

```synx
!active

base_workers 4
worker_threads:calc base_workers * 2
memory_mb:calc base_workers * 512
half_mem:calc (base_workers * 512) / 2
```

Expressions are evaluated after all other keys have been resolved. Forward references work. Circular references produce a parse error.

---

### :alias

Create a reference to another key. The alias resolves to the target's value.

**Syntax:** `key:alias other_key`

Supports dot-paths for nested keys.

```synx
!active

primary_host db.internal
replica_host:alias primary_host

config
  host db.internal

reporting_host:alias config.host
```

---

### :secret

Wrap a value as a Secret type. Serializes to `[SECRET]` to prevent exposure in logs.

**Syntax:** `key:secret value`

```synx
!active

db_password:secret s3cureP@ss!
api_key:secret sk-abc123-xyz789
```

> The `:secret` marker wraps the literal value. Use `.reveal()` in your code to access the real value.

---

### :default

Provide a literal fallback value. Most commonly chained after `:env`.

**Syntax:** `key:marker:default:FALLBACK_VALUE argument`

```synx
!active

# If PORT is not set, use 8080
port:env:default:8080 PORT

# If HOST is not set, use localhost
host:env:default:localhost HOST

# Standalone default
log_level:default info
```

The fallback value is written after the last colon: `:env:default:8080 PORT`.

---

### :random

Pick a random item from a child list at parse time. Optional weight numbers control probability.

**Syntax:** `key:random [weight1 weight2 ...]`

```synx
!active

# Equal probability
greeting:random
  - Hello
  - Hi there
  - Hey

# Weighted: 70% common, 20% rare, 10% legendary
loot:random 70 20 10
  - Common Sword
  - Rare Shield
  - Legendary Helm
```

**Weight rules:**

| Situation | Behavior |
|---|---|
| No weights | All items equally probable |
| Sum = 100 | Used as-is |
| Sum ≠ 100 | Auto-normalized (proportions preserved) |
| Fewer weights than items | Remainder split equally |
| More weights than items | Extra weights ignored |

---

### :include / :import

Include another SYNX file and merge its contents at the current level.

**Syntax:** `key:include path/to/file.synx`

```synx
!active

db_data:include ./db.synx
db_user:alias db_data.user
db_name:alias db_data.name
```

`:import` is a complete alias for `:include` — both behave identically:

```synx
!active
db:import ./config/db.synx
cache:import ./config/cache.synx
```

Max depth: 16 levels. Max file size: 10 MiB. Circular includes are detected.

---

### :template

Interpolate a string template with `{key}` placeholders.

**Syntax:** `key:template "string with {placeholders}"`

```synx
!active

server
  host localhost
  port 8080
api_url:template http://{server.host}:{server.port}/api
```

Supports dot-paths: `{server.host}`, `{database.port}`.

---

### :split

Split a string value into an array.

**Syntax:** `key:split:DELIMITER value`

Keywords: `space`, `comma`, `pipe`, `dot`, `slash`, `dash`, `newline`, `semi`, `tab`. Default: comma.

```synx
!active

colors:split red, green, blue
# → ["red", "green", "blue"]

hosts:split:comma db1.internal,db2.internal,db3.internal
# → ["db1.internal", "db2.internal", "db3.internal"]

words:split:space hello world foo bar
# → ["hello", "world", "foo", "bar"]

segments:split:slash /usr/local/bin
# → ["", "usr", "local", "bin"]
```

---

### :join

Join an array of child values into a single string.

**Syntax:** `key:join:DELIMITER`

Keywords: `slash`, `pipe`, `comma`, `space`, `dot`, `dash`, `newline`, `semi`, `tab`. Default: comma.

```synx
!active

path:join:slash
  - home
  - user
  - documents
# → "home/user/documents"

env_list:join:pipe
  - development
  - staging
  - production
# → "development|staging|production"
```

---

### :unique

Remove duplicate elements from a child array, preserving original order.

**Syntax:** `key:unique`

```synx
!active

tags:unique
  - rust
  - systems
  - rust
  - open-source
# → ["rust", "systems", "open-source"]
```

Deduplication is case-sensitive string comparison.

---

### :geo

Select a value based on the runtime region setting.

**Syntax:** `key:geo`

```synx
!active

cdn:geo
  - US us-east.cdn.example.com
  - EU eu-west.cdn.example.com
  - AP ap-south.cdn.example.com
```

Uses `options.region` to select. If no match, the first item is the fallback.

---

### :i18n

Select a translated value based on the runtime locale.

**Syntax:** `key:i18n`

```synx
!active

greeting:i18n
  en Hello
  ru Привет
  de Hallo
```

**Pluralization** — append the count field name: `:i18n:count_field`

```synx
!active

items_label:i18n:item_count
  en
    one {count} item
    other {count} items
  ru
    one {count} предмет
    few {count} предмета
    many {count} предметов
    other {count} предметов
```

Locale is set via `options.lang` (default: `en`). Plural categories follow CLDR rules (en: one/other, ru: one/few/many/other). `{count}` is substituted with the referenced field's value.

---

### :ref

Reference another key and pass the value to subsequent markers in the chain.

**Syntax:** `key:ref target_key`

```synx
!active

base_rate 100
adjusted:ref:calc:*2 base_rate
```

The shorthand `:ref:calc:*2 base_rate` resolves `base_rate` → 100, then computes `100 * 2` = 200.

---

### :clamp

Clamp a numeric value to a minimum–maximum range.

**Syntax:** `key:clamp:MIN:MAX value`

```synx
!active

volume:clamp:0:100 150
# → 100

brightness:clamp:0:255 -10
# → 0

opacity:clamp:0.0:1.0 0.75
# → 0.75
```

Error if `MIN > MAX`.

---

### :round

Round a numeric value to N decimal places.

**Syntax:** `key:round:PLACES value`

```synx
!active

price:round:2 19.999
# → 19.99

pi:round:4 3.14159265
# → 3.1416

whole:round:0 42.7
# → 43
```

Default: 0 decimal places (rounds to integer).

---

### :format

Format a value using a printf-style pattern.

**Syntax:** `key:format:PATTERN value`

```synx
!active

price:format:%.2f 19.9
# → "19.90"

code:format:%04d 42
# → "0042"
```

Supported patterns: `%.Nf` (float), `%d` (int), `%0Nd` (zero-padded). Max width/precision: 4096/1024.

---

### :map

Map child values through a lookup table.

**Syntax:** `key:map:source_key`

```synx
!active

status_labels
  200 OK
  404 Not Found
  500 Internal Server Error

codes
  - 200
  - 404
  - 500

labels:map:status_labels
  - 200
  - 404
  - 500
# → ["OK", "Not Found", "Internal Server Error"]
```

No match returns `null`.

---

### :once

Generate a value once at first parse and cache it for subsequent reads.

**Syntax:** `key:once:TYPE`

Types: `uuid`, `timestamp`, `random`.

```synx
!active

instance_id:once:uuid
# → "550e8400-e29b-41d4-a716-446655440000" (same on every read)

created_at:once:timestamp
# → "2026-04-01T12:00:00Z" (frozen at first parse)
```

The cached value is stored in a `.synx.lock` sidecar file. Re-parsing without cache produces a new value.

---

### :version

Compare a value against a semantic version constraint. Returns a boolean.

**Syntax:** `key:version:OP:REQUIRED value`

Operators: `>=`, `<=`, `>`, `<`, `==`, `!=`.

```synx
!active

app_ok:version:>=:1.0.0 1.2.3
# → true

engine_ok:version:>=:2.0.0 1.9.5
# → false
```

Parses semantic versions (x.y.z…).

---

### :watch

Read an external JSON or SYNX file at parse time and inject its content.

**Syntax:** `key:watch:KEY_PATH ./file.json`

```synx
!active

# Read entire file
all_flags:watch ./flags.synx

# Extract a specific key from a JSON file
db_host:watch:database.host ./infra.json
```

Optional key path after `:watch:` extracts a specific nested value. Max depth: 16, max size: 10 MiB. Path-jail enforced.

> Despite the name, `:watch` does **not** set up a file system watcher. It reads the file once at parse time.

---

### :fallback

Provide a fallback file path. If the primary source is missing, the fallback is used.

**Syntax:** `key:fallback:FALLBACK_PATH value`

```synx
!active

config:fallback:./defaults.synx ./overrides.synx
theme:fallback:./themes/default.synx ./themes/custom.synx
```

Also applies if the value is null or empty.

---

### :spam

Rate-limit access to a value.

**Syntax:** `key:spam:MAX:WINDOW_SEC target`

```synx
!active

api_endpoint:spam:5:60 https://api.example.com/data
# Allow max 5 accesses per 60 seconds.
# On the 6th call: "SPAM_ERR: exceeded 5 calls per 60s"
```

Window defaults to 1 second if omitted. The counter lives in process memory and resets on restart.

---

### :prompt

Attach an LLM prompt label to a key.

**Syntax:** `key:prompt:LABEL value`

```synx
!llm

system:prompt:system You are a helpful assistant. Answer concisely.
context:prompt:user The user is building a Rust web service.
```

---

### :vision

Reference an image file path for vision model input. No engine transformation — applications detect via metadata.

**Syntax:** `key:vision path/to/image.png`

```synx
!llm

screenshot:vision ./screenshots/app_ui.png
diagram:vision ./docs/architecture.png
```

Supported formats: PNG, JPEG, GIF, WEBP.

---

### :audio

Reference an audio file for audio model input. No engine transformation.

**Syntax:** `key:audio path/to/file.mp3`

```synx
!llm

recording:audio ./meetings/standup_2026-04-01.mp3
```

Supported formats: MP3, WAV, OGG, FLAC.

---

### :inherit

Inherit all fields from one or more parent objects (mixin-style merge). Child fields take priority.

**Syntax:** `child:inherit parent1 parent2 ...`

```synx
!active

base
  host localhost
  port 8080
  debug false

production:inherit base
  host prod.example.com
  debug false

staging:inherit base production
  host staging.example.com
# staging gets: host staging.example.com, port 8080, debug false
```

`:inherit` runs as a pre-pass before all other markers. Parents are merged left to right, then the child's own fields override the result.

---

## Constraints

Constraints appear in square brackets after the key name, before any marker or value. Multiple constraints are comma-separated. Constraints only work in `!active` mode.

**Syntax:** `key[constraint1, constraint2:arg] value`

**With markers:** `key[constraint1, constraint2:arg]:marker value`

| Constraint | Argument | Description |
|---|---|---|
| `type` | `int\|float\|bool\|string\|array\|object` | Assert value type |
| `required` | none | Value must be present and non-null |
| `min` | number | Minimum value or minimum length |
| `max` | number | Maximum value or maximum length |
| `pattern` | regex string | String must match the regex |
| `enum` | `value1\|value2\|…` | Value must be one of the listed options |
| `readonly` | none | Value cannot be overridden |

```synx
!active

port[type:int, min:1024, max:65535] 8080
host[required, type:string] localhost
mode[enum:development|staging|production] development
slug[type:string, pattern:^[a-z0-9-]+$] my-app
workers[type:int, min:1, max:32] 4

# Constraints with markers
db_port[type:int, min:1024]:env:default:5432 DB_PORT
api_host[required]:env API_HOST
```

### type

Assert the value's data type.

```synx
!active
port[type:int] 8080
name[type:string] Alice
ratio[type:float] 3.14
enabled[type:bool] true
```

### required

Value must be present and non-null. Parsing fails if the field is missing or empty.

```synx
!active
api_key[required]:env API_KEY
name[required, min:1] Alice
```

### min / max

For **numbers** — restricts the value range. For **strings** and **arrays** — restricts the length.

```synx
!active
volume[min:0, max:100] 75
username[min:3, max:30] alice
password[min:8] s3cur3P@ss
```

### pattern

String must match a regular expression (RE2 syntax).

```synx
!active
email[pattern:^[^@]+@[^@]+\.[^@]+$] user@example.com
slug[pattern:^[a-z0-9-]+$] my-cool-app
```

### enum

Value must be one of the listed options, separated by `|`.

```synx
!active
mode[enum:development|staging|production] development
color[enum:red|green|blue] green
```

### readonly

Value cannot be overridden by includes or patches.

```synx
!active
version[readonly] 3.6.0
```

---

## Marker Chaining

Markers chain with the `:` separator. Each marker in the chain processes the result of the previous one:

```synx
!active

# Read PORT from env, fall back to 8080 if not set
port:env:default:8080 PORT

# Resolve base_rate, then multiply by 2
total:ref:calc:*2 base_rate

# Read env with constraint
db_port[type:int, min:1024]:env:default:5432 DB_PORT
```

The general order is: `key(type)[constraints]:marker1:marker2:... value`

---

## SYNXL — Record Streams (`.synxl`)

**New in 3.7.0.** SYNXL ("SYNX Lines") is a **record-stream format for datasets** — the SYNX-native counterpart of JSONL and CSV. It is a separate format with its own version axis and its own normative specification: [`docs/spec/SYNXL-1-NORMATIVE.md`](../spec/SYNXL-1-NORMATIVE.md) (format version **1**). It embeds SYNX 3.7 for the nested part of a record.

A SYNXL document is **not** a SYNX document: its root is a sequence of records, not an object. `Synx.parse` will not read one — use the SYNXL entry points below.

### The shape of a document

A document declares a **field list** once, then carries records:

```synxl
!synxl 1
!fields id[type:int, required] ; score[type:float] ; messages[block]

1 ; 0.91
  messages
    - role system
      content You are a helpful assistant.
    - role user
      content |+
          def f(x):
              return x + 1

2 ; 0.74
  messages
    - role user
      content Привет
```

Record 1 projects to:

```json
{"id":1,"messages":[{"content":"You are a helpful assistant.","role":"system"},{"content":"def f(x):\n    return x + 1","role":"user"}],"score":0.91}
```

The rules that matter in practice:

| Element | Meaning |
|---|---|
| `!synxl 1` | Mandatory prologue. Without it a `;`-delimited text file is indistinguishable from a dataset. |
| `!fields …` | Ordered field declarations, `;`-separated. Same `(type)` and `[constraints]` surface as a SYNX key line. |
| Record line | Any line at indent 0 that is not a prologue, a field list, or a comment. Split on `;`, positionally matched. |
| `[block]` field | Value comes from the record's **indented block**, parsed by the ordinary SYNX 3.7 parser — nesting, lists and `|+` all work there. |
| Empty inline part | JSON `null`. An empty string is written `""` — "absent" and "empty text" are distinct, unlike CSV. |
| `;` alone on a line | The all-null record. |
| Indent 0 | Structural. That single-byte rule is what makes streaming, appending and sharding possible. |

Other things worth knowing:

- **Schema evolution.** A new `!fields` line anywhere in the file replaces the schema for records after it. Existing records are untouched, so a producer that gains a column just appends.
- **No inline comment stripping.** In a dataset `#` and `//` are content — hashtags, URLs, code — so SYNXL does not truncate values at them the way SYNX does. Comments exist only as whole lines at indent 0.
- **Diagnostics, not silence.** A record with too few or too many parts, a failed cast, an unknown block key — each is reported as a diagnostic on the result and the record keeps parsing. Nothing is dropped silently.
- **Validation is opt-in.** Declared `[constraints]` are recorded as metadata and only enforced when you ask, because `pattern:` means a regex per cell.
- **Untrusted input is safe.** Directives are disabled inside a record's block, so a dataset row can never become an `!include` file-read primitive.
- **Size.** There is no whole-file limit — datasets are routinely gigabytes. The cap is 16 MiB *per record*, and an oversized record is truncated with a diagnostic rather than failing the file.

### Reading it from code

```rust
// Rust — synx-core
use std::fs::File;
use std::io::BufReader;
use synx_core::synxl::{self, SynxlStreamReader};

// Whole document in memory
let doc = synxl::parse_lines(&std::fs::read_to_string("chat.synxl")?)?;
println!("version {}, {} records", doc.version, doc.len());

let first = doc.records[0].as_object().unwrap();   // records are Value::Object
println!("{:?}", first.get("id"));                 // Some(Int(1))
println!("{}", doc.to_json());                     // canonical JSON array
println!("{}", doc.to_ndjson());                   // one object per line
for d in &doc.diagnostics { eprintln!("{d}"); }

// Streaming off disk — live memory is one record, whatever the file size
for record in SynxlStreamReader::new(BufReader::new(File::open("chat.synxl")?))? {
    let record = record?;                          // an Err ends the document
    let obj = record.value.as_object().unwrap();   // + record.index / .line / .diagnostics
    println!("{:?}", obj.get("id"));
}

// Writing — quoting and promotion of multi-line values to blocks are automatic
let text = synxl::write_lines(
    &[synxl::FieldDecl::new("id"), synxl::FieldDecl::new_block("messages")],
    &doc.records,
)?;

// Opt-in constraint checking
let checked = synxl::parse_lines_with(&src, &synxl::SynxlOptions { validate: true })?;
```

`SynxlReader` (borrowed text) and `SynxlReaderOwned` (owns its `String`, so it can be stored or returned) are the in-memory streaming variants; `SynxlStreamReader` streams the document itself from any `io::BufRead`.

```typescript
// TypeScript / JavaScript — @aperturesyndicate/synx-format
import { Synx } from '@aperturesyndicate/synx-format';

const doc = Synx.parseSynxl(text);
doc.version;                     // 1
doc.records[0].values.id;        // 1
doc.records[0].values.messages;  // [{ role: 'system', content: '…' }, …]
doc.records[0].line;             // 1-based source line
doc.diagnostics;                 // SynxlDiagnostic[]

Synx.synxlToJSON(text);          // canonical JSON array
Synx.synxlToNDJSON(text);        // one object per line

// Streaming — sync over text, async over a file or a chunk stream
for (const record of Synx.streamSynxl(text)) console.log(record.values.id);
for await (const record of Synx.streamSynxlFile('chat.synxl')) console.log(record.values.id);

// From disk
const fromDisk = Synx.loadSynxlSync('chat.synxl');   // or: await Synx.loadSynxl('chat.synxl')

// Writing, and saving to disk
const out = Synx.writeSynxl([{ id: 1, messages: [{ role: 'user', content: 'Hi' }] }]);
Synx.saveSynxlSync('out.synxl', records);

// Opt-in constraint checking
Synx.parseSynxl(text, { validate: true }).diagnostics;
```

Hard errors throw `SynxlError` (a subclass of `SynxError`) carrying a machine-readable `condition` and the 1-based `line`.

```python
# Python — synx_native
import synx_native as synx

doc = synx.synxl_parse(text)
doc.version                      # 1
len(doc)                         # 2
doc.records[0]["id"]             # 1 — records are plain dicts
doc.records[0]["messages"]       # [{'role': 'system', 'content': '…'}, …]
doc.diagnostics                  # list of dicts
doc.to_json(); doc.to_ndjson()

doc = synx.synxl_load("chat.synxl")

# Streaming — one record in memory at a time
for record in synx.synxl_stream(text): ...
for record in synx.synxl_stream_file("chat.synxl"): ...

# The *_records variants yield SynxlRecord objects instead of bare dicts
for record in synx.synxl_stream_records(text):
    print(record.index, record.line, record["id"], record.diagnostics)

# Writing: `fields` declares the columns; omit it to synthesize from the keys
synx.synxl_write(records, fields=["id", {"name": "messages", "block": True}])

# Opt-in constraint checking; hard errors raise synx.SynxlError
synx.synxl_parse(text, validate=True).diagnostics
synx.synxl_to_json(text); synx.synxl_to_ndjson(text)
```

### CLI

Every `.synxl` input is streamed, so memory stays at one record no matter how large the file is.

```bash
# Canonical JSON array, or one object per line
synx synxl parse chat.synxl
synx synxl parse chat.synxl --format ndjson --output chat.ndjson

# Exit 0 = ok, 1 = the document violates the spec, 2 = I/O failure
synx synxl validate chat.synxl
synx synxl validate chat.synxl --constraints --strict   # also enforce [constraints]; diagnostics fail too

# Conversion, both directions
synx synxl convert chat.synxl --to jsonl
synx synxl convert chat.synxl --to csv --block-json     # block fields have no CSV form without this
synx synxl convert data.jsonl --to synxl
synx synxl convert data.csv   --to synxl --delimiter ';'

# Shard into standalone documents — each shard repeats the prologue and the
# field list in effect at the split point
synx synxl split chat.synxl -n 10000 --output-dir shards/ --prefix part
```

### Why not JSONL or CSV

Against **JSONL**: field names and structural punctuation are not repeated per record, which is the dominant token cost of JSONL for LLM consumption; multi-line text stays readable instead of `\n`-escaped; types are declared once rather than inferred per record. The trade is that SYNXL records are homogeneous by construction — heterogeneous shapes need a new field list or a separate file.

Against **CSV**: types and constraints are declared; `null` and the empty string are distinguishable; nesting and multi-line text need no escaping; comments exist; the schema may evolve mid-file. There are no quote-doubling rules and no dialects — the delimiter is always `;`.

### Support

SYNXL is implemented in **Rust** (`synx-core`), **TypeScript** (`@aperturesyndicate/synx-format`), **Python** (`synx_native`), and the **CLI** (`synx synxl`). The native parsers for C++, Dart, .NET, Go, Java, Swift and Godot/GDScript implement SYNX 3.7 — including `|+` — but do **not** read `.synxl` yet.

---

## Language Bindings

SYNX has 12 official language bindings that all pass the same conformance test suite:

| Language | Package | Key API |
|---|---|---|
| **Rust** | `synx-core` (crates.io) | `Synx::parse()`, `parse_active()`, `stringify()`, `format()`, `diff()`, `compile()`, `decompile()` |
| **CLI** | `synx-cli` (cargo install) | `synx parse`, `synx diff`, `synx format`, `synx compile`, `synx query` |
| **JavaScript/TS** | `@aperturesyndicate/synx-format` (npm) | `Synx.parse()`, `loadSync()`, `toJSON()`, `toYAML()`, `diff()` |
| **Python** | `synx-format` (PyPI) | `synx_native.parse()`, `stringify()`, `format()`, `diff()`, `compile()` |
| **C#** | `APERTURESyndicate.Synx` (NuGet) | `SynxFormat.Parse()`, `ParseActive()`, `Deserialize<T>()`, `Serialize<T>()` |
| **C** | `synx.h` + `libsynx_c` | `synx_parse()`, `synx_stringify()`, `synx_diff()`, `synx_compile()` |
| **C++** | `synx/synx.hpp` (header-only) | `synx::parse()`, `synx::stringify()`, `synx::diff()` |
| **Go** | `bindings/go` (cgo) | `synx.Parse()`, `synx.Stringify()`, `synx.Diff()` |
| **Swift** | `bindings/swift` (SPM) | `SynxEngine.parse()`, `stringify()`, `diff()`, `compile()` |
| **Kotlin/JVM** | `synx-engine` (JNA) | `SynxEngine.parse()`, `stringify()`, `diff()` |
| **WebAssembly** | `bindings/wasm` | `parse()`, `stringify()`, `format()`, `diff()`, `compile()` |
| **Mojo** | `bindings/mojo` (CPython interop) | `parse_json()`, `stringify_json()`, `diff_json()` |

All of them read SYNX 3.7, `|+` included. The table covers the **SYNX** API only — the separate **SYNXL** surface (`parse_lines` / `parseSynxl` / `synxl_parse` / `synx synxl`) exists in Rust, TypeScript, Python and the CLI, and nowhere else yet.

---

## Binary Format & Diff

### Binary Format (.synxb)

SYNX supports a compact binary encoding for fast parsing and smaller file sizes:

```bash
# Compile to binary
synx compile config.synx
# → config.synxb

# Decompile back to text
synx decompile config.synxb
# → config.synx
```

Binary SYNX is round-trip safe — compile then decompile produces identical output.

### Structural Diff

Compare two SYNX documents and get typed change operations:

```bash
synx diff old.synx new.synx
```

Returns JSON with `Added`, `Removed`, and `Modified` operations with dot-separated key paths.

---

## JSON Schema

### Generation

Use `!schema` to export a JSON Schema Draft 2020-12 from your document:

```synx
!schema

name[required, type:string]
port[type:int, min:1, max:65535]
mode[enum:development|staging|production]
```

### Validation

Validate JSON against a schema:

```bash
synx json-validate data.json schema.json
```

---

## Tools & Editors

### VS Code

Full language support via the SYNX extension: syntax highlighting, real-time diagnostics, IntelliSense (28 markers, 7 constraints), Go to Definition, Find References, formatting, color preview, inlay hints, live preview panel.

```bash
code --install-extension APERTURESyndicate.synx-vscode
```

### synx-lsp

Standalone Language Server (LSP over stdio) for any editor:

```bash
cargo install --path crates/synx-lsp
```

Capabilities: real-time diagnostics (15 checks), completion (markers, constraints, directives), document symbols.

Supported editors: Neovim, Helix, Zed, Emacs, JetBrains, Sublime Text, Visual Studio.

### MCP Server

Connect Claude Desktop (or any MCP client) to `synx-mcp`:

6 tools: `validate`, `parse`, `format`, `read_path`, `write_path`, `apply_patch`.

---

## Packages

SYNX has a package system for distributing configuration and custom WASM markers.

### Package Types

- **SYNX Config** — Reusable configuration files and templates
- **WASM Markers** — Custom markers compiled to WebAssembly

### Registry

Packages are published to `synx.aperturesyndicate.com`.

### CLI Commands

```bash
# Create a new package
synx create

# Install a package
synx install @scope/package-name

# Publish a package
synx login
synx publish

# Search for packages
synx search keyword

# Package info
synx info @scope/package-name
```

### Package Manifest (synx-pkg.synx)

```synx
name @myscope/my-package
version 1.0.0
description My reusable SYNX config
type config
license MIT
```

### WASM Markers

Custom markers compiled to WebAssembly using the SYNX ABI v1:

- `synx_alloc()` — memory allocation
- `synx_markers()` — list supported markers
- `synx_apply()` — apply a marker to a value

8 capability types: `string`, `fs`, `net`, `env`, `encoding`, `hashing`, `time`, `random`.

---

## Security

SYNX is designed to be **safe by default** — no code execution, no eval, no network calls from the parser.

| Risk | SYNX | YAML |
|---|---|---|
| Code execution | **No** | Yes (`!!python/object/apply`) |
| Network calls | **No** | Depends on loader |
| Shell injection | **No** — `:calc` uses safe whitelist operators | Depends on loader |

### Built-in Protections

| Protection | Limit |
|---|---|
| Path jail | `:include`, `:watch`, `:fallback` can't escape project root |
| Include depth | Max 16 levels (configurable) |
| File size | Included files > 10 MiB rejected |
| Calc expression | Max 4096 characters |
| Env isolation | Optional sandboxed env map |

---

## Performance

Input bounds: 16 MiB max file, 128 nesting levels, 1M array elements.

### Rust (criterion)

| Benchmark | Time |
|---|---|
| `Synx::parse` (110 keys) | ~39 µs |
| `parse_to_json` (110 keys) | ~42 µs |
| `Synx::parse` (4 keys) | ~1.2 µs |

### Python (10K iterations)

| Parser | µs/parse |
|---|---|
| `json.loads` (3.3 KB) | 13.04 µs |
| `synx_native.parse` (2.5 KB) | 55.44 µs |
| `yaml.safe_load` (2.5 KB) | 3,698 µs |

SYNX parses **67× faster** than YAML in Python.

**Fuzzing:** 7,177 regression test cases across 3 fuzz targets (parser, binary codec, formatter).

---

## FAQ

**Why not YAML?**  
YAML has many surprising edge cases: `NO` is boolean, `1:23` is a sexagesimal number, the Norway problem (`NO` as country code), multi-document streams, implicit type coercion. SYNX avoids all of these by design.

**Can I use tabs?**  
No. SYNX uses spaces only (2-space canonical indent). Tab characters are a parse error.

**Do I need quotes?**  
Never. The value is everything after the first space. If you need an empty string, use `""`.

**Is SYNX a superset of JSON?**  
No. SYNX is a different format that represents the same data model. Use `synx parse` / `synx convert` to convert between them.

**What is the frozen spec?**  
SYNX v3.6.0 is frozen as of April 2026 and remains the canonical interoperability baseline. New surface syntax is **additive** and arrives in new normative version files: 3.7 adds `|+` and nothing else, so every 3.6 document parses identically under a 3.7 parser.

**What is SYNXL, and do I need it?**  
SYNXL (`.synxl`) is a separate record-stream format for datasets — see [SYNXL — Record Streams](#synxl--record-streams-synxl). Use `.synx` for configuration and single documents; use `.synxl` when you have many homogeneous records (training samples, eval sets, table exports) and would otherwise reach for JSONL or CSV. The two formats have independent version numbers.

---

<p align="center">
  <strong>SYNX v3.7</strong> — Built for AI and humans by <a href="https://aperturesyndicate.com">APERTURESyndicate</a>
</p>
