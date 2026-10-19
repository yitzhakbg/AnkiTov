#!/bin/sh
# Per-artifact smoke gate (T-123): boot a built backend binary cold and require
# the same functional probe used everywhere else in the repo —
#   GET /                      → 200 (dashboard)
#   POST /api/v1/auth/register → 200 + token
# before an artifact is allowed to ship.
#
# Usage: smoke.sh <backend-binary> [port] [config-yaml]
set -eu

BIN=${1:?usage: smoke.sh <backend-binary> [port] [config-yaml]}
PORT=${2:-5199}
[ -f "$BIN" ] || { echo "smoke: binary not found: $BIN" >&2; exit 1; }
# Resolve to an absolute path up front: the smoke run cd's into a temp dir
# (the server creates its DB relative to CWD), which would break a relative $BIN.
BIN=$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")

# Loco resolves config/ relative to CWD; stage it into the smoke dir. Default
# lookup: next to the binary, then the repo layout (backend/config/).
CONFIG=${3:-}
if [ -z "$CONFIG" ]; then
  for c in "$(dirname "$BIN")/config/production.yaml" backend/config/production.yaml; do
    [ -f "$c" ] && CONFIG=$c && break
  done
fi
[ -n "$CONFIG" ] && [ -f "$CONFIG" ] || { echo "smoke: no production.yaml found (pass it as arg 3)" >&2; exit 1; }
CONFIG=$(cd "$(dirname "$CONFIG")" && pwd)/$(basename "$CONFIG")
command -v curl >/dev/null 2>&1 || { echo "smoke: curl is required" >&2; exit 1; }

TMP=$(mktemp -d "${TMPDIR:-/tmp}/ankitov-smoke.XXXXXX")
PID=""
cleanup() {
  [ -n "$PID" ] && kill "$PID" 2>/dev/null || true
  rm -rf "$TMP"
}
trap cleanup EXIT

SECRET="smoke-$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')"
# Isolation: absolute DB URL + data dir pinned inside $TMP so no test
# residue can reach the user's default data dir regardless of CWD or
# backend data-dir defaults.
DB_URL="sqlite://$TMP/smoke.db?mode=rwc"
ANKITOV_DATA_DIR="$TMP/data"
mkdir -p "$ANKITOV_DATA_DIR"
cd "$TMP"
mkdir -p config
cp "$CONFIG" config/production.yaml
ANKITOV_JWT_SECRET="$SECRET" ANKITOV_PORT="$PORT" \
DATABASE_URL="$DB_URL" ANKITOV_DATA_DIR="$ANKITOV_DATA_DIR" \
  "$BIN" start -e production > server.log 2>&1 &
PID=$!

code=""
i=0
while [ "$i" -lt 30 ]; do
  code=$(curl -s -o /dev/null -w '%{http_code}' "http://localhost:$PORT/" 2>/dev/null || true)
  [ "$code" = "200" ] && break
  sleep 1
  i=$((i + 1))
done
if [ "$code" != "200" ]; then
  echo "SMOKE FAIL: GET / never returned 200 (last code: '${code:-none}')"
  tail -n 30 server.log 2>/dev/null || true
  exit 1
fi

BODY='{"username":"smoke-user","email":"smoke@example.test","password":"Sup3r-Secret-Passw0rd!","full_name":"Smoke Test","role":"student"}'
# First-run setup gate: a fresh DB must report setup_required:true (the
# empty-DB admin bootstrap wizard depends on it).
SETUP=$(curl -s "http://localhost:$PORT/api/v1/setup/status" 2>/dev/null || true)
if ! printf '%s' "$SETUP" | grep -q '"setup_required":true'; then
  echo "SMOKE FAIL: setup status on a fresh DB must be {\"setup_required\":true}: ${SETUP:-<empty>}"
  exit 1
fi
RESP=$(curl -s -X POST -H 'Content-Type: application/json' -d "$BODY" \
  "http://localhost:$PORT/api/v1/auth/register" 2>/dev/null || true)
if ! printf '%s' "$RESP" | grep -q '"token"'; then
  echo "SMOKE FAIL: register response missing token: ${RESP:-<empty>}"
  tail -n 30 server.log 2>/dev/null || true
  exit 1
fi

echo "SMOKE PASS: GET / 200 + register token minted ($BIN)"
