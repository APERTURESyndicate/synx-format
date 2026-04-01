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
2. Serialize the result to JSON with **keys sorted alphabetically** at every nesting level.
3. Compare the JSON string byte-for-byte against the `.expected.json` file (trailing newline stripped).

Numbers: `42` is an integer, `3.14` is a float. `true`/`false`/`null` are their JSON equivalents.

## Running (Rust)

```bash
cargo test -p synx-core --test conformance
```

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
