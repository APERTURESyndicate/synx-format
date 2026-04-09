# package-vsix.ps1
# Packages the SYNX Visual Studio extension into a VSIX package.
# Uses classic VSIX layout (extension.vsixmanifest + payload files),
# which is accepted by VSIXInstaller for VS 2017/2019/2022.

param(
    [string]$BuildOutput = "SynxLanguageService\bin\Release\net472",
    [string]$VsixManifest = "SynxLanguageService\source.extension.vsixmanifest",
    [string]$OutFile     = "SynxLanguageService.vsix"
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
Push-Location $scriptDir

# --- Helpers ---

$utf8NoBom = New-Object System.Text.UTF8Encoding($false)

function Add-ZipBytes([System.IO.Compression.ZipArchive]$archive, [string]$entryName, [byte[]]$bytes) {
    $entry = $archive.CreateEntry($entryName, [System.IO.Compression.CompressionLevel]::Optimal)
    $dest  = $entry.Open()
    $dest.Write($bytes, 0, $bytes.Length)
    $dest.Dispose()
}

function Add-ZipFile([System.IO.Compression.ZipArchive]$archive, [string]$entryName, [string]$filePath) {
    $entry = $archive.CreateEntry($entryName, [System.IO.Compression.CompressionLevel]::Optimal)
    $src   = [System.IO.File]::OpenRead($filePath)
    $dest  = $entry.Open()
    $src.CopyTo($dest)
    $src.Dispose()
    $dest.Dispose()
}

try {
    $buildOut = Resolve-Path $BuildOutput
    $manifestPath = Resolve-Path $VsixManifest
    $outPath  = Join-Path $scriptDir $OutFile

    Write-Host "[package] Build output : $buildOut"
    Write-Host "[package] Manifest     : $manifestPath"

    if (Test-Path $outPath) { Remove-Item $outPath -Force }

    # --- Gather files to include ---
    # Each entry: @{ ZipPath = "..."; FilePath = "..." }
    $files = [System.Collections.ArrayList]::new()

    # extension.vsixmanifest
    [void]$files.Add(@{ ZipPath = "extension.vsixmanifest"; FilePath = $manifestPath.Path })

    # Main DLL
    $mainDll = Join-Path $buildOut "SynxLanguageService.dll"
    [void]$files.Add(@{ ZipPath = "SynxLanguageService.dll"; FilePath = $mainDll })

    # PkgDef (required for VS Package registration and marketplace)
    $pkgDefPath = Join-Path $buildOut "SynxLanguageService.pkgdef"
    if (Test-Path $pkgDefPath) {
        [void]$files.Add(@{ ZipPath = "SynxLanguageService.pkgdef"; FilePath = $pkgDefPath })
        Write-Host "[package] PkgDef       : included" -ForegroundColor Green
    } else {
        Write-Host "[package] WARNING: SynxLanguageService.pkgdef not found - generating stub" -ForegroundColor Yellow
        # Generate a minimal pkgdef so the marketplace accepts the VSIX
        $stubPkgdef = Join-Path $buildOut "SynxLanguageService.pkgdef"
        "// Auto-generated pkgdef for SynxLanguageService" | Set-Content $stubPkgdef -Encoding UTF8
        [void]$files.Add(@{ ZipPath = "SynxLanguageService.pkgdef"; FilePath = $stubPkgdef })
    }

    # Newtonsoft.Json.dll
    $newtonsoftPath = Join-Path $buildOut "Newtonsoft.Json.dll"
    if (Test-Path $newtonsoftPath) {
        [void]$files.Add(@{ ZipPath = "Newtonsoft.Json.dll"; FilePath = $newtonsoftPath })
    }

    # LICENSE.txt (required by VS Marketplace)
    $licensePath = Join-Path $scriptDir "LICENSE.txt"
    if (Test-Path $licensePath) {
        [void]$files.Add(@{ ZipPath = "LICENSE.txt"; FilePath = $licensePath })
        Write-Host "[package] License      : included" -ForegroundColor Green
    }

    # Resources
    $resDir = Join-Path $buildOut "Resources"
    if (Test-Path $resDir) {
        Get-ChildItem $resDir -File | ForEach-Object {
            [void]$files.Add(@{ ZipPath = "Resources/$($_.Name)"; FilePath = $_.FullName })
        }
    }

    # --- Read identity from vsixmanifest ---
    [xml]$vsixXml = Get-Content $manifestPath -Raw
    $ns = New-Object System.Xml.XmlNamespaceManager($vsixXml.NameTable)
    $ns.AddNamespace("v", "http://schemas.microsoft.com/developer/vsx-schema/2011")
    $identity = $vsixXml.SelectSingleNode("//v:Identity", $ns)
    $vsixId   = $identity.GetAttribute("Id")
    $version  = $identity.GetAttribute("Version")
    $publisher = $identity.GetAttribute("Publisher")

    Write-Host "[package] VSIX ID      : $vsixId"
    Write-Host "[package] Version      : $version"

    # --- Create ZIP/VSIX ---
    $zip = [System.IO.Compression.ZipFile]::Open($outPath, [System.IO.Compression.ZipArchiveMode]::Create)

    # 1. [Content_Types].xml (MUST be first - OPC spec)
    $contentTypes = @'
<?xml version="1.0" encoding="utf-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="vsixmanifest" ContentType="text/xml" />
  <Default Extension="rels"         ContentType="application/vnd.openxmlformats-package.relationships+xml" />
  <Default Extension="dll"          ContentType="application/octet-stream" />
  <Default Extension="png"          ContentType="image/png" />
  <Default Extension="svg"          ContentType="image/svg+xml" />
  <Default Extension="pkgdef"       ContentType="application/octet-stream" />
  <Default Extension="txt"          ContentType="text/plain" />
</Types>
'@
    Add-ZipBytes $zip "[Content_Types].xml" ($utf8NoBom.GetBytes($contentTypes))

    # 2. _rels/.rels
    $rels = @'
<?xml version="1.0" encoding="utf-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
        <Relationship Type="http://schemas.microsoft.com/developer/vsx-schema/2011" Target="extension.vsixmanifest" Id="R1" />
</Relationships>
'@
    Add-ZipBytes $zip "_rels/.rels" ($utf8NoBom.GetBytes($rels))

    # 3. All payload files
    foreach ($f in $files) {
        Add-ZipFile $zip $f.ZipPath $f.FilePath
    }

    $zip.Dispose()

    $finalSize = (Get-Item $outPath).Length
    $sizeKB = [math]::Round($finalSize / 1KB, 1)

    Write-Host "[package] VSIX created : $outPath" -ForegroundColor Green
    Write-Host "[package] Size         : $sizeKB KB"
    Write-Host "[package] Files        : $($files.Count) payload + 2 metadata" -ForegroundColor Green

} finally {
    if ($zip) { try { $zip.Dispose() } catch {} }
    Pop-Location
}
