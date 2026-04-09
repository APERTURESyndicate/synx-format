#!/bin/sh
set -e
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/assynx_text_tools.wasm ./markers.wasm
echo "Built markers.wasm"
