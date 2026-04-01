# How SYNX helps Claude on very long prompts (100k+ tokens)

Claude supports large context windows (on the order of hundreds of thousands of tokens). A known limitation of long prompting is **lost in the middle**: models tend to recall and use information from the **start** and **end** of the context more reliably than from the **middle**.

This guide describes **anchor markers** and structural habits so SYNX-heavy prompts stay scannable—for both the model and humans—without changing the SYNX specification itself.

## Why anchors matter

Anchors are *repeated, visually distinct landmarks* (usually **comment lines**) that:

1. **Name** what is about to appear (section title, entity id, domain).
2. **Repeat** critical identifiers so they appear more than once in the transcript.
3. **Stay copy-paste stable** so search (`Ctrl+F`) and vertical scanning find the same string everywhere.

Plain prose buries facts in sentences. JSON buries keys inside punctuation. SYNX already uses **one key per line** and **indentation for nesting**, which is easier to skim than dense `{ "k": ... }` blobs.

## SYNX-native patterns (no adapter required)

- **Use comments as section headers** (`# ...` or `// ...`) every few screenfuls of data.
- **Keep a short “table of contents”** in a comment block at the **top** (and optionally **repeat** the same list after huge middle sections).
- **Prefer shallow trees**: push rarely-needed detail under clearly named keys so the root index stays short.
- **Stable ids**: repeat the same `request_id`, `ticket`, or `entity_key` in the TOC, in section headers, and next to the payload.

## Adapter-assisted anchors (`synx-adapter`)

The SYNX-Adapter can inject two kinds of machine-generated anchors on top of your serialized context.

### 1. Key index (`anchor_index`)

For a **dict-shaped** root, emit one comment line per **top-level key** before the body, e.g.:

```text
# @anchor: users
# @anchor: orders
# @anchor: meta
users
  ...
```

**Python:** `pack_for_llm(data, anchor_index=True)`  
**Node:** `packForLlm(data, { anchorIndex: true })`  
**CLI:** `synx-context --anchor-index < data.json`

### 2. Section anchors (`section_anchors`)

Before each **root-level** line that looks like `key` or `key value`, insert a matching anchor comment:

```text
# @anchor: server
server
  host 0.0.0.0
  port 8080
```

**Python:** `pack_for_llm(data, section_anchors=True)`  
**Node:** `packForLlm(data, { sectionAnchors: true })`  
**CLI:** `synx-context --section-anchors < data.json`

Default prefix is `# @anchor`; override with `anchor_prefix` / `anchorPrefix` / `--anchor-prefix`.

### Heuristic limits

Section detection uses a simple rule: **non-indented** lines matching `key` / `key value` (not comments or `!` directives). Nested keys are **not** prefixed—only the visual “chapter starts” at column 0. Adjust your structure or add manual `#` headers inside deep sections if needed.

## Prompt recipe for 100k-token bundles

1. **System or first user turn:** one paragraph stating “Data below is SYNX: key, then value; indent means nesting.”
2. **Critical constraints** (must-follow rules) in the **first** ~1–3k tokens **and** briefly **restated** before the final instruction.
3. **TOC / anchor index** at the top; **optional duplicate TOC** before the closing task description.
4. **Section anchors** on large dict serializations so the middle of the file still has searchable `key` labels.

Pair with token-focused notes: [`../anthropic-token-notes.md`](../anthropic-token-notes.md).

## Anthropic tool use (function calling)

When a tool returns a **large structured payload**, serializing the result as SYNX can shrink the string compared to minified JSON (fewer delimiters per field in many shapes). That reduces what you send back in the next **user** turn as `tool_result` **text** content—often lowering **latency and cost**.

- **Python:** `from synx_adapter.anthropic_tools import tool_result_synx_body, tool_input_from_text`
- **Node:** `import { toolResultSynxBody, toolInputFromText } from '@aperturesyndicate/synx-format-adapter/anthropic'`

`tool_input_from_text` tries **JSON** first (typical for strict tool schemas), then **SYNX** via the native parser if JSON fails—useful if you deliberately accept SYNX-shaped arguments in custom tools.

## Claude Artifacts: visual tree

For a **React** artifact, you can render SYNX as an indented tree without bundling the full parser: see [`../../integrations/artifacts/synx-visualizer/`](../../integrations/artifacts/synx-visualizer/). Ask the model to “emit SYNX for this dataset” and pass the string into `SynxVisualizer`.

---

*This document is advisory. Always measure tokenizer counts on your real payloads; SYNX is optimized for human/model readability and often smaller character count than JSON, but token savings depend on your content and tokenizer.*
