# SYNX — native C++ parser

`parsers/cpp` is the **native** SYNX parser written in C++17 — no FFI to the
Rust `synx-core` crate, no `synx-c` cdylib at runtime. It mirrors the
canonical Rust engine 1:1 (parse / `!active` / `.synxb` / canonical JSON /
diff / format / `!tool`).

Targets:

* **Unreal Engine 5** plugins (no RTTI, no exceptions — see `examples/ue5/`).
* Standalone C++ tools and servers that need a header + static-lib drop-in.
* Game engines and embedded runtimes that ban third-party dynamic libraries.

| Property | Value |
|---|---|
| Standard | C++17 |
| Exceptions / RTTI | Off by default (`SYNX_NO_EXCEPTIONS=ON`) |
| Public deps | zlib (raw DEFLATE for `.synxb`) — optional |
| Header tree | `include/synx/*.hpp` |
| Implementation | `src/*.cpp` |
| Tests | `tests/` (zero-dep micro-runner) |
| Wire-compat with Rust | yes (canonical JSON + `.synxb`) |

## Quick start

```bash
cmake -S parsers/cpp -B parsers/cpp/build -DSYNX_BUILD_TESTS=ON
cmake --build parsers/cpp/build
ctest --test-dir parsers/cpp/build
```

CMake options:

| Option | Default | Effect |
|---|---|---|
| `SYNX_BUILD_TESTS` | `OFF` | Build & register unit + conformance tests. |
| `SYNX_USE_ZLIB` | `AUTO` | `AUTO` finds zlib if available; `OFF` disables `.synxb`. |
| `SYNX_NO_EXCEPTIONS` | `ON` | Compile with `-fno-exceptions -fno-rtti` (UE5-compatible). |

## API at a glance

```cpp
#include "synx/synx.hpp"

// Static parse
synx::Object root = synx::Synx::parse_root("name App\nport 8080\n");

// Active engine with env vars
synx::Options opts;
opts.env.emplace();
opts.env->emplace("PORT", "9090");
synx::Object active = synx::Synx::parse_active_root(
    "!active\nport:env:default:3000 PORT\n", opts);

// Canonical JSON
std::string j = synx::Synx::to_json(synx::Value::make_object(std::move(root)));

// Binary
auto bytes = synx::Synx::compile("name App\n", false);
if (bytes.ok()) {
    auto restored = synx::Synx::decompile(bytes.value());
}
```

### Custom markers (no WASM)

```cpp
synx::Options opts;
opts.marker_fns["upper"] = [](const std::string& key,
                              const std::vector<std::string>& args,
                              const synx::Value& v) {
    std::string s = synx::detail::value_to_string(v); // internal helper
    for (auto& c : s) c = std::toupper(static_cast<unsigned char>(c));
    return synx::Value::make_string(std::move(s));
};
auto r = synx::Synx::parse_active_root(
    "!active\nslug:upper hello", opts);
```

## Layout

```
parsers/cpp/
├── CMakeLists.txt
├── include/synx/
│   ├── synx.hpp           # top-level facade
│   ├── value.hpp          # Value, Object, Array, Pair
│   ├── meta.hpp           # Meta, Constraints
│   ├── options.hpp        # Options, IncludeDirective, UseDirective
│   ├── parse_result.hpp   # ParseResult
│   ├── result.hpp         # Result<T> / Error
│   ├── parser.hpp         # parse(), reshape_tool_output()
│   ├── engine.hpp         # resolve()
│   ├── calc.hpp           # safe_calc()
│   ├── json.hpp           # to_json()
│   ├── stringify.hpp      # stringify()
│   ├── formatter.hpp      # format()
│   ├── diff.hpp           # diff()
│   └── binary.hpp         # compile(), decompile()
├── src/
│   ├── value.cpp parser.cpp engine.cpp engine_markers.cpp calc.cpp
│   ├── json.cpp stringify.cpp formatter.cpp diff.cpp binary.cpp synx.cpp
│   └── engine_internal.hpp  (private)
├── tests/
│   ├── CMakeLists.txt
│   ├── test_helpers.hpp test_main.cpp
│   └── test_*.cpp
└── examples/
    └── ue5/                # drop-in UE5 plugin template
```

## Limits (parity with Rust)

| Cap | Value |
|---|---|
| Input bytes | 16 MiB |
| Indexed line starts | 2 million |
| Parse nesting depth | 128 |
| Multiline block | 1 MiB |
| List items per list | 1,048,576 |
| `!include` directives per file | 4096 |
| Marker chain segments | 512 |
| Engine resolve depth | 512 |
| Serialize / JSON depth | 128 |
| Constraint enum parts | 4096 |

## What's unsupported (versus Rust core)

* **WASM custom markers** — replaced by `Options::marker_fns` (C++ `std::function`).
* **`:signing`** — Ed25519 signature verification module not yet ported.
* `:vision` / `:audio` are passthrough envelopes; consumers attach actual
  media bytes outside the parser. This matches the Rust core's surface.

## Wire compatibility

* **JSON**: byte-for-byte identical to Rust ryu/itoa output for typical inputs.
  Floats round-tripped through `%.17g` with mandatory `.0` suffix.
* **`.synxb`**: identical magic / version / flags / string table / DEFLATE
  payload as `crates/synx-core`. Files written here decompile under Rust /
  C# / GDScript and vice-versa.
* **Canonical text format**: same sort key and indentation rules as the
  Rust `fmt_canonical`.
