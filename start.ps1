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

$port = 8788
if (Test-Path (Join-Path $here "config.toml")) {
    $m = Select-String -Path (Join-Path $here "config.toml") -Pattern '^\s*port\s*=\s*(\d+)' | Select-Object -First 1
    if ($m) { $port = [int]$m.Matches[0].Groups[1].Value }
}
$base = "http://127.0.0.1:$port"

function Test-Router { try { Invoke-WebRequest $base -TimeoutSec 1 -ErrorAction Stop | Out-Null; return $true } catch { return ($null -ne $_.Exception.Response) } }

$startedOurs = $false
if (Test-Router) {
    Write-Host "Router already running on :$port (reusing existing instance)." -ForegroundColor Cyan
} else {
    $startedOurs = $true
    $router = Start-Process -FilePath $exe -WorkingDirectory $here -PassThru -RedirectStandardOutput (Join-Path $here "router.log") -RedirectStandardError (Join-Path $here "router.log")
    $deadline = (Get-Date).AddSeconds(5)
    while ((Get-Date) -lt $deadline -and -not (Test-Router)) { Start-Sleep -Milliseconds 200 }
    Write-Host "Started router on :$port." -ForegroundColor Green
}

try {
    $env:ANTHROPIC_BASE_URL  = $base
    $env:ANTHROPIC_AUTH_TOKEN = "dummy-local-token"
    Write-Host "Launching Claude Code (default tier = deepseek-v4-pro; /model opus for real Opus)." -ForegroundColor Green
    claude @args
}
finally {
    if ($startedOurs -and $router -and -not $router.HasExited) {
        Stop-Process -Id $router.Id -ErrorAction SilentlyContinue
        Write-Host "Router stopped." -ForegroundColor DarkGray
    }
}
