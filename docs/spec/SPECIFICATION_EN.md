# 📘 SYNX — Full Format Specification

**Version:** 4.2  
**Name:** SYNX (Syndicate Exchange), pronounced "Sine-X"  
**File extension:** `.synx`

---

## 1. Philosophy

SYNX is a data format that strips away all syntactic noise — quotes, commas, brackets, colons around values. All that remains is **key**, **space**, **value**.

| Criteria | JSON | YAML | SYNX |
|---|---|---|---|
| Quotes | Required | Sometimes | No |
| Commas / brackets | Yes | No | No |
| Built-in logic | No | No | Yes (`!active`) |
| Tokens for AI | ~100% | ~75% | ~40% |

---

## 2. Basic Syntax (always works)

### 2.1 Key–Value

Everything after the **first space** is the value. No quotes needed.

```synx
name John
age 25
phrase I love programming and drinking coffee!
```

JSON equivalent:
```json
{
  "name": "John",
  "age": 25,
  "phrase": "I love programming and drinking coffee!"
}
```

**Key rules:**
- No spaces (use `_` or `camelCase`)
- Cannot start with `-`, `#`, `//`, `!`
- May contain letters of any alphabet, digits, `_`, `-` (inside)

---

### 2.2 Automatic Type Detection

The parser detects types automatically:

```synx
greeting Hello World
count 42
rate 3.14
enabled true
disabled false
empty null
```

Result:
```json
{
  "greeting": "Hello World",
  "count": 42,
  "rate": 3.14,
  "enabled": true,
  "disabled": false,
  "empty": null
}
```

**Explicit type casting** (if you need a number as a string):
```synx
zip_code(string) 90210
id(int) 007
```

Supported casts: `(int)`, `(float)`, `(bool)`, `(string)`

---

### 2.3 Nesting (objects)

An indent of **2 spaces** creates a nested object. A key without a value is a group (folder).

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

> ⚠️ Use **spaces**, not TABs. The standard is **2 spaces** per level.

---

### 2.4 Lists

The `- ` (dash + space) symbol creates a list item. Items without names form an ordered collection.

```synx
inventory
  - Sword
  - Shield
  - Health Potion
```

Result:
```json
{
  "inventory": ["Sword", "Shield", "Health Potion"]
}
```

**List of objects** (each item is a mini-record):

```synx
garage
  - make BMW
    color black
    year 2023
  - make Audi
    color white
    year 2021
```

Result:
```json
{
  "garage": [
    { "make": "BMW", "color": "black", "year": 2023 },
    { "make": "Audi", "color": "white", "year": 2021 }
  ]
}
```

---

### 2.5 Multiline Text (blocks)

The `|` character after a key starts a text block. Everything indented below is part of that text.

```synx
description |
  This is a long description
  that spans multiple lines.
  Each line is joined with a newline character.
```

Result:
```json
{
  "description": "This is a long description\nthat spans multiple lines.\nEach line is joined with a newline character."
}
```

Forced line break inside a single line: `/n`

```synx
banner Welcome!/nEnjoy the game!
```

Result:
```
Welcome!
Enjoy the game!
```

---

### 2.6 Comments

**Two styles** are supported:

```synx
# This is a comment (Python/YAML style)
// This is also a comment (JS/C++ style)

name John  # Inline comment after value
port 8080 // Also inline
```

Comments are completely ignored by the parser.

---

## 3. `!active` Mode — Live Config

### 3.1 What Is It

By default a `.synx` file contains **static data**. Just keys and values, like JSON.

But if the **first line** of the file says `!active`, the file becomes a **live config** — enabling:

- **Functions** (`:random`, `:calc`, `:env`, etc.)
- **Constraints** (`[min:3]`, `[type:int]`, etc.)

```synx
!active

port:env PORT
greeting:random
  - Hello!
  - Welcome!
```

Without `!active` — markers `:random`, `:calc` and square brackets `[]` are **completely ignored**. The parser reads the file as plain text:

```synx
// No !active — functions DO NOT work
port:env PORT           // key = "port:env", value = "PORT" (just a string)
greeting:random          // key = "greeting:random", value = {} (object)
```

> 💡 This is a security feature: a static file will **never** execute code, access environment variables, or run commands.

---

### 3.2 Full List of Functions (`:`)

Functions are written with a colon immediately after the key name: `key:function value`.

---

#### `:random` — Random Selection

Selects **one** element from a list on each parse.

**Equal chances** (no percentages):
```synx
!active

battle_cry:random
  - Time to win!
  - Ha-ha-ha!
  - For the Syndicate!
```
Each item has a 33.3% chance.

**Weighted random** (with percentages):
```synx
!active

// Percentages go after :random separated by spaces
// Order of percentages matches order of items
loot:random 70 20 10
  - Common Chest
  - Rare Chest
  - Legendary Chest
```

Here:
- "Common Chest" drops with **70%** chance
- "Rare Chest" — **20%**
- "Legendary Chest" — **10%**

**Percentage rules:**
| Situation | Behavior |
|---|---|
| No percentages | All items are equally likely |
| Sum = 100 | Used as-is |
| Sum ≠ 100 | Automatically normalized (proportions preserved) |
| Fewer percentages than items | Remainder is split evenly among items without a percentage |
| More percentages than items | Extra percentages are ignored |

Normalization example:
```synx
!active

// 2 + 1 = 3, normalized: ~66.7% and ~33.3%
class:random 2 1
  - Warrior
  - Mage
```

Partial percentages example:
```synx
!active

// First gets 80%, remaining 20% split between two: 10% each
drop:random 80
  - Nothing
  - Sword
  - Shield
```

---

#### `:calc` — Expression Evaluation

Evaluates an arithmetic expression. Can reference other keys by name.

```synx
!active

price 100
tax:calc price * 0.2
total:calc price + tax
```

Result:
```
price 100
tax 20    // ← your program receives this value
total 120 // ← your program receives this value
```

As a JSON file:
```json
{
  "price": 100,
  "tax": 20,
  "total": 120
}
```

**Supported operations:** `+`, `-`, `*`, `/`, `%` (remainder), `(`, `)`.

> ⚠️ Arithmetic only. No arbitrary code. The parser uses a safe evaluator, not `eval()`.

---

#### `:env` — Environment Variable

Substitutes the value of a system environment variable.

```synx
!active

port:env PORT
home_dir:env HOME
```

If the variable is not found — the value will be `null`.

**With a default** (via `:default`):
```synx
!active

port:env:default:8080 PORT
```
If `PORT` is not set → returns `8080`.

---

#### `:alias` — Reference to Another Key

Copies the value of another key. Does not duplicate data — references it.

```synx
!active

admin_email alex@example.com
complaints_email:alias admin_email
```

Result:
```json
{
  "admin_email": "alex@example.com",
  "complaints_email": "alex@example.com"
}
```

---

#### `:secret` — Hidden Value

The value is readable by your program but **not shown** in logs, during `freeze`, or when serialized to JSON. Protects against accidental leaks.

```synx
!active

api_key:secret sk-1234567890abcdef
db_password:secret P@ssw0rd!
```

With `console.log(data)`, `print(data)`, `JSON.stringify(data)` — shows `"[SECRET]"`.  
To **get the real value**, use the `.reveal()` method:

```javascript
// JavaScript / TypeScript
const key = data.api_key;          // SynxSecret object
console.log(String(key));           // "[SECRET]"
console.log(JSON.stringify(key));   // '"[SECRET]"'
console.log(key.reveal());          // "sk-1234567890abcdef"  ← real value
```

```python
# Python
key = data['api_key']               # SynxSecret object
print(key)                          # [SECRET]
print(key.reveal())                 # sk-1234567890abcdef  ← real value
```

> ⚠️ **Important:** Never log the result of `.reveal()`. This method is intended only for passing the value to APIs, database connections, etc.

---

#### `:default` — Default Value

Sets a fallback if the main value is empty or not found. Most often combined with `:env`.

```synx
!active

// If PORT env var is not set — will be 3000
port:env:default:3000 PORT

// Just a default value (if value = null or empty)
theme:default dark
```

---

#### `:unique` — Remove Duplicates from List

Keeps only unique elements.

```synx
!active

tags:unique
  - action
  - rpg
  - action
  - simulation
  - rpg
```

Result: `["action", "rpg", "simulation"]`

---

#### `:include` — Include Another File

Inserts the contents of another `.synx` file into the current one. Path is relative to the current file.

```synx
!active

// Pull database settings from a separate file
database:include ./db.synx
```

Where `db.synx`:
```synx
host localhost
port 5432
name mydb
```

Result:
```json
{
  "database": {
    "host": "localhost",
    "port": 5432,
    "name": "mydb"
  }
}
```

---

#### `:geo` — Value by Geolocation / Region

Selects a value based on user region (determined by IP or system locale).

```synx
!active

currency:geo
  - US USD
  - EU EUR
  - GB GBP
```

> This function requires runtime support. The parser passes the current region to the engine.

---

#### `:template` — String Interpolation

Substitutes `{placeholders}` with values from other keys. Supports dot-path for nested access.

```synx
!active

first_name John
last_name Doe
greeting:template Hello, {first_name} {last_name}!
```

Result: `"Hello, John Doe!"`

**With nested references** (dot-path):
```synx
!active

server
  host localhost
  port 8080

db_url:template http://{server.host}:{server.port}/db
```

Result: `"http://localhost:8080/db"`

---

#### `:split` — Split String into Array

Splits a string by a delimiter into an array of items. Auto-casts values if they look like numbers.

**Default delimiter (comma):**
```synx
!active

colors:split red, green, blue
```

Result: `["red", "green", "blue"]`

**With delimiter keyword:**
```synx
!active

// Space delimiter
words:split:space hello world foo

// Pipe delimiter
flags:split:pipe read write execute
```

Supported delimiter keywords: `space`, `pipe`, `dash`, `dot`, `semi`, `tab`

**Auto-casting:**
```synx
!active

numbers:split 1, 2, 3
```

Result: `[1, 2, 3]` (auto-detected as integers)

---

#### `:join` — Join Array into String

Joins array elements into a string with a delimiter.

**Default delimiter (comma):**
```synx
!active

tags:join
  - action
  - rpg
  - adventure
```

Result: `"action, rpg, adventure"`

**With delimiter keyword:**
```synx
!active

path:join:slash
  - home
  - user
  - documents
```

Result: `"home/user/documents"`

---

### 3.3 Functions Summary Table

| Function | Description | Example |
|---|---|---|
| `:random` | Random item from list | `phrase:random` |
| `:random N N N` | Weighted random (percentages) | `loot:random 70 20 10` |
| `:calc` | Arithmetic evaluation | `total:calc price * 1.2` |
| `:env` | Environment variable | `port:env PORT` |
| `:alias` | Reference to another key | `copy:alias original` |
| `:secret` | Value hidden from logs | `password:secret abc123` |
| `:default` | Default value | `theme:default dark` |
| `:default:X` | Fallback (in combination) | `port:env:default:8080 PORT` |
| `:unique` | List deduplication | `tags:unique` |
| `:include` | Include external file | `db:include ./db.synx` |
| `:geo` | Value by region | `currency:geo` |
| `:template` | String interpolation | `greeting:template Hello, {name}!` |
| `:split` | String → array | `colors:split red, green, blue` |
| `:join` | Array → string | `tags:join` |

**Chaining functions** — via `:` chain:
```synx
!active
port:env:default:8080 PORT
```

---

### 3.4 Constraints (`[]`) — Data Validation

Constraints are written in square brackets between the key and the function (or value). They only work in `!active` mode.

General syntax:
```
key[constraint1:value, constraint2:value]:function value
```

---

#### `min` / `max` — Minimum and Maximum

For **numbers** — restricts the value range.  
For **strings** — restricts the length (character count).

```synx
!active

// String from 3 to 30 characters
app_name[min:3, max:30] TotalWario

// Number from 1 to 100
volume[min:1, max:100] 75

// Minimum only
password[min:8] mypassword123
```

---

#### `required` — Required Field

The parser will throw an error if the value is empty or missing.

```synx
!active

api_key[required]:env API_KEY
name[required, min:1] Wario
```

---

#### `pattern` — Regular Expression

The value must match a regex pattern.

```synx
!active

country_code[pattern:^[A-Z]{2}$] US
phone[pattern:^\+\d{10,15}$] +12025551234
```

---

#### `enum` — Allowed Values

The value must be one of the listed options.

```synx
!active

theme[enum:light|dark|auto] dark
region[enum:EU|US|GB|AS] US
```

---

#### `readonly` — Read Only

The value cannot be changed via API / hot-reload of the config. Only manual file editing.

```synx
!active

version[readonly] 2.0.0
```

---

### 3.5 Constraints Summary Table

| Constraint | Description | Syntax |
|---|---|---|
| `min` | Minimum (length or value) | `[min:3]` |
| `max` | Maximum (length or value) | `[max:100]` |
| `type` | Data type | `[type:int]` |
| `required` | Required field | `[required]` |
| `pattern` | Regex validation | `[pattern:^\d+$]` |
| `enum` | List of allowed values | `[enum:a\|b\|c]` |
| `readonly` | Modification forbidden | `[readonly]` |

Constraints can be combined with commas:
```synx
!active
password[required, min:8, max:64, type:string] MyP@ssw0rd
```

---

## 4. Full Examples

### 4.1 Plain File (no `!active`)

A simple static config. No magic — just data.

```synx
# TotalWario game config (static)

app_name TotalWario
version 2.0.0

server
  host 0.0.0.0
  port 8080
  ssl_enabled false

gameplay
  base_hp 100
  boss_hp 500
  max_players 16
  greeting Prepare to fight.

map_rotation
  - Arena of Doom
  - Crystal Caverns
  - Wario Stadium

rules |
  1. No cheating.
  2. Respect the Syndicate.
  3. Have fun!

credits
  lead_dev KaiserBerg
  studio APERTURESyndicate
  year 2026
```

Result (`Synx.parse()`):
```json
{
  "app_name": "TotalWario",
  "version": "2.0.0",
  "server": {
    "host": "0.0.0.0",
    "port": 8080,
    "ssl_enabled": false
  },
  "gameplay": {
    "base_hp": 100,
    "boss_hp": 500,
    "max_players": 16,
    "greeting": "Prepare to fight."
  },
  "map_rotation": [
    "Arena of Doom",
    "Crystal Caverns",
    "Wario Stadium"
  ],
  "rules": "1. No cheating.\n2. Respect the Syndicate.\n3. Have fun!",
  "credits": {
    "lead_dev": "KaiserBerg",
    "studio": "APERTURESyndicate",
    "year": 2026
  }
}
```

---

### 4.2 File with `!active` — Live Config

The same config, but with dynamic functions and validation.

```synx
!active
# Live config for TotalWario

app_name[required, min:3, max:30] TotalWario
version[readonly] 2.0.0

server
  // Port from environment variable, fallback to 8080
  port:env:default:8080 PORT
  host 0.0.0.0
  ssl_enabled false

gameplay
  base_hp 100
  // Engine calculates: 100 * 5 = 500
  boss_hp:calc base_hp * 5
  max_players[type:int, min:2, max:64] 16
  difficulty[enum:easy|normal|hard|nightmare] normal

  // A random phrase on every parse
  greeting:random
    - Welcome to the arena!
    - Prepare to fight.
    - Wario time!

  // Weighted random: common 70%, rare 20%, legendary 10%
  loot_tier:random 70 20 10
    - common
    - rare
    - legendary

map_rotation
  - Arena of Doom
  - Crystal Caverns
  - Wario Stadium

rules |
  1. No cheating.
  2. Respect the Syndicate.
  3. Have fun!

// Include database settings from a separate file
database:include ./db.synx

// Secrets — won't appear in logs
api_key[required]:secret sk-live-abc123def456

credits
  lead_dev KaiserBerg
  studio APERTURESyndicate
  year 2026
  contact:alias lead_dev
```

Result of one `Synx.parse()` call:
```json
{
  "app_name": "TotalWario",
  "version": "2.0.0",
  "server": {
    "port": 8080,
    "host": "0.0.0.0",
    "ssl_enabled": false
  },
  "gameplay": {
    "base_hp": 100,
    "boss_hp": 500,
    "max_players": 16,
    "difficulty": "normal",
    "greeting": "Wario time!",
    "loot_tier": "common"
  },
  "map_rotation": [
    "Arena of Doom",
    "Crystal Caverns",
    "Wario Stadium"
  ],
  "rules": "1. No cheating.\n2. Respect the Syndicate.\n3. Have fun!",
  "database": {
    "host": "localhost",
    "port": 5432,
    "name": "mydb"
  },
  "api_key": "[SECRET]",
  "credits": {
    "lead_dev": "KaiserBerg",
    "studio": "APERTURESyndicate",
    "year": 2026,
    "contact": "KaiserBerg"
  }
}
```

> On the next `parse()` call, `greeting` and `loot_tier` will be different — that's the point of `:random`.

---

## 5. Using in Code — Directly, Without Conversion

> 🚀 **Core Principle:** SYNX is read by your code **directly**. No conversion to JSON is needed. The library parses the `.synx` file itself and returns a native object for your language — `dict` in Python, `Object` in JavaScript. You work with data immediately.

### 5.1 Installation

```bash
# JavaScript / TypeScript
npm install @aperturesyndicate/synx-format

# Python
pip install synx-format
```

---

### 5.2 Python — Reading `.synx` Directly

```python
from synx import Synx

# Read a .synx file — and get a dict immediately
data = Synx.load('config.synx')

# Done. Data is ready. Work with it like a regular dictionary:
print(data['app_name'])                # "TotalWario"
print(data['server']['port'])          # 8080
print(data['gameplay']['base_hp'])     # 100
print(data['gameplay']['boss_hp'])     # 500 (computed by :calc)
print(data['gameplay']['greeting'])    # "Wario time!" (chosen by :random)

# Lists are regular lists:
for map_name in data['map_rotation']:
    print(f'Loading map: {map_name}')

# Nesting is regular dicts:
if data['server']['ssl_enabled']:
    print('SSL is enabled')
```

**`Synx.load(path)`** — reads the file and parses in one call.  
**`Synx.parse(text)`** — parses a string (if you already have the content).

```python
# Alternatively — from a string:
with open('config.synx', 'r', encoding='utf-8') as f:
    data = Synx.parse(f.read())
```

---

### 5.3 JavaScript — Reading `.synx` Directly

```javascript
const Synx = require('@aperturesyndicate/synx-format');

// Read a .synx file — get a regular JS object
const data = Synx.loadSync('config.synx');

// Work with it like a normal object using dot notation:
console.log(data.app_name);               // "TotalWario"
console.log(data.server.port);             // 8080
console.log(data.gameplay.base_hp);        // 100
console.log(data.gameplay.boss_hp);        // 500
console.log(data.gameplay.greeting);       // random phrase

// Lists are regular Arrays:
data.map_rotation.forEach(map => {
    console.log(`Loading map: ${map}`);
});

// Destructuring:
const { base_hp, boss_hp, greeting } = data.gameplay;
console.log(`Boss HP: ${boss_hp}, greeting: ${greeting}`);
```

**`Synx.loadSync(path)`** — reads the file and parses synchronously.  
**`Synx.load(path)`** — async version (returns `Promise`).  
**`Synx.parse(text)`** — parses a string.

```javascript
// Async variant:
const data = await Synx.load('config.synx');

// From a string:
const fs = require('fs');
const text = fs.readFileSync('config.synx', 'utf-8');
const data = Synx.parse(text);
```

---

### 5.4 TypeScript — With Type Safety

```typescript
import Synx from '@aperturesyndicate/synx-format';

// Define the structure of your config:
interface GameConfig {
  app_name: string;
  version: string;
  server: {
    port: number;
    host: string;
    ssl_enabled: boolean;
  };
  gameplay: {
    base_hp: number;
    boss_hp: number;
    greeting: string;
    loot_tier: string;
  };
  map_rotation: string[];
}

// The parser returns a typed object:
const data = Synx.loadSync<GameConfig>('config.synx');

console.log(data.gameplay.boss_hp);  // number — autocomplete works
console.log(data.server.port);       // number
```

---

### 5.5 Using SYNX Data for CSS

SYNX returns a plain object — `Object` in JS, `dict` in Python. You can use this data to generate CSS, CSS custom properties, or pass it into any styling system.

**Example: CSS custom properties from SYNX in Node.js**

```synx
# theme.synx
primary #5a6eff
secondary #ff5a8a
font_size 16
border_radius 8
spacing 12
```

```javascript
const Synx = require('@aperturesyndicate/synx-format');
const fs = require('fs');

const theme = Synx.loadSync('theme.synx');

// Generate CSS custom properties
const css = `:root {
  --color-primary: ${theme.primary};
  --color-secondary: ${theme.secondary};
  --font-size: ${theme.font_size}px;
  --border-radius: ${theme.border_radius}px;
  --spacing: ${theme.spacing}px;
}`;

fs.writeFileSync('theme.css', css);
```

**Example: Inline styles in React**

```tsx
import Synx from '@aperturesyndicate/synx-format';

const theme = Synx.loadSync('theme.synx');

function Button({ children }) {
  return (
    <button style={{
      backgroundColor: theme.primary,
      borderRadius: `${theme.border_radius}px`,
      padding: `${theme.spacing}px`,
      fontSize: `${theme.font_size}px`,
    }}>
      {children}
    </button>
  );
}
```

**Example: CSS-in-JS (styled-components, Tailwind config)**

```javascript
// tailwind.config.js
const Synx = require('@aperturesyndicate/synx-format');
const theme = Synx.loadSync('theme.synx');

module.exports = {
  theme: {
    extend: {
      colors: {
        primary: theme.primary,
        secondary: theme.secondary,
      },
      spacing: {
        base: `${theme.spacing}px`,
      },
    },
  },
};
```

> SYNX doesn't replace CSS — it provides **data** (colors, sizes, tokens) that your code uses
to generate styles. This is especially useful for design systems and theming.

---

### 5.6 Rust — Type-Safe Parsing

The SYNX Rust crate (`synx`) provides zero-dependency parsing with a rich type system.

**Installation (Cargo.toml):**
```toml
[dependencies]
synx = "4.1"
```

**Basic Usage:**
```rust
use synx::Synx;

fn main() {
    // Parse a .synx file
    let data = Synx::parse_file("config.synx").unwrap();
    
    // Access values with type safety
    if let Some(port) = data.get("server").and_then(|s| s.get("port")) {
        let port_num: i64 = port.as_int().unwrap_or(8080);
        println!("Server running on port {}", port_num);
    }
    
    // Work with nested objects
    let app_name = data
        .as_object()
        .and_then(|obj| obj.get("app_name"))
        .and_then(|v| v.as_str());
    
    // Convert to JSON if needed
    let json = serde_json::to_string(&data).unwrap();
}
```

**Value type methods:**
- `.as_str()` → `Option<&str>`
- `.as_int()` → `Option<i64>`
- `.as_float()` → `Option<f64>`
- `.as_bool()` → `Option<bool>`
- `.as_object()` → `Option<&HashMap<String, Value>>`
- `.as_array()` → `Option<&Vec<Value>>`
- `.is_null()` → `bool`

**Indexing:**
```rust
let value = data["server"]["port"];
```

**Note:** The Rust parser currently supports **static mode** (no `:random`, `:calc`, `:env` functions). Use it for parsing SYNX data files. For active mode, use the JavaScript or Python libraries.

---

### 5.7 C — FFI to `synx-core` (JSON strings)

The supported C integration is **`bindings/c-header`**: the header `include/synx.h` plus a **shared or static library** produced by the Rust crate **`synx-c`** (`cdylib` / `staticlib`). Functions return **UTF-8 JSON strings** (or byte buffers for `.synxb`); there is **no** `SynxValue` AST in C — use your JSON library if you need a native tree.

**Build the library** (from the repo root):

```bash
cargo build -p synx-c --release
# Linux: target/release/libsynx_c.so
# macOS: target/release/libsynx_c.dylib
# Windows: target/release/synx_c.dll (+ MSVC import .lib)
```

**Include:**

```c
#include "synx.h"
```

**Memory:** every `char*` result is heap-allocated; the caller must `synx_free()` exactly once. Byte buffers from `synx_compile` must be released with `synx_free_bytes(ptr, len)`.

**Functions (v3.6.0):** `synx_parse`, `synx_parse_active`, `synx_stringify`, `synx_format`, `synx_parse_tool`, `synx_compile`, `synx_decompile`, `synx_is_synxb`, `synx_diff` — see `bindings/c-header/include/synx.h`.

**Minimal example:**

```c
#include "synx.h"
#include <stdio.h>

int main(void) {
    char *json = synx_parse("name John\nage 25\n");
    if (!json) {
        fputs("parse failed\n", stderr);
        return 1;
    }
    puts(json);
    synx_free(json);
    return 0;
}
```

> **Parity:** This path uses the **same** engine as **`synx-core` 3.6.x** (`!active`, `!tool`, `.synxb`, canonical JSON).

---

### 5.8 C++ — thin wrapper (`bindings/cpp`)

The supported C++ surface is **`bindings/cpp/include/synx/synx.hpp`** (namespace `synx`) — a **thin C++17 wrapper** over **§5.7**. It exposes `std::optional`-based helpers for `std::string_view` / `std::string` and `std::vector<unsigned char>` for `.synxb`. You must link the same **`synx-c`** library and place both include directories on your path (`bindings/cpp/include`, `bindings/c-header/include`).

**Example:**

```cpp
#include <synx/synx.hpp>
#include <iostream>

int main() {
    auto json = synx::parse("name Wario\nage 30\n");
    if (!json) return 1;
    std::cout << *json << '\n';
    return 0;
}
```

**Wrappers:** `parse`, `parse_active`, `stringify`, `format`, `parse_tool`, `diff`, `compile`, `decompile`, `is_synxb` — same semantics as `synx.h`.

Build notes: `bindings/cpp/README.md` and `bindings/cpp/CMakeLists.txt` (optional minimal example).

> **Parity:** Behavior is **identical** to **Rust `synx-core`** for all FFI-exported operations.

---

### 5.9 C# — .NET 8 library (`parsers/dotnet`)

The supported C# implementation is **`parsers/dotnet/src/Synx.Core`**, targeting **.NET 8.0**. NuGet package ID **`APERTURESyndicate.Synx`** (the ID **`Synx.Core`** is taken on nuget.org).

**Install via NuGet:**
```bash
dotnet add package APERTURESyndicate.Synx
```

Listing: [nuget.org/packages/APERTURESyndicate.Synx](https://www.nuget.org/packages/APERTURESyndicate.Synx). Until the package is on the feed, use a project reference or local `.nupkg` — see [`parsers/dotnet/README.md`](../../parsers/dotnet/README.md).

**Basic usage:**
```csharp
using Synx;

var map = SynxFormat.Parse("server\n  host 0.0.0.0\n  port 8080\n");
if (map["server"] is SynxValue.Obj server
    && server.Map["port"] is SynxValue.Int port)
    Console.WriteLine(port.Value);

var json = SynxFormat.ToJson(map);

// !active resolution (markers, constraints, includes)
var resolved = SynxFormat.ParseActive("!active\nport:env:default:8080 PORT\n");
```

**Entry points (`SynxFormat`):** `Parse`, `ParseActive`, `ParseFull`, `ParseFullActive`, `ParseTool`, `ToJson`.

**Values:** `SynxValue` discriminated records — `Null`, `Bool`, `Int`, `Float`, `Str`, `Secret`, `Arr`, `Obj`.

> **Parity:** Static parse and canonical JSON match **`synx-core`** for covered conformance cases; **`ParseActive`** runs the managed `!active` engine. **`.synxb`** in C# is not implemented yet (use **`synx-core`** or bindings that wrap **`synx-c`**).

---

### 5.10 Go — cgo binding to `synx-c` (JSON API)

The supported Go integration is **`bindings/go`**: **cgo** wraps the same **`synx.h`** / **`synx-c`** library as C and C++. Functions return **UTF-8 strings** (JSON or SYNX text) or **byte slices** (`.synxb`); use **`encoding/json`** if you need a tree.

**Requirements:** Go 1.21+, `CGO_ENABLED=1`, a C toolchain, and **`synx_c`** installed/built (`cargo build -p synx-c --release`). Linux/macOS link flags default to `-L../../target/release -lsynx_c` relative to `bindings/go`. **Windows:** link `synx_c.dll.lib` via `CGO_LDFLAGS`, put `synx_c.dll` on `PATH` — see [`bindings/go/README.md`](../../bindings/go/README.md).

**Example:**

```go
package main

import (
    "fmt"
    synx "github.com/APERTURESyndicate/synx-format/bindings/go"
)

func main() {
    j, err := synx.Parse("name Wario\nage 30\n")
    if err != nil {
        panic(err)
    }
    fmt.Println(j)
}
```

**API (v3.6.0):** `Parse`, `ParseActive`, `Stringify`, `Format`, `ParseTool`, `Compile`, `Decompile`, `IsSynxb`, `Diff` — mirror `synx.h`.

> **Parity:** Same engine as **`synx-core`** (`!active`, `!tool`, `.synxb`, canonical JSON).

---

### 5.11 Mojo — Python interop (`bindings/mojo`)

[Mojo](https://docs.modular.com/mojo/) can call **CPython** via [`Python.import_module`](https://docs.modular.com/mojo/manual/python/python-from-mojo). The supported way to get **SYNX 3.6.0 parity** with **`synx-core`** is to import **`synx_native`** (the same **PyO3 / maturin** wheel as `pip install synx-format`). This is **not** a standalone Mojo grammar implementation (that would be a separate, large port — analogous to maintaining `synx-js` alongside Rust).

**Setup:** Install Python **`synx_native`** (wheel from this repo or PyPI). Optionally `Python.add_to_path(...)` if you built locally with `maturin develop`.

**String helpers (also on `synx_native` for thin callers):** `parse_to_json`, `parse_active_to_json`, `parse_tool_to_json`, `stringify_json`, `format`, `compile_hex`, `decompile_hex`, `is_synxb_hex`, `diff_json`.

**Example (`bindings/mojo/examples/demo.mojo` pattern):**

```mojo
from std.python import Python
from synx.interop import parse_json

def main() raises:
    var j = parse_json("name Wario\nage 30\n")
    print(j)
```

See [`bindings/mojo/README.md`](../../bindings/mojo/README.md).

> **Parity:** Identical to **`synx-core`** because execution runs inside **`synx_native`** (Rust).

---

### 5.12 Kotlin / JVM — `bindings/kotlin` (JNA + `synx-c`)

The supported JVM integration in this repository is **`bindings/kotlin`**: **`SynxEngine`** loads **`synx_c`** through [**JNA**](https://github.com/java-native-access/jna). Parse results are **canonical JSON** `String` values (or **`ByteArray`** for `.synxb`) unless you decode them yourself.

**1. Build the native library** (repo root): `cargo build -p synx-c --release`.

**2. Build / test** from `bindings/kotlin`: set **`SYNX_LIB_DIR`** to the directory containing `libsynx_c` / `synx_c.dll`, then `./gradlew test`. See [`bindings/kotlin/README.md`](../../bindings/kotlin/README.md).

**Example:**

```kotlin
import com.aperturesyndicate.synx.SynxEngine

val json = SynxEngine.parse("name Wario\nage 30\n")
val tool = SynxEngine.parseTool("!tool\nweb_search\n  query test\n")
val active = SynxEngine.parseActive("!active\nport:env:default:8080 PORT\n")
```

**API (v3.6.0):** `SynxEngine.parse`, `parseActive`, `stringify`, `format`, `parseTool`, `diff`, `compile`, `decompile`, `isSynxb` — mirrors [`bindings/c-header/include/synx.h`](../../bindings/c-header/include/synx.h).

> **Parity:** Same engine as **`synx-core`** (including **`!active`**, **`!tool`**, **`.synxb`**, **`diff`**). Tooling uses **JDK 17+** for Gradle; published **`com.aperturesyndicate:synx-kotlin`** may follow (use **`publishToMavenLocal`** until then).

---

### 5.13 Swift — SwiftPM + `synx-c` (C interop)

The supported Swift integration is **`bindings/swift`**: a **Swift Package** that wraps **`synx.h`** / **`synx-c`** via **`CSynx`** (`module.modulemap`, `link "synx_c"`). Types are `String` / `Data`; parse results are **canonical JSON** strings unless you decode yourself.

**Build the C library** (repo root): `cargo build -p synx-c --release`, then link `libsynx_c` when building the package (`swift build -Xlinker -L… -Xlinker -lsynx_c`). See [`bindings/swift/README.md`](../../bindings/swift/README.md).

**Example:**

```swift
import Synx

let json = try SynxEngine.parse("name Wario\nage 30\n")
print(json)

let tool = try SynxEngine.parseTool("!tool\nweb_search\n  query test\n")
```

**API (v3.6.0):** `SynxEngine.parse`, `parseActive`, `stringify(json:)`, `format`, `parseTool`, `diff`, `compile`, `decompile(_:)`, `isSynxb` — mirrors `synx.h`.

> **Parity:** Same engine as **`synx-core`**. `Sources/CSynx/synx.h` must stay in sync with [`bindings/c-header/include/synx.h`](../../bindings/c-header/include/synx.h).

---

### 5.14 Lua — Zero-Dependency Parser

**Installation:**

Copy `synx.lua` into your project:

```lua
local synx = require("synx")
```

**Usage:**

```lua
local synx = require("synx")

-- Parse from string
local data = synx.parse(text)
print(data:get("server"):get("port"):as_int())  -- 8080

-- Parse from file
local data = synx.parse_file("config.synx")
print(data:get("server"):get("host"):as_str())  -- localhost

-- Full parse (mode detection)
local result = synx.parse_full(text)
print(result.mode)  -- "static" or "active"
```

**API:**

- `synx.parse(text)` → `SynxValue`
- `synx.parse_full(text)` → `{ root, mode }`
- `synx.parse_file(path)` → `SynxValue, err`
- `SynxValue:get(key)` — child by key or 1-based index
- `SynxValue:as_bool()`, `:as_int()`, `:as_float()`, `:as_str()`
- `SynxValue:len()`, `:keys()`, `:type()`, `:is_null()`

> 📌 **Note:** The Lua parser is static-only. No active function evaluation. Works with Lua 5.1+ and LuaJIT.

---

### 5.15 Dart / Flutter — Native Parser

**Installation (pubspec.yaml):**

```yaml
dependencies:
  synx:
    git:
      url: https://github.com/APERTURESyndicate/synx-format.git
      path: packages/synx-dart
```

**Usage:**

```dart
import 'package:synx/synx.dart';

// Parse from string
final data = Synx.parse(text);
print(data['server']['port'].asInt); // 8080

// Parse from file
final data = Synx.parseFile('config.synx');

// Full parse (mode detection)
final result = Synx.parseFull(text);
print(result.mode); // SynxMode.active
```

**Flutter usage:**

```dart
import 'package:synx/synx.dart';
import 'package:flutter/services.dart' show rootBundle;

Future<SynxValue> loadConfig() async {
  final text = await rootBundle.loadString('assets/config.synx');
  return Synx.parse(text);
}
```

**API:**

- `Synx.parse(text)` → `SynxValue`
- `Synx.parseFull(text)` → `SynxParseResult { root, mode }`
- `Synx.parseFile(path)` → `SynxValue`
- `SynxValue[key]` — subscript by string key or int index
- `.asBool`, `.asInt`, `.asFloat`, `.asStr`
- `.length`, `.keys`, `.type`, `.isNull`

> 📌 **Note:** The Dart parser is static-only. No active function evaluation. Requires Dart 3.0+.

---

### 5.16 PHP — Native Parser

**Installation:**

Copy `Synx.php` into your project:

```php
require_once 'Synx.php';
```

**Usage:**

```php
require_once 'Synx.php';

// Parse from string
$data = Synx::parse($text);
echo $data->get('server')->get('port')->asInt(); // 8080

// Parse from file
$data = Synx::loadFile('config.synx');

// Full parse (mode detection)
$result = Synx::parseFull($text);
echo $result->mode; // "static" or "active"
```

**API:**

- `Synx::parse($text)` → `SynxValue`
- `Synx::parseFull($text)` → `SynxParseResult { root, mode }`
- `Synx::loadFile($path)` → `SynxValue`
- `->get($key)` — child by string key or int index
- `->asBool()`, `->asInt()`, `->asFloat()`, `->asStr()`
- `->length()`, `->keys()`, `->type()`, `->isNull()`

> 📌 **Note:** The PHP parser is static-only. No active function evaluation. Requires PHP 8.0+.

---

### 5.17 Bash / PowerShell — Shell Parsers

#### Bash (4.0+)

**Installation:**

```bash
source synx.sh
```

**Usage:**

```bash
source synx.sh

synx_parse_file "config.synx"
echo "$(synx_get server.host)"     # localhost
echo "$(synx_get server.port)"     # 8080
echo "$(synx_type server.port)"    # int
echo "$(synx_mode)"                # static

# Arrays
echo "$(synx_get items.0)"         # Sword
echo "$(synx_get items.__length)"  # 3
```

**API:**

- `synx_parse "$text"` — parse SYNX text
- `synx_parse_file "$path"` — read and parse file
- `synx_get "$path"` — value at dot-notation path
- `synx_type "$path"` — type name
- `synx_mode` — document mode

> **Note:** Values are stored in flat associative arrays with dot-notation keys (e.g. `server.host`).

#### PowerShell (5.1+)

**Installation:**

```powershell
. .\Synx.ps1
```

**Usage:**

```powershell
. .\Synx.ps1

$result = Read-SynxFile "config.synx"
$result.Root.server.host           # localhost
$result.Root.server.port           # 8080 (int)
$result.Root.items[0]              # Sword
$result.Mode                       # static
```

**API:**

- `ConvertFrom-Synx $text` → `PSCustomObject { Root, Mode }`
- `Read-SynxFile $path` → same

Objects are `[ordered]@{}`, arrays are native PowerShell arrays, scalars are typed (`[long]`, `[double]`, `[bool]`, `[string]`, `$null`).

> 📌 **Note:** Both shell parsers are static-only. No active function evaluation.

---

### 5.18 Comparison: SYNX Directly vs JSON

| | JSON | SYNX |
|---|---|---|
| **Reading (JS)** | `JSON.parse(fs.readFileSync(...))` | `Synx.loadSync('file.synx')` |
| **Reading (Python)** | `json.load(open(...))` | `Synx.load('file.synx')` |
| **Reading (Rust)** | `serde_json::from_str(...)` | `Synx::parse_file(...)` |
| **Reading (C)** | `cJSON_Parse(...)` | `synx_parse(text)` |
| **Reading (C++)** | `nlohmann::json::parse(...)` | `synx::Synx::load(path)` |
| **Reading (C#)** | `JsonSerializer.Deserialize(...)` | `SynxFormat.Parse(File.ReadAllText(path))` |
| **Reading (Go)** | `json.Unmarshal(...)` | `synx.ParseFile(path)` |
| **Reading (Java)** | `new ObjectMapper().readTree(...)` | `Synx.load(path)` |
| **Reading (Swift)** | `JSONDecoder().decode(...)` | `Synx.load(path)` |
| **Reading (Lua)** | `cjson.decode(...)` | `synx.parse(text)` |
| **Reading (Dart)** | `jsonDecode(...)` | `Synx.parse(text)` |
| **Reading (PHP)** | `json_decode(...)` | `Synx::parse($text)` |
| **Reading (Bash)** | `jq '.key' file.json` | `synx_get "key"` |
| **Reading (PowerShell)** | `ConvertFrom-Json` | `ConvertFrom-Synx` |
| **Intermediate format** | None | None — same! |
| **What you get** | Object / dict / Value | Object / dict / Value (same) |
| **Built-in logic** | No — write it yourself | `:random`, `:calc`, `:env`, etc. |
| **Validation** | No — need a separate lib | `[min:3, type:int]` in the file |
| **File size** | ~100% | ~40% smaller |

**By language:**

| Language | Package | Method | Notes |
|---|---|---|---|
| JavaScript | `@aperturesyndicate/synx-format` | `Synx.loadSync()` | Full engine (active + static) |
| Python | `synx-format` | `Synx.load()` | Full engine (active + static) |
| Rust | `synx` (crates.io) | `Synx::parse()` | Zero-dependency, static-only |
| C | `synx.h` + `synx-c` lib | `synx_parse()` → JSON | FFI to `synx-core` 3.6.x |
| C++ | `synx/synx.hpp` + same lib | `synx::parse()` → `optional<string>` | Thin wrapper; same engine |
| C# | `APERTURESyndicate.Synx` (NuGet) | `SynxFormat` / parser API | .NET 8 library; `Synx.Core` ID was taken on nuget.org |
| Go | `bindings/go` (cgo) + `synx-c` | `synx.Parse()` → JSON | Same engine as Rust |
| Mojo | `bindings/mojo` + CPython `synx_native` | `parse_json()` / `parse_active_json()` … | Full parity via PyO3; not a pure-Mojo parser |
| Kotlin/JVM | `bindings/kotlin` + `synx-c` | `SynxEngine.parse()` → JSON `String` | JNA; JDK 17+ build; same engine as Rust |
| Swift | `bindings/swift` + `synx-c` | `SynxEngine.parse()` → JSON `String` | C interop; same engine as Rust |
| Lua | `synx.lua` (copy) | `synx.parse()` | Lua 5.1+, zero-dep, static-only |
| Dart/Flutter | `synx` (pub.dev / git) | `Synx.parse()` | Dart 3.0+, static-only |
| PHP | `Synx.php` (copy) | `Synx::parse()` | PHP 8.0+, zero-dep, static-only |
| Bash | `synx.sh` (copy) | `synx_get()` | Bash 4.0+, zero-dep, static-only |
| PowerShell | `Synx.ps1` (copy) | `ConvertFrom-Synx` | PowerShell 5.1+, zero-dep, static-only |

---

## 6. Conversion to JSON (optional)

> 📎 JSON conversion is an **optional** feature. It's only needed if you want to pass data to a system that exclusively understands JSON (third-party APIs, legacy code, etc.).

### 6.1 Via VS Code (SYNX Extension)

If the **SYNX for VS Code** extension is installed:

1. Open a `.synx` file
2. **Right-click** on the file
3. Select **"Convert to JSON"**
4. A `.json` file will appear alongside

### 6.2 Via Terminal (CLI)

```bash
# Convert to JSON and print to console
synx to-json config.synx

# Convert and save to file
synx to-json config.synx -o config.json

# Freeze an !active config into a static .synx (without functions)
synx freeze active_config.synx -o static_config.synx
```

### 6.3 Programmatically (if you really need to)

```python
import json
from synx import Synx

data = Synx.load('config.synx')
json_string = json.dumps(data, ensure_ascii=False, indent=2)
```

```javascript
const Synx = require('@aperturesyndicate/synx-format');
const data = Synx.loadSync('config.synx');
const jsonString = JSON.stringify(data, null, 2);
```

> But again: **this is not needed to work with SYNX**. Your code reads `.synx` directly.

---

## 7. Quick Reference

```
KEY VALUE                        → simple pair
key                              → empty object (group)
  nested_key value               → nesting (2 spaces)
key |                            → multiline text
  line 1                           (block)
  line 2
list                             → list
  - item 1
  - item 2
# comment                       → single-line comment
// comment                      → single-line comment

─── Only with !active ──────────────────────────
key:random                       → random item
key:random 70 20 10              → weighted random
key:calc A * B                   → arithmetic
key:env VAR                      → environment variable
key:env:default:X VAR            → env with default
key:alias other_key              → reference to another key
key:secret value                 → hidden value
key:unique                       → list deduplication
key:include ./file.synx          → include file
key[min:N, max:N]                → length/value constraints
key[type:int]                    → data type
key[required]                    → required field
key[enum:a|b|c]                  → allowed values
key[pattern:regex]               → regex validation
key[readonly]                    → read-only
```
