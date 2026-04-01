# AI integrations

| Path | Role |
|------|------|
| [`synx-adapter/`](synx-adapter/) | **SYNX-Adapter** — pack structured context as SYNX before sending to an LLM (vs JSON): fewer characters in many cases; optional `<synx_data>` XML wrapper; **anchor markers** for long prompts; **Anthropic tool_result** helpers (`anthropic_tools` / `anthropic` export). |
| [`../artifacts/synx-visualizer/`](../artifacts/synx-visualizer/) | **SYNX-Visualizer** — React snippet for Claude Artifacts (indented SYNX tree). |
| [`../mcp/synx-mcp`](../mcp/synx-mcp/) | MCP: `validate` / `parse` / `format` from an agent (Claude, etc.). |

See [`synx-adapter/README.md`](synx-adapter/README.md). Long-context guide: [`../../docs/guides/long-context-synx.md`](../../docs/guides/long-context-synx.md).
