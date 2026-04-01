@echo off
setlocal EnableExtensions

echo.
echo ============================================================
echo   SYNX - Publish to PyPI
echo   Package : synx-format
echo ============================================================
echo.

cd /d "%~dp0bindings\python"

:: ── 1. Check Python ──────────────────────────────────────────
where py >nul 2>&1
if errorlevel 1 (
    echo [ERROR] python not found. Install from https://www.python.org
    exit /b 1
)
for /f %%V in ('py --version 2^>^&1') do echo [INFO]  %%V

:: ── 2. Check Rust / cargo ────────────────────────────────────
where cargo >nul 2>&1
if errorlevel 1 (
    echo [ERROR] cargo not found. Install Rust from https://rustup.rs
    exit /b 1
)

:: ── 3. Ensure maturin is installed ───────────────────────────
echo.
echo [1/4] Checking maturin...
py -m maturin --version >nul 2>&1
if errorlevel 1 (
    echo [INFO]  maturin not found - installing...
    py -m pip install "maturin>=1.4,<2.0" --quiet
    if errorlevel 1 (
        echo [ERROR] Failed to install maturin.
        exit /b 1
    )
)
for /f %%V in ('py -m maturin --version 2^>^&1') do echo [INFO]  %%V

:: ── 4. Build release wheel ───────────────────────────────────
echo.
echo [2/4] Building wheel (release)...
for /f "delims=" %%P in ('where py') do set PY_LAUNCHER=%%P
for /f "delims=" %%I in ('py -c "import sys; print(sys.executable)"') do set PYTHON_EXE=%%I
set PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
echo [INFO]  PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
py -m maturin build --release --interpreter "%PYTHON_EXE%"
if errorlevel 1 (
    echo [ERROR] maturin build failed. Check Rust errors above.
    exit /b 1
)

:: ── 5. Locate the built wheel ────────────────────────────────
echo.
echo [3/4] Locating built wheel...
set WHEEL_DIR=%~dp0target\wheels
if not exist "%WHEEL_DIR%" (
    echo [ERROR] Wheel directory not found: %WHEEL_DIR%
    exit /b 1
)
for /f "delims=" %%F in ('dir /b /o-d "%WHEEL_DIR%\synx_format-*.whl" 2^>nul') do (
    set WHEEL_FILE=%WHEEL_DIR%\%%F
    goto :found_wheel
)
echo [ERROR] No .whl file found in %WHEEL_DIR%
exit /b 1

:found_wheel
echo [INFO]  Wheel: %WHEEL_FILE%

:: ── 6. Ensure twine is installed ─────────────────────────────
py -m twine --version >nul 2>&1
if errorlevel 1 (
    echo [INFO]  twine not found - installing...
    py -m pip install twine --quiet
)

:: ── 7. Upload to PyPI ────────────────────────────────────────
echo.
echo [4/4] Uploading to PyPI...
echo       (You will be prompted for your PyPI username/password or API token)
echo       Tip: use __token__ as username and your API token as password.
echo.
py -m twine upload "%WHEEL_FILE%"
if errorlevel 1 (
    echo.
    echo [ERROR] Upload failed.
    echo         If the version already exists, bump the version in Cargo.toml + pyproject.toml.
    exit /b 1
)

echo.
echo ============================================================
echo   Done! Package published:
echo   https://pypi.org/project/synx-format/
echo ============================================================
echo.

endlocal
