# SYNX-Visualizer (React) — Claude Artifacts

Small **React** component that renders SYNX (or SYNX-like indented text) as a **scrollable, monospace** tree in the **Artifacts** panel. It does not embed the full SYNX parser: it highlights **indentation** and **comments** (`#`, `//`) so long payloads stay readable.

## Usage in Claude

1. Open **Artifacts** and create a **React** artifact.
2. Paste `SynxVisualizer.tsx` (or merge the `SynxVisualizer` function into your artifact bundle).
3. Ask Claude something like: *“Generate the dataset below in SYNX and assign it to `sample`.”*
4. Render:

```tsx
import SynxVisualizer from './SynxVisualizer';

const sample = `
# @anchor: config
config
  name demo
  debug false
users
  - id 1
    name Ada
`;

export default function App() {
  return (
    <div style={{ padding: 24 }}>
      <h2 style={{ marginTop: 0 }}>SYNX preview</h2>
      <SynxVisualizer synx={sample} />
    </div>
  );
}
```

## Props

| Prop | Type | Default | Description |
|------|------|---------|-------------|
| `synx` | `string` | — | SYNX source text |
| `indentStep` | `number` | `2` | Spaces per nesting level (tabs count as 2 spaces) |

## See also

- Long-context + anchors: [`docs/guides/long-context-synx.md`](../../../docs/guides/long-context-synx.md)
- SYNX-Adapter (packing & tool helpers): [`integrations/ai/synx-adapter/README.md`](../../ai/synx-adapter/README.md)
