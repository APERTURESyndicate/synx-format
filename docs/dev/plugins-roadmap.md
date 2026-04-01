# Plugins and package registry (roadmap)

**Status:** design stub only. There is **no** `!use @scope/pkg` resolver, **no** plugin ABI, and **no** runtime registration of custom markers in `synx-core` today. This page describes the **intended direction** so tooling and docs can converge later.

## Goals (future)

- **Named packages** such as `@username/myplugin` with semver and a **lockfile** for reproducible builds.
- **Declarative libraries** first: archives of `.synx` + a small manifest (safe to cache and audit).
- Optional later phase: **code plugins** (e.g. Wasm) with signing, sandboxing, and explicit opt-in — out of scope until declarative packages exist.

## Planned package layout (illustrative)

```text
my-plugin/
├── synx-plugin.toml    # name, version, entry, exports (TBD)
├── src/
│   └── main.synx       # or multiple .synx files
└── README.md
```

Exact manifest keys and archive format will be specified when work starts; expect alignment with a **registry HTTP API** (e.g. synx-page) and a **local cache** (`~/.cache/synx/...` or project `vendor/synx/`).

## Language surface (candidates)

- Single import primitive, e.g. **`!use @scope/name`** or **`!import @scope/name`**, resolved **before** `!active` engine evaluation.
- Keep distinct from file-based **`:include` / `!include`**: registry packages are **versioned named artifacts**, not arbitrary paths.

## Custom markers and “commands”

- **Phase 1:** “New markers” are **documentation + convention** inside shared `.synx` libraries (templates, presets). The core engine’s **built-in** marker set stays frozen per **CORE-FREEZE** until a new normative language version.
- **Phase 2:** True **engine extensions** require a **versioned ABI**, parser hooks, and security review — not started.

## Contributing

Discussion and RFC-style proposals should reference this file and **avoid** changing frozen conformance behavior without a new normative doc version.
