# altkey transparent intercept — SETUP. Run as Administrator.
#
# Installs the local CA into the machine trust store and points the AI provider
# API domains at 127.0.0.1, so apps that hardcode api.openai.com / api.anthropic.com
# transparently hit altkey. PERSONAL machine only. Run teardown.ps1 to undo.

#Requires -RunAsAdministrator
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$hosts = "$env:SystemRoot\System32\drivers\etc\hosts"
$domains = @("api.openai.com", "chatgpt.com", "chat.openai.com", "api.anthropic.com")
$marker = "# --- altkey transparent ---"

if (-not (Test-Path "$here\ca.crt")) { throw "ca.crt not found — run: python gen_certs.py first" }

Write-Host "1/2 Installing local CA into LocalMachine\Root ..." -ForegroundColor Cyan
certutil -addstore -f Root "$here\ca.crt" | Out-Null
Write-Host "    CA installed." -ForegroundColor Green

Write-Host "2/2 Adding hosts entries ..." -ForegroundColor Cyan
$content = Get-Content $hosts -Raw -ErrorAction SilentlyContinue
if ($content -notmatch [regex]::Escape($marker)) {
    Add-Content $hosts "`r`n$marker"
    foreach ($d in $domains) { Add-Content $hosts "127.0.0.1`t$d" }
    Add-Content $hosts "# --- end altkey ---"
    Write-Host "    hosts entries added." -ForegroundColor Green
} else {
    Write-Host "    hosts entries already present." -ForegroundColor Yellow
}

ipconfig /flushdns | Out-Null
Write-Host "`nDone. Now launch altkey in transparent mode (separate terminal):" -ForegroundColor Cyan
Write-Host '  cd C:\Users\gsent\Desktop\altkey\local' -ForegroundColor White
Write-Host '  .\transparent\run-transparent.ps1' -ForegroundColor White
Write-Host "`nThen any app hitting api.openai.com / api.anthropic.com hits altkey." -ForegroundColor White
Write-Host "Undo everything later with: .\transparent\teardown.ps1 (as Admin)" -ForegroundColor DarkGray
