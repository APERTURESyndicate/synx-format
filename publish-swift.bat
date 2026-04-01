@echo off
REM From repo root:  cmd: publish-swift.bat   PowerShell: .\publish-swift.bat
setlocal EnableExtensions
cd /d "%~dp0"

echo.
echo ============================================================
echo   SYNX - SwiftPM + synx_c (local build checks)
echo ============================================================
echo   PowerShell: .\publish-swift.bat
echo.
echo.
echo Swift has no NuGet-like central registry for this package.
echo Publishing = tag semver on GitHub + consumers add Package.swift URL dependency.
echo Optional: list on https://swiftpackageindex.com/
echo You must still ship or build libsynx_c per platform (this script builds Rust side).
echo.

where cargo >nul 2>&1
if errorlevel 1 (
  echo [ERROR] cargo not found.
  exit /b 1
)

echo [1/2] cargo build -p synx-c --release ...
cargo build -p synx-c --release
if errorlevel 1 exit /b 1

where swift >nul 2>&1
if errorlevel 1 (
  echo [INFO] swift not in PATH - skipped swift build. macOS/Linux CI can run swift test.
  goto :eof
)

echo [2/2] swift build (bindings\swift^) ...
set "SYNX_LIB_DIR=%~dp0target\release"
pushd bindings\swift
swift build -Xlinker -L -Xlinker "%SYNX_LIB_DIR%" -Xlinker -lsynx_c
set "ERR=%ERRORLEVEL%"
popd
if not "%ERR%"=="0" exit /b 1

echo [OK] See bindings\swift\README.md for SPM consumer snippet.
echo.
