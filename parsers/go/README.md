# SYNX — native Go parser

`parsers/go` is the **native** SYNX parser written in pure Go. No cgo, no
`synx-c` cdylib at runtime, no `CGO_ENABLED=1`. Mirrors the canonical Rust
engine 1:1 (parse / `!active` / `.synxb` / canonical JSON / diff / format /
`!tool`).

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
| Wire-compat with Rust | yes (canonical JSON + `.synxb` raw DEFLATE) |

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
├── engine.go       // Resolve, helpers, validation
├── markers.go      // 27 builtin marker functions
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

## What's unsupported (versus Rust core)

* **WASM custom markers** — replaced by `Options.MarkerFns` (Go closures).
  Builtin markers always win over a custom marker with the same name.
* **`:signing`** — Ed25519 signature verification not ported (consistent
  with the C++ / Swift / Java / GDScript ports).
* `:vision` / `:audio` are passthrough envelopes; consumers attach media
  bytes outside the parser.

## Wire compatibility

* **JSON**: byte-for-byte identical to Rust ryu/itoa output. Floats use
  `%.17g` with a mandatory `.0` suffix.
* **`.synxb`**: identical magic / version / flags / string table / DEFLATE
  payload as `crates/synx-core`. Files written here decompile under
  Rust / C# / Swift / Java / GDScript / C++ and vice-versa. Compression goes
  through `compress/flate.NewWriter(_, flate.BestCompression)` — raw DEFLATE
  (no zlib wrapper) — matching `miniz_oxide::deflate::compress_to_vec(_, 9)`.
* **Canonical text format**: same sort key and indentation rules as the
  Rust `fmt_canonical`.
