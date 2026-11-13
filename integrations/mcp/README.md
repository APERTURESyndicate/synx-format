> Main SYNX site: https://synx.aperturesyndicate.com/

# MCP — Model Context Protocol (Claude Desktop & compatible clients)

[`synx-mcp/`](synx-mcp/) exposes SYNX to agents:

| Tool | Purpose |
|------|---------|
| `synx_validate_path` | `synx validate` on a file under `SYNX_MCP_ROOT[S]` |
| `synx_validate_text` | validate raw `.synx` string |
| `synx_parse_json` | parse → JSON (optional `--active`) |
| `synx_format_text` | canonical `synx format` output |
| `synx_read_path` / `synx_write_path` / `synx_apply_patch` | files under `SYNX_MCP_ROOT[S]` (real-path sandbox, credential files refused) |

Resource **`synx://docs/overview`** — short syntax snippet for system grounding.

**Requires:** `synx` binary on `PATH`, or set env **`SYNX_CLI`** to the full path of the executable.

The CLI child process gets a minimal environment: `:env` in a document reads only the names listed in **`SYNX_MCP_PASS_ENV`**. Details — [`synx-mcp/README.md`](synx-mcp/README.md).

```bash
cd synx-mcp && npm install && node src/index.js
```

See also [`docs/claude.md`](../../docs/claude.md).
