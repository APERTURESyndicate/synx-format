#!/usr/bin/env bash
# Release / vendor assurance checks (Linux/macOS). Run from repo root.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ -z "${PYO3_PYTHON:-}" ]] && command -v python3 >/dev/null 2>&1; then
  export PYO3_PYTHON="$(command -v python3)"
fi

step() {
  echo ""
  echo "=== $1 ==="
  shift
  "$@"
}

step "Rust: synx-core (unit + conformance)" cargo test -p synx-core

if [[ -f crates/synx-cli/Cargo.toml ]]; then
  step "Rust: synx-cli build" cargo build -p synx-cli -q
fi

if [[ -n "${PYO3_PYTHON:-}" ]]; then
  step "Rust: full workspace (PYO3_PYTHON set)" cargo test
else
  echo ""
  echo "=== Rust: full workspace (skipped — set PYO3_PYTHON for PyO3 bindings) ==="
fi

step ".NET: Synx.Core tests" bash -c 'cd parsers/dotnet && dotnet test --verbosity minimal'

step ".NET: FuzzReplay build" \
  dotnet build parsers/dotnet/tools/Synx.FuzzReplay/Synx.FuzzReplay.csproj -c Release

shopt -s nullglob
files=(tests/conformance/cases/*.synx)
if ((${#files[@]})); then
  step ".NET: FuzzReplay conformance corpus" \
    dotnet run -c Release --project parsers/dotnet/tools/Synx.FuzzReplay/Synx.FuzzReplay.csproj -- --bench "${files[@]}"
fi
shopt -u nullglob

if [[ -f benchmarks/rust/Cargo.toml ]]; then
  step "Rust: benchmarks crate release build" bash -c 'cd benchmarks/rust && cargo build --release -q'
fi

echo ""
echo "All steps completed."
