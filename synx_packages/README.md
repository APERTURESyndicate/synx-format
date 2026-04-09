# synx_packages

Local package directory for SYNX `!use` directives and WASM marker packs.

Packages installed via `synx install` are stored here. Each package follows the layout:

```
synx_packages/
  @scope/
    package-name/
      synx-pkg.synx       # Package manifest (name, version, author, etc.)
      README.md            # Package documentation (rendered on registry)
      src/
        main.synx          # Entry point — loaded by !use (config packages)
        lib.rs             # Rust source — compiled to markers.wasm (WASM packages)
      markers.wasm         # Compiled WASM binary (marker packages only)
  template/                # Starter template — copy and customize
```

## Packages

- `@assynx/text-tools` — Official text utility markers: `:upper` `:lower` `:reverse` `:base64` `:hash` `:truncate` `:pad` `:count`

## Template

The `template/` folder is a ready-to-use starter project for creating custom WASM markers.
Copy it, rename, add your markers, build, and publish. Or run `synx create` to scaffold interactively.
