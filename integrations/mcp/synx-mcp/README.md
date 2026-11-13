> Main SYNX site: https://synx.aperturesyndicate.com/

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

`synx_validate_path`, `synx_read_path`, `synx_write_path` and `synx_apply_patch` only run when you define one of:

| Variable | Meaning |
|----------|---------|
| `SYNX_MCP_ROOT` | Single absolute directory; every `path` must lie inside it |
| `SYNX_MCP_ROOTS` | Comma-separated absolute roots ( containment check in order ) |

- Containment is judged on the **real** path: a symlink or junction inside a root that leads out of it is refused, for reads and for writes.
- Credential files (`.env`, `.env.*` except `.env.example`, `*.pem`, `*.key`, `*.p8`, `id_rsa`, `.npmrc`, …) and `.git` / `.ssh` / `.aws` / `.gnupg` directories are refused. Set `SYNX_MCP_ALLOW_SECRET_FILES=1` to allow them.
- `synx_read_path` returns the value of a literal `:secret` key as `[SECRET]` (`db_password:secret hunter2` → `db_password:secret [SECRET]`); `key:secret:env NAME` is left as written. `synx_write_path` refuses text that would put the mask over the real value — change other lines with `synx_apply_patch`.

Max file size: **10 MB** per read/write.

## Environment of the `synx` process

The CLI resolves `!active` documents, and the text often comes from the model. The child process therefore does **not** inherit the server's environment: `:env` sees nothing but the names you list.

| Variable | Meaning |
|----------|---------|
| `SYNX_MCP_PASS_ENV` | Comma-separated variable names `:env` may read (e.g. `APP_ENV,PORT`) |

A run of the CLI is stopped after 60 seconds.

## Tools

- `synx_validate_path` (under the sandbox roots) / `synx_validate_text`
- `synx_parse_json` (`active: true` is only needed by synx CLI versions before 3.8)
- `synx_format_text`
- `synx_read_path` — read UTF-8 file under sandbox
- `synx_write_path` — atomic write under sandbox
- `synx_apply_patch` — ordered unique substring replacements (no regex, replacement text taken literally)

## Tests

```bash
node --test
```

## Resource

- `synx://docs/overview` — short syntax reference for agents.
