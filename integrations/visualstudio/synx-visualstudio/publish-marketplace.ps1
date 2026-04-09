# publish-marketplace.ps1
param(
    [string]$Pat = "",
    [string]$VsixPath = "$PSScriptRoot\SynxLanguageService.vsix",
    [string]$Publisher = "APERTURESyndicate"
)

if (-not $Pat) {
    $Pat = Read-Host "Enter your Personal Access Token (Marketplace > Manage scope)"
}

if (-not (Test-Path $VsixPath)) {
    Write-Error "VSIX not found: $VsixPath"
    exit 1
}

Add-Type -AssemblyName System.Net.Http

$authValue = [Convert]::ToBase64String([System.Text.Encoding]::ASCII.GetBytes(":$Pat"))
$vsixBytes  = [System.IO.File]::ReadAllBytes($VsixPath)

Write-Host "[publish] VSIX     : $([math]::Round($vsixBytes.Length / 1KB, 1)) KB"
Write-Host "[publish] Publisher: $Publisher"

$client = New-Object System.Net.Http.HttpClient
$client.DefaultRequestHeaders.Authorization = `
    New-Object System.Net.Http.Headers.AuthenticationHeaderValue("Basic", $authValue)
$client.DefaultRequestHeaders.Add("X-TFS-FedAuthRedirect", "Suppress")
$client.DefaultRequestHeaders.Add("Accept", "application/json;api-version=6.0-preview")

# --- Step 1: verify publisher ---
Write-Host ""
Write-Host "=== Step 1: Publisher check ===" -ForegroundColor Cyan
$pubUrl = "https://marketplace.visualstudio.com/_apis/gallery/publishers/$Publisher`?api-version=6.0-preview"
$r = $client.GetAsync($pubUrl).Result
$body = $r.Content.ReadAsStringAsync().Result
Write-Host "  Status : $([int]$r.StatusCode)"
Write-Host "  Body   : $body"

# --- Step 2: upload VSIX ---
Write-Host ""
Write-Host "=== Step 2: Upload VSIX ===" -ForegroundColor Cyan
$uploadUrl = "https://marketplace.visualstudio.com/_apis/gallery/publishers/$Publisher/drafts?api-version=6.0-preview"
Write-Host "  URL    : $uploadUrl"

$content = [System.Net.Http.ByteArrayContent]::new($vsixBytes)
$content.Headers.ContentType = [System.Net.Http.Headers.MediaTypeHeaderValue]::new("application/octet-stream")

$r2 = $client.PostAsync($uploadUrl, $content).Result
$body2 = $r2.Content.ReadAsStringAsync().Result
$code2 = [int]$r2.StatusCode

if ($r2.IsSuccessStatusCode) {
    Write-Host "  Status : $code2 OK" -ForegroundColor Green
    Write-Host "  Body   : $body2"
} else {
    Write-Host "  Status : $code2" -ForegroundColor Red
    Write-Host "  Body   : $body2" -ForegroundColor Red
}

$client.Dispose()
