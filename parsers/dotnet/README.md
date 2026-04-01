# SYNX for .NET

C# parser and JSON emitter aligned with [`synx-core`](../../crates/synx-core/): same tree rules as `parser::parse`, deterministic JSON as `to_json` (sorted object keys, matching escapes and float formatting via `Utf8JsonWriter`).

## Requirements

- [.NET SDK 8.0](https://dotnet.microsoft.com/download/dotnet/8.0) or later (for local development you can bump `<TargetFramework>` in the `.csproj` files if you only have a newer runtime).

## Layout

| Path | Purpose |
|------|---------|
| `Synx.sln` | Solution |
| `src/Synx.Core/` | Library: `SynxFormat.Parse`, `SynxFormat.ParseTool`, `SynxFormat.ToJson`, `SynxValue` |
| `tests/Synx.Core.Tests/` | xUnit — runs repo root `tests/conformance/cases/*.synx` + `.expected.json` (same as Rust `synx-core` conformance test) |
| `tools/Synx.FuzzReplay/` | Console tool: replay **arbitrary files** through `Parse` + `ToJson` (strict UTF-8 only — matches Rust `fuzz_parse` filter). Use alongside [`crates/synx-core/fuzz/`](../../crates/synx-core/fuzz/) artifacts. |

## Commands

```bash
cd parsers/dotnet
dotnet test
dotnet build -c Release

# Replay fuzz corpora / minimized inputs on the C# parser (must be valid UTF-8)
dotnet run -c Release --project tools/Synx.FuzzReplay -- path/to/fuzz/artifacts/fuzz_parse/minimized-from-*
# Optional timing (parse + ToJson per file)
dotnet run -c Release --project tools/Synx.FuzzReplay -- --bench tests/conformance/cases/*.synx
```

## API (preview)

- **`SynxFormat.Parse(string)`** — static parse → `Dictionary<string, SynxValue>` (no `!active` engine: markers are not resolved).
- **`SynxFormat.ParseActive(string, SynxOptions?)`** — parse + resolve (`:calc`, `:env`, `:ref`, `:alias`, constraints, includes, interpolation, …) — targets parity with `synx-core::engine::resolve`.
- **`SynxFormat.ParseFull` / `ParseFullActive`** — full `SynxParseResult` including `Metadata` and `Includes`.
- **`SynxFormat.ParseTool(string)`** — parse + `reshape_tool_output` (matches Rust when the file is not `!active`).
- **`SynxFormat.ToJson`** — canonical JSON string for a value or root map.

### Consume from NuGet (after publish)

Package ID: **[`APERTURESyndicate.Synx`](https://www.nuget.org/packages/APERTURESyndicate.Synx)** (the ID **`Synx.Core`** is already used by another package on nuget.org). **Project folder** in this repo remains **`Synx.Core`**; **namespaces** are **`Synx`**.

```bash
dotnet add package APERTURESyndicate.Synx
```

**Prerelease / local build:** add a project reference into your solution, or pack a feed:

```bash
# from repo root — produces artifacts/nuget/APERTURESyndicate.Synx.*.nupkg (push optional)
.\publish-csharp.bat

dotnet add package APERTURESyndicate.Synx --source /absolute/path/to/synx-format/artifacts/nuget
```

### Publish to nuget.org (maintainers)

1. Bump **`Version`** in `src/Synx.Core/Synx.Core.csproj`.
2. Create an [API key](https://www.nuget.org/account/apikeys) whose **glob** includes `APERTURESyndicate.Synx` or `*`.
3. **PowerShell:** `$env:NUGET_API_KEY = '…token only…'` — **do not** wrap the key in `<` `>`.
4. Repo root: **`.\publish-csharp.bat`** (clears old `*.nupkg` in `artifacts/nuget`, pushes only **`APERTURESyndicate.Synx.*.nupkg`**).

Next: `.synxb`, `diff` in C#; broaden engine parity tests beyond smoke cases.

**Tests:** library targets **.NET 8**; test project and **`Synx.FuzzReplay`** may target **.NET 10** on machines where only a newer runtime is installed — adjust `<TargetFramework>` in the respective `.csproj` files if `dotnet run` fails with a framework error.
