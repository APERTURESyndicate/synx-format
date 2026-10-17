# SYNX — native Swift parser

`parsers/swift` is the **native** SYNX parser, written in pure Swift 5.9 — no
FFI to the Rust `synx-core` crate, no `synx-c` cdylib at runtime. It mirrors
the canonical Rust engine 1:1 (parse / `!active` / `.synxb` / canonical JSON /
diff / format / `!tool`) at SYNX **3.8.0** (`Synx.version`): objects keep
source order and `!active` documents resolve in one pass in document order.
Keys and strings compare as UTF-8 bytes, as in Rust — not by Swift's canonical
equivalence (`"é" == "e\u{301}"` is true for `String`, but they are two keys).

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
│   ├── SynxParser.swift     # text → tree (over UTF-8 bytes)
│   ├── SynxTree.swift       # mutable working tree for the parser and engine
│   ├── SynxEngine.swift     # !active resolution, pre-passes, files
│   ├── SynxMarkers.swift    # the markers, in synx-core's order
│   ├── SynxValidate.swift   # [constraints], [pattern], JSON for :watch
│   ├── SynxText.swift       # Unicode whitespace / case, key index
│   ├── SynxNumber.swift     # numbers as Rust parses and prints them
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

* **WASM custom markers** — replaced by `SynxOptions.markerFns` (`(key, args, value) -> SynxValue`),
  run where synx-core runs WASM markers: after the built-ins, the first registered
  non-built-in marker of a chain, with the parts after it as `args`.
* **SYNXL** (`.synxl` datasets) — not ported.
* **`[pattern]`** runs on `NSRegularExpression` (ICU). Patterns synx-core's `regex`
  refuses (look-around, back-references, atomic groups, possessive quantifiers)
  and nested quantifiers (`(a+)+`) are skipped, as there.
* **`:signing`** — Ed25519 signature verification module not ported (same as
  C# / GDScript / C++ ports).
* `:vision` / `:audio` are passthrough envelopes; consumers attach actual
  media bytes outside the parser.

## Wire compatibility

* **JSON**: byte-for-byte identical to Rust (`ryu` float layout, shortest
  round-trip digits).
* **`.synxb`**: magic / version / flags / string table (ordered by use count) /
  payload are byte-identical to `crates/synx-core`; the DEFLATE stream comes
  from the `Compression` framework, so compressed bytes may differ while both
  read each other's files.
* **Canonical text format** and **stringify**: byte-identical to Rust
  `fmt_canonical` and `serialize` (the SYNXL §14.1 writer).
