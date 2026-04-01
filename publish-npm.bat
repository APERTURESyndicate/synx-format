@echo off
setlocal EnableExtensions

echo.
echo ============================================================
echo   SYNX - Publish to npm
echo   Package : @aperturesyndicate/synx-format
echo ============================================================
echo.

set "SCRIPT_DIR=%~dp0"
set "TARGET_DIR="

if exist "%SCRIPT_DIR%packages\synx-js\package.json" (
    set "TARGET_DIR=%SCRIPT_DIR%packages\synx-js"
) else if exist "%SCRIPT_DIR%package.json" (
    set "TARGET_DIR=%SCRIPT_DIR%"
) else (
    echo [ERROR] Could not locate packages\synx-js\package.json.
    echo         Run this script from repository root or packages\synx-js.
    exit /b 1
)

cd /d "%TARGET_DIR%"
echo [PATH]  Working directory: %CD%

if not "%~1"=="" (
    echo [0/4] Setting package version to %~1 ...
    call npm version %~1 --no-git-tag-version
    if errorlevel 1 (
        echo [ERROR] Version bump failed. Use semver like 3.0.1 or patch/minor/major.
        exit /b 1
    )
)

:: ── 1. Check npm is available ────────────────────────────────
where npm >nul 2>&1
if errorlevel 1 (
    echo [ERROR] npm not found. Install Node.js from https://nodejs.org
    exit /b 1
)

:: ── 2. Check login status ────────────────────────────────────
echo [1/4] Checking npm login status...
call npm whoami >nul 2>&1
if errorlevel 1 (
    if not "%NPM_TOKEN%"=="" (
        echo [AUTH]  Using NPM_TOKEN from environment...
        call npm config set //registry.npmjs.org/:_authToken=%NPM_TOKEN% >nul 2>&1
    ) else (
        echo [AUTH]  You are not logged in. Running: npm login
        call npm login
        if errorlevel 1 (
            echo [ERROR] Login failed. Aborting.
            exit /b 1
        )
    )
)
for /f %%U in ('call npm whoami 2^>nul') do echo [AUTH]  Logged in as: %%U

call npm whoami >nul 2>&1
if errorlevel 1 (
    echo [ERROR] npm auth failed. Provide NPM_TOKEN or run npm login manually.
    exit /b 1
)

:: ── 3. Install dependencies ──────────────────────────────────
echo.
echo [2/4] Installing dependencies...
call npm install --silent
if errorlevel 1 (
    echo [ERROR] npm install failed.
    exit /b 1
)

:: ── 4. Build TypeScript ──────────────────────────────────────
echo.
echo [3/4] Building TypeScript...
call npm run build
if errorlevel 1 (
    echo [ERROR] Build failed. Fix TypeScript errors before publishing.
    exit /b 1
)

:: ── 5. Publish ───────────────────────────────────────────────
echo.
echo [4/4] Publishing to npm...
call npm publish --access public
if errorlevel 1 (
    echo.
    echo [ERROR] Publish failed.
    echo         If the version already exists, bump the version in package.json first.
    exit /b 1
)

echo.
echo ============================================================
echo   Done! Package published:
echo   https://www.npmjs.com/package/@aperturesyndicate/synx-format
echo ============================================================
echo.

echo [POST] Deprecating old package @aperturesyndicate/synx ...
echo        This keeps installs working but points users to the new name.
call npm deprecate @aperturesyndicate/synx@"*" "DEPRECATED: moved to @aperturesyndicate/synx-format. Install: npm i @aperturesyndicate/synx-format"
if errorlevel 1 (
    echo [WARN] Could not deprecate @aperturesyndicate/synx (maybe no perms or already deprecated).
)

endlocal
