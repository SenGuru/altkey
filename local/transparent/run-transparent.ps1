# Launch altkey in transparent mode: HTTPS on 443 with the local cert, accepting
# any key. Run from the local/ directory. Needs Administrator to bind port 443.
#Requires -RunAsAdministrator
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$env:ALTKEY_TRANSPARENT = "1"
$env:ALTKEY_TLS_CERT = "$here\server.crt"
$env:ALTKEY_TLS_KEY = "$here\server.key"
$env:ALTKEY_PORT = "443"
$env:ALTKEY_HOST = "127.0.0.1"
Write-Host "altkey transparent mode → https://127.0.0.1:443 (intercepting api.openai.com, api.anthropic.com)" -ForegroundColor Cyan
python -m app.main
