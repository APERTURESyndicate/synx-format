#!/usr/bin/env bash
# Publish the crate in the current directory to crates.io, unless that exact
# version is already there. Re-pushing a tag must not turn a finished release
# into a red X: "already exists on the index" is the expected state, not a
# failure. Any other cargo error still fails the job.
#
# Usage: publish-crate.sh <crate-name>       (run from the crate directory)
set -euo pipefail

crate="$1"
version="$(cargo pkgid | sed 's/.*[#@]//')"

if [ "${IS_DRY_RUN:-false}" = "true" ]; then
  # A crate that depends on a sibling of this same release cannot be packaged
  # before that sibling is on the index: the dry run then checks what it can —
  # the file list of the package and a build against the sibling in the tree.
  if cargo publish --dry-run --allow-dirty 2> dry-run.err; then
    exit 0
  fi
  cat dry-run.err >&2
  if grep -q "failed to select a version for the requirement \`synx-core" dry-run.err; then
    echo "::notice title=Dry run::${crate} ${version} needs synx-core ${version} on crates.io — checked the file list and the build instead"
    rm -f dry-run.err
    cargo package --list --allow-dirty
    cargo build --release
    exit 0
  fi
  exit 1
fi

published="$(curl -sS --max-time 30 \
  -H 'User-Agent: synx-format release workflow (github.com/APERTURESyndicate/synx-format)' \
  "https://crates.io/api/v1/crates/${crate}/${version}" || true)"

if printf '%s' "$published" | grep -q "\"num\":\"${version}\""; then
  echo "::notice title=Already published::${crate} ${version} is already on crates.io — nothing to do"
  exit 0
fi

cargo publish
