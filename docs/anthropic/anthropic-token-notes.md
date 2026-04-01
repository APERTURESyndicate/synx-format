# SYNX snippets × Anthropic tokenizer (manual check)

Anthropic does not publish an exact client-side tokenizer spec in this repo; **token counts depend on the model and tokenization endpoint** you use in Claude / API / Workbench.

## Sample pack A — SYNX-style plain text (paste into Claude’s token counter)

Plain text only (no markdown fences), as you might paste into a counter UI:

```
:env :default :calc :alias :template :prompt :vision :audio :spam :random :unique :geo :join
!active !tool !schema !lock !include
#!mode:active
[min:3,max:30,required,readonly,type:int,pattern:^[a-z]+$,enum:a|b|c]
- item
|
###
// comment
# comment
```

### Reference measurement — Pack A (user-reported)

| Metric   | Value |
|----------|------:|
| Tokens   | 92    |
| Characters | 247 |

≈ **2.68 chars/token** for that sample. Use only as an order-of-magnitude check, not a guarantee across Claude 3.5 / 4 / Opus, locales, or punctuation.

## Sample pack B — JSON “schema cheat sheet” (same conceptual markers)

Structured JSON covering similar directives/constraints (not byte-for-byte equivalent to Pack A, but same *role*: vocabulary for tools and validation):

```json
{
  "metadata": {
    "mode": "active"
  },
  "directives": {
    "tags": [
      "env", "default", "calc", "alias", "template",
      "prompt", "vision", "audio", "spam", "random",
      "unique", "geo", "join"
    ],
    "flags": [
      "active", "tool", "schema", "lock", "include"
    ]
  },
  "schema": {
    "min": 3,
    "max": 30,
    "required": true,
    "readonly": true,
    "type": "int",
    "pattern": "^[a-z]+$",
    "enum": ["a", "b", "c"]
  },
  "content": {
    "items": ["item"],
    "separators": {
      "pipe": "|",
      "section": "###"
    },
    "blocks": {
      "code_start": "```"
    }
  }
}
```

### Reference measurement — Pack B (user-reported)

| Metric     | Value |
|------------|------:|
| Tokens     | 249   |
| Characters | 625   |

≈ **2.51 chars/token**.

## Side-by-side (same session / counter UI)

| Pack | Role | Chars | Tokens | Chars/token |
|------|------|------:|-------:|------------:|
| **A** (SYNX-like text) | Marker lines + structure hints | 247 | 92 | ~2.68 |
| **B** (JSON) | Same *idea* as a structured cheat sheet | 625 | 249 | ~2.51 |

In this run, **JSON used about 2.5× more characters and ~2.7× more tokens** than the compact SYNX-style text pack. Your exact ratios will differ by model and by how much real data you pack (lists, long strings, nesting).

## Practical guidance

- Treat **marker lines as “rare tokens”** only after you measure them in *your* UI; BPE-style tokenizers often merge punctuation unpredictably.
- Prefer **consistent marker spelling** (`:calc` not `calc:`) so prompts stay stable.
- For Claude-friendly framing, you can wrap SYNX in a thin XML envelope via **`synx-adapter`** (`wrap_xml=True`, tag `synx_data`) so the model sees familiar `<tag>…</tag>` boundaries without replacing SYNX syntax inside.
- Combine **`pack_for_llm` / `synx-context`** with your tokenizer UI and measure end-to-end on the target model.
