# Synx.FuzzReplay

Feeds files through **`SynxFormat.Parse`** and **`SynxFormat.ToJson`** for parity with Rust **`fuzz_parse`** on UTF-8 inputs.

- Skips files that are not well-formed UTF-8 (Rust harness ignores those too).
- Exit code **1** on thrown parse/emit errors.

```bash
dotnet run -c Release --project tools/Synx.FuzzReplay -- file1 file2
dotnet run -c Release --project tools/Synx.FuzzReplay -- --bench ../../../../../tests/conformance/cases/*.synx
```

Run from repo root or `parsers/dotnet` (adjust paths). See [`crates/synx-core/fuzz/`](../../../../../crates/synx-core/fuzz/README.md).
