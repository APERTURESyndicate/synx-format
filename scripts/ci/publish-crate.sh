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
  cargo publish --dry-run --allow-dirty
  exit 0
fi

published="$(curl -sS --max-time 30 \
  -H 'User-Agent: synx-format release workflow (github.com/APERTURESyndicate/synx-format)' \
  "https://crates.io/api/v1/crates/${crate}/${version}" || true)"

if printf '%s' "$published" | grep -q "\"num\":\"${version}\""; then
  echo "::notice title=Already published::${crate} ${version} is already on crates.io — nothing to do"
  exit 0
fi

cargo publish
