@echo off
REM From repo root:  cmd: publish-mojo.bat   PowerShell: .\publish-mojo.bat
setlocal EnableExtensions
cd /d "%~dp0"

echo.
echo ============================================================
echo   SYNX - Mojo (ships as Python wheel: synx-format on PyPI)
echo ============================================================
echo   PowerShell: .\publish-mojo.bat
echo.
echo.
echo Mojo uses **Python from Mojo** to import synx_native - publish **bindings\python** with maturin.
echo.
echo Register: https://pypi.org/account/register/
echo API token: https://pypi.org/manage/account/token/  ^> set PYPI_API_TOKEN for CI
echo Local dry-run:  cd bindings\python ^&^& maturin build --release
echo Publish:        cd bindings\python ^&^& maturin publish
echo.
echo Mojo sample code: bindings\mojo\ - see bindings\mojo\README.md
echo.

where maturin >nul 2>&1
if errorlevel 1 (
  echo [INFO] maturin not in PATH. Install: pip install maturin
  exit /b 0
)

pushd bindings\python
echo Running: maturin publish ...
maturin publish
set "ERR=%ERRORLEVEL%"
popd
exit /b %ERR%
