@echo off
REM From repo root:  cmd: publish-cpp.bat   PowerShell: .\publish-cpp.bat
setlocal EnableExtensions
cd /d "%~dp0"

echo.
echo ============================================================
echo   SYNX - C++ SDK + synx_c (release build)
echo ============================================================
echo   PowerShell: .\publish-cpp.bat
echo.
echo.
echo This repo does NOT publish to a single "C++ registry".
echo You typically ship:
echo   - bindings\cpp\include\synx\synx.hpp
echo   - bindings\c-header\include\synx.h
echo   - target\release\libsynx_c.* (or synx_c.dll + .lib on Windows)
echo.
echo Register / distribute via:
echo   - GitHub Releases (zip the above per platform), or
echo   - vcpkg / Conan recipe (community or your feed), or
echo   - CMake FetchContent pointing at this repo + cargo-built lib.
echo.

where cargo >nul 2>&1
if errorlevel 1 (
  echo [ERROR] cargo not found.
  exit /b 1
)

echo [1/1] cargo build -p synx-c --release ...
cargo build -p synx-c --release
if errorlevel 1 exit /b 1

echo.
echo [OK] Native library in target\release\
echo      See bindings\cpp\README.md for CMake / include paths.
echo.
