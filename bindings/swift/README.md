# SYNX Swift package (`Synx`)

**Version:** 3.6.0 — thin Swift wrapper over the **`synx-c`** C library (Rust **`synx-core`**). Same grammar, `!active`, `!tool`, `.synxb`, and canonical JSON.

## Build `synx_c` (repo root)

```bash
cargo build -p synx-c --release
```

Artifacts:

- macOS: `target/release/libsynx_c.dylib`
- Linux: `target/release/libsynx_c.so`

The Swift module name for the C side is **`CSynx`** (`Sources/CSynx/module.modulemap` links `-lsynx_c`).

## Build this package

From `bindings/swift`, point the linker at the directory that contains `libsynx_c.*`:

```bash
export SYNX_LIB_DIR=/absolute/path/to/synx-format/target/release
swift build \
  -Xlinker -L -Xlinker "$SYNX_LIB_DIR" \
  -Xlinker -lsynx_c
swift test \
  -Xlinker -L -Xlinker "$SYNX_LIB_DIR" \
  -Xlinker -lsynx_c
```

On **macOS**, if the loader cannot find the dylib at runtime, either:

- set `DYLD_LIBRARY_PATH` to include `$SYNX_LIB_DIR`, **or**
- copy / symlink `libsynx_c.dylib` next to your built product (app / tests).

**iOS / embedded:** ship an **XCFramework** (or static `libsynx_c.a`) and adjust `module.modulemap` / `Package.swift` — not automated in this folder yet.

## C header copy

`Sources/CSynx/synx.h` is a **mirror** of [`../c-header/include/synx.h`](../c-header/include/synx.h). When the C API changes, update both (see root `EDITING.md`).

## API

`SynxEngine` static methods map 1:1 to `synx.h`:

| Swift | C |
|-------|---|
| `parse` | `synx_parse` |
| `parseActive` | `synx_parse_active` |
| `stringify(json:)` | `synx_stringify` |
| `format` | `synx_format` |
| `parseTool` | `synx_parse_tool` |
| `diff` | `synx_diff` |
| `compile` | `synx_compile` |
| `decompile` | `synx_decompile` |
| `isSynxb` | `synx_is_synxb` |

Errors: `SynxEngineError` when the FFI returns `NULL`.

## Kotlin / JVM

**`bindings/kotlin`** provides **`SynxEngine`** over **`synx-c`** via **JNA** — same API surface as this Swift package (JSON `String` / `.synxb` `ByteArray`). See [`../kotlin/README.md`](../kotlin/README.md).
