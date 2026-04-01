# Fuzzing synx-core

Three fuzz targets exercise the parser, binary codec, and formatter with arbitrary inputs.

## Prerequisites

```bash
cargo install cargo-fuzz
```

Requires nightly Rust (`cargo +nightly`).

## Parser / engine bounds (hostile input)

`synx-core` caps work and memory from untrusted text (see `parser.rs` / `lib.rs` / `engine.rs`):

- **Input size:** UTF-8 prefix **16 MiB** (`clamp_synx_text`) for `parse` and canonical `format`.
- **Lines:** at most **`MAX_LINE_STARTS`** (2 000 000) lines indexed — longer inputs are truncated at a safe newline boundary.
- **Indent nesting:** **128** stack frames (was 2048) for nested object keys.
- **Multiline `|` blocks:** **1 MiB** per block body.
- **Lists:** **1 048 576** items per list.
- **`!include`:** **4096** directives per file.
- **Stringify / JSON / formatter:** recursion depth **128** (`serialize`, `write_json`, `fmt_parse`).
- **Engine scratch strings:** **4 MiB** for `replace_word` / template interpolation (`MAX_ENGINE_SCRATCH_STRING`).

## Targets

| Target | What it exercises |
|--------|-------------------|
| `fuzz_parse` | `parse`, `to_json`, `stringify`, `resolve`, `reshape_tool_output` |
| `fuzz_compile` | `compile`, `decompile`, `is_synxb`, round-trip fidelity |
| `fuzz_format` | `format`, `safe_calc` |

## Running

```bash
cd crates/synx-core

# Quick smoke (30 seconds)
cargo +nightly fuzz run fuzz_parse -- -max_total_time=30

# Long run (1 hour)
cargo +nightly fuzz run fuzz_parse -- -max_total_time=3600

# All three targets
for t in fuzz_parse fuzz_compile fuzz_format; do
  cargo +nightly fuzz run $t -- -max_total_time=60
done
```

## Corpus

Interesting inputs found by the fuzzer are saved under `fuzz/corpus/<target>/`.
Crash reproducers go to `fuzz/artifacts/<target>/`.

As of 3.6.0, `fuzz_parse` has been hammered at scale (≈ **50M executions** across a large corpus). Any “hundreds of crashes” that clustered into the same location were treated as a single root-cause bug and fixed.

### Coverage (llvm-cov)

A source-based coverage report for a long `fuzz_parse` run is checked in under:

- `crates/synx-core/fuzz/coverage/fuzz_parse/html/`

The report was **pruned** to remove **0.00% line coverage** rows/pages (files we never executed in this project’s fuzz target). If you regenerate coverage, you can prune again with:

```bash
python3 crates/synx-core/fuzz/scripts/prune_coverage_html.py crates/synx-core/fuzz/coverage/fuzz_parse/html
```

### Replaying a single input (regression)

If you pass one or more files after the target name, libFuzzer **only runs those inputs** and exits. You will see:

```text
*** NOTE: fuzzing was not performed, you have only
***       executed the target code on a fixed set of inputs.
```

That is **not an error** — it means “no mutation loop, just regression replay.” Exit code **0** and no sanitizer report means the target did not crash on those bytes.

To actually fuzz (mutate and search for bugs), run without trailing file paths, or pass a **directory** as seed corpus, for example:

```bash
cd crates/synx-core
cargo +nightly fuzz run fuzz_parse fuzz/corpus/fuzz_parse/ -- -max_total_time=60
```

## C# parser replay (cross-language)

`cargo-fuzz` stays **Rust-only**. To hammer the **.NET** implementation with the same bytes:

1. Build **`Synx.FuzzReplay`** ([`parsers/dotnet/tools/Synx.FuzzReplay`](../../../parsers/dotnet/tools/Synx.FuzzReplay/README.md)):  
   `dotnet build -c Release parsers/dotnet/tools/Synx.FuzzReplay/Synx.FuzzReplay.csproj`
2. Pass **valid UTF-8** files only (invalid UTF-8 is skipped — same filter as Rust `fuzz_parse`’s `from_utf8` check).
3. Example:  
   `dotnet run -c Release --project parsers/dotnet/tools/Synx.FuzzReplay -- fuzz/artifacts/fuzz_parse/minimized-from-*`

For a **full** vendor-style pass (Rust tests + .NET tests + conformance replay), run [`scripts/verify-release-quality.sh`](../../../scripts/verify-release-quality.sh) or [`scripts/verify-release-quality.ps1`](../../../scripts/verify-release-quality.ps1).

### “Like `-jobs` / `-workers`” for C# (corpus replay only)

`Synx.FuzzReplay` is **not** libFuzzer: it **does not mutate** bytes. For parallel throughput over a **corpus** (same class of inputs you replay under Rust), build once then fan out with `xargs`:

```bash
cd parsers/dotnet
dotnet build -c Release tools/Synx.FuzzReplay/Synx.FuzzReplay.csproj

# Optional managed heap ceiling (~2 GiB); the process may still use more for native + JIT
export DOTNET_GCHeapHardLimit=$((2048 * 1024 * 1024))

# 12 concurrent processes × 80 files per `dotnet exec` (tune -P / -n)
find ../../crates/synx-core/fuzz/corpus/fuzz_parse -type f 2>/dev/null | \
  xargs -P 12 -n 80 dotnet exec \
  tools/Synx.FuzzReplay/bin/Release/net10.0/Synx.FuzzReplay.dll
```

Point `find` at `../../crates/synx-core/fuzz/artifacts/fuzz_parse` (or `minimized-from-*`) if you only replay crashes. If your `Synx.FuzzReplay` targets **net8.0**, change the path under `bin/Release/`. **Coverage-guided mutation** on .NET needs another tool (e.g. SharpFuzz); it is out of scope here.

## Python (`synx_native`) replay

Same idea as C#: **not** libFuzzer — replay **corpus** bytes through the **PyO3** binding so it matches `fuzz_parse` (UTF-8 only, then parse / JSON / stringify / `parse_active` / `parse_tool`).

Script: [`fuzz/scripts/synx_fuzz_replay.py`](scripts/synx_fuzz_replay.py).

```bash
# From repo root — install the extension module once
pip install -e bindings/python

cd crates/synx-core/fuzz/scripts
python3 synx_fuzz_replay.py --bench ../../../../tests/conformance/cases/*.synx
```

Parallel (e.g. 12 workers, batches of 20 files per Python process):

```bash
cd crates/synx-core/fuzz/scripts
export PYTHONHASHSEED=0
find ../corpus/fuzz_parse -type f 2>/dev/null | \
  xargs -P 12 -n 20 python3 synx_fuzz_replay.py
```

Optional RAM hint (Linux): run under `systemd-run -p MemoryMax=2G --uid=$(id -u)` or `ulimit -v` — CPython does not expose a single `DOTNET_GCHeapHardLimit`-style knob.

## CI

A short fuzz run (~60 seconds per target) can be added to CI:

```yaml
- name: Fuzz synx-core
  run: |
    cargo +nightly fuzz run fuzz_parse -- -max_total_time=60
    cargo +nightly fuzz run fuzz_compile -- -max_total_time=60
    cargo +nightly fuzz run fuzz_format -- -max_total_time=60
```
