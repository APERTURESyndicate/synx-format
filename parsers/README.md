# Parsers and grammars

Everything that **parses** or **defines syntax** for SYNX lives under this tree (or is linked here until a folder move finishes).

| Path | Role |
|------|------|
| [`crates/synx-core/`](../crates/synx-core/) | Canonical **Rust** parser, engine (`!active`), stringify, `diff`, `.synxb`, fuzz targets |
| [`crates/synx-cli/`](../crates/synx-cli/) | **Rust** CLI (`synx`) — uses `synx-core` |
| [`packages/synx-js/`](../packages/synx-js/) | **TypeScript** reference parser + npm package `@aperturesyndicate/synx-format` |
| [`tree-sitter-synx/`](../tree-sitter-synx/) | **Tree-sitter** grammar + highlight queries (editors / Linguist) |
| [`dotnet/`](dotnet/) | **C#** (`Synx.Core` project, NuGet **`APERTURESyndicate.Synx`**) — parity with `synx-core` parse/JSON/`!active`; **`Synx.FuzzReplay`** for corpus replay; see [`dotnet/README.md`](dotnet/README.md) |
| [`../bindings/cpp/`](../bindings/cpp/) | **C++** — `synx/synx.hpp` (C++17) over **`synx-c`** FFI: **same** grammar, markers, `!tool`, `.synxb`, canonical JSON as Rust |
| [`../bindings/go/`](../bindings/go/) | **Go** — cgo over **`synx-c`**: same engine; `Parse` / `ParseActive` / … return JSON strings or `.synxb` bytes |
| [`../bindings/mojo/`](../bindings/mojo/) | **Mojo** — `Python.import_module("synx_native")` wrappers: same engine as Rust (not a standalone Mojo lexer/parser port) |
| [`../bindings/swift/`](../bindings/swift/) | **Swift** — SwiftPM **`SynxEngine`** over **`synx-c`** (`String` / `Data` API) |
| [`../bindings/kotlin/`](../bindings/kotlin/) | **Kotlin/JVM** — **`SynxEngine`** via **JNA** + **`synx-c`** (`String` / `ByteArray` API) |

`crates/synx-lsp` is **not** a parser — it is a language server; see [`integrations/README.md`](../integrations/README.md).

## Future layout (optional cleanup)

When convenient (e.g. after closing IDE / clearing fuzz locks), Rust crates can be moved to `parsers/rust/` and JS to `parsers/javascript/` without losing history — use `git mv`. Until then, paths above stay canonical for tooling and CI.
