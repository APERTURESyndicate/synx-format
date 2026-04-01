**Version:** SYNX 3.6.0 · **Core frozen** · MIT License · [Repository](https://github.com/APERTURESyndicate/synx-format)

SYNX is a data format that gets out of your way. No quotes. No commas. No brackets. Just keys, values, and — when you need it — a powerful engine that makes your config file think for itself. This guide walks you through everything from "what is a key-value pair" to deploying production configs across twelve programming languages.

---

## What is SYNX?

Imagine JSON, but without the punctuation tax. Here is the same data in three formats:

```json
{
  "server": {
    "host": "localhost",
    "port": 8080
  },
  "debug": true,
  "tags": ["web", "api"]
}
```

```yaml
server:
  host: localhost
  port: 8080
debug: true
tags:
  - web
  - api
```

```synx
server
  host localhost
  port 8080
debug true
tags
  - web
  - api
```

SYNX wins on **readability** and **AI token cost** — about 40% fewer tokens than JSON and 24% fewer than YAML on the same data. It also wins on **power**: add `!active` to the top and your config file can read environment variables, do math, pick random values, reference other keys, and validate its own data.

> **Formal specification:** [`docs/spec/SYNX-3.6-NORMATIVE.md`](../spec/SYNX-3.6-NORMATIVE.md). This guide prioritizes understanding. The spec is the final word on edge cases.

---

## Quick Start

### Install in 30 seconds

Pick your language from the table, run the command, and you're ready.

| Language | Install command | Package |
|----------|----------------|---------|
| **Rust** (library) | `cargo add synx-core` | `synx-core` on crates.io |
| **Rust** (CLI binary) | `cargo install synx-cli --locked` | `synx-cli` |
| **Node.js / Browser** | `npm install @aperturesyndicate/synx-format` | npm |
| **Python** | `pip install synx-format` | PyPI |
| **C# / .NET** | `dotnet add package APERTURESyndicate.Synx` | NuGet |
| **Go** | clone monorepo · `bindings/go` | cgo module |
| **Swift** | SwiftPM · `bindings/swift` | local package |
| **Kotlin / JVM** | `publishToMavenLocal` · `bindings/kotlin` | JNA |
| **C++** | CMake · `bindings/cpp` | header-only |
| **C** | link `synx-c` · `synx.h` | C ABI |
| **Mojo** | `pip install synx-format` + Mojo interop | CPython |
| **WASM** | build from `bindings/wasm` | wasm-bindgen |

### Write your first `.synx` file

Create `hello.synx`:

```synx
name World
language SYNX
version 3.6.0
features
  - no quotes
  - no commas
  - just works
```

### Parse it

```javascript
// JavaScript / TypeScript
import { Synx } from '@aperturesyndicate/synx-format';
import { readFileSync } from 'fs';

const data = Synx.parse(readFileSync('hello.synx', 'utf8'));
console.log(data.name);        // "World"
console.log(data.features[1]); // "no commas"
```

```python
# Python
import synx_native, pathlib

data = synx_native.parse(pathlib.Path('hello.synx').read_text())
print(data['name'])           # World
print(data['features'][1])    # no commas
```

```rust
// Rust
use synx_core::Synx;
use std::fs;

let data = Synx::parse(&fs::read_to_string("hello.synx")?, None)?;
```

```bash
# CLI
synx parse hello.synx
```

```csharp
// C# / .NET
using Synx;

var text = File.ReadAllText("hello.synx");
var data = SynxFormat.Parse(text);
```

That's it. You can go much further — but this is all you need to start.

---

## Core Syntax

### Keys and Values

The rule is simple: **everything after the first space on a line is the value**.

```synx
name Alice
city New York City
description I love programming, coffee, and late nights
```

No quotes needed. Spaces inside values are fine. The first space is the separator — everything after it belongs to the value.

**Key naming rules:**
- Use letters, digits, `_`, `-` (not at the start)
- No spaces (use `snake_case` or `camelCase`)
- Cannot begin with `-`, `#`, `/`, `!` (these are reserved)

### Automatic Type Detection

SYNX automatically converts values to the right type:

```synx
greeting  Hello World    # → string "Hello World"
count     42             # → integer 42
ratio     3.14           # → float 3.14
enabled   true           # → boolean true
disabled  false          # → boolean false
nothing   null           # → null (also: ~)
```

**Force a specific type** using `(type)` cast syntax:

```synx
zip_code(string)  90210   # → "90210" not 90210
id(int)           007     # → 7
score(float)      100     # → 100.0
flag(bool)        1       # → true
```

| Cast | What it does | Example |
|------|-------------|---------|
| `(string)` | Always a string, even if it looks like a number | `zip(string) 01234` → `"01234"` |
| `(int)` | Integer; decimals are truncated | `n(int) 3.9` → `3` |
| `(float)` | Floating-point number | `x(float) 100` → `100.0` |
| `(bool)` | Boolean; `1`/`yes`/`on` → `true` | `f(bool) 1` → `true` |

### Nesting (Objects)

Indent child keys by **exactly 2 spaces** to create a nested object. A key with no value and children becomes an object:

```synx
database
  host localhost
  port 5432
  credentials
    user admin
    password secret
```

Resulting JSON:

```json
{
  "database": {
    "host": "localhost",
    "port": 5432,
    "credentials": {
      "user": "admin",
      "password": "secret"
    }
  }
}
```

> **Only spaces, never tabs.** SYNX is strict about this — tabs cause a parse error. Your editor's "convert tabs to spaces" setting is your friend.

### Arrays (Lists)

Lines beginning with `- ` (dash + space) create array items:

```synx
fruits
  - apple
  - banana
  - cherry
```

→ `{ "fruits": ["apple", "banana", "cherry"] }`

**Array of objects** — each `-` starts a new object, children are its properties:

```synx
users
  - name Alice
    role admin
    age 30
  - name Bob
    role viewer
    age 25
```

→

```json
{
  "users": [
    { "name": "Alice", "role": "admin", "age": 30 },
    { "name": "Bob", "role": "viewer", "age": 25 }
  ]
}
```

**Mixed arrays** — arrays can contain both primitive values and objects:

```synx
items
  - simple string
  - 42
  - key value
    nested true
```

### Comments

Three comment styles, all ignored by the parser:

```synx
# Hash comment — Python / shell style
// Double slash comment — JavaScript / C++ style

name Alice  # Inline after a value
port 8080   // Also works inline

###
  Block comment — everything between triple-hash fences
  is completely ignored, including blank lines.
###
```

### Multiline Text

Use `|` after a key to start a text block. All indented lines below are joined with newlines:

```synx
message |
  Dear user,
  Your account has been activated.
  Welcome to the platform!
```

→ `"Dear user,\nYour account has been activated.\nWelcome to the platform!"`

For an inline newline, use the escape sequence `/n`:

```synx
banner Welcome!/nEnjoy the experience!
```

→ `"Welcome!\nEnjoy the experience!"`

---

## Two Modes: Static and Active

Every SYNX file works in one of two modes. Understanding this is the most important concept in SYNX.

### Static Mode (the default)

No `!active` at the top = **plain data file**. Markers like `:env` and `:calc` are treated as literal text. Nothing executes. This is perfectly safe to use with any input.

```synx
# This is static mode. No magic.
server
  host localhost
  port 8080
  workers 4
```

Use static mode when you want a simple, predictable config — equivalent to JSON or YAML in terms of behavior.

### Active Mode

Put `!active` as the **very first line** of the file. Now **markers and constraints come to life** — the engine reads environment variables, evaluates math, validates data, and more.

```synx
!active

port:env:default:8080  PORT
workers:calc              cpu_count * 2
debug                     false
```

> **Security note:** Without `!active`, a `.synx` file cannot read environment variables, run calculations, or access the filesystem. Static mode is safe to parse from untrusted sources.

**Which parse function to call:**

| What you want | What to call |
|--------------|-------------|
| Plain data, no engine | `parse` / `Synx::parse` / `SynxFormat.Parse` |
| Markers and constraints resolved | `parse_active` / `Synx::parse_active` / `SynxFormat.ParseActive` |
| `!tool` document for LLM tool calls | `parse_tool` / `Synx::parse_tool` / `SynxFormat.ParseTool` |

---

## Markers — The Active Engine

Markers are written with a colon right after the key name: `key:marker value`. They only execute in `!active` mode.

You can chain multiple markers: `port:env:default:8080 PORT` means "read env var PORT, fall back to 8080".

### :env — Read Environment Variables

Reads a value from the system's environment variables at parse time.

```synx
!active

port:env       PORT
api_key:env    API_KEY
home_dir:env   HOME
```

If the environment variable doesn't exist, the value becomes `null`. Use `:default` to provide a fallback:

```synx
!active

port:env:default:3000  PORT    # 3000 if PORT is not set
host:env:default:localhost  HOST
debug:env:default:false  DEBUG
```

> **Testing tip:** Most parsers let you inject a fake environment dictionary so you can test without touching real env vars. See the language-specific examples below.

### :calc — Safe Arithmetic

Evaluates an arithmetic expression. Can reference other keys in the file by name.

```synx
!active

price       100
tax:calc    price * 0.2
total:calc  price + tax
discount:calc  price * 0.15
final:calc  total - discount
```

Result: `tax = 20`, `total = 120`, `discount = 15`, `final = 105`.

**Supported operators:** `+`, `-`, `*`, `/`, `%` (remainder), `(`, `)`.

**Referencing nested keys** using dot-path:

```synx
!active

server
  base_port  8000
  api_port:calc  server.base_port + 1
  admin_port:calc  server.base_port + 2
```

> **No arbitrary code.** `:calc` uses a safe recursive-descent evaluator — no `eval()`, no shell commands, no file access. Only pure math.

### :alias — Reference Another Key

Copies the value of another key. If the source changes, the alias stays in sync.

```synx
!active

primary_email   hello@example.com
billing_email:alias   primary_email
support_email:alias   primary_email
```

→ All three resolve to `"hello@example.com"`.

**Dot-path aliases** let you reference nested keys:

```synx
!active

database
  host  localhost
  port  5432

db_host:alias  database.host
db_port:alias  database.port
```

> **Circular alias detection:** If key A aliases B and B aliases A, the parser throws an error rather than looping forever.

### :secret — Hidden Values

The value is stored and accessible in your program, but automatically masked in logs, string output, and JSON serialization.

```synx
!active

api_key:secret    sk-abc123def456
db_password:secret  P@ssw0rd!
jwt_secret:secret   my-very-long-secret-key
```

When you print the object or serialize it, secrets show as `"[SECRET]"`. To read the real value in your code:

```javascript
const key = data.api_key;
console.log(String(key));   // "[SECRET]"  — safe to log
console.log(key.reveal());  // "sk-abc123def456"  — use for API calls only
```

```python
key = data['api_key']
print(key)          # [SECRET]
print(key.reveal()) # sk-abc123def456
```

> **Rule:** Only call `.reveal()` at the point of use — when making an API call, opening a database connection, etc. Never log, store, or transmit the revealed value.

### :default — Fallback Value

Sets a fallback value if the key is `null` or missing. Most useful combined with `:env`.

```synx
!active

# Standalone default (if value not provided or null)
theme:default  dark
locale:default  en-US

# Combined with :env (the most common pattern)
port:env:default:8080    PORT
host:env:default:0.0.0.0  HOST
```

### :random — Random Selection

Picks one item from a list at parse time. Useful for A/B testing, load distribution, or generating varied responses.

**Equal probability:**

```synx
!active

greeting:random
  - Hello!
  - Hi there!
  - Welcome!
  - Hey!
```

Each item has a 25% chance. A different one is chosen every time the file is parsed.

**Weighted probability** (numbers after `:random` are percentages matching item order):

```synx
!active

loot_quality:random  70 25 5
  - Common
  - Rare
  - Legendary
```

70% Common, 25% Rare, 5% Legendary.

**Percentage rules:**

| Situation | Behavior |
|-----------|----------|
| No percentages | All items equally likely |
| Sum = 100 | Used as-is |
| Sum ≠ 100 | Normalized automatically (proportions preserved) |
| Fewer percentages than items | Remaining items split the leftover evenly |
| More percentages than items | Extra percentages ignored |

**Partial weights example** — first item gets 80%, remaining 20% split between two:

```synx
!active

outcome:random  80
  - nothing
  - sword
  - shield
```

### :include — Compose from Multiple Files

Inserts the contents of another `.synx` file. Paths are relative to the current file.

```synx
!active

database:include    ./config/database.synx
cache:include       ./config/redis.synx
features:include    ./config/feature-flags.synx
```

Where `config/database.synx` might contain:

```synx
host      localhost
port      5432
name      myapp_prod
pool_min  2
pool_max  20
```

The result is as if you had written those keys inline under the `database` key.

**Security:** Paths cannot escape the base directory — `../../../etc/passwd` style paths are rejected. Maximum include depth is 16 levels.

### :template — String Interpolation

Substitutes `{placeholder}` patterns with values from other keys. Dot-paths work for nested access.

```synx
!active

first_name   Alice
last_name    Smith
greeting:template  Hello, {first_name} {last_name}!
```

→ `"Hello, Alice Smith!"`

**Nested key references:**

```synx
!active

server
  host  localhost
  port  8080

api_url:template   http://{server.host}:{server.port}/api
docs_url:template  http://{server.host}:{server.port}/docs
```

→ `"http://localhost:8080/api"`, `"http://localhost:8080/docs"`

### :split — String to Array

Splits a delimited string into an array. Values that look like numbers are auto-cast.

```synx
!active

# Default delimiter: comma
colors:split  red, green, blue

# Explicit delimiter keywords
words:split:space    hello world foo
path:split:slash     home user documents
flags:split:pipe     read write execute
ports:split:semi     8080;8081;8082

# Numbers are auto-cast
counts:split  1, 2, 3, 4, 5
```

Available delimiter keywords: `space`, `pipe`, `dash`, `dot`, `semi`, `tab`.

Result of `colors:split red, green, blue` → `["red", "green", "blue"]`
Result of `counts:split 1, 2, 3` → `[1, 2, 3]` (integers, not strings)

### :join — Array to String

Joins an array into a string.

```synx
!active

# Default: comma-space
tags:join
  - action
  - rpg
  - adventure

# Custom delimiter
path:join:slash
  - usr
  - local
  - bin

csv_line:join:semi
  - Alice
  - 30
  - admin
```

Results: `"action, rpg, adventure"` · `"usr/local/bin"` · `"Alice;30;admin"`

Available delimiter keywords: same as `:split` plus `comma` (explicit).

### :unique — Deduplicate an Array

Removes duplicate values, preserving the first occurrence of each.

```synx
!active

roles:unique
  - admin
  - editor
  - admin
  - viewer
  - editor
```

→ `["admin", "editor", "viewer"]`

### :geo — Value by Region

Selects a value based on the geographic region provided to the engine. Useful for CDN endpoints, currency, or locale-specific config.

```synx
!active

cdn_endpoint:geo
  - US  https://cdn-us.example.com
  - EU  https://cdn-eu.example.com
  - AS  https://cdn-as.example.com
  - default  https://cdn-global.example.com
```

The engine uses the `region` option you pass at parse time (e.g. `Options { region: Some("EU".into()) }`). If no region matches and there is a `default` line, that is used.

### :i18n — Internationalization

Selects a value based on the language provided to the engine.

```synx
!active

welcome_message:i18n
  - en  Welcome to the platform!
  - ru  Добро пожаловать на платформу!
  - de  Willkommen auf der Plattform!
  - ja  プラットフォームへようこそ！
  - default  Welcome!
```

Pass `lang: Some("ru".into())` in options and the value becomes `"Добро пожаловать на платформу!"`.

### :spam — Rate Limiting

Rate-limits access to a value. If the call count exceeds the maximum within the time window, the parser returns an error string instead of the value.

Arguments are colon-chained: `key:spam:MAX:WINDOW_SEC target`.

```synx
!active

api_endpoint:spam:5:60 https://api.example.com/data
# Allow max 5 accesses per 60 seconds.
# On the 6th call within that window the value becomes:
# "SPAM_ERR: 'https://api.example.com/data' exceeded 5 calls per 60s"
```

> **Runtime state:** The rate-limit counter lives in process memory. It resets when the process restarts. It is not persisted across parses unless hosted in a long-running daemon.

### :prompt — LLM Context Formatting

Formats a sub-tree as a labeled SYNX code fence for use in LLM prompts. Reduces the token cost of injecting structured data into AI context.

```synx
!active

app_config:prompt:AppConfig
  name     MyCoolApp
  version  2.1.0
  debug    false
  users    1500
```

This marker causes the key's value to be a formatted string like:

```
AppConfig (SYNX):
```synx
name MyCoolApp
version 2.1.0
debug false
users 1500
```
```

### :vision and :audio — Media Hints

Metadata markers that attach type hints to values, used by AI adapters to signal media modality. The data value is passed through unchanged.

```synx
!active

hero_image:vision   https://cdn.example.com/hero.jpg
intro_audio:audio   https://cdn.example.com/intro.mp3
```

### :import — Import a Single Key from Another File

Like `:include`, but imports only one specific key from the target file.

```synx
!active

db_host:import  ./database.synx:host
db_port:import  ./database.synx:port
```

This keeps your main config clean while pulling individual values from shared config files.

### :watch — Read External File at Parse Time

Reads an external file (JSON or SYNX) at parse time and injects its content as the value. Optionally extracts a specific key path from the file.

```synx
!active

# Read entire file
all_flags:watch ./flags.synx

# Extract a specific key from a JSON file
db_host:watch:database.host ./infra.json
```

The file is read once at parse time. The path is relative to the current file's directory and enforced by path-jail security. Max depth: 16, max file size: 10 MiB.

> **Not a live watcher.** Despite the name, `:watch` does **not** set up a file system watcher. It reads the file at parse time. Live-reload is handled externally by the VS Code extension or synx daemon.

### :fallback — Resilient File Inclusion

Provides a fallback file path. If the primary source file is missing, the fallback path is used instead. The fallback path is written as a colon-chained suffix.

```synx
!active

config:fallback:./defaults.synx ./overrides.synx
theme:fallback:./themes/default.synx ./themes/custom.synx
```

### :ref — Resolve and Chain

References another key in the document or a JSON pointer path. The ref is resolved and the value is substituted. Unlike `:alias`, `:ref` can be chained with subsequent markers to transform the resolved value.

```synx
!active

base_rate 100
adjusted:ref:calc:*2 base_rate
# → 200 (resolves base_rate, then feeds into :calc)

schema_def:ref #/definitions/User
```

### :clamp — Numeric Range Clamping

Clamps a numeric value to a minimum–maximum range.

```synx
!active

volume:clamp:0:100 150
# → 100

brightness:clamp:0:255 -10
# → 0

opacity:clamp:0.0:1.0 0.75
# → 0.75
```

### :round — Numeric Rounding

Rounds a numeric value to a specified number of decimal places.

```synx
!active

price:round:2 19.999
# → 19.99

pi:round:4 3.14159265
# → 3.1416

whole:round:0 42.7
# → 43
```

### :format — Printf-Style Formatting

Formats a value using a printf-style pattern.

```synx
!active

price:format:%.2f 19.9
# → "19.90"

code:format:%04d 42
# → "0042"
```

### :map — Lookup Table

Maps child values through a lookup key, transforming each element.

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

### :once — Generate Once, Persist

Generates a value once at first parse and caches it for subsequent reads. Commonly used for unique identifiers.

```synx
!active

instance_id:once:uuid
# → "550e8400-e29b-41d4-a716-446655440000" (same on every read)

created_at:once:timestamp
# → "2026-04-01T12:00:00Z" (frozen at first parse)
```

> **Persistence:** The cached value is stored alongside the document. Re-parsing without cache produces a new value.

### :version — Semantic Version Comparison

Compares a value against a semantic version constraint. Returns a boolean (`true` or `false`).

```synx
!active

app_ok:version:>=:1.0.0 1.2.3
# → true

engine_ok:version:>=:2.0.0 1.9.5
# → false
```

### :inherit — Object Inheritance (Mixin Merge)

Inherits all fields from one or more parent objects into the current object (mixin-style merge). Child fields take priority over parent fields.

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

`:inherit` runs as a pre-pass before all other markers. Parents are merged left to right, then the child's own fields override the result. This is useful for DRY configuration where many objects share a common base.

---

## Constraints — Data Validation

Constraints live in square brackets between the key name and the marker (or value). They validate the key's final value **after** markers have resolved. They only run in `!active` mode.

**General syntax:**

```
key[constraint1, constraint2:value]:marker  value
```

### required

Throws an error if the value is `null`, empty, or missing.

```synx
!active

api_key[required]:env   API_KEY
username[required]      alice
```

If `API_KEY` is not set and there is no default, the parse fails with a clear error.

### type

Validates that the resolved value is of the correct type.

```synx
!active

port[type:int]          8080
ratio[type:float]       0.95
enabled[type:bool]      true
name[type:string]       Alice
```

Accepted type names: `int`, `float`, `bool`, `string`.

### min and max

For **numbers**: restricts the value range.
For **strings**: restricts the character count.
For **arrays**: restricts the number of items.

```synx
!active

# Number range
volume[min:0, max:100]      75
temperature[min:-273]       20

# String length
username[min:3, max:32]     alice_wonder
password[min:8, max:128]    s3cur3p@ss

# Required + min — common combo
name[required, min:1]       Alice
```

### pattern

The value must match a regular expression.

```synx
!active

country_code[pattern:^[A-Z]{2}$]     US
hex_color[pattern:^#[0-9A-Fa-f]{6}$] #FF5733
phone[pattern:^\+\d{10,15}$]         +12025551234
slug[pattern:^[a-z0-9-]+$]           my-cool-project
```

> **Security:** Regex patterns are validated against ReDoS (catastrophic backtracking) attacks before use.

### enum

The value must be one of the listed options (separated by `|`).

```synx
!active

theme[enum:light|dark|auto]         dark
environment[enum:dev|staging|prod]  prod
log_level[enum:debug|info|warn|error|fatal]  info
```

### readonly

The value cannot be changed through the runtime manipulation API (`.set()`, `.remove()`, etc.). It can still be read normally.

```synx
!active

schema_version[readonly]   3
api_version[readonly]      v2
```

### Combining Constraints

Multiple constraints are comma-separated and all must pass:

```synx
!active

password[required, type:string, min:8, max:64]  MySuperSecret!
port[required, type:int, min:1024, max:65535]:env  PORT
username[required, min:3, max:32, pattern:^[a-zA-Z0-9_]+$]  alice_99
```

---

## Directives

Directives appear at the top of a file (before any keys) and change how the entire file is interpreted.

### !active

Makes markers and constraints live. Must be the first line of the file.

```synx
!active

key:marker value
```

### !tool — LLM Tool Calls

Reshapes the entire document into a structured tool-call envelope:

```json
{ "tool": "tool_name", "params": { ... } }
```

```synx
!tool
web_search
  query    latest Rust stable release
  lang     en
  results  10
```

→

```json
{
  "tool": "web_search",
  "params": {
    "query": "latest Rust stable release",
    "lang": "en",
    "results": 10
  }
}
```

**Combining `!tool` with `!active`** — markers resolve first, then the reshape happens:

```synx
!tool
!active
web_search
  query:env:default:rust news  SEARCH_QUERY
  lang:env:default:en          LANG
  results:env:default:10       MAX_RESULTS
```

### !schema — Multiple Tool Definitions

Like `!tool` but defines an array of tools in one document:

```json
{ "tools": [ { "name": "...", "params": { ... } }, ... ] }
```

```synx
!schema
web_search
  query  latest news
  lang   en
translate
  text   Hello world
  to     ru
generate_image
  prompt  A futuristic city at night
  size    1024x1024
```

→

```json
{
  "tools": [
    { "name": "web_search", "params": { "query": "latest news", "lang": "en" } },
    { "name": "translate", "params": { "text": "Hello world", "to": "ru" } },
    { "name": "generate_image", "params": { "prompt": "A futuristic city at night", "size": "1024x1024" } }
  ]
}
```

### !llm — LLM Envelope Hint

Optional metadata marker that signals the document is intended for LLM consumption. The data tree is **unchanged** — this only sets `ParseResult.llm = true` in the parsed result.

```synx
!llm

agent_name   ResearchBot
model        claude-opus
temperature  0.7
max_tokens   4096
```

The `.synxb` binary format preserves this flag in its header.

---

## Parsers and Language Support

### How It All Fits Together

There are two underlying engines, and every language binding uses one of them:

```
synx-core (Rust)
     │
     ├─ PyO3 ────────────── Python (synx_native)
     ├─ napi-rs ─────────── Node native addon
     ├─ wasm-bindgen ─────── WebAssembly
     ├─ synx-cli ─────────── CLI binary
     ├─ synx-lsp ─────────── Language Server
     │
     └─ synx-c (C ABI)
           │
           ├─ C++ ────────── synx/synx.hpp (header-only)
           ├─ Go ─────────── bindings/go (cgo)
           ├─ Swift ──────── SynxEngine (SwiftPM)
           └─ Kotlin ─────── SynxEngine (JNA)

Separate managed implementations (same conformance tests):
     ├─ @aperturesyndicate/synx-format ── TypeScript/JS (pure TS)
     └─ APERTURESyndicate.Synx ────────── C# (.NET 8)
```

**Critical difference — custom environment injection:**

| Binding family | Can inject custom `env` dict? |
|----------------|-------------------------------|
| Rust `synx-core`, Python `synx_native`, Node native, C# | ✅ Yes — pass an options object |
| `synx-c` wrappers (Go, Swift, Kotlin, C FFI) | ❌ No — uses real process env |
| WASM | ❌ No — `Options::default()` |

If you need to inject fake env vars for testing in Go/Swift/Kotlin, shell-out to the `synx` CLI with env vars set, or use Rust/Python/C# bindings instead.

---

### Rust — `synx-core`

The reference implementation. All other Rust-based bindings are thin wrappers around this.

**Install:**

```toml
[dependencies]
synx-core = "3.6.0"
```

**Complete API:**

| Method | When to use | Returns |
|--------|------------|---------|
| `Synx::parse(text, opts)` | Static file, or you only want structure | `HashMap<String, Value>` |
| `Synx::parse_active(text, opts)` | File has `!active`, resolve markers | `HashMap<String, Value>` |
| `Synx::parse_full(text)` | Inspect mode/locks/includes before resolving | `ParseResult` |
| `Synx::parse_tool(text, opts)` | `!tool` document → tool call shape | `HashMap<String, Value>` |
| `Synx::stringify(value)` | Value tree → SYNX text | `String` |
| `Synx::format(text)` | Canonical SYNX (sorted, 2-space) | `String` |
| `Synx::compile(text, resolved)` | Text → `.synxb` binary | `Vec<u8>` |
| `Synx::decompile(bytes)` | `.synxb` → SYNX text | `Result<String, _>` |
| `Synx::is_synxb(bytes)` | Detect binary magic header | `bool` |
| `Synx::diff(a, b)` | Compare two root maps | `DiffResult` |

**Options:**

| Field | Type | Effect |
|-------|------|--------|
| `env` | `HashMap<String, String>` | Fake environment for `:env` (overrides real process env) |
| `base_path` | `String` | Base directory for `:include`, `:watch`, relative paths |
| `region` | `String` | Value for `:geo` marker |
| `lang` | `String` | Language for `:i18n` marker |
| `max_include_depth` | `usize` | Cap on nested file inclusion (default: 16) |

**Example — production config with injected test env:**

```rust
use synx_core::{Synx, Options};
use std::collections::HashMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Real production parse — reads actual environment
    let config = Synx::parse_active(
        &std::fs::read_to_string("app.synx")?,
        &Options {
            base_path: Some("./config".into()),
            ..Default::default()
        }
    )?;

    // Test with injected fake env
    let mut fake_env = HashMap::new();
    fake_env.insert("PORT".into(), "9999".into());
    fake_env.insert("DB_HOST".into(), "test-db".into());

    let test_config = Synx::parse_active(
        "!active\nport:env PORT\ndb_host:env DB_HOST\n",
        &Options {
            env: Some(fake_env),
            ..Default::default()
        }
    )?;

    println!("port = {:?}", test_config.get("port"));
    Ok(())
}
```

**Value variants you'll encounter:**

| Variant | Description |
|---------|-------------|
| `Value::String(s)` | Plain string |
| `Value::Int(n)` | Integer (`i64`) |
| `Value::Float(f)` | Float (`f64`) |
| `Value::Bool(b)` | Boolean |
| `Value::Null` | Null / missing |
| `Value::Array(vec)` | Ordered list |
| `Value::Object(map)` | Nested object |
| `Value::Secret(s)` | Hidden value — use `as_secret()` for the real string |

---

### CLI — `synx`

The command-line tool. Use it for CI validation, scripting, and one-off operations. No library integration needed.

**Install:**

```bash
cargo install synx-cli --locked
```

**All commands:**

| Command | What it does |
|---------|-------------|
| `synx parse config.synx` | Parse → print canonical JSON to stdout |
| `synx parse --active config.synx` | Parse with `!active` resolution |
| `synx validate config.synx` | Exit 0 on success, 1 on errors |
| `synx validate --strict config.synx` | Strict mode (warnings become errors) |
| `synx format config.synx` | Print canonical SYNX to stdout |
| `synx format --write config.synx` | Format in-place |
| `synx convert config.synx` | SYNX → JSON |
| `synx convert data.json` | JSON → SYNX |
| `synx diff old.synx new.synx` | Structural diff |
| `synx query server.host config.synx` | Extract one value by dot-path |
| `synx query items.0 data.synx` | Array indexing in queries |
| `synx compile config.synx` | → `config.synxb` binary |
| `synx compile --resolved config.synx` | Compile with markers resolved |
| `synx decompile config.synxb` | Binary → SYNX text |
| `synx schema config.synx` | Generate JSON Schema from constraints |
| `synx json-validate data.json schema.json` | Validate JSON against schema |

**CI example** — validate all config files in a GitHub Actions workflow:

```yaml
- name: Install synx CLI
  run: cargo install synx-cli --locked

- name: Validate configs
  run: synx validate --strict configs/*.synx
```

Or use the ready-made GitHub Action:

```yaml
- uses: APERTURESyndicate/synx-format/.github/actions/synx@main
  with:
    files: "**/*.synx"
    strict: true
```

---

### JavaScript and TypeScript

**Install:**

```bash
npm install @aperturesyndicate/synx-format
```

Works in Node.js, Bun, Deno, and any bundler (Vite, webpack, esbuild). For browser use, import the ESM bundle or the IIFE bundle.

**Core API:**

| Method | What it does |
|--------|-------------|
| `Synx.parse(text, options?)` | Parse string → plain JavaScript object |
| `Synx.loadSync(path, options?)` | Read file, then parse |
| `Synx.load(path, options?)` | Async version of `loadSync` |
| `Synx.stringify(obj, active?)` | Object → SYNX text |
| `Synx.format(text)` | Canonical SYNX |
| `Synx.diff(a, b)` | Structural diff of two parsed objects |
| `Synx.schema(text)` | Generate JSON Schema from `!active` constraints |
| `Synx.toJSON(obj)` | Export as JSON string |
| `Synx.toYAML(obj)` | Export as YAML string |
| `Synx.toEnv(obj)` | Export as `.env` file format |
| `Synx.get(obj, path)` | Read nested value by dot-path |
| `Synx.set(obj, path, value)` | Update nested value (respects `[readonly]`) |
| `Synx.watch(path, cb)` | Hot-reload on file change (Node only) |

**SynxOptions:**

| Option | Effect |
|--------|--------|
| `env` | `Record<string, string>` — fake environment for `:env` |
| `basePath` | Base directory for `:include` resolution |
| `region` | Region string for `:geo` |
| `lang` | Language code for `:i18n` |
| `strict` | Throw `SynxError` on error tokens in the result |
| `maxIncludeDepth` | Cap include nesting (default: 16) |

**Complete working example:**

```typescript
import { Synx } from '@aperturesyndicate/synx-format';

// 1. Parse a static config
const config = Synx.parse(`
name    MyApp
version 1.0.0
server
  host localhost
  port 3000
features
  - auth
  - logging
`);

console.log(config.name);           // "MyApp"
console.log(config.server.port);    // 3000
console.log(config.features[0]);    // "auth"

// 2. Parse with active markers and fake env (great for tests)
const active = Synx.parse(`
!active
port:env:default:8080  PORT
db_url:template        postgres://{db_host}:5432/{db_name}
db_host                localhost
db_name                myapp
`, { env: { PORT: '9000' } });

console.log(active.port);    // 9000
console.log(active.db_url);  // "postgres://localhost:5432/myapp"

// 3. Structural diff
const v1 = Synx.parse('name Alice\nrole admin\n');
const v2 = Synx.parse('name Alice\nrole editor\nactive true\n');
const d = Synx.diff(v1, v2);
// d.changed  → { role: { from: "admin", to: "editor" } }
// d.added    → { active: true }
// d.removed  → {}

// 4. Hot-reload (Node.js)
Synx.watch('./config.synx', (newConfig) => {
  console.log('Config reloaded:', newConfig);
});
```

**Browser (CDN / IIFE):**

```html
<script src="https://unpkg.com/@aperturesyndicate/synx-format/dist/synx.browser.js"></script>
<script>
  const data = Synx.parse('name World\ngreeting Hello!');
  console.log(data.greeting); // "Hello!"
</script>
```

---

### Python

**Install:**

```bash
pip install synx-format
```

This installs the `synx_native` module — a PyO3 binding that calls `synx-core` directly. No intermediary, full options support.

**Complete function reference:**

| Function | Arguments | Returns | Notes |
|----------|-----------|---------|-------|
| `parse(text)` | SYNX string | `dict` | Static parse only |
| `parse_to_json(text)` | SYNX string | JSON `str` | Faster for string-only pipelines |
| `parse_active(text, env=None, base_path=None)` | + optional dict, optional str | `dict` | Resolves `!active` markers |
| `parse_active_to_json(...)` | same | JSON `str` | Same, but returns string |
| `parse_tool(text, env=None, base_path=None)` | | `dict` | `!tool` reshape |
| `parse_tool_to_json(...)` | | JSON `str` | |
| `stringify(obj)` | Python dict/value | SYNX `str` | |
| `stringify_json(json_text)` | JSON string | SYNX `str` | JSON → SYNX |
| `format(text)` | | canonical SYNX `str` | |
| `compile(text, resolved=False)` | | `bytes` | `.synxb` binary |
| `decompile(data)` | `bytes` | SYNX `str` | |
| `compile_hex(text, resolved=False)` | | hex `str` | For string-only bridges |
| `decompile_hex(hex)` | | SYNX `str` | |
| `is_synxb(data)` | `bytes` | `bool` | |
| `is_synxb_hex(hex)` | | `bool` | |
| `diff(a, b)` | two `dict` values | `dict` diff | |
| `diff_json(text_a, text_b)` | two SYNX strings | JSON `str` | Parses, then diffs |
| `to_prompt_block(text, label)` | | `str` | LLM context wrapper |

**Full example — Flask app config:**

```python
import synx_native
import os

# Load and parse config with real env vars
config = synx_native.parse_active(
    open('config/app.synx').read(),
    base_path='config/'
)

# Load for tests — inject fake env, never touch real process
def load_test_config():
    return synx_native.parse_active(
        """
        !active
        port:env:default:8080      PORT
        db_host:env:default:localhost  DB_HOST
        db_name:env:default:testdb     DB_NAME
        secret_key[required]:env   SECRET_KEY
        """,
        env={
            'PORT': '5000',
            'DB_HOST': 'test-postgres',
            'DB_NAME': 'app_test',
            'SECRET_KEY': 'test-only-secret'
        }
    )

# Secret values
test = load_test_config()
print(test)  # {'port': 5000, 'db_host': 'test-postgres', ...}
# secrets show as "[SECRET]" if you had :secret markers

# Diff two configs
old = synx_native.parse("version 1\nflag true\n")
new = synx_native.parse("version 2\nflag true\nnew_key added\n")
delta = synx_native.diff(old, new)
# {'added': {'new_key': 'added'}, 'removed': {}, 'changed': {'version': ...}, 'unchanged': {'flag': True}}
```

---

### C# / .NET

**Install:**

```bash
dotnet add package APERTURESyndicate.Synx
```

> The NuGet ID is `APERTURESyndicate.Synx` (not `Synx.Core` — that name was taken). See [nuget.org/packages/APERTURESyndicate.Synx](https://nuget.org/packages/APERTURESyndicate.Synx).

This is a **managed .NET 8 implementation** — no native DLL required. The parser is pure C#, aligned with the Rust reference via the conformance test suite.

**SynxOptions:**

| Property | Type | Effect |
|----------|------|--------|
| `Env` | `Dictionary<string, string>` | Inject fake env for `:env` markers |
| `Region` | `string` | Value for `:geo` |
| `Lang` | `string` | Language for `:i18n` |
| `BasePath` | `string` | Base directory for `:include` |
| `MaxIncludeDepth` | `int` | Cap include nesting |

**SynxFormat API:**

| Method | Behaviour |
|--------|-----------|
| `Parse(text)` | Static parse → `Dictionary<string, SynxValue>` |
| `ParseActive(text, options?)` | Parse + run engine (markers, constraints) |
| `ParseFull(text)` | Returns `SynxParseResult` with mode/tool flags, before resolve |
| `ParseFullActive(text, options?)` | Parse + resolve, returns full result |
| `ParseTool(text)` | `!tool` reshape |
| `ToJson(value)` / `ToJson(map)` | Canonical JSON string |
| `Stringify(value)` | SynxValue tree → SYNX text |
| `Serialize<T>(obj)` | Typed object → SYNX text |
| `Deserialize<T>(text, jsonOptions?)` | Parse then deserialize directly into `T` |
| `DeserializeActive<T>(text, synxOptions?, jsonOptions?)` | Parse + engine, then deserialize into `T` |
| `Deserialize(text, type)` | Runtime-type deserialization |
| `DeserializeActive(text, type, options?)` | Active variant with runtime type |
| `DeserializeAsync<T>(stream)` | Async stream-based deserialization |
| `DeserializeActiveAsync<T>(stream, options?)` | Async active variant |
| `SerializeAsync<T>(stream, obj)` | Async stream-based serialization |
| `LoadFileAsync<T>(path)` | One-line file → typed object |
| `SaveFileAsync(path, obj)` | One-line typed object → file |
| `FromJson(json)` | JSON string → SYNX text |
| `Format(text)` | Canonical SYNX (sorted keys, 2-space indent) |
| `Diff(a, b)` | Structural diff → list of changes |
| `DiffJson(textA, textB)` | Diff → JSON string |
| `Compile(text, resolved?)` | SYNX → `.synxb` binary |
| `Decompile(data)` | `.synxb` → SYNX text |
| `IsSynxb(data)` | Check magic header |

**Hello World:**

```csharp
using Synx;

// 1. Static parse
var text = """
name    Alice
age     30
active  true
roles
  - admin
  - editor
""";

var data = SynxFormat.Parse(text);

// SynxValue helpers — like Rust's value.as_str() / value["key"]
string? name = data["name"].AsString();  // "Alice"
long? age    = data["age"].AsInt();      // 30
bool? active = data["active"].AsBool();  // true

// ToString() unwraps primitives for display
Console.WriteLine(data["name"]);    // Alice
Console.WriteLine(data["age"]);     // 30

// JSON export (for interop)
Console.WriteLine(SynxFormat.ToJson(data));

// 2. Active mode with injected env (perfect for unit tests)
var config = SynxFormat.ParseActive("""
!active
port:env:default:8080        PORT
host:env:default:localhost   HOST
debug:env:default:false      DEBUG
app_name[required]           MyApp
""", new SynxOptions {
    Env = new Dictionary<string, string>(StringComparer.Ordinal) {
        ["PORT"] = "3000",
        ["HOST"] = "0.0.0.0"
    }
});

Console.WriteLine(SynxFormat.ToJson(config));
// {"app_name":"MyApp","debug":false,"host":"0.0.0.0","port":3000}
```

**Typed deserialization — directly into your POCO:**

```csharp
using Synx;
using System.Text.Json;

public record AppSettingsData(int RetryCount, int RetryDelayMinutes);

// Static parse
var settings = SynxFormat.Deserialize<AppSettingsData>("""
    RetryCount 3
    RetryDelayMinutes 5
    """);

// Active mode (resolves :env, :calc, constraints…)
var config = SynxFormat.DeserializeActive<AppSettingsData>(
    File.ReadAllText("config.synx"),
    synxOptions: new SynxOptions { BasePath = "/data" },
    jsonOptions: new JsonSerializerOptions { PropertyNameCaseInsensitive = true });
```

This replaces the manual pattern:
```csharp
// before
JsonSerializer.Deserialize<AppSettingsData>(SynxFormat.ToJson(SynxFormat.Parse(text)))
// after
SynxFormat.Deserialize<AppSettingsData>(text)
```

**Production config loader:**

```csharp
using Synx;
using System.IO;

public static class AppConfig
{
    public static Dictionary<string, SynxValue> Load(string path)
    {
        var text = File.ReadAllText(path);
        return SynxFormat.ParseActive(text, new SynxOptions {
            BasePath = Path.GetDirectoryName(Path.GetFullPath(path))
        });
    }

    public static string? Get(Dictionary<string, SynxValue> config, string key)
    {
        return config.TryGetValue(key, out var v) ? v.AsString() : null;
    }

    public static long? GetInt(Dictionary<string, SynxValue> config, string key)
    {
        return config.TryGetValue(key, out var v) ? v.AsInt() : null;
    }
}
```

**Format — Canonical Reformat:**

Sorts keys alphabetically, normalizes to 2-space indent, strips comments. Same output as `synx format` CLI.

```csharp
var messy = "age 30\n  name   Alice\n# comment";
var canonical = SynxFormat.Format(messy);
// age 30
// name Alice
```

**Diff — Structural Comparison:**

```csharp
var a = SynxFormat.Parse("name Alice\nage 30");
var b = SynxFormat.Parse("name Bob\nage 30\nemail bob@test.com");

var changes = SynxFormat.Diff(a, b);
foreach (var op in changes)
    Console.WriteLine(op);  // Changed: name Alice → Bob, Added: email

// Get diff as JSON
var json = SynxFormat.DiffJson("x 1\ny 2", "x 1\ny 3\nz new");
```

**Compile / Decompile — Binary `.synxb`:**

```csharp
// Compile to binary
byte[] binary = SynxFormat.Compile("name Alice\nage 30");
File.WriteAllBytes("config.synxb", binary);

// Compile with resolved values (after ParseActive)
byte[] resolved = SynxFormat.Compile(activeText, resolved: true);

// Decompile back to SYNX text
string text = SynxFormat.Decompile(File.ReadAllBytes("config.synxb"));

// Check if a file is .synxb binary
bool isBinary = SynxFormat.IsSynxb(File.ReadAllBytes("config.synxb"));  // true
```

---

### C++ (C++17)

**Setup:**

1. Build `synx-c` from the monorepo: `cargo build --release -p synx-c`
2. Copy `bindings/c-header/include/synx.h` and `bindings/cpp/include/synx/synx.hpp` to your include path
3. Link against `libsynx_c` (`.so` / `.dylib` / `.dll`)

**CMakeLists.txt:**

```cmake
cmake_minimum_required(VERSION 3.15)
project(MyApp)
set(CMAKE_CXX_STANDARD 17)

add_executable(myapp main.cpp)
target_include_directories(myapp PRIVATE ${SYNX_INCLUDE_DIR})
target_link_directories(myapp PRIVATE ${SYNX_LIB_DIR})
target_link_libraries(myapp PRIVATE synx_c)
```

**API (header-only, `synx/synx.hpp`):**

All functions return `std::optional<std::string>` — `nullopt` on error. `compile` returns `std::optional<std::vector<unsigned char>>`.

```cpp
#include "synx/synx.hpp"
#include <iostream>
#include <fstream>
#include <sstream>

int main() {
    // Read file
    std::ifstream f("config.synx");
    std::string text((std::istreambuf_iterator<char>(f)),
                      std::istreambuf_iterator<char>());

    // 1. Static parse → JSON
    auto json = synx::parse(text);
    if (!json) {
        std::cerr << "Parse failed\n";
        return 1;
    }
    std::cout << *json << "\n";

    // 2. Active mode
    auto active = synx::parse_active(text);
    if (active) std::cout << "Active: " << *active << "\n";

    // 3. Tool call document
    std::string tool_doc = "!tool\nweb_search\n  query hello world\n  lang en\n";
    auto tool = synx::parse_tool(tool_doc);
    if (tool) std::cout << "Tool: " << *tool << "\n";

    // 4. Diff two configs
    std::string v1 = "name Alice\nrole admin\n";
    std::string v2 = "name Alice\nrole editor\n";
    auto d = synx::diff(v1, v2);
    if (d) std::cout << "Diff: " << *d << "\n";

    // 5. Binary format
    auto bin = synx::compile(text, false);
    if (bin) {
        std::cout << "Compiled " << bin->size() << " bytes\n";
        auto restored = synx::decompile(*bin);
        if (restored) std::cout << "Restored: " << *restored << "\n";
    }

    return 0;
}
```

**Complete function reference:**

| Function | C++ signature | Notes |
|----------|--------------|-------|
| `synx::parse` | `optional<string>(string)` | Static parse → JSON |
| `synx::parse_active` | `optional<string>(string)` | Active parse → JSON |
| `synx::stringify` | `optional<string>(string json)` | JSON → SYNX |
| `synx::format` | `optional<string>(string)` | Canonical SYNX |
| `synx::parse_tool` | `optional<string>(string)` | `!tool` reshape |
| `synx::diff` | `optional<string>(string, string)` | Diff JSON |
| `synx::compile` | `optional<vector<uint8_t>>(string, bool)` | → `.synxb` |
| `synx::decompile` | `optional<string>(vector<uint8_t>)` | `.synxb` → text |
| `synx::is_synxb` | `bool(vector<uint8_t>)` | Magic check |

> **Memory:** The C++ header manages all memory automatically. There is no need to call `synx_free` manually — the optional wrappers handle it in destructors.

---

### Go

**Setup:**

The binding uses cgo and links against `libsynx_c`.

```bash
go get github.com/APERTURESyndicate/synx-format/go
```

> **cgo required.** The Go binding uses cgo and requires the synx-core shared library. See the module README for platform-specific build instructions.

**API:**

```go
import (
    "encoding/json"
    "fmt"
    "log"

    "github.com/APERTURESyndicate/synx-format/go/synx"
)

// Parse returns canonical JSON as a string
jsonStr, err := synx.Parse("name Alice\nage 30")
if err != nil {
    log.Fatal(err)
}

// Unmarshal into a Go map for field access
var data map[string]any
json.Unmarshal([]byte(jsonStr), &data)
fmt.Println(data["name"])  // Alice
fmt.Println(data["age"])   // 30

// Or unmarshal into a typed struct
type Config struct {
    Name   string  `json:"name"`
    Age    float64 `json:"age"`
}
var cfg Config
json.Unmarshal([]byte(jsonStr), &cfg)

// Stringify: JSON string → SYNX text
text, err := synx.Stringify(jsonStr)

// Format: canonical SYNX reformat
formatted, err := synx.Format("name   Alice\n  age 30")

// Diff: structural comparison → JSON
diff, err := synx.Diff("x 1\ny 2", "x 1\ny 3")

// Compile / Decompile: binary .synxb
bytes, err := synx.Compile("name Alice", false)
text, err = synx.Decompile(bytes)
```

| Function | Returns | Notes |
|----------|---------|-------|
| `Parse(text string)` | `(string, error)` | JSON output |
| `ParseActive(text string)` | `(string, error)` | Active mode, real process env |
| `Stringify(json string)` | `(string, error)` | JSON → SYNX |
| `Format(text string)` | `(string, error)` | Canonical SYNX |
| `ParseTool(text string)` | `(string, error)` | Tool call reshape |
| `Diff(a, b string)` | `(string, error)` | Diff JSON |
| `Compile(text string, resolved bool)` | `([]byte, error)` | Binary format |
| `Decompile(data []byte)` | `(string, error)` | Binary → text |
| `IsSynxb(data []byte)` | `bool` | Magic check |

---

### Swift

**Setup:**

Swift Package Manager binding via SynxEngine (FFI to synx-core).

```swift
// Package.swift dependency
.package(url: "https://github.com/APERTURESyndicate/synx-format", from: "3.6.0")
```

**API:**

```swift
import SynxEngine
import Foundation

// parse() returns a JSON string
let json = try SynxEngine.parse("name Alice\nage 30")

// Decode into native types via JSONSerialization or Codable
if let data = json.data(using: .utf8),
   let obj = try JSONSerialization.jsonObject(with: data) as? [String: Any] {
    print(obj["name"] as? String ?? "")  // Alice
    print(obj["age"] as? Int ?? 0)       // 30
}

// Or use Codable for typed access
struct Config: Codable { let name: String; let age: Int }
let config = try JSONDecoder().decode(Config.self, from: json.data(using: .utf8)!)

// stringify: JSON → SYNX text
let text = try SynxEngine.stringify(json)

// format, diff, compile, decompile also available
let formatted = try SynxEngine.format("name   Alice")
let changes = try SynxEngine.diff("x 1", "x 2")
```

---

### Kotlin / JVM

JNA binding via synx-core shared library. Works with any JVM language.

```kotlin
// build.gradle.kts
implementation("com.aperturesyndicate:synx-engine:3.6.0")
```

**API:**

```kotlin
import com.aperturesyndicate.synx.SynxEngine
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

// parse() returns a canonical JSON string
val json = SynxEngine.parse("name Alice\nage 30")

// Decode with kotlinx.serialization, Gson, or Jackson
val obj = Json.parseToJsonElement(json).jsonObject
println(obj["name"]?.jsonPrimitive?.content)  // Alice
println(obj["age"]?.jsonPrimitive?.content)    // 30

// Or deserialize into a data class
@kotlinx.serialization.Serializable
data class Config(val name: String, val age: Int)
val config = Json.decodeFromString<Config>(json)

// stringify: JSON → SYNX text
val text = SynxEngine.stringify(json)

// format, diff, compile, decompile also available
val formatted = SynxEngine.format("name   Alice")
val changes = SynxEngine.diff("x 1", "x 2")
```

---

### WebAssembly

The WASM binding is the foundation of the npm package `@aperturesyndicate/synx-format`. It compiles synx-core to WASM using wasm-bindgen and ships with JavaScript/TypeScript glue code.

**Direct WASM usage:**

```javascript
import init, { parse, stringify } from './synx_bg.wasm.js';

await init();  // load WASM module

const result = parse("name Alice\nage 30");
console.log(JSON.parse(result));
```

The WASM build is compatible with Cloudflare Workers, Deno Deploy, and other WASM-capable edge runtimes. Use the npm package directly — it ships the WASM binary as an asset.

---

### Mojo

CPython interop binding. Uses the Python `synx_native` extension under the hood.

```mojo
from synx.interop import parse_json, parse_active_json, stringify_json
from synx.interop import format_synx, diff_json, compile_hex, decompile_hex

fn main() raises:
    # Parse → JSON string
    let json = parse_json("name Alice\nage 30")
    print(json)

    # Parse with !active engine
    let config = parse_active_json("!active\nport:env:default:8080 PORT")

    # Stringify: JSON → SYNX text
    let text = stringify_json(json)
    print(text)

    # Format: canonical SYNX reformat
    let formatted = format_synx("name   Alice\n  age 30")

    # Diff: structural comparison → JSON
    let changes = diff_json("x 1\ny 2", "x 1\ny 3")

    # Compile / decompile (hex-encoded binary)
    let hex = compile_hex("name Alice\nage 30", False)
    let back = decompile_hex(hex)
```

---

## Tools & Editors

### VS Code Extension

**Install:**

```bash
code --install-extension APERTURESyndicate.synx-vscode
```

Or search for **SYNX** in the Extensions panel.

**Features:**

- Syntax highlighting for `.synx` files
- Real-time diagnostics (tabs, odd indentation, duplicate keys, unknown markers)
- Completion for markers, constraints, and directives
- Document symbol outline
- Format on save
- Hover documentation for markers
- Live reload via `:watch`

### synx-lsp — Language Server

```bash
cargo install --path crates/synx-lsp
```

The server communicates via **stdio** using standard LSP protocol. Run it as `synx-lsp` with no arguments.

| Capability | Description |
|------------|-------------|
| Diagnostics | Tabs, odd indentation, duplicate keys, unknown markers/constraints |
| Completion | Markers (`:env`, `:calc`, …), constraints, directives |
| Document Symbols | Full document outline with nesting |

### Neovim

**LSP Configuration:**

```lua
-- init.lua
vim.lsp.start({
  name = 'synx-lsp',
  cmd = { 'synx-lsp' },
  root_dir = vim.fn.getcwd(),
  filetypes = { 'synx' },
})
```

**Tree-sitter:**

```lua
require('nvim-treesitter.configs').setup {
  ensure_installed = { 'synx' },
  highlight = { enable = true }
}
```

### Other Editors

**Helix:**

```toml
# ~/.config/helix/languages.toml
[[language]]
name = "synx"
scope = "source.synx"
file-types = ["synx"]
language-servers = ["synx-lsp"]

[language-server.synx-lsp]
command = "synx-lsp"
```

**Zed:** Settings → Language Servers → Add custom server: command `synx-lsp`, languages `SYNX`.

**Emacs (eglot):**

```elisp
(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs
               '(synx-mode . ("synx-lsp"))))
```

**JetBrains:** Settings → Languages & Frameworks → Language Server → Add: command `synx-lsp`, file pattern `*.synx`.

**Sublime Text:**

```json
// LSP.sublime-settings
{
  "clients": {
    "synx-lsp": {
      "command": ["synx-lsp"],
      "selector": "source.synx",
      "enabled": true
    }
  }
}
```

**Visual Studio 2022+:** Install the VSIX from `integrations/visualstudio/` via Extensions → Manage Extensions.

### MCP Server

The `synx-mcp` server exposes SYNX operations as MCP tools to any MCP-compatible client (Claude Desktop, Claude Code, etc.).

**Available Tools:**

| Tool | Description |
|------|-------------|
| `validate` | Check syntax and constraints of a `.synx` file |
| `parse` | Parse a SYNX string or file to JSON |
| `format` | Format a SYNX document canonically |
| `synx_read_path` | Read a file (gated by `SYNX_MCP_ROOT`) |
| `synx_write_path` | Atomic write (temp + rename) |
| `synx_apply_patch` | Replace substrings in a file |

**Claude Desktop Configuration:**

```json
{
  "mcpServers": {
    "synx": {
      "command": "node",
      "args": ["/path/to/synx-mcp/index.js"],
      "env": {
        "SYNX_MCP_ROOT": "/path/to/your/project"
      }
    }
  }
}
```

Multiple roots: `"SYNX_MCP_ROOTS": "path1,path2"`. File size limit: 10 MB per file.

---

## Binary Format (.synxb)

SYNX can be compiled to a binary format (`.synxb`) for fast parsing and compact storage. The binary format encodes the same data model as text SYNX but uses length-prefixed binary encoding instead of UTF-8 text.

**Compile:**

```bash
synx compile config.synx -o config.synxb
```

```rust
use synx_core::compile;
let bytes = compile(&value)?;
std::fs::write("config.synxb", &bytes)?;
```

**Decompile:**

```bash
synx decompile config.synxb
```

```rust
use synx_core::decompile;
let bytes = std::fs::read("config.synxb")?;
let value = decompile(&bytes)?;
```

**Trade-offs:**

- **Faster parsing** — no tokenization, no indent counting
- **Smaller files** — key interning and compact integer encoding
- **Not human-editable** — use text SYNX for config files humans will modify
- **Round-trip safe** — compile → decompile produces identical data (not identical text)

---

## Structural Diff

Compare two SYNX documents and get a typed list of changes: additions, deletions, and modifications, each with a dot-separated key path.

```bash
synx diff old.synx new.synx
```

```rust
use synx_core::{parse, diff, DiffOp};

let a = parse("x 1\ny 2\nz old")?;
let b = parse("x 1\ny 3\nw new")?;

for op in diff(&a, &b) {
    match op {
        DiffOp::Added { path, value } =>
            println!("+ {} = {}", path, value),
        DiffOp::Removed { path } =>
            println!("- {}", path),
        DiffOp::Modified { path, from, to } =>
            println!("~ {} : {} → {}", path, from, to),
    }
}
```

```javascript
import { Synx } from '@aperturesyndicate/synx-format';

const ops = Synx.diff(
  Synx.parse("x 1\ny 2"),
  Synx.parse("x 1\ny 3\nz new")
);
// [{ op: "modified", path: "y", from: 2, to: 3 },
//  { op: "added",    path: "z", value: "new" }]
```

---

## JSON Schema

### Generation

Generate a Draft 2020-12 JSON Schema from an `!active` SYNX document's constraints.

```bash
synx schema config.synx
```

```synx
!active
port[type:int, min:1024, max:65535] 8080
host[required, type:string] localhost
debug[type:bool] false
```

→

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "properties": {
    "port":  { "type": "integer", "minimum": 1024, "maximum": 65535 },
    "host":  { "type": "string" },
    "debug": { "type": "boolean" }
  },
  "required": ["host"]
}
```

**Rust API:**

```rust
use synx_core::schema_json;

let parsed = synx_core::parse(input)?;
let schema = schema_json::metadata_to_json_schema(&parsed)?;
```

**JavaScript API:**

```typescript
import { Synx } from '@aperturesyndicate/synx-format';

const schema = Synx.schema(`
!active
port[type:int, min:1024] 8080
host[required] localhost
`);
```

### Validation

Validate JSON data against a generated or external JSON Schema.

```bash
# Validate a JSON file against a JSON schema
synx json-validate data.json schema.json

# Self-validate: generate schema from !active doc and validate it
synx validate --self-schema config.synx

# Validate using an external schema
synx validate --json-schema schema.json config.synx
```

---

## Reference

### Conformance Suite

All official bindings are tested against the same 11 conformance test cases. Each test consists of a `.synx` input file and a `.expected.json` file. Any binding that produces identical JSON output for all 11 tests is considered conformant.

| # | Name | What it tests |
|---|------|---------------|
| 01 | `scalar-types` | All scalars: string, int, float, bool, null |
| 02 | `nesting` | Nested objects (3+ levels deep) |
| 03 | `arrays` | Arrays of scalars and objects |
| 04 | `type-casting` | `key(int)`, `key(float)`, `key(bool)`, `key(string)` |
| 05 | `comments` | `#`, `//`, and `### ... ###` multi-line comments |
| 06 | `multiline` | Multi-line values via indentation |
| 07 | `mixed` | Mixed structure: objects + arrays at same level |
| 08 | `strings-with-spaces` | Values containing spaces without quotes |
| 09 | `empty-values` | `key ""` (empty string), `key ~` (null) |
| 10 | `tool-mode` | `!tool` and `!schema` reshape output |
| 11 | `llm-directive` | `!llm` — data tree unchanged |

```bash
# Rust
cargo test -p synx-core --test conformance

# C#
cd parsers/dotnet && dotnet test

# JavaScript
cd packages/synx-js && npm test
```

### Performance

**Input Bounds:**

SYNX enforces hard limits to protect against hostile input:

| Limit | Value |
|-------|-------|
| Maximum input size | 16 MiB |
| Maximum nesting depth | 128 levels |
| Maximum array elements | 1,000,000 |
| Maximum block size | 1 MiB |
| Calc expression length | 4,096 characters |
| Include depth | 16 levels |
| Include file size | 10 MB |

**Fuzzing:**

The parser is continuously fuzz-tested with three targets:

- `fuzz_parse` — parser + engine with arbitrary input
- `fuzz_compile` — binary codec round-trip (compile → decompile)
- `fuzz_format` — formatter stability

The fuzz corpus contains **7,177** interesting inputs discovered during long fuzzing runs. These are used as regression tests on every CI run.

```bash
cargo install cargo-fuzz
cargo fuzz run fuzz_parse
cargo fuzz run fuzz_compile
cargo fuzz run fuzz_format
```

### Security

**Input Validation:** Never parse untrusted SYNX without size limits. The parser enforces hard limits (16 MiB, depth 128) but you should add application-level checks before passing data to the parser.

**Environment Markers:** The `:env` marker reads from the process environment. Ensure that sensitive environment variables are not accessible in contexts where untrusted users can influence the SYNX source.

**Include Paths:** The `:include` marker resolves paths relative to the document. In untrusted input scenarios, disable `:include` by setting `SYNX_DISABLE_INCLUDE=1` or using the `ParseOptions::no_includes()` API flag.

> **Never parse untrusted `!active` documents with `:secret`.** The `:secret` marker connects to your secrets backend. Only process `!active` documents from trusted sources.

### FAQ

**Why not just use YAML?**
YAML has many footguns: the Norway problem (country code `NO` becomes `false`), automatic type coercion, multi-document streams, anchors with complex scoping rules, and whitespace sensitivity that differs from SYNX's simpler model. SYNX deliberately narrows the feature surface to eliminate these surprises.

**Can I use tabs for indentation?**
No. Tabs are a parse error. Use 2 spaces (canonical) or any consistent number of spaces. The formatter normalizes to 2-space indent.

**Do I need quotes for strings with spaces?**
No. Everything after the key (and optional marker) is treated as the value, including spaces. Quotes are only needed to express an empty string: `key ""`.

**Do I always need `!active`?**
Only if you need markers (`:env`, `:calc`, etc.) or constraints (`[type:int]`). Plain data files — configs without dynamic resolution, data exports, API payloads — work perfectly in static mode.

**Is SYNX output always valid JSON?**
Yes. `synx parse` and all `parse()` APIs return a JSON-compatible value. `synx convert --to json` produces strict JSON.

**Can the spec change?**
SYNX v3.6.0 is a frozen specification. The grammar will not change. New functionality (if any) would be additive and released under a new major version.

