# Release / vendor assurance checks for synx-format (Windows PowerShell).
# Run from repo root:  .\scripts\verify-release-quality.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

# PyO3 bindings need a Python 3.x for `cargo test` at workspace root.
if (-not $env:PYO3_PYTHON) {
    $candidates = @(
        "$env:LOCALAPPDATA\Programs\Python\Python311\python.exe",
        "$env:LOCALAPPDATA\Programs\Python\Python312\python.exe",
        "$env:LOCALAPPDATA\Programs\Python\Python310\python.exe"
    )
    foreach ($p in $candidates) {
        if (Test-Path $p) { $env:PYO3_PYTHON = $p; break }
    }
}

function Step($name, $block) {
    Write-Host "`n=== $name ===" -ForegroundColor Cyan
    & $block
    if ($LASTEXITCODE -ne 0) { throw "Step failed: $name (exit $LASTEXITCODE)" }
}

Step "Rust: synx-core unit + conformance" {
    cargo test -p synx-core
}
if (Test-Path "crates/synx-cli/Cargo.toml") {
    Step "Rust: synx-cli build" { cargo build -p synx-cli -q }
}
Step "Rust: full workspace tests (bindings need PYO3_PYTHON)" {
    if (-not $env:PYO3_PYTHON) {
        Write-Host "PYO3_PYTHON not set and no default Python found; skip full `cargo test` (run synx-core-only above)." -ForegroundColor Yellow
        return
    }
    cargo test
}
Step ".NET: Synx.Core tests" {
    Push-Location "parsers\dotnet"
    dotnet test --verbosity minimal
    Pop-Location
}
Step ".NET: FuzzReplay tool build + conformance corpus (parse + ToJson)" {
    $cases = @(Get-ChildItem -Path "tests\conformance\cases\*.synx" -ErrorAction SilentlyContinue)
    dotnet build "parsers\dotnet\tools\Synx.FuzzReplay\Synx.FuzzReplay.csproj" -c Release
    if ($cases.Count -eq 0) { Write-Host "No conformance .synx files - skip replay"; return }
    $paths = $cases | ForEach-Object { $_.FullName }
    & dotnet run -c Release --project "parsers\dotnet\tools\Synx.FuzzReplay\Synx.FuzzReplay.csproj" -- --bench @paths
}
Step "Optional: Rust bench crate (release check only)" {
    if (Test-Path "benchmarks\rust\Cargo.toml") {
        Push-Location "benchmarks\rust"
        cargo build --release -q 2>$null
        Pop-Location
    }
}

Write-Host "`nAll steps completed." -ForegroundColor Green
