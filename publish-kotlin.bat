@echo off
REM From repo root:  cmd: publish-kotlin.bat   PowerShell: .\publish-kotlin.bat
setlocal EnableExtensions
cd /d "%~dp0"

echo.
echo ============================================================
echo   SYNX - Kotlin JVM (synx-kotlin) - Maven Local
echo ============================================================
echo   PowerShell: .\publish-kotlin.bat
echo.
echo.
echo This runs:  gradlew publishToMavenLocal  (tests skipped; they need synx_c + JDK)
echo.
echo Maven Central later:
echo   Register: https://central.sonatype.org/publish/publish-guide/
echo   You need Sonatype namespace for groupId com.aperturesyndicate + GPG signing in Gradle.
echo.

where cargo >nul 2>&1
if errorlevel 1 (
  echo [ERROR] cargo not found.
  exit /b 1
)

echo [1/2] cargo build -p synx-c --release ...
cargo build -p synx-c --release
if errorlevel 1 exit /b 1

pushd bindings\kotlin
echo [2/2] gradlew publishToMavenLocal -x test ...
call gradlew.bat publishToMavenLocal -x test --no-daemon
set "ERR=%ERRORLEVEL%"
popd
if not "%ERR%"=="0" exit /b 1

echo.
echo [OK] Coordinates: com.aperturesyndicate:synx-kotlin (local ~/.m2)
echo      See bindings\kotlin\README.md
echo.
