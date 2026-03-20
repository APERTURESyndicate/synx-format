# AI Memory Templates (SYNX)

Canonical SYNX templates for building AI agent memory systems.

## Files

| File | Purpose | Mode |
|------|---------|------|
| `core.synx` | Agent identity, personality, rules | Static (read-only) |
| `state.synx` | Working memory — user profile, context, session | `!active` (read/write) |

## How it works

1. **core.synx** is parsed once at startup and embedded in the system prompt as raw SYNX.  
   The model sees the native SYNX structure — not a reformatted text version.

2. **state.synx** uses `!active` mode so markers like `:default`, `:env`, `:template` are resolved at read time.  
   The model updates specific fields via tool calls (e.g. `update_memory block="user_profile" key="name" value="Alex"`).

3. Long-term memory is stored in a separate database (SQLite FTS5 recommended).  
   SYNX handles the structured, human-readable layer; the database handles search at scale.

## Usage with Python

```python
from synx_native import parse, parse_active, stringify, to_prompt_block

# Core — static parse, embed as raw SYNX in prompt
core_text = open("core.synx").read()
core_data = parse(core_text)
prompt_section = to_prompt_block(core_text, "Core memory")

# State — active parse with marker resolution
state_text = open("state.synx").read()
state_data = parse_active(state_text)
prompt_section = to_prompt_block(state_text, "Working memory")
```

## Design principle

> SYNX for structure and human-readability.  
> Database for search and scale.  
> The model sees raw SYNX — not JSON, not flattened text.
