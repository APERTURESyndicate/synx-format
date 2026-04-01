# SYNX-Adapter

Bridge for AI stacks: turns dicts / object context into **SYNX text** (and optionally a ` ```synx` fenced block, or an **XML wrapper** such as `<synx_data>` for Claude-friendly boundaries).

## Why

- In many cases SYNX is shorter than JSON (fewer `{}`, `""`, `,` per field).
- Fewer **prompt characters** → often fewer **input tokens** → lower API cost (measure with `estimate_vs_json` on your payloads).
- The model still gets an explicit structure; add one line of instruction: data below is SYNX (key then value; indentation is nesting).
- **Long context:** optional **anchor comments** (`anchor_index`, `section_anchors`) help scanning and mitigate “lost in the middle” on huge prompts — see [`docs/guides/long-context-synx.md`](../../docs/guides/long-context-synx.md).

This is **not** a drop-in replacement for strict JSON Schema agent protocols; it is **context packing** inside a text prompt.

## Layout

| Package | Path | Dependencies |
|---------|------|--------------|
| **Python** | [`python/`](python/) | `synx-format` (`synx_native`); optional `langchain-core`, `llama-index-core`. |
| **Node.js** | [`nodejs/`](nodejs/) | peer: `@aperturesyndicate/synx-format`; optional `@langchain/core`. |
| **Rust** | [`RUST.md`](RUST.md) | Use **`synx-core`**: `Synx::stringify` + your own prompt wrapper. |

## Python — quick start

```bash
pip install "synx-adapter @ git+https://github.com/APERTURESyndicate/synx-format.git#subdirectory=integrations/ai/synx-adapter/python"
# or after PyPI publish: pip install synx-adapter
```

```python
from synx_adapter import pack_for_llm, estimate_vs_json

ctx = {"server": {"host": "0.0.0.0", "port": 8080}, "debug": False}
prompt_block = pack_for_llm(
    ctx,
    label="Runtime config",
    anchor_index=True,
    section_anchors=True,
)
print(estimate_vs_json(ctx))
```

### XML wrapper (Claude-style outer tags)

Wrap the same SYNX payload in a tag the model already parses well (XML), **without** converting SYNX to XML:

```python
xml_block = pack_for_llm(
    ctx,
    wrap_fence=False,
    wrap_xml=True,
    xml_tag="synx_data",
)
# <synx_data><![CDATA[
# ...synx lines...
# ]]></synx_data>
```

Use `wrap_fence=True` and `wrap_xml=True` together if you want a fenced block **inside** CDATA.

### CLI (`synx-context`)

```bash
synx-context --label "Logs" --stats < payload.json
synx-context --no-fence < payload.json
synx-context --xml --no-fence < payload.json
synx-context --xml --xml-tag synx_data < payload.json
```

### LangChain

```bash
pip install "synx-adapter[langchain]"
```

```python
from synx_adapter.langchain import SynxSystemMessage

messages = [SynxSystemMessage({"tools": ["read_file"], "budget": 10_000})]
```

### LlamaIndex

```bash
pip install "synx-adapter[llama]"
```

```python
from synx_adapter.llama_index import pack_nodes_synx
```

## Node.js

```bash
cd nodejs && npm install
```

```javascript
import { packForLlm, estimateVsJson } from '@aperturesyndicate/synx-format-adapter';

const xml = packForLlm(data, {
  wrapFence: false,
  wrapXml: true,
  xmlTag: 'synx_data',
});
```

```javascript
import { toolResultSynxBody, toolInputFromText } from '@aperturesyndicate/synx-format-adapter/anthropic';

const text = toolResultSynxBody({ rows, meta });
const args = toolInputFromText(rawString); // JSON first, else Synx.parse
```

Peer: `@aperturesyndicate/synx-format`.

### CLI

From `nodejs/` during local dev: `npm link`, then `synx-context --stats < file.json` or `synx-context --xml --no-fence < file.json`. Add `--anchor-index` / `--section-anchors` for long-context packing.

## Claude Artifacts

React **`SynxVisualizer`** (indented tree preview): [`integrations/artifacts/synx-visualizer/`](../../artifacts/synx-visualizer/).

## Rust (same idea)

```rust
let text = synx_core::Synx::stringify(&value);
let prompt = format!("Context (SYNX):\n```synx\n{text}\n```");
```

No separate crate required — it’s all in `synx-core`.

## Publishing

- **PyPI:** `cd python && python -m build && twine upload dist/*` (after `pyproject` / package name `synx-adapter` are set).
- **npm:** `cd nodejs && npm publish --access public` under `@aperturesyndicate/synx-format-adapter`.
