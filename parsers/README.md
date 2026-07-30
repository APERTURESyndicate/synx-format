> Main SYNX site: https://synx.aperturesyndicate.com/

# Parsers and grammars

Native parser implementations of the SYNX format — each is a from-scratch
port of `synx-core`, NOT an FFI wrapper. One source of truth per language,
no `.dll` / `.so` dependency to ship.

| Path | Language | Role |
|------|----------|------|
| [`../crates/synx-core/`](../crates/synx-core/) | **Rust** | Canonical parser, engine (`!active`), stringify, `diff`, `.synxb`, **SYNXL** (`src/synxl.rs`), fuzz targets |
| [`../crates/synx-cli/`](../crates/synx-cli/)   | **Rust** | CLI (`synx`) — uses `synx-core`; **SYNXL** via `synx synxl parse\|validate\|convert\|split` |
| [`../packages/synx-js/`](../packages/synx-js/) | **TypeScript** | Reference parser + npm `@aperturesyndicate/synx-format` (no native deps); **SYNXL** (`src/synxl.ts`) |
| [`cpp/`](cpp/)         | **C++17** | Header + sources, CMake project; conformance + unit tests |
| [`dart/`](dart/)       | **Dart 3** | Pure Dart, pub.dev `synx`; full engine + binary |
| [`dotnet/`](dotnet/)   | **C# / .NET 8** | NuGet `APERTURESyndicate.Synx` + `Synx.FuzzReplay` for corpus replay |
| [`go/`](go/)           | **Go**     | Pure Go module (no cgo), `go test ./...` |
| [`java/`](java/)       | **Java 17** | Maven `com.aperturesyndicate:synx` |
| [`swift/`](swift/)     | **Swift 5** | SwiftPM `Synx`, no FFI |
| [`../integrations/godot/synx-gdscript/`](../integrations/godot/synx-gdscript/) | **GDScript / Godot 4** | Pure-GDScript engine packaged as a Godot editor addon (`addons/synx`); lives under `integrations/` because it ships as a Godot plugin, but is a from-scratch parser on par with the entries above |

`../tree-sitter-synx/` holds the editor/Linguist grammar (separate from the
runtime parsers above). `../crates/synx-lsp` is the language server, not a
parser — see [`../integrations/README.md`](../integrations/README.md).

## SYNXL (`.synxl`) support

Every parser listed above reads **SYNX 3.7**, including the `|+`
indent-preserving multiline opener.

**SYNXL** — the record-stream format specified in
[`../docs/spec/SYNXL-1-NORMATIVE.md`](../docs/spec/SYNXL-1-NORMATIVE.md) — is a
*separate* format on its own version axis, and is **not** implemented by the
native parsers yet. As of 3.7.0 it exists in:

| Implementation | SYNXL entry point |
|---|---|
| Rust `synx-core` | `synx_core::synxl::{parse_lines, SynxlReader, SynxlReaderOwned, SynxlStreamReader, write_lines}` |
| Rust CLI `synx` | `synx synxl parse` · `validate` · `convert` · `split` |
| TypeScript `packages/synx-js` | `Synx.parseSynxl` · `streamSynxl` · `streamSynxlFile` · `writeSynxl` · `synxlToJSON` / `synxlToNDJSON` |
| Python `bindings/python` | `synx_native.synxl_parse` · `synxl_load` · `synxl_stream` · `synxl_stream_file` · `synxl_write` |

`cpp/`, `dart/`, `dotnet/`, `go/`, `java/`, `swift/` and the Godot/GDScript
engine do **not** read `.synxl`. Adding it means implementing the whole of
SYNXL-1 and passing [`../tests/conformance-synxl/cases/`](../tests/conformance-synxl/cases/),
which is the practical conformance contract; block-field parsing delegates to
the parser's existing SYNX 3.7 implementation rather than a second tree builder.

The `../bindings/` tree holds the **non-native** language surfaces that
still need to call into the canonical engine:

| Path | Language | Why FFI |
|------|----------|---------|
| `../bindings/c-header/` | **C ABI** (`synx.h`) | Reference C surface for downstream FFI |
| `../bindings/node/`     | **Node.js** | N-API for raw-speed; pure JS lives in `packages/synx-js` |
| `../bindings/python/`   | **Python**  | PyO3 binding — no native Python parser yet |
| `../bindings/kotlin/`   | **Kotlin/JVM** | JNA over `synx-c` — JVM users on Java can use `parsers/java` directly |
| `../bindings/mojo/`     | **Mojo**    | Wraps the Python build; experimental |
| `../bindings/wasm/`     | **WebAssembly** | Browser/edge target of the Rust engine |
