# SYNX × Claude (Anthropic)

## 1) MCP server (first priority)

Add **`synx-mcp`** so the model can validate and parse `.synx` without guessing syntax.

**Claude Desktop** (`claude_desktop_config.json`), example:

```json
{
  "mcpServers": {
    "synx": {
      "command": "node",
      "args": [
        "A:/path/to/synx-format/integrations/mcp/synx-mcp/src/index.js"
      ],
      "env": {
        "SYNX_CLI": "A:/path/to/synx.exe",
        "SYNX_MCP_ROOT": "A:/path/to/your/workspace"
      }
    }
  }
}
```

Adjust paths. Install deps once: `cd integrations/mcp/synx-mcp && npm install`.

Alternatively, after publishing to npm: `"command": "npx"`, `"args": ["-y", "@aperturesyndicate/synx-format-mcp"]` (when the package exists).

Optional: paste **[`anthropic-system-prompt.txt`](anthropic-system-prompt.txt)** into the model system prompt for stricter SYNX output. Tokenizer notes (manual UI counts): **[`anthropic-token-notes.md`](anthropic-token-notes.md)**.

## 2) Grounding (second priority)

Pin the model to the **normative spec** and **conformance** outputs:

- Spec: `docs/spec/SPECIFICATION_EN.md`
- Tests: `tests/conformance/`

In project instructions or a **Skill**, state: “Before editing `.synx`, call `synx_validate_text` or read `synx://docs/overview`.”

## Two things to do first (summary)

1. **Enable MCP** (`synx-mcp` + `SYNX_CLI`).
2. **Reference spec + conformance** in the agent’s system/project prompt so behavior stays aligned with `synx-core`.
