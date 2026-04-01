@echo off
REM From repo root:  cmd: publish-csharp.bat   PowerShell: .\publish-csharp.bat
setlocal EnableExtensions
cd /d "%~dp0"

echo.
echo ============================================================
echo   SYNX - Pack (and optionally push) APERTURESyndicate.Synx to NuGet
echo ============================================================
echo   PowerShell: use .\publish-csharp.bat ^(current dir is not on PATH^)
echo.
echo Register: https://www.nuget.org/  ^> Account ^> API keys
echo Push uses env: NUGET_API_KEY
echo   PowerShell: use ONLY the key string, e.g.  $env:NUGET_API_KEY = 'oy2...'   NOT  '^<oy2...^>'
echo.
echo If push returns 403:
echo   - Old  Synx.Core.*.nupkg  in artifacts\nuget: this script now deletes *.nupkg / *.snupkg there before pack
echo     so  dotnet nuget push  does not upload the wrong package ID.
echo   - PowerShell: no spaces/newline in NUGET_API_KEY; key must allow Push for this package / *.
echo   - Prefix reservation / other owner: docs.microsoft.com nuget id-prefix-reservation
echo.

where dotnet >nul 2>&1
if errorlevel 1 (
  echo [ERROR] dotnet SDK not found. Install .NET 8+ SDK.
  exit /b 1
)

if not exist "artifacts\nuget" mkdir "artifacts\nuget"

echo [0/2] Clearing artifacts\nuget\*.nupkg / *.snupkg ^(stale Synx.Core.* breaks push * glob^) ...
del /q "%~dp0artifacts\nuget\*.nupkg" 2>nul
del /q "%~dp0artifacts\nuget\*.snupkg" 2>nul

echo [1/2] dotnet pack APERTURESyndicate.Synx ^(Synx.Core.csproj^) ...
dotnet pack "parsers\dotnet\src\Synx.Core\Synx.Core.csproj" -c Release -o "%~dp0artifacts\nuget"
if errorlevel 1 exit /b 1

if not defined NUGET_API_KEY (
  echo.
  echo [INFO] NUGET_API_KEY not set - .nupkg is in artifacts\nuget\
  echo        To push: set NUGET_API_KEY=*** then re-run this script.
  exit /b 0
)

echo [2/2] dotnet nuget push APERTURESyndicate.Synx.*.nupkg only ...
set "PUSHED="
for /f "delims=" %%F in ('dir /b "%~dp0artifacts\nuget\APERTURESyndicate.Synx.*.nupkg" 2^>nul') do (
  set "PUSHED=1"
  dotnet nuget push "%~dp0artifacts\nuget\%%F" -k "%NUGET_API_KEY%" -s "https://api.nuget.org/v3/index.json" --skip-duplicate
  if errorlevel 1 (
    echo.
    echo [ERROR] Push failed for %%F
    exit /b 1
  )
)
if not defined PUSHED (
  echo [ERROR] No APERTURESyndicate.Synx.*.nupkg found in artifacts\nuget\
  exit /b 1
)

echo [OK] Push complete (or skip-duplicate).
echo Consumers:  dotnet add package APERTURESyndicate.Synx
echo.
