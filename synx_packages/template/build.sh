#!/bin/bash
set -e
echo "Building WASM markers..."
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/my_synx_markers.wasm markers.wasm
echo "Done: markers.wasm"
