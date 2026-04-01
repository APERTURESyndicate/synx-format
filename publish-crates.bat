@echo off
setlocal EnableExtensions

echo.
echo ============================================================
echo   SYNX - Publish to crates.io
echo   Crates: synx-core + synx-format
echo ============================================================
echo.

cd /d "%~dp0"

set "PUBLISH_FLAGS="
if /I "%~1"=="--allow-dirty" (
    set "PUBLISH_FLAGS=--allow-dirty"
    echo [INFO] allow-dirty mode enabled. Uncommitted changes will be allowed.
)

REM --- 1. Check cargo ---
where cargo >nul 2>&1
if errorlevel 1 (
    echo [ERROR] cargo not found. Install Rust from https://rustup.rs
    exit /b 1
)
for /f %%V in ('cargo --version 2^>^&1') do echo [INFO]  %%V

REM --- 2. Check login ---
echo.
echo [1/4] Checking crates.io auth...
echo       If you have not logged in yet, get your API token from:
echo       https://crates.io/settings/tokens
echo       Then run: cargo login YOUR_TOKEN
echo.
cargo login --help >nul 2>&1

REM --- 3. Dry-run synx-core ---
echo [2/4] Running dry-run check for synx-core...
cargo publish --dry-run -p synx-core %PUBLISH_FLAGS% 2>&1
if errorlevel 1 (
    echo.
    echo [ERROR] Dry-run failed for synx-core. See cargo output above.
    echo.
    echo Common causes:
    echo   - Not logged in: cargo login YOUR_TOKEN
    echo   - Version already on crates.io: bump version in crates\synx-core\Cargo.toml
    echo   - Missing Cargo.toml fields for publishing
    echo   - Uncommitted git changes: commit, or run this script as:
    echo       publish-crates.bat --allow-dirty
    exit /b 1
)

echo.
echo [INFO] synx-format depends on synx-core. Publish synx-core first, then synx-format.

REM --- 4. Publish synx-core ---
echo.
echo [3/4] Publishing synx-core to crates.io...
cargo publish -p synx-core %PUBLISH_FLAGS%
if errorlevel 1 (
    echo.
    echo [ERROR] Publish failed for synx-core.
    echo   If the version already exists on crates.io, bump crates\synx-core\Cargo.toml
    exit /b 1
)

REM --- 5. Publish synx-format ---
echo.
echo [4/4] Publishing synx-format to crates.io...
cargo publish -p synx-format %PUBLISH_FLAGS%
if errorlevel 1 (
    echo.
    echo [ERROR] Publish failed for synx-format.
    echo   If you just published synx-core, wait 1-2 minutes and run only step 4, or re-run this script.
    exit /b 1
)

echo.
echo ============================================================
echo   Done. Crates published:
echo   https://crates.io/crates/synx-core
echo   https://crates.io/crates/synx-format
echo ============================================================
echo.
echo   Install:
echo     cargo add synx-core
echo     cargo add synx-format
echo.

endlocal
