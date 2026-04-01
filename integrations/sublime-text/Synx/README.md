# SYNX — Sublime Text

Syntax highlighting + **LSP** (`synx-lsp`) for the SYNX format.

## 1) Install this package

**Manual (recommended for development)**

1. **Preferences → Browse Packages…**
2. Create folder `Synx` (or clone this repo path `integrations/sublime-text/Synx` into `Packages/Synx`).
3. Restart Sublime Text.

You should see **`synx.sublime-syntax`** and this `README.md` in the package folder.

**Optional — via Git on your machine**

```bash
# Example: clone repo and symlink into Sublime’s Packages (path varies by OS)
ln -s /path/to/synx-format/integrations/sublime-text/Synx ~/Library/Application\ Support/Sublime\ Text/Packages/Synx
```

Publishing to **Package Control** later: submit a PR to [Package Control channel](https://github.com/wbond/package_control_channel) with your public Git URL, or document your own **repository** URL in `repositories.json` (see Package Control docs).

## 2) Syntax

Open any `*.synx` file — if association is wrong, use **View → Syntax → SYNX** (after the package is installed).

The bundled `synx.sublime-syntax` covers directives (`!active`, `!include`, …), comments, keys, markers, lists.

## 3) LSP (diagnostics, completion, outline)

1. Install **LSP** via [Package Control](https://packagecontrol.io/packages/LSP).
2. Build or install `synx-lsp` so it is on `PATH` (see `crates/synx-lsp/README.md`).
3. Open **Preferences → Package Settings → LSP → Settings**, merge:

```json
{
  "clients": {
    "synx-lsp": {
      "enabled": true,
      "command": ["synx-lsp"],
      "selector": "source.synx",
      "file_patterns": ["*.synx"]
    }
  }
}
```

On Windows, if `synx-lsp` is not on PATH, use the full path:

```json
"command": ["C:\\path\\to\\synx-lsp.exe"]
```

4. Open a `.synx` file — LSP should start automatically (check LSP log panel if not).

## 4) Publish when ready

- **Package Control:** follow [official instructions](https://packagecontrol.io/docs/submitting_a_package) (tags `st3-4.0`, readable `package.json`-style meta, or legacy channel submission).
- **Manual:** ship a `.zip` of this folder and ask users to unzip into `Packages/Synx`.

Keep the package **small**: do not commit Sublime’s `Cache` or workspace files.
