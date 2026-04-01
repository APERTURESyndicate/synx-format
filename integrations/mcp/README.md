# MCP — Model Context Protocol (Claude Desktop & compatible clients)

[`synx-mcp/`](synx-mcp/) exposes SYNX to agents:

| Tool | Purpose |
|------|---------|
| `synx_validate_path` | `synx validate` on a file |
| `synx_validate_text` | validate raw `.synx` string |
| `synx_parse_json` | parse → JSON (optional `--active`) |
| `synx_format_text` | canonical `synx format` output |

Resource **`synx://docs/overview`** — short syntax snippet for system grounding.

**Requires:** `synx` binary on `PATH`, or set env **`SYNX_CLI`** to the full path of the executable.

```bash
cd synx-mcp && npm install && node src/index.js
```

See also [`docs/claude.md`](../../docs/claude.md).
