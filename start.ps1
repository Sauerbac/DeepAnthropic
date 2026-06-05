# Launches cc-router, then Claude Code pointed at it. Stops the router on exit.
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$exe  = Join-Path $here "target\release\cc-router.exe"

if (-not (Test-Path (Join-Path $here "config.toml"))) {
    throw "config.toml missing. Copy config.toml.example to config.toml and fill in your tokens."
}
if (-not (Test-Path $exe)) {
    Write-Host "Building release binary..." -ForegroundColor Cyan
    Push-Location $here; cargo build --release; Pop-Location
}

# The router reads config.toml from its working directory, so start it in $here.
$router = Start-Process -FilePath $exe -WorkingDirectory $here -PassThru

try {
    # Give the listener a moment to bind.
    $deadline = (Get-Date).AddSeconds(5)
    while ((Get-Date) -lt $deadline) {
        try { Invoke-WebRequest "http://127.0.0.1:8788" -TimeoutSec 1 -ErrorAction Stop | Out-Null; break }
        catch { if ($_.Exception.Response) { break } ; Start-Sleep -Milliseconds 200 }
    }

    $env:ANTHROPIC_BASE_URL  = "http://127.0.0.1:8788"
    $env:ANTHROPIC_AUTH_TOKEN = "dummy-local-token"  # real upstream creds live in the router
    Write-Host "Router up. Launching Claude Code (default tier = DeepSeek v4-pro; /model opus for real Opus)." -ForegroundColor Green
    claude @args
}
finally {
    if ($router -and -not $router.HasExited) {
        Stop-Process -Id $router.Id -ErrorAction SilentlyContinue
        Write-Host "Router stopped." -ForegroundColor DarkGray
    }
}
