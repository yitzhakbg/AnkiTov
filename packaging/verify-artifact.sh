#!/usr/bin/env bash
# verify-artifact.sh — post-build artifact gate.
#
# WHY (build-procedure hardening, 2026-11-22 campaign):
#   * run 45035830408: dpkg-deb rejected "# comments" inside the control
#     stanza → BOTH linux jobs failed AFTER the build+smoke passed.
#   * mirror-built .deb shipped without the FSL LICENSE (oss-templates/
#     absent in the mirror) — found only by manual inspection.
# This gate runs AFTER assemble in CI (and locally) so an artifact that
# would ship incomplete/deformed fails HERE, before the Release job.
#
# Usage: verify-artifact.sh <path/to/ankitov-<v>-linux-<arch>.deb>
set -euo pipefail

DEB="${1:?usage: verify-artifact.sh <deb>}"
# resolve BEFORE cd'ing anywhere (ar is run from a temp cwd)
DEB_ABS="$(cd "$(dirname "$DEB")" && pwd)/$(basename "$DEB")"
[ -f "$DEB_ABS" ] || { echo "FAIL: no such file: $DEB_ABS" >&2; exit 1; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

FAIL=0
fail() { echo "FAIL: $1" >&2; FAIL=1; }
ok()   { echo "ok:   $1"; }

# ── extract (deb = ar{control.tar.zst,data.tar.zst}) ────────────────────────
( cd "$TMP" && ar x "$DEB_ABS" )
mkdir -p "$TMP/c" "$TMP/d"
tar --zstd -xf "$TMP/control.tar.zst" -C "$TMP/c" 2>/dev/null || tar -xf "$TMP/control.tar.zst" -C "$TMP/c"
tar --zstd -xf "$TMP/data.tar.zst"   -C "$TMP/d" 2>/dev/null || tar -xf "$TMP/data.tar.zst" -C "$TMP/d"

# ── control: fields + no comments inside the stanza (dpkg-deb rejects them) ─
CTL="$TMP/c/control"
[ -f "$CTL" ] || fail "control file missing"
if [ -f "$CTL" ]; then
  for field in Package Version Architecture Depends; do
    grep -q "^${field}:" "$CTL" && ok "control ${field}" || fail "control missing ${field}"
  done
  if grep -q "^[[:space:]]*#" "$CTL"; then
    fail "control stanza contains comment lines (dpkg-deb parses stanzas strictly — move comments to the assembling shell script)"
  else
    ok "control has no comment lines"
  fi
  grep -q "^Depends:" "$CTL" && ok "control declares Depends (static musl binary needs ca-certificates at minimum)" || true
fi

# ── payload completeness ────────────────────────────────────────────────────
check() { [ -e "$TMP/d/$1" ] && ok "payload $1" || fail "payload missing $1"; }
check "opt/ankitov/backend"
check "opt/ankitov/ankitov-launch"
check "opt/ankitov/config/production.yaml"
check "usr/share/doc/ankitov/LICENSES/THIRD-PARTY-NOTICES.md"
check "usr/share/doc/ankitov/LICENSE"

SEEDS=$(find "$TMP/d/opt/ankitov/content/seed/packages" -name "*.apkg" 2>/dev/null | wc -l)
[ "$SEEDS" -ge 1 ] && ok "payload seed packs present (${SEEDS} apkg)" || fail "payload has no seed packs under /opt/ankitov/content/seed/packages"

# ── backend is static (musl) — no host library floor ───────────────────────
BIN="$TMP/d/opt/ankitov/backend"
if [ -f "$BIN" ]; then
  if file "$BIN" | grep -qE "static-pie|statically linked|static"; then
    ok "backend is static (musl)"
  else
    fail "backend is NOT static — dynamic linking breaks the no-apt-floor promise"
  fi
fi

# ── verdict ────────────────────────────────────────────────────────────────
if [ "$FAIL" -eq 0 ]; then
  echo "PASS: $(basename "$DEB")"
else
  echo "FAIL: $(basename "$DEB") — see lines above" >&2
  exit 1
fi
