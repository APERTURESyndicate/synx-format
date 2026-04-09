# @assynx/text-tools

8 text utility WASM markers for SYNX.

## Markers

| Marker | Description | Example |
|--------|-------------|---------|
| `:upper` | Convert to uppercase | `title:upper hello` → `HELLO` |
| `:lower` | Convert to lowercase | `slug:lower HELLO` → `hello` |
| `:reverse` | Reverse a string | `rev:reverse abc` → `cba` |
| `:base64` | Base64 encode | `token:base64 secret` → `c2VjcmV0` |
| `:hash` | FNV-1a hash (hex) | `fp:hash data` → `16-digit hex` |
| `:truncate` | Truncate with `...` | `preview:truncate:10 long text...` |
| `:pad` | Left-pad to width | `id:pad:8:0 42` → `00000042` |
| `:count` | Character count | `len:count hello` → `5` |

## Usage

```synx
!active
!use @assynx/text-tools

title:upper hello world
slug:lower HELLO-WORLD
```

## Build from source

```bash
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/assynx_text_tools.wasm ./markers.wasm
```
