# SYNX Go binding (`synx`) — cgo over `synx-core`

**Version:** 3.6.0 — same engine as Rust via the **`synx-c`** shared library.

This module is **not** a second parser: it calls `synx_parse`, `synx_parse_active`, `synx_stringify`, `synx_format`, `synx_parse_tool`, `synx_compile`, `synx_decompile`, `synx_is_synxb`, and `synx_diff` from [`synx.h`](../c-header/include/synx.h).

## Requirements

- Go **1.21+**
- **`CGO_ENABLED=1`**
- A **C compiler** (same as usual for cgo: `gcc`/`clang` on Linux/macOS; on Windows often **MinGW-w64** or MSVC-backed toolchain that Go supports)
- Native library **`synx_c`** on the link path (see below)

## Build `synx_c` (repo root)

```bash
cargo build -p synx-c --release
```

Produces (typical):

- Linux: `target/release/libsynx_c.so`
- macOS: `target/release/libsynx_c.dylib`
- Windows: `target/release/synx_c.dll` (+ import `.lib` for some linkers)

**Linux / macOS:** `synx.go` defaults to **`-L../../target/release -lsynx_c`** (relative to `bindings/go`). Build with `cargo build -p synx-c --release` first.

**Windows (Rust `x86_64-pc-windows-msvc`):** the static `synx_c.lib` is **MSVC** and cannot be linked by **MinGW** `gcc` (cgo’s usual linker). Link the **import** library and load the **DLL** at run time:

```powershell
$env:CGO_LDFLAGS = "A:\full\path\to\repo\target\release\synx_c.dll.lib"
$env:CGO_LDFLAGS_ALLOW = ".*"
$env:PATH = "A:\full\path\to\repo\target\release;" + $env:PATH
```

(`CGO_LDFLAGS_ALLOW` is required when the flag path looks “non-standard” to the Go toolchain.)

Alternatively build **`synx-c`** for **`x86_64-pc-windows-gnu`** and use `-lsynx_c` like Linux (artifact under `target/x86_64-pc-windows-gnu/release/`).

Use **release** for normal use; **debug** works if you point `CGO_LDFLAGS` / `-L` at `target/debug`.

## Use as a module (this repo)

From another module in the same checkout:

```go
import synx "github.com/APERTURESyndicate/synx-format/bindings/go"
```

```bash
go mod edit -replace github.com/APERTURESyndicate/synx-format/bindings/go=../bindings/go
```

Optional: set **absolute** library path if the default `-L` is wrong for your layout:

```bash
export CGO_LDFLAGS="-L$PWD/target/release -lsynx_c"
# Windows (PowerShell, MinGW-style): $env:CGO_LDFLAGS="-L/$(pwd)/target/release -lsynx_c"
```

On **Windows**, linking `synx_c` from MSVC with **gcc** cgo may need extra steps (import library format). Prefer **WSL/Linux/macOS** for the smoothest path, or align your C toolchain with how Rust built the DLL.

## API

| Go function | FFI |
|-------------|-----|
| `Parse` | `synx_parse` |
| `ParseActive` | `synx_parse_active` |
| `Stringify` | `synx_stringify` |
| `Format` | `synx_format` |
| `ParseTool` | `synx_parse_tool` |
| `Compile` | `synx_compile` |
| `Decompile` | `synx_decompile` |
| `IsSynxb` | `synx_is_synxb` |
| `Diff` | `synx_diff` |

String functions return `(string, error)`; `error` is `ErrSynx` when the C API returns NULL. `Compile` returns `([]byte, error)`.

## Example

```go
package main

import (
	"fmt"
	synx "github.com/APERTURESyndicate/synx-format/bindings/go"
)

func main() {
	j, err := synx.Parse("name Synx\ncount 3\n")
	if err != nil {
		panic(err)
	}
	fmt.Println(j)
}
```

## Tests

From `bindings/go` after `cargo build -p synx-c --release`:

```bash
CGO_ENABLED=1 go test ./...
```

**Windows (PowerShell):** set `CGO_LDFLAGS`, `CGO_LDFLAGS_ALLOW`, and prepend `target\release` to `PATH` as in the Windows section above, then `go test ./...`.

## Conformance

Behavior matches `tests/conformance/` and `synx-core` **3.6.x**; this package only marshals C strings and bytes.
