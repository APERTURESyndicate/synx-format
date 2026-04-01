# @aperturesyndicate/synx-format-mcp

MCP server (stdio) for SYNX — wraps the **`synx`** CLI.

## Setup

```bash
npm install
# Requires `synx` on PATH, or:
set SYNX_CLI=C:\path\to\synx.exe   # Windows
export SYNX_CLI=/usr/local/bin/synx  # Unix

# Optional: allow MCP read/write tools (see Sandbox below)
set SYNX_MCP_ROOT=A:\your\project   # Windows
export SYNX_MCP_ROOT=/home/you/proj  # Unix

node src/index.js
```

## Claude Desktop

See [`docs/claude.md`](../../../docs/claude.md).

## Sandbox (filesystem tools)

Read/write/patch tools only run when you define one of:

| Variable | Meaning |
|----------|---------|
| `SYNX_MCP_ROOT` | Single absolute directory; every `path` must lie inside it |
| `SYNX_MCP_ROOTS` | Comma-separated absolute roots ( containment check in order ) |

Max file size: **10 MB** per read/write.

## Tools

- `synx_validate_path` / `synx_validate_text`
- `synx_parse_json` (optional `active: true`)
- `synx_format_text`
- `synx_read_path` — read UTF-8 file under sandbox
- `synx_write_path` — atomic write under sandbox
- `synx_apply_patch` — ordered unique substring replacements (no regex)

## Resource

- `synx://docs/overview` — short syntax reference for agents.
