#!/usr/bin/env bash
# smoke-deb.sh — T-207 one-step commissioning gate.
#
# Proves the .deb commissions itself on a pristine, bare, out-of-the-box
# Debian 12: install → service active → first admin claimable → authed call
# → reboot survival. Wired into release.yml after verify-artifact.sh.
#
# Usage: bash packaging/smoke-deb.sh dist/ankitov-<v>-linux-<arch>.deb
#
# Requires: docker (the container runs systemd as PID 1 via --privileged).
# The .deb arch must match the docker image arch (debian:12 is multi-arch;
# the runner's native arch is used — x86_64 job → amd64 deb, arm job → arm64).
set -euo pipefail

DEB="${1:?usage: smoke-deb.sh <ankitov-*.deb>}"
[ -f "$DEB" ] || { echo "smoke-deb: not a file: $DEB" >&2; exit 1; }
DEB="$(cd "$(dirname "$DEB")" && pwd)/$(basename "$DEB")"
command -v docker >/dev/null 2>&1 || { echo "smoke-deb: docker not on PATH" >&2; exit 2; }

# Derive the docker platform from the deb's arch tag so the gate works on
# both x86_64 and arm64 runners (ubuntu-24.04-arm is native arm64).
case "$DEB" in
    *x86_64*)  PLAT=linux/amd64;  DARCH=amd64 ;;
    *aarch64*) PLAT=linux/arm64;  DARCH=arm64 ;;
    *) echo "smoke-deb: cannot infer arch from $DEB (want ...-linux-x86_64.deb or ...-linux-aarch64.deb)" >&2; exit 1 ;;
esac
IMAGE="debian12-systemd-smoke:$DARCH"
NAME="ankitov-smoke-$$"
BASE="http://127.0.0.1:5150/api/v1"
step() { printf '\n=== smoke-deb: %s\n' "$1"; }
fail() { printf 'smoke-deb: FAILED at "%s"\n' "$1" >&2; docker logs "$NAME" 2>/dev/null | tail -30 >&2 || true; exit 1; }
cleanup() { docker rm -f "$NAME" >/dev/null 2>&1 || true; }
trap cleanup EXIT

step "prepare systemd-enabled debian:12 image (base image ships no systemd)"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    printf 'FROM debian:12\nRUN apt-get update -qq && apt-get install -y -qq --no-install-recommends systemd && apt-get clean\n' \
        | docker build --platform "$PLAT" -t "$IMAGE" - >/dev/null
fi

step "launch pristine debian:12 with systemd as PID 1"
docker run -d --name "$NAME" --privileged --platform "$PLAT" "$IMAGE" /lib/systemd/systemd >/dev/null
for _ in $(seq 1 60); do
    state="$(docker exec "$NAME" systemctl is-system-running 2>/dev/null || true)"
    [ -n "$state" ] && break
    sleep 1
done
[ -n "$state" ] || fail "systemd never became ready"
echo "systemd state: $state"

step "install the deb (apt resolves Depends: ca-certificates, openssl)"
docker cp "$DEB" "$NAME:/tmp/ankitov.deb"
docker exec "$NAME" bash -c 'apt-get update -qq >/dev/null && apt-get install -y -qq curl /tmp/ankitov.deb >/dev/null' \
    || fail "apt install"

step "service is active"
[ "$(docker exec "$NAME" systemctl is-active ankitov)" = "active" ] || fail "service not active"
docker exec "$NAME" systemctl is-enabled ankitov >/dev/null || fail "service not enabled"

step "setup/status reachable (setup_required=true on the empty DB)"
status="$(docker exec "$NAME" curl -fsS "$BASE/setup/status")" || fail "setup/status"
echo "$status"
echo "$status" | grep -q '"setup_required":true' || fail "setup_required not true"

step "POST /api/v1/setup claims the first admin (returns JWT)"
resp="$(docker exec "$NAME" curl -fsS -X POST "$BASE/setup" \
    -H 'Content-Type: application/json' \
    -d '{"username":"smokeadmin","email":"smoke@ankitov.test","password":"smoke-password-1","full_name":"Smoke Admin"}')" \
    || fail "POST /setup"
echo "$resp" | head -c 200; echo
JWT="$(printf '%s' "$resp" | sed -n 's/.*"token":"\([^"]*\)".*/\1/p')"
[ -n "$JWT" ] || fail "no token in setup response"

step "authed management call returns 200"
code="$(docker exec "$NAME" curl -s -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer $JWT" "$BASE/management/classes")" || fail "authed call"
[ "$code" = "200" ] || fail "authed management call returned $code (want 200)"
echo "management/classes -> $code"

step "second setup attempt is rejected (admin already exists)"
code="$(docker exec "$NAME" curl -s -o /dev/null -w '%{http_code}' -X POST "$BASE/setup" \
    -H 'Content-Type: application/json' \
    -d '{"username":"smokeadmin2","email":"smoke2@ankitov.test","password":"smoke-password-2"}')" || true
echo "second POST /setup -> $code (expect 4xx)"
case "$code" in 4*) ;; *) fail "second setup not rejected (got $code)" ;; esac

step "reboot survival: restart the container, service must come back"
docker restart "$NAME" >/dev/null
for _ in $(seq 1 60); do
    st="$(docker exec "$NAME" systemctl is-active ankitov 2>/dev/null || true)"
    [ "$st" = "active" ] && break
    sleep 1
done
[ "$st" = "active" ] || fail "service not active after container restart"
status="$(docker exec "$NAME" curl -fsS "$BASE/setup/status")" || fail "setup/status after restart"
echo "$status"
echo "$status" | grep -q '"setup_required":false' || fail "admin did not survive restart"

step "secret env survived: still 0600, still the same secret"
s1="$(docker exec "$NAME" sha256sum /etc/ankitov/ankitov.env | cut -d' ' -f1)"
docker exec "$NAME" stat -c '%a' /etc/ankitov/ankitov.env | grep -q '^600$' || fail "env file not 0600"
[ -n "$s1" ] || fail "env file missing"

printf '\n=== smoke-deb: ALL GREEN — install → active → claimable → reboot survival\n'
