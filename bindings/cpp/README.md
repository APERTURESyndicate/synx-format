# SYNX C++ SDK (`synx-cpp`)

**Version:** 3.6.0 (format + engine parity with `synx-core`)

This is a **thin C++17 header wrapper** around the [`synx-c`](../c-header/) C ABI. The native library is the **same Rust implementation** as the rest of the monorepo: identical grammar, `!active` markers, `!tool` / `!schema` reshape, `.synxb` compile/decompile, and canonical JSON rules.

## What you get

- Header: `include/synx/synx.hpp`
- Depends on: `bindings/c-header/include/synx.h` + a built **`synx_c`** shared or static library

## Build the native library

From the repository root:

```bash
cargo build -p synx-c --release
```

Artifacts (typical names):

| Platform | Shared library |
|----------|----------------|
| Linux | `target/release/libsynx_c.so` |
| macOS | `target/release/libsynx_c.dylib` |
| Windows | `target/release/synx_c.dll` (+ import `.lib` for MSVC link) |

## Use from CMake

```bash
cd bindings/cpp
cmake -B build -DSYNX_C_LIBRARY=/absolute/path/to/libsynx_c.so
cmake --build build
```

Set `SYNX_C_LIBRARY` to the **full path** of the `synx_c` library built above (on Windows, often the `.lib` next to the DLL when using MSVC).

## Include paths

- `bindings/cpp/include` — for `#include <synx/synx.hpp>`
- `bindings/c-header/include` — for `synx.h` (pulled in by `synx.hpp`)

## API surface

Free functions in namespace `synx`, all mirroring `synx.h`:

| C++ | C |
|-----|---|
| `parse` | `synx_parse` |
| `parse_active` | `synx_parse_active` |
| `stringify` | `synx_stringify` |
| `format` | `synx_format` |
| `parse_tool` | `synx_parse_tool` |
| `compile` | `synx_compile` |
| `decompile` | `synx_decompile` |
| `is_synxb` | `synx_is_synxb` |
| `diff` | `synx_diff` |

String operations return `std::optional<std::string>`; `nullopt` means the engine rejected input (same as `NULL` from C). `compile` returns `std::optional<std::vector<unsigned char>>`.

## Example

```cpp
#include <synx/synx.hpp>
#include <iostream>

int main() {
    auto j = synx::parse_active("!active\nport:env:default:8080 PORT\n");
    if (!j) return 1;
    std::cout << *j << '\n';
    return 0;
}
```

## Conformance

Behavior is defined by `tests/conformance/` and `synx-core` 3.6.x — this binding adds **no alternate parser**.
