# SYNX Kotlin/JVM (`synx-kotlin`)

**Version:** 3.6.0 — **`SynxEngine`** wraps **`synx-c`** via [**JNA**](https://github.com/java-native-access/jna) (same Rust **`synx-core`** engine as Swift/Go). Not a pure-Kotlin grammar implementation.

## Prerequisites

- **JDK 17+** (for `gradlew`; Kotlin compiles against toolchain **17** in `build.gradle.kts`).
- Built **`synx_c`** shared library from the repo root:

```bash
cargo build -p synx-c --release
```

Artifacts: `target/release/libsynx_c.so`, `libsynx_c.dylib`, or `synx_c.dll`.

## Build & test

From **`bindings/kotlin`**:

```bash
export SYNX_LIB_DIR=/absolute/path/to/synx-format/target/release   # Unix
# PowerShell: $env:SYNX_LIB_DIR = "A:\path\to\synx-format\target\release"

./gradlew test
```

On Windows use **`gradlew.bat`** if **`./gradlew`** is not available.

`SYNX_LIB_DIR` is passed to the JVM for **`jna.library.path`** before `libsynx_c` loads. If unset, Gradle sets it to **`../../target/release`** (tests only).

## Dependency

**Maven Central:** not published from this monorepo yet. Install locally:

```bash
./gradlew publishToMavenLocal
```

Then:

```kotlin
repositories { mavenLocal() }
dependencies {
    implementation("com.aperturesyndicate:synx-kotlin:3.6.0")
    implementation("net.java.dev.jna:jna:5.15.0")
}
```

Or add the **`jar`** from `build/libs/` plus **`jna-5.15.0.jar`** to your classpath (runtime still needs **`synx_c`** on `jna.library.path`).

## API

`SynxEngine` mirrors [`bindings/c-header/include/synx.h`](../../c-header/include/synx.h):

| Kotlin | C |
|--------|---|
| `parse` | `synx_parse` |
| `parseActive` | `synx_parse_active` |
| `stringify` | `synx_stringify` |
| `format` | `synx_format` |
| `parseTool` | `synx_parse_tool` |
| `diff` | `synx_diff` |
| `compile` | `synx_compile` |
| `decompile` | `synx_decompile` |
| `isSynxb` | `synx_is_synxb` |

Returns are **`String`** or **`ByteArray`**; failures throw **`SynxEngineError`**.

## Android

Ship **`libsynx_c.so`** per ABI (or static `.a`) and set **`jna.library.path`** / unpack natives as your app does for other JNA `.so` assets. This folder does not automate NDK packaging.

## Swift

See [`../swift/README.md`](../swift/README.md) for the same engine via SwiftPM.
