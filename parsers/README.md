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

## Development status

Active development — new releases, fixes, fuzzing and differential testing —
currently goes into the implementations that carry real demand:

- **Rust `synx-core`** and everything built on it: the `synx` CLI and the
  Python, Node and WASM bindings;
- **TypeScript `packages/synx-js`** (npm `@aperturesyndicate/synx-format`).

The native ports (C++, Dart, .NET, Go, Java, Swift, Godot/GDScript) were
brought to SYNX 3.8.0 together with the core: each passes the conformance
suite byte for byte and was checked against synx-core on thousands of
generated documents (parse and resolve, `format`, `stringify`, `.synxb`).
None of them reads or writes SYNXL yet — that is Rust, TypeScript and the
bindings built on synx-core. Their demand is an order of magnitude lower, so
new work still lands in the two implementations above first; for a new
project, prefer Rust or TypeScript.

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

Passing the fixtures is necessary but not sufficient — a full audit of the four
implementations above found 33 defects while every suite was green. A new
implementation should also satisfy the four obligations in
[the suite's README](../tests/conformance-synxl/README.md#what-a-runner-must-check),
and the two checks that live outside the fixtures:

| Check | Where | What it catches |
|---|---|---|
| §14.1 round-trip on every accepted case | in the conformance runner itself | a writer that emits values it cannot read back |
| §15.3 shardability on every accepted case | [`../crates/synx-cli/src/synxl.rs`](../crates/synx-cli/src/synxl.rs) tests | a splitter that loses the field list in effect at the split point |
| randomised invariants and known-hostile inputs | [`../crates/synx-core/tests/property_synxl.rs`](../crates/synx-core/tests/property_synxl.rs) | panics, superlinear blow-ups, reader disagreement |
| deep fuzzing | [`../crates/synx-core/fuzz/fuzz_targets/fuzz_synxl.rs`](../crates/synx-core/fuzz/fuzz_targets/fuzz_synxl.rs) | the same, far deeper (needs nightly) |
| cross-implementation differential | [`../scripts/ci/diff-implementations.mjs`](../scripts/ci/diff-implementations.mjs) | §1.4.3 divergence on inputs no fixture covers |

The last one is the important one for a *new* parser: §1.4.3 measures
conformance on byte-identical projections against the reference
implementation, and two suites can both be green while the implementations
disagree on everything the fixtures happen not to cover.

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
