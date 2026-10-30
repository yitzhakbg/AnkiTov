@echo off
rem AnkiTov launcher (Windows) — the "download -> run" entry point (T-123 Phase 1).
rem
rem Default (double-click): runs the backend in THIS console window (foreground);
rem Ctrl-C or closing the window stops it. A browser tab opens once the server
rem is ready.
rem
rem Subcommands:
rem   start-ankitov.bat            foreground run + browser (default)
rem   start-ankitov.bat stop       stop a background instance (see below)
rem   start-ankitov.bat status     report whether AnkiTov is running
rem
rem Environment overrides:
rem   ANKITOV_DATA_DIR   data directory (default %%LOCALAPPDATA%%\AnkiTov)
rem   ANKITOV_PORT       HTTP port (default 5150)
rem
rem The backend binary must sit next to this script (name: backend.exe).
setlocal enabledelayedexpansion

set "APP_DIR=%~dp0"
set "BACKEND=%APP_DIR%backend.exe"
if not exist "%BACKEND%" (
  echo ankitov-launch: backend.exe not found next to the launcher ^(%BACKEND%^)
  exit /b 1
)

set "DATA_DIR=%ANKITOV_DATA_DIR%"
if "%DATA_DIR%"=="" set "DATA_DIR=%LOCALAPPDATA%\AnkiTov"
if not exist "%DATA_DIR%" mkdir "%DATA_DIR%"

set "PORT=%ANKITOV_PORT%"
if "%PORT%"=="" set "PORT=5150"

set "PID_FILE=%DATA_DIR%\ankitov.pid"
set "LOG_FILE=%DATA_DIR%\ankitov.log"
set "SECRET_FILE=%DATA_DIR%\secret"
set "URL=http://localhost:%PORT%"

rem --- JWT secret: generate once via PowerShell CSPRNG, persist, reuse ---------
if exist "%SECRET_FILE%" (
  set /p SECRET=<"%SECRET_FILE%"
) else (
  for /f "usebackq delims=" %%A in (`powershell -NoProfile -Command "$b = New-Object byte[] 32; (New-Object System.Security.Cryptography.RNGCryptoServiceProvider).GetBytes($b); [BitConverter]::ToString($b).Replace('-','').ToLower()"`) do set "SECRET=%%A"
  if not defined SECRET (
    echo ankitov-launch: cannot generate JWT secret
    exit /b 1
  )
  > "%SECRET_FILE%" echo(%SECRET%
)
if not defined SECRET (
  echo ankitov-launch: secret file %SECRET_FILE% is empty
  exit /b 1
)

if /i "%~1"=="stop"   goto :stop
if /i "%~1"=="status" goto :status

rem --- config bootstrap ---------------------------------------------------------
rem Loco resolves config/ relative to the process CWD, and we run with CWD =
rem the data dir (relative sqlite:// URL). Ship config\production.yaml next to
rem this script; copy it into the data dir on first run (copy-if-missing keeps
rem user edits across upgrades).
if exist "%~dp0config\production.yaml" (
  if not exist "%DATA_DIR%\config" mkdir "%DATA_DIR%\config"
  if not exist "%DATA_DIR%\config\production.yaml" copy /y "%~dp0config\production.yaml" "%DATA_DIR%\config\production.yaml" >nul
)

rem --- run (foreground) --------------------------------------------------------
rem Database URL is relative to the data dir: we cd into it first, which avoids
rem Windows drive-letter quirks in sqlite:// URLs.
cd /d "%DATA_DIR%"
set "ANKITOV_JWT_SECRET=%SECRET%"
set "ANKITOV_PORT=%PORT%"
set "DATABASE_URL=sqlite://ankitov.db?mode=rwc"
rem Seed packs ship inside the artifact; CWD is the data dir, so point the
rem backend at the artifact-relative copy when present.
if exist "%APP_DIR%content\seed\" set "ANKITOV_SEED_DIR=%APP_DIR%content\seed"

echo AnkiTov starting at %URL% (data: %DATA_DIR%; close this window or Ctrl-C to stop)
rem Open the browser once the server has had a moment to boot.
start "" cmd /c "timeout /t 3 /nobreak >nul & start "" http://localhost:%PORT%"
"%BACKEND%" start -e production
goto :eof

:stop
if not exist "%PID_FILE%" (
  echo AnkiTov is not running
  exit /b 0
)
set /p PID=<"%PID_FILE%"
taskkill /PID %PID% /F >nul 2>&1
del "%PID_FILE%" 2>nul
echo AnkiTov stopped
goto :eof

:status
if not exist "%PID_FILE%" (
  echo AnkiTov is not running
  exit /b 1
)
set /p PID=<"%PID_FILE%"
tasklist /FI "PID eq %PID%" 2>nul | find "%PID%" >nul
if errorlevel 1 (
  echo AnkiTov is not running
  del "%PID_FILE%" 2>nul
  exit /b 1
)
echo AnkiTov is running ^(pid %PID%^) at %URL%
