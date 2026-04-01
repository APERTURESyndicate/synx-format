# SYNX for Mojo (`bindings/mojo`)

**Version:** 3.6.0 — **same semantics as `synx-core`**, without maintaining a second grammar implementation in Mojo.

## What this is (and is not)

- **This folder** provides **`synx/interop.mojo`** — thin wrappers around Modular’s **[Calling Python from Mojo](https://docs.modular.com/mojo/manual/python/python-from-mojo)** API, importing **`synx_native`** (the **PyO3 / maturin** extension — same as `pip install synx-format`).
- **It is not** a from-scratch Mojo tokenizer/parser/engine: duplicating `synx-core` + `!active` + `.synxb` + `!tool` in pure Mojo would be a large, separate project (similar in scope to maintaining `@aperturesyndicate/synx-format` alongside Rust).

**Honest analogy with this repo:**

| You said | In-repo reality |
|----------|------------------|
| “like JS” | `packages/synx-js` is a **second** implementation (large maintenance). |
| “like Python” | `bindings/python` is **Rust (`synx-core`) via PyO3**, not a pure Python parser. |

This Mojo path matches **Python’s** pattern: **one engine** (`synx_native`), many language frontends.

## Requirements

- [Mojo](https://docs.modular.com/mojo/) + default **CPython** visible to Mojo  
- Python package **`synx-format`** (provides `synx_native`):
  ```bash
  pip install synx-format
  ```
  Or from this repo: `cd bindings/python && maturin develop` (uses the same Python env Mojo will load).

## API (string / Bool)

| Mojo (`synx.interop`) | Python (`synx_native`) |
|------------------------|-------------------------|
| `parse_json` | `parse_to_json` |
| `parse_active_json` | `parse_active_to_json` |
| `parse_tool_json` | `parse_tool_to_json` |
| `stringify_json` | `stringify_json` |
| `format_synx` | `format` |
| `diff_json` | `diff_json` |
| `compile_hex` | `compile_hex` (`.synxb` as lowercase hex) |
| `decompile_hex` | `decompile_hex` |
| `is_synxb_hex` | `is_synxb_hex` (magic probe from hex prefix) |

**Note:** `parse_*_json` / `diff_json` / `stringify_json` use **canonical JSON** (sorted object keys) from Rust, aligned with `synx-core::to_json`.

## Run the demo

From **`bindings/mojo`** (after `pip install synx-format` into the Python Mojo uses):

```bash
mojo run examples/demo.mojo
```

Expected output: one line of JSON for `name` / `count`.

## Layout

```
bindings/mojo/
  README.md
  synx/
    interop.mojo
  examples/
    demo.mojo
```

## Conformance

Behavior is defined by **`synx-core` 3.6.x** and **`tests/conformance/`** — Mojo code only forwards strings to Python.
