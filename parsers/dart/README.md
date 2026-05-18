# SYNX — native Dart parser

`parsers/dart` is the **native** SYNX parser for Dart and Flutter, written in
pure Dart 3. No FFI, no platform-specific code. Mirrors the canonical Rust
engine 1:1 (parse / `!active` / `.synxb` / canonical JSON / diff / format /
`!tool`).

Targets:

* **Flutter** mobile / desktop / web — drop-in dependency.
* **Dart server / CLI** — `dart:io` available, all features work.
* **Web** — every feature except `.synxb` compile/decompile (which uses
  `dart:io` `ZLibCodec`). Use `synx.compile/decompile` on the server side and
  ship pre-compiled bytes to the browser.

| Property | Value |
|---|---|
| Dart SDK | 3.0+ (sealed classes, records, pattern matching) |
| External deps | None (only `dart:io`, `dart:convert`, `dart:typed_data`) |
| Build | `dart pub get` + `dart analyze` + `dart test` |
| Tests | `package:test` |
| Wire-compat with Rust | yes (canonical JSON + `.synxb` raw DEFLATE) |

## Quick start

Add to `pubspec.yaml`:

```yaml
dependencies:
  synx:
    path: ../parsers/dart   # or via git ref
```

Then:

```dart
import 'package:synx/synx.dart';

void main() {
  // Static parse
  final root = parse('name App\nport 8080\n');
  print(toJson(synxObject(root)));

  // Active engine with env vars
  final opts = SynxOptions()..env = {'PORT': '9090'};
  final active = parseActive('!active\nport:env:default:3000 PORT\n', opts);
  print(toJson(synxObject(active)));

  // Binary
  final compiled = compile('name App\n');
  if (compiled.ok) {
    final text = decompileToText(compiled.bytes!);
    print(text);
  }
}
```

### Custom markers

```dart
final opts = SynxOptions()
  ..markerFns['upper'] = (key, args, value) {
    if (value is SynxStr) return synxString(value.value.toUpperCase());
    return value;
  };
final r = parseActive('!active\nslug:upper hello', opts);
```

## Layout

```
parsers/dart/
├── pubspec.yaml
├── lib/
│   ├── synx.dart                 # public API
│   └── src/
│       ├── value.dart            # sealed class SynxValue + 8 subclasses
│       ├── meta.dart             # Meta, Constraints, ParseResult, directives
│       ├── options.dart          # Options, SynxMarkerFn
│       ├── parser.dart           # text → tree
│       ├── calc.dart             # safeCalc
│       ├── json.dart             # toJson canonical
│       ├── stringify.dart        # stringify
│       ├── formatter.dart        # format canonical
│       ├── diff.dart             # diff + diffToValue
│       ├── binary.dart           # .synxb compile/decompile (ZLibCodec raw)
│       └── engine.dart           # resolve() + all 27 markers
└── test/
    ├── value_test.dart
    ├── parser_test.dart
    ├── calc_test.dart
    ├── json_test.dart
    ├── stringify_test.dart
    ├── diff_test.dart
    ├── binary_test.dart
    ├── engine_test.dart
    └── conformance_test.dart
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

* **WASM custom markers** — replaced by `SynxOptions.markerFns` (Dart closures).
  Builtin markers always win over a custom marker with the same name.
* **`:signing`** — Ed25519 signature verification not ported.
* `:vision` / `:audio` are passthrough envelopes; consumers attach media
  bytes outside the parser.
* **Web `.synxb`** — `ZLibCodec` lives in `dart:io`, not in `dart:html`. On
  the web, parse / engine / JSON / format / diff all work; compile/decompile
  raise a missing-library error at runtime.

## Wire compatibility

* **JSON**: byte-for-byte identical to Rust ryu/itoa output for typical inputs.
  Floats use Dart's `double.toString()` (shortest round-trip) plus a mandatory
  `.0` suffix so JSON parsers see a Float, not an Int.
* **`.synxb`**: identical magic / version / flags / string table / DEFLATE
  payload as `crates/synx-core`. Compression goes through
  `ZLibCodec(raw: true, level: 9)` — raw DEFLATE — matching Rust
  `miniz_oxide::deflate::compress_to_vec(_, 9)` / Go `compress/flate level 9` /
  Java `Deflater(9, nowrap=true)`.
* **Canonical text format**: same sort key and indentation rules as Rust
  `fmt_canonical`.
