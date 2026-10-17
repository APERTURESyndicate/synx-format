# SYNX — native Go parser

`parsers/go` is the **native** SYNX parser written in pure Go. No cgo, no
`synx-c` cdylib at runtime, no `CGO_ENABLED=1`. Implements **SYNX 3.8.0** as
`crates/synx-core` does it: parse, `!active` resolution, `.synxb`, canonical
JSON, stringify, diff, format and `!tool` give the reference's output byte for
byte (the conformance corpus in `tests/conformance` is replayed by
`conformance_test.go`).

Targets:

* **Go 1.21+** — covers go modules and stdlib `compress/flate`.
* **Any GOOS / GOARCH** — pure Go means cross-compile is one `GOOS=linux
  GOARCH=arm64 go build ./...` away (the cgo-based `bindings/go` cannot do this
  without a C cross toolchain).

| Property | Value |
|---|---|
| Go version | 1.21+ |
| External deps | None (only stdlib: `compress/flate`, `regexp`, `math/rand`) |
| Build | `go build ./...` |
| Tests | `go test ./...` |
| Wire-compat with Rust | yes (canonical JSON, `.synxb` payload) |
| Version | `synx.Version` = `3.8.0` |

## Quick start

```bash
go get github.com/aperturesyndicate/synx
go test ./...
```

```go
package main

import (
    "fmt"
    "github.com/aperturesyndicate/synx"
)

func main() {
    // Static parse
    root := synx.ParseRoot("name App\nport 8080\n")
    fmt.Println(synx.ToJSON(synx.Object_(root)))

    // Active engine with env vars
    opts := synx.Options{Env: map[string]string{"PORT": "9090"}}
    active := synx.ParseActive("!active\nport:env:default:3000 PORT\n", opts)
    fmt.Println(synx.ToJSON(synx.Object_(active)))

    // Binary
    bytes, _ := synx.CompileText("name App\n", false)
    text, _ := synx.DecompileToText(bytes)
    fmt.Println(text)
}
```

### Custom markers

```go
opts := synx.Options{
    MarkerFns: map[string]synx.MarkerFn{
        "upper": func(key string, args []string, value synx.Value) synx.Value {
            if s, ok := synx.AsString(value); ok {
                return synx.String(strings.ToUpper(s))
            }
            return value
        },
    },
}
root := synx.ParseActive("!active\nslug:upper hello", opts)
```

## Layout

```
parsers/go/
├── go.mod
├── value.go        // Value interface + concrete types
├── object.go       // ordered key/value container
├── meta.go         // Meta, Constraints, ParseResult, directives
├── options.go      // Options, MarkerFn
├── parser.go       // text → tree
├── calc.go         // SafeCalc recursive descent
├── json.go         // ToJSON canonical
├── stringify.go    // Stringify + Format
├── diff.go         // Diff + DiffToValue
├── binary.go       // .synxb compile/decompile (compress/flate raw DEFLATE)
├── engine.go       // Resolve: document-order walk, markers, inheritance
├── markers.go      // marker helpers: :format, :version, files, !use, :once
├── constraints.go  // constraints, registries, field validation
├── synx.go         // top-level wrappers
└── *_test.go       // unit + conformance corpus runner
```

## Limits (parity with Rust)

| Cap | Value |
|---|---|
| Input bytes | 16 MiB |
| Indexed line starts | 2 million |
| Parse nesting depth | 128 |
| Multiline block | 1 MiB |
| List items per list | 1,048,576 |
| `!include` directives | 4096 |
| Marker chain segments | 512 |
| Engine resolve depth | 512 |
| Serialize / JSON depth | 128 |
| Constraint enum parts | 4096 |

## Active mode

Resolution follows synx-core: one pass per object in document order — for
each key, first its child object (or each object item of its list), then the
key's own markers, then `{…}` interpolation over the object — so a marker sees
every key above it already resolved, at any depth. Declarations belong to the
object that holds the key: `ParseResult.Metadata` is an ordered
`*MetadataTree` keyed by metadata path, where a list item is `list.#0` and a
`.` or `\` inside a key is escaped (`a\.b`); `MetadataTree.Get(path)` returns
an ordered `*MetaMap`.

## What's unsupported (versus Rust core)

* **SYNXL** (`.synxl`) is not implemented — see `../README.md`.
* **WASM custom markers** — replaced by `Options.MarkerFns` (Go closures),
  applied after the built-in markers, before the key's constraints. Builtin
  markers always win over a custom marker with the same name.
* **`[pattern]`** runs on Go's `regexp` (RE2). The reference uses Rust's
  `regex` crate; both are RE2-style, but the dialects differ at the edges
  (Unicode classes such as `\d`, `\pL`, unusual repetition syntax), so a rare
  pattern can match differently or be accepted by one and skipped by the other.
* **`:signing`** — Ed25519 signature verification not ported (consistent
  with the C++ / Swift / Java / GDScript ports).
* `:vision` / `:audio` are passthrough envelopes; consumers attach media
  bytes outside the parser.

## Wire compatibility

* **JSON**: byte-for-byte identical to Rust ryu/itoa output (shortest
  round-trip floats in ryu's layout: `1e-7`, `1.2345678901234569e23`, `5.0`).
* **`.synxb`**: identical magic / version / flags and an identical
  uncompressed payload — the string table is ordered by use count as in
  synx-core, so the same document always compiles to the same bytes. Files
  written here decompile under Rust / C# / Swift / Java / GDScript / C++ and
  vice-versa. Compression goes through
  `compress/flate.NewWriter(_, flate.BestCompression)` — raw DEFLATE, no zlib
  wrapper; the compressed bytes differ from `miniz_oxide`'s, the decoded
  payload does not.
* **Stringify / decompile**: the SYNXL §14.1 block writer of synx-core —
  strings that would read back as another type or a comment are quoted, `|+`
  bodies are kept, secrets are written as `[SECRET]`.
* **Canonical text format**: same sort key (code point order of the
  lowercased key, stable) and indentation rules as the Rust `fmt_canonical`;
  a `|` / `|+` body is content and is never sorted.
