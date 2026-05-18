# SYNX UE5 plugin template

Drop-in Unreal Engine 5 plugin that exposes the native SYNX parser to C++ and
Blueprint, with full parity to Rust `synx-core` 3.6.x. **No third-party Rust
toolchain or cdylib required.**

## Layout

```
<Project>/Plugins/SynxPlugin/
├── SynxPlugin.uplugin
└── Source/
    ├── Synx/                ← this module
    │   ├── Synx.Build.cs
    │   ├── Private/
    │   │   ├── Synx.cpp
    │   │   ├── SynxBlueprintLibrary.cpp
    │   │   └── SynxCoreUnity.cpp
    │   └── Public/
    │       └── SynxBlueprintLibrary.h
    └── SynxCore/            ← `parsers/cpp` checkout (sibling)
        ├── include/synx/*.hpp
        └── src/*.cpp
```

`SynxCore/` is just the standalone `parsers/cpp/` tree, dropped or symlinked
into your plugin so UE5's build system sees one filesystem unit.

## Build settings

* C++17
* `bUseRTTI = false`
* `bEnableExceptions = false`
* `SYNX_NO_EXCEPTIONS=1`, `SYNX_HAVE_ZLIB=1` defines (set by `Synx.Build.cs`)
* Depends on `Core`, `CoreUObject`, `Engine`, `zlib`, `Json`, `JsonUtilities`

## Blueprint usage

```cpp
FString JsonOutput = USynxBlueprintLibrary::SynxParseToJson(MySynxText);

TArray<FString> Keys   = { "PORT" };
TArray<FString> Values = { "9090" };
FString Resolved = USynxBlueprintLibrary::SynxParseActiveToJson(
    TEXT("!active\nport:env:default:3000 PORT"), Keys, Values);
```

## C++ usage

```cpp
#include "synx/synx.hpp"

synx::ParseResult r = synx::parse("name App\nport 8080\n");
FString Json = FString(UTF8_TO_TCHAR(synx::to_json(r.root).c_str()));
```

## Why native C++?

* **No FFI**: no `synx-c` cdylib to ship, no Rust toolchain on the dev machine,
  no MSVC/MinGW symbol mismatch headaches.
* **UE5 friendly**: compiles with the same flags Epic uses (`/EHs-c- /GR-`).
* **Static linkable** into a single editor/build pipeline.
* **Wire-compatible** `.synxb`: bytes produced by this module decompile under
  Rust/C#/GDScript and vice-versa.

## What's not yet wired up

* WASM custom markers (Rust core supports `!use @scope/markers` with `.wasm`).
  In UE5 use `synx::Options::marker_fns` to register C++ `std::function`s
  instead — that path *is* implemented.
* `:signing` (Ed25519 signature verification).
