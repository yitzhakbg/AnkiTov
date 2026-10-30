#!/bin/sh
# AnkiTov launcher — the "download → run" entry point (T-123 Phase 1 packaging).
#
# Starts the AnkiTov backend in production mode with:
#   * a per-user data directory (SQLite database lives there; no external DB)
#   * a generated JWT secret persisted on first run (mode 0600)
#   * HTTP on the configured port (default 5150)
#
# Usage:
#   ankitov-launch start    start in background, wait for readiness, open the
#                           browser (default; used by the macOS .app and the
#                           Linux .desktop entry)
#   ankitov-launch run      run in the foreground (Ctrl-C stops) — best from a
#                           terminal
#   ankitov-launch stop     stop a background instance
#   ankitov-launch status   report whether AnkiTov is running
#
# Environment overrides:
#   ANKITOV_DATA_DIR    data directory (default: per-OS application-data dir)
#   ANKITOV_PORT        HTTP port (default 5150)
#   ANKITOV_NO_BROWSER  set to 1 to skip opening the browser (used by smoke tests)
#
# The backend binary must sit next to this script (name: `backend`).

set -eu

APP_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
BACKEND="$APP_DIR/backend"

case "$(uname -s)" in
  Darwin) DATA_DEFAULT="$HOME/Library/Application Support/AnkiTov" ;;
  *)      DATA_DEFAULT="${XDG_DATA_HOME:-$HOME/.local/share}/ankitov" ;;
esac
DATA_DIR="${ANKITOV_DATA_DIR:-$DATA_DEFAULT}"
PORT="${ANKITOV_PORT:-5150}"
PID_FILE="$DATA_DIR/ankitov.pid"
LOG_FILE="$DATA_DIR/ankitov.log"
SECRET_FILE="$DATA_DIR/secret"
URL="http://localhost:$PORT"

die() { printf 'ankitov-launch: %s\n' "$1" >&2; exit 1; }

[ -x "$BACKEND" ] || die "backend binary not found next to the launcher ($BACKEND)"
mkdir -p "$DATA_DIR" 2>/dev/null || die "cannot create data directory $DATA_DIR"

# --- JWT secret: generate once, persist 0600, reuse on every later run --------
if [ ! -s "$SECRET_FILE" ]; then
  SECRET=$(
    if command -v openssl >/dev/null 2>&1; then
      openssl rand -hex 32
    else
      od -An -N32 -tx1 /dev/urandom | tr -d ' \n'
    fi
  ) || die "cannot generate JWT secret"
  [ -n "$SECRET" ] || die "cannot generate JWT secret"
  (umask 077 && printf '%s' "$SECRET" > "$SECRET_FILE") || die "cannot persist secret"
fi
SECRET=$(cat "$SECRET_FILE")
[ -n "$SECRET" ] || die "secret file $SECRET_FILE is empty"

# --- Seed deck packs ----------------------------------------------------------
# Seed packs (content/seed/packages/*.apkg) ship inside the artifact so the
# Solo Web Reviewer's import-seed endpoint works with zero external setup.
# The backend runs with CWD = $DATA_DIR, so its relative default cannot see
# them — export the artifact-relative path when the packs are present.
if [ -d "$APP_DIR/content/seed" ]; then
  ANKITOV_SEED_DIR="$APP_DIR/content/seed"
  export ANKITOV_SEED_DIR
fi

# --- Config bootstrap ---------------------------------------------------------
# Loco resolves its config folder relative to the process CWD (no env override
# in loco 0.11), and the launcher runs the server with CWD = $DATA_DIR so the
# SQLite URL can stay relative (spaces in the macOS data path would otherwise
# corrupt the sqlite:// URL). So: ship config/production.yaml inside the
# artifact next to this script and copy it into the data dir on first run.
# Copy-if-missing keeps user edits to the data-dir copy across upgrades.
CONFIG_SRC="$APP_DIR/config/production.yaml"
if [ -f "$CONFIG_SRC" ]; then
  mkdir -p "$DATA_DIR/config" 2>/dev/null || die "cannot create $DATA_DIR/config"
  if [ ! -f "$DATA_DIR/config/production.yaml" ]; then
    cp "$CONFIG_SRC" "$DATA_DIR/config/production.yaml" || die "cannot install config"
  fi
fi

# The database URL is relative to the data dir: we cd into it before starting,
# which sidesteps URL-encoding (macOS "Application Support") and Windows
# drive-letter quirks in sqlite:// URLs alike.
is_running() {
  [ -f "$PID_FILE" ] || return 1
  kill -0 "$(cat "$PID_FILE")" 2>/dev/null
}

wait_ready() {  # $1 = max seconds
  i=0
  while [ "$i" -lt "$1" ]; do
    if command -v curl >/dev/null 2>&1; then
      code=$(curl -s -o /dev/null -w '%{http_code}' "$URL/" 2>/dev/null || true)
      [ "$code" = "200" ] && return 0
    elif command -v nc >/dev/null 2>&1; then
      nc -z localhost "$PORT" 2>/dev/null && return 0
    fi
    sleep 1
    i=$((i + 1))
  done
  return 1
}

open_browser() {
  [ "${ANKITOV_NO_BROWSER:-0}" = "1" ] && return 0
  case "$(uname -s)" in
    Darwin) open "$URL" >/dev/null 2>&1 || true ;;
    *) command -v xdg-open >/dev/null 2>&1 && xdg-open "$URL" >/dev/null 2>&1 || true ;;
  esac
}

cmd_start() {
  if is_running; then
    printf 'AnkiTov is already running (pid %s) at %s\n' "$(cat "$PID_FILE")" "$URL"
    open_browser
    exit 0
  fi
  cd "$DATA_DIR"
  ANKITOV_JWT_SECRET="$SECRET" ANKITOV_PORT="$PORT" \
  DATABASE_URL="sqlite://ankitov.db?mode=rwc" \
    nohup "$BACKEND" start -e production >> "$LOG_FILE" 2>&1 &
  echo $! > "$PID_FILE"
  if wait_ready 30; then
    printf 'AnkiTov is running at %s\n  data: %s\n  log:  %s\n  stop: %s stop\n' \
      "$URL" "$DATA_DIR" "$LOG_FILE" "$0"
    open_browser
  else
    printf 'ankitov-launch: server did not become ready within 30s — log tail:\n' >&2
    tail -n 20 "$LOG_FILE" >&2 2>/dev/null || true
    exit 1
  fi
}

cmd_run() {
  printf 'AnkiTov starting in the foreground at %s (Ctrl-C stops)\n' "$URL"
  cd "$DATA_DIR"
  exec env ANKITOV_JWT_SECRET="$SECRET" ANKITOV_PORT="$PORT" \
    DATABASE_URL="sqlite://ankitov.db?mode=rwc" \
    "$BACKEND" start -e production
}

cmd_stop() {
  if ! is_running; then
    rm -f "$PID_FILE"
    printf 'AnkiTov is not running\n'
    exit 0
  fi
  pid=$(cat "$PID_FILE")
  kill "$pid" 2>/dev/null || true
  i=0
  while [ "$i" -lt 10 ] && kill -0 "$pid" 2>/dev/null; do
    sleep 1
    i=$((i + 1))
  done
  if kill -0 "$pid" 2>/dev/null; then
    kill -9 "$pid" 2>/dev/null || true
  fi
  rm -f "$PID_FILE"
  printf 'AnkiTov stopped\n'
}

cmd_status() {
  if is_running; then
    printf 'AnkiTov is running (pid %s) at %s\n' "$(cat "$PID_FILE")" "$URL"
  else
    printf 'AnkiTov is not running\n'
    exit 1
  fi
}

case "${1:-start}" in
  start)  cmd_start ;;
  run)    cmd_run ;;
  stop)   cmd_stop ;;
  status) cmd_status ;;
  *) die "unknown command '$1' (use: start | run | stop | status)" ;;
esac
