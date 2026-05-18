# SYNX — native Swift parser

`parsers/swift` is the **native** SYNX parser, written in pure Swift 5.9 — no
FFI to the Rust `synx-core` crate, no `synx-c` cdylib at runtime. It mirrors
the canonical Rust engine 1:1 (parse / `!active` / `.synxb` / canonical JSON /
diff / format / `!tool`).

Targets:

* **iOS / macOS / tvOS / watchOS** — works out of the box with the
  `Compression` framework (raw DEFLATE for `.synxb`).
* **Linux / Windows Swift toolchains** — `.synxb` compile / decompile return
  `.unsupportedPlatform`; the rest of the API works (parse / engine / format
  / JSON / diff). Add a zlib bridge target to enable the binary format.

| Property | Value |
|---|---|
| Swift tools | 5.9+ |
| Concurrency | `Sendable`-safe public types |
| External deps | None (Foundation + Compression on Apple) |
| Build | `swift build` / Xcode SPM dependency |
| Tests | `swift test` (XCTest) |
| Wire-compat with Rust | yes (canonical JSON + `.synxb` raw DEFLATE) |

## Quick start (SPM)

In your `Package.swift`:

```swift
dependencies: [
    .package(name: "Synx", path: "../parsers/swift"),
],
targets: [
    .target(name: "App", dependencies: ["Synx"]),
]
```

## API at a glance

```swift
import Synx

// Static parse
let root = Synx.parse("name App\nport 8080\n")

// Active engine with env vars
var opts = SynxOptions()
opts.env = ["PORT": "9090"]
let active = Synx.parseActive("!active\nport:env:default:3000 PORT\n", options: opts)

// Canonical JSON
let j = Synx.toJSON(active)

// Binary
switch Synx.compile("name App\n") {
case .success(let bytes):
    if case .success(let text) = Synx.decompile(bytes) {
        print(text)
    }
case .failure(let e):
    print("compile error: \(e)")
}
```

### Custom markers

```swift
var opts = SynxOptions()
opts.markerFns["upper"] = { key, args, value in
    if case .string(let s) = value { return .string(s.uppercased()) }
    return value
}
let r = Synx.parseActive("!active\nslug:upper hello", options: opts)
```

## Layout

```
parsers/swift/
├── Package.swift
├── Sources/Synx/
│   ├── Synx.swift           # top-level facade
│   ├── SynxValue.swift      # enum, SynxObject, SynxMeta, SynxConstraints, SynxParseResult
│   ├── SynxOptions.swift    # options + MarkerFn closure type
│   ├── SynxParser.swift     # text → tree
│   ├── SynxEngine.swift     # !active resolver + Resolver class
│   ├── SynxMarkers.swift    # all 27 markers as pure functions
│   ├── SynxCalc.swift       # safe arithmetic
│   ├── SynxJSON.swift       # canonical JSON
│   ├── SynxStringify.swift  # value → SYNX text
│   ├── SynxFormatter.swift  # canonical reformat
│   ├── SynxDiff.swift       # structural diff
│   └── SynxBinary.swift     # .synxb compile / decompile
└── Tests/SynxTests/         # XCTest suites + conformance corpus runner
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

* **WASM custom markers** — replaced by `SynxOptions.markerFns` (`(key, args, value) -> SynxValue`).
  Builtin markers always win over custom ones with the same name.
* **`:signing`** — Ed25519 signature verification module not ported (same as
  C# / GDScript / C++ ports).
* `:vision` / `:audio` are passthrough envelopes; consumers attach actual
  media bytes outside the parser.

## Wire compatibility

* **JSON**: byte-for-byte identical to Rust ryu/itoa output for typical inputs.
  Floats use `%.17g` with a mandatory `.0` suffix so round-trip preserves the
  Float vs Int distinction.
* **`.synxb`**: identical magic / version / flags / string table / DEFLATE
  payload as `crates/synx-core`. Files written here decompile under Rust /
  C# / GDScript / C++ and vice-versa.
* **Canonical text format**: same sort key and indentation rules as the
  Rust `fmt_canonical`.
