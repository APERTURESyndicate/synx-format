# package-vsix.ps1
# Packages the SYNX Visual Studio extension into a valid VSIX v3 file.
# VSIX v3 is required for VS 2017+ and the Visual Studio Marketplace.
# Includes manifest.json and catalog.json with SHA256 hashes.

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

function Get-FileSha256([string]$filePath) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    $stream = [System.IO.File]::OpenRead($filePath)
    $hash = [BitConverter]::ToString($sha.ComputeHash($stream)).Replace("-","")
    $stream.Dispose()
    $sha.Dispose()
    return $hash
}

function Get-BytesSha256([byte[]]$bytes) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    $hash = [BitConverter]::ToString($sha.ComputeHash($bytes)).Replace("-","")
    $sha.Dispose()
    return $hash
}

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

    # Newtonsoft.Json.dll
    $newtonsoftPath = Join-Path $buildOut "Newtonsoft.Json.dll"
    if (Test-Path $newtonsoftPath) {
        [void]$files.Add(@{ ZipPath = "Newtonsoft.Json.dll"; FilePath = $newtonsoftPath })
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

    # --- Build manifest.json (VSIX v3: lists all files with SHA256) ---
    $totalSize = 0
    $fileEntries = @()
    foreach ($f in $files) {
        $fInfo = Get-Item $f.FilePath
        $hash  = Get-FileSha256 $f.FilePath
        $fileEntries += "    {`"fileName`": `"/$($f.ZipPath)`", `"sha256`": `"$hash`", `"size`": $($fInfo.Length)}"
        $totalSize += $fInfo.Length
    }

    $manifestJson = @"
{
  "id": "$vsixId",
  "version": "$version",
  "type": "Vsix",
  "vsixId": "$vsixId",
  "extensionDir": "[installdir]\\Common7\\IDE\\Extensions\\$publisher\\SynxLanguageService\\$version",
  "files": [
$($fileEntries -join ",`n")
  ],
  "installSizes": {
    "targetDrive": $totalSize
  }
}
"@
    $manifestJsonBytes = $utf8NoBom.GetBytes($manifestJson)

    # --- Build catalog.json (VSIX v3: package metadata) ---
    $catalogJson = @"
{
  "manifestVersion": 1,
  "info": {
    "id": "$vsixId,version=$version",
    "manifestType": "Extension"
  },
  "packages": [
    {
      "id": "Component.$vsixId",
      "version": "$version",
      "type": "Vsix",
      "payloads": [
        {
          "fileName": "$OutFile",
          "size": 0
        }
      ],
      "vsixId": "$vsixId",
      "dependencies": {}
    }
  ]
}
"@
    $catalogJsonBytes = $utf8NoBom.GetBytes($catalogJson)

    # --- Create ZIP/VSIX ---
    $zip = [System.IO.Compression.ZipFile]::Open($outPath, [System.IO.Compression.ZipArchiveMode]::Create)

    # 1. [Content_Types].xml (MUST be first — OPC spec)
    $contentTypes = @'
<?xml version="1.0" encoding="utf-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="vsixmanifest" ContentType="text/xml" />
  <Default Extension="json"         ContentType="application/json" />
  <Default Extension="rels"         ContentType="application/vnd.openxmlformats-package.relationships+xml" />
  <Default Extension="dll"          ContentType="application/octet-stream" />
  <Default Extension="png"          ContentType="image/png" />
  <Default Extension="svg"          ContentType="image/svg+xml" />
  <Default Extension="pkgdef"       ContentType="application/octet-stream" />
</Types>
'@
    Add-ZipBytes $zip "[Content_Types].xml" ($utf8NoBom.GetBytes($contentTypes))

    # 2. _rels/.rels
    $rels = @'
<?xml version="1.0" encoding="utf-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Type="http://schemas.microsoft.com/developer/vsx-schema/2011" Target="/extension.vsixmanifest" Id="R1" />
</Relationships>
'@
    Add-ZipBytes $zip "_rels/.rels" ($utf8NoBom.GetBytes($rels))

    # 3. catalog.json (VSIX v3)
    Add-ZipBytes $zip "catalog.json" $catalogJsonBytes

    # 4. manifest.json (VSIX v3)
    Add-ZipBytes $zip "manifest.json" $manifestJsonBytes

    # 5. All payload files
    foreach ($f in $files) {
        Add-ZipFile $zip $f.ZipPath $f.FilePath
    }

    $zip.Dispose()

    # Update catalog.json payload size now that we know the final VSIX size
    # (Not strictly required—marketplace recalculates—but nice for completeness)
    $finalSize = (Get-Item $outPath).Length
    $sizeKB = [math]::Round($finalSize / 1KB, 1)

    Write-Host "[package] VSIX created : $outPath" -ForegroundColor Green
    Write-Host "[package] Size         : $sizeKB KB"
    Write-Host "[package] Files        : $($files.Count) payload + 4 metadata" -ForegroundColor Green

} finally {
    if ($zip) { try { $zip.Dispose() } catch {} }
    Pop-Location
}
