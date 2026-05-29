# altkey transparent intercept — TEARDOWN. Run as Administrator.
# Removes the CA from the trust store and the hosts entries. Fully reverses setup.ps1.

#Requires -RunAsAdministrator
$ErrorActionPreference = "Continue"
$hosts = "$env:SystemRoot\System32\drivers\etc\hosts"
$marker = "# --- altkey transparent ---"
$endmarker = "# --- end altkey ---"

Write-Host "1/2 Removing local CA from trust store ..." -ForegroundColor Cyan
# Remove by the CA's common name.
certutil -delstore Root "altkey local CA" | Out-Null
Write-Host "    CA removed (if it was present)." -ForegroundColor Green

Write-Host "2/2 Removing hosts entries ..." -ForegroundColor Cyan
if (Test-Path $hosts) {
    $lines = Get-Content $hosts
    $out = @(); $skip = $false
    foreach ($l in $lines) {
        if ($l -eq $marker) { $skip = $true; continue }
        if ($l -eq $endmarker) { $skip = $false; continue }
        if (-not $skip) { $out += $l }
    }
    Set-Content $hosts $out -Encoding ASCII
    Write-Host "    hosts entries removed." -ForegroundColor Green
}
ipconfig /flushdns | Out-Null
Write-Host "`nTransparent intercept fully removed. api.openai.com etc. go to the real servers again." -ForegroundColor White
