> Main SYNX site: https://synx.aperturesyndicate.com/

# SYNX conformance test suite

A set of `.synx` input files and their expected JSON outputs. Any compliant SYNX parser must produce byte-identical JSON for every case.

## Directory layout

```
tests/conformance/
├── README.md
└── cases/
    ├── 001-scalar-types.synx
    ├── 001-scalar-types.expected.json
    ├── 002-nested-objects.synx
    ├── 002-nested-objects.expected.json
    ├── ...
    └── 010-tool-mode.synx / .expected.json
```

## Comparison rules

1. Parse the `.synx` file. If it starts with `!tool`, use `parse_tool` instead of `parse`.
   If the parser reports the document as `!active` (the `100`–`199` cases), resolve it with
   default options before serialising; those cases never depend on the environment.
2. Serialize the result to JSON with **keys sorted alphabetically** at every nesting level.
3. Compare the JSON string byte-for-byte against the `.expected.json` file (trailing newline stripped).

Numbers: `42` is an integer, `3.14` is a float. `true`/`false`/`null` are their JSON equivalents.

## Running (Rust)

```bash
cargo test -p synx-core --test conformance
```

## Running (every implementation)

Each runner reads this directory and compares byte for byte; CI runs all of them
(`bindings-smoke` → `core-tests`, `synxl-cross`, `native-parsers`).

| Implementation | Command | Runner |
|---|---|---|
| Rust | `cargo test -p synx-core --test conformance` | `crates/synx-core/tests/conformance.rs` |
| TypeScript | `npm --prefix packages/synx-js test` | `packages/synx-js/tests/conformance.test.ts` |
| Go | `go test ./...` in `parsers/go` | `conformance_test.go` |
| .NET | `dotnet test` in `parsers/dotnet` | `tests/Synx.Core.Tests/ConformanceTests.cs` |
| Java | `mvn test` in `parsers/java` | `src/test/.../ConformanceTest.java` |
| Dart | `dart test` in `parsers/dart` | `test/conformance_test.dart` |
| Swift | `swift test` in `parsers/swift` | `Tests/SynxTests/ConformanceTests.swift` |
| C++ | `ctest` after `cmake -DSYNX_BUILD_TESTS=ON` in `parsers/cpp` | `tests/test_conformance.cpp` |

A runner that only checks that a case *parses* proves nothing: until 3.7.4 five
of the native runners did exactly that, and the parser fixes of 3.7.2/3.7.3
never reached those implementations.

## Running (CLI)

```bash
for f in tests/conformance/cases/*.synx; do
  expected="${f%.synx}.expected.json"
  [ -f "$expected" ] || continue
  got=$(synx parse "$f")
  want=$(cat "$expected" | tr -d '\n')
  if [ "$got" = "$want" ]; then
    echo "PASS $(basename $f)"
  else
    echo "FAIL $(basename $f)"
    echo "  got:  $got"
    echo "  want: $want"
  fi
done
```

## Adding cases

1. Create `NNN-descriptive-name.synx` in `cases/`.
2. Create the matching `.expected.json` with sorted keys, no trailing whitespace, and a single trailing newline.
3. Run `cargo test -p synx-core --test conformance` to verify.

The numbering convention: `001`–`099` basic syntax, `100`–`199` active mode, `200`+ edge cases.
