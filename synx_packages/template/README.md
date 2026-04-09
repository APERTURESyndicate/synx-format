# SYNX WASM Marker Package — Template

Starter template for creating custom SYNX WASM markers. Copy this folder, add your own markers, build, and publish.

## Quick Start

```bash
# 1. Copy template
cp -r synx_packages/template my-markers
cd my-markers

# 2. Edit synx-pkg.synx — set your scope, name, description
# 3. Edit src/lib.rs — add marker names and implementations
# 4. Build
cargo build --target wasm32-unknown-unknown --release

# 5. Publish
synx publish
```

Or use the CLI scaffolder:

```bash
synx create
```

## Adding Markers

1. Add the marker name to the JSON array in `synx_markers()`
2. Add a `match` arm in `synx_apply()` with your logic
3. Rebuild with `build.bat` (Windows) or `build.sh` (Linux/macOS)

## ABI v1

Your WASM module must export three functions:

| Export | Signature | Purpose |
|--------|-----------|---------|
| `synx_alloc` | `(i32) → i32` | Allocate guest memory for host writes |
| `synx_markers` | `() → i64` | Return JSON array of marker names |
| `synx_apply` | `(i32, i32) → i64` | Apply marker, return JSON result |

### Request format (synx_apply input)
```json
{"marker": "shout", "value": "hello", "args": ["3"]}
```

### Response format (synx_apply output)
```json
{"value": "HELLO!!!"}
```
or on error:
```json
{"error": "description"}
```

## Capabilities

Declare capabilities in `synx-pkg.synx` under `capabilities`:
- `string` — string operations (read-only)
- `fs` — file system access
- `net` — network access
- `env` — environment variable access
