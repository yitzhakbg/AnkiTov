# Per-artifact smoke gate (T-123) — Windows variant of packaging/smoke.sh.
# Boots a built backend.exe cold and requires GET / == 200 and
# POST /api/v1/auth/register == 200 + token before the artifact ships.
#
# Usage: powershell -File packaging/smoke.ps1 -BinPath target/release/backend.exe [-Port 5199]
param(
  [Parameter(Mandatory = $true)][string]$BinPath,
  [int]$Port = 5199,
  [string]$ConfigPath = ''
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path $BinPath)) { throw "smoke: binary not found: $BinPath" }

# Absolutize BEFORE any Push-Location: relative -BinPath/-ConfigPath must resolve
# against the invocation CWD (repo root), not the smoke temp dir we cd into below.
$BinPath = (Resolve-Path $BinPath).Path
if ($ConfigPath) { $ConfigPath = (Resolve-Path $ConfigPath).Path }

# Loco resolves config/ relative to CWD; stage it into the smoke dir. Default
# lookup: next to the binary, then the repo layout (backend/config/).
if (-not $ConfigPath) {
  foreach ($c in @((Join-Path (Split-Path $BinPath) 'config/production.yaml'), 'backend/config/production.yaml')) {
    if (Test-Path $c) { $ConfigPath = $c; break }
  }
}
if (-not $ConfigPath -or -not (Test-Path $ConfigPath)) { throw "smoke: no production.yaml found (pass -ConfigPath)" }

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("ankitov-smoke-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
New-Item -ItemType Directory -Path (Join-Path $tmp 'config') | Out-Null
Copy-Item $ConfigPath (Join-Path $tmp 'config/production.yaml')
$prevPwd = Get-Location
Push-Location $tmp

$prevSecret = $env:ANKITOV_JWT_SECRET
$prevPort = $env:ANKITOV_PORT
$prevDb = $env:DATABASE_URL
$prevDataDir = $env:ANKITOV_DATA_DIR
$env:ANKITOV_JWT_SECRET = "smoke-" + [guid]::NewGuid().ToString('N')
$env:ANKITOV_PORT = "$Port"
# Isolation: DB resolves against $tmp (Push-Location above); data dir
# pinned inside $tmp so no test residue can reach the user's default
# data dir.
$env:DATABASE_URL = "sqlite://smoke.db?mode=rwc"
$env:ANKITOV_DATA_DIR = Join-Path $tmp 'data'
New-Item -ItemType Directory -Path $env:ANKITOV_DATA_DIR | Out-Null

$p = $null
try {
  $p = Start-Process -FilePath (Resolve-Path $BinPath) `
    -ArgumentList 'start', '-e', 'production' `
    -PassThru -NoNewWindow `
    -RedirectStandardOutput (Join-Path $tmp 'server.out.log') `
    -RedirectStandardError (Join-Path $tmp 'server.err.log')

  $ok = $false
  foreach ($i in 1..30) {
    try {
      $r = Invoke-WebRequest -UseBasicParsing -Uri "http://localhost:$Port/" -TimeoutSec 2
      if ($r.StatusCode -eq 200) { $ok = $true; break }
    } catch { Start-Sleep -Seconds 1 }
  }
  if (-not $ok) {
    Write-Host '--- server.err.log (tail) ---'
    Get-Content (Join-Path $tmp 'server.err.log') -Tail 30 -ErrorAction SilentlyContinue
    # Loco/tracing logs to STDOUT — smoke.sh (bash) tails a combined server.log,
    # so the Windows variant must dump both halves or boot errors stay invisible.
    Write-Host '--- server.out.log (tail) ---'
    Get-Content (Join-Path $tmp 'server.out.log') -Tail 40 -ErrorAction SilentlyContinue
    if ($p) {
      if ($p.HasExited) { Write-Host "server process EXITED, code: $($p.ExitCode)" }
      else { Write-Host "server process still running (pid $($p.Id))" }
    }
    throw "SMOKE FAIL: GET / never returned 200"
  }

  $body = '{"username":"smoke-user","email":"smoke@example.test","password":"Sup3r-Secret-Passw0rd!","full_name":"Smoke Test","role":"student"}'
  # First-run setup gate: a fresh DB must report setup_required:true (the
  # empty-DB admin bootstrap wizard depends on it).
  $setup = Invoke-RestMethod -Method Get -Uri "http://localhost:$Port/api/v1/setup/status" -TimeoutSec 5
  if (-not $setup.setup_required) {
    throw "SMOKE FAIL: setup status on a fresh DB must be setup_required:true (got: $($setup | ConvertTo-Json -Compress))"
  }
  $resp = Invoke-RestMethod -Method Post -Uri "http://localhost:$Port/api/v1/auth/register" `
    -ContentType 'application/json' -Body $body
  if (-not $resp.token) { throw "SMOKE FAIL: register response missing token" }

  Write-Host "SMOKE PASS: GET / 200 + register token minted ($BinPath)"
} finally {
  if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
  Pop-Location
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
  $env:ANKITOV_JWT_SECRET = $prevSecret
  $env:ANKITOV_PORT = $prevPort
  $env:DATABASE_URL = $prevDb
  $env:ANKITOV_DATA_DIR = $prevDataDir
}
