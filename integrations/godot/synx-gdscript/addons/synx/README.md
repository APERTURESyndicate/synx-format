# SYNX for Godot — Native GDScript Parser

Native GDScript implementation of the SYNX (Active Data Format) parser, engine,
binary `.synxb` codec, diff, stringify, and tool-mode reshape. No native
dependencies, no FFI, no C/Rust — pure GDScript.

> Main SYNX site: https://synx.aperturesyndicate.com/
> Source of truth: `synx-core` (Rust). This implementation targets full parity.

## Install

1. Copy `addons/synx/` into your Godot project at `res://addons/synx/`.
2. Project → Project Settings → Plugins → enable **SYNX**.

That's it. Everything is registered through GDScript `class_name` so the API is
available globally:

```gdscript
var data := Synx.parse_to_variant("name Alice\nage 30")
# data == { "name": "Alice", "age": 30 }
```

## Quick start

### Static parse

```gdscript
var text := """
name Alice
age 30
server
  host 0.0.0.0
  port 8080
inventory
  - Sword
  - Shield
"""

# As plain Variant (Dictionary/Array of strings/ints/...).
var d := Synx.parse_to_variant(text)
print(d["server"]["port"])  # 8080
```

### Active mode (`!active`)

```gdscript
var text := """
!active
host:env:default:0.0.0.0 HOST
port[min:1, max:65535]:env:default:3000 PORT
double_port:calc port * 2
"""

var opts := SynxOptions.new()
opts.env = { "PORT": "8080" }
var d := Synx.parse_active_to_variant(text, opts)
print(d["port"])         # 8080
print(d["double_port"])  # 16160
```

### Tool mode

```gdscript
var text := """
!tool
web_search
  query test
  lang ru
"""
var d := Synx.parse_tool(text)
# d == { "tool": SynxValue("web_search"), "params": SynxValue({ "query": ..., "lang": ... }) }
```

### Binary `.synxb`

```gdscript
var bin := Synx.compile(text, false)
var unpacked := Synx.decompile(bin)
if unpacked["ok"]:
    print(unpacked["text"])
```

### JSON output (canonical)

```gdscript
var r := Synx.parse_full_active(text, opts)
var json := Synx.to_json(r)
```

## Supported features

| Feature | Status |
|---|---|
| Static parse / nested objects / lists / multiline `|` | ✅ |
| Comments (`#`, `//`, `###...###`) | ✅ |
| Quoted strings | ✅ |
| Type hints `(int)` / `(string)` / `(random)` | ✅ |
| Constraints `[min, max, required, type, enum, pattern, readonly]` | ✅ |
| `!active` / `!lock` / `!tool` / `!schema` / `!llm` directives | ✅ |
| `!include` / `!use` directives | ✅ |
| `:env`, `:default`, `:calc`, `:ref`, `:alias`, `:secret` | ✅ |
| `:random` (with weights), `:unique`, `:geo` | ✅ |
| `:split`, `:join`, `:sort`, `:sum`, `:replace`, `:map` | ✅ |
| `:clamp`, `:round`, `:format`, `:fallback`, `:once`, `:version` | ✅ |
| `:include`, `:import`, `:watch` (file IO, sandboxed) | ✅ |
| `:i18n` with CLDR plural rules (ru/uk/be/pl/cs/sk/ar/fr/pt/ja/zh/ko/vi/th + default) | ✅ |
| `:inherit` (multi-parent merge) | ✅ |
| `{interpolation}` (`{key}`, `{key.path}`, `{key:alias}`, `{key:include}`) | ✅ |
| `:prompt`, `:vision`, `:audio` | ✅ |
| `.synxb` binary format (DEFLATE-compressed, wire-compatible with Rust) | ✅ |
| Structural diff | ✅ |
| Canonical formatter | ✅ |
| Tool-mode reshape (`!tool` / `!tool + !schema`) | ✅ |
| `__proto__` / `constructor` / `prototype` key rejection | ✅ |
| Resource caps (input size, nesting depth, multiline body, list items) | ✅ |
| **WASM marker packages** | ❌ replaced by `marker_callables` (`Callable`-based) |

### Custom markers via Callables

Where the Rust/C# engines use WASM modules for user-defined markers, this
GDScript engine exposes a `marker_callables` map. Register any GDScript
`Callable` and it becomes a marker available to your SYNX files:

```gdscript
var opts := SynxOptions.new()
opts.marker_callables = {
    "uppercase": func(value: SynxValue, args: PackedStringArray) -> SynxValue:
        return SynxValue.make_string(value.as_string().to_upper()),
}
var d := Synx.parse_active("!active\ntitle:uppercase hello", opts)
# d.title == "HELLO"
```

Builtin markers always win over Callable markers of the same name.

## Path sandboxing

`:include`, `:import`, `:watch`, `:fallback` resolve paths inside a base
directory (default `res://`). Absolute paths, `res://`, `user://`, and `../`
traversal are rejected with `INCLUDE_ERR` / `WATCH_ERR`. Override the base via
`SynxOptions.base_path`.

## Resource limits (same caps as Rust)

| Cap | Value |
|---|---|
| Max input size | 16 MiB |
| Max line count | 2,000,000 |
| Max parse nesting depth | 128 |
| Max multiline block size | 1 MiB |
| Max list items per list | 1,048,576 |
| Max `!include` directives | 4,096 |
| Max constraint enum parts | 4,096 |
| Max marker chain segments | 512 |
| Max calc expression length | 4,096 chars |
| Max calc resolved length | 64 KiB |
| Max `:include`/`:watch` file size | 10 MiB |
| Max include nesting depth | 16 (configurable via `SynxOptions.max_include_depth`) |

## Running the test suite

```bash
godot --headless --path integrations/godot/synx-gdscript --script tests/test_runner.gd
```

The runner executes 35+ unit tests plus the shared conformance corpus from
`tests/conformance/cases/*.synx` (if reachable from the project root).

## Layout

```
addons/synx/
  plugin.cfg                 — plugin manifest
  plugin.gd                  — editor plugin stub
  synx.gd                    — top-level facade (Synx.*)
  synx_value.gd              — tagged-union value type
  synx_meta.gd               — per-key metadata
  synx_parse_result.gd       — parse result + IncludeDirective / UseDirective
  synx_options.gd            — engine options
  synx_rng.gd                — RNG façade
  synx_parser.gd             — static parser
  synx_engine.gd             — !active engine core + includes/jail
  synx_engine_markers.gd     — marker dispatch + constraints + plural rules
  synx_safe_calc.gd          — sandboxed calc evaluator
  synx_stringify.gd          — value → SYNX text
  synx_json.gd               — value → canonical JSON
  synx_formatter.gd          — canonical .synx reformatter
  synx_binary.gd             — .synxb compile / decompile
  synx_diff.gd               — structural diff
tests/
  test_runner.gd             — SceneTree-based headless test runner
examples/
  demo.tscn                  — minimal scene (optional)
```

## Parity status

All conformance cases under `tests/conformance/cases/` should produce the same
canonical JSON output as `synx-core` (Rust) and `@aperturesyndicate/synx-format`
(TypeScript). Where a marker mutates external state (`:once` writes to
`.synx.lock`, `:spam` keeps in-process buckets), GDScript implementation follows
the same semantics.

## License

MIT — same as `synx-format`.
