#!/bin/sh
# AnkiTov one-command installer (Linux, x86_64).
#
#   curl -fsSL https://github.com/yitzhakbg/AnkiTov/releases/download/v0.6.0/install-ankitov.sh | sh
#
# Installs, in order: flatpak (if missing) → flathub remote → freedesktop
# runtime → the AnkiTov bundle. Then:
#
#   flatpak run org.ankitov.AnkiTov classroom     # students on this network join via the printed URL
#   flatpak run org.ankitov.AnkiTov               # solo (teacher only)
#   flatpak run org.ankitov.AnkiTov stop          # stop a background instance
set -eu

REPO="yitzhakbg/AnkiTov"
APP_ID="org.ankitov.AnkiTov"
RUNTIME_VER="25.08"

say() { printf '\n== %s\n' "$1"; }
die() { printf 'install-ankitov: %s\n' "$1" >&2; exit 1; }

[ "$(uname -m)" = "x86_64" ] || die "only x86_64 is packaged right now (aarch64 bundle pending)."

# --- 1. flatpak itself ----------------------------------------------------------
if ! command -v flatpak >/dev/null 2>&1; then
  say "Installing flatpak (uses your package manager; may ask for your password)..."
  SUDO=""; [ "$(id -u)" = 0 ] || SUDO="sudo"
  if command -v apt-get >/dev/null 2>&1; then $SUDO apt-get update && $SUDO apt-get install -y flatpak
  elif command -v dnf >/dev/null 2>&1; then $SUDO dnf install -y flatpak
  elif command -v pacman >/dev/null 2>&1; then $SUDO pacman -Sy --noconfirm flatpak
  elif command -v zypper >/dev/null 2>&1; then $SUDO zypper install -y flatpak
  elif command -v eopkg >/dev/null 2>&1; then $SUDO eopkg install -y flatpak
  else die "flatpak is not installed and no known package manager was found (apt/dnf/pacman/zypper/eopkg). Install flatpak manually, then re-run this script."
  fi
fi

# --- 2. flathub remote (user installation) --------------------------------------
say "Adding the flathub remote (user)..."
flatpak remote-add --if-not-exists --user flathub https://dl.flathub.org/repo/flathub.flatpakrepo || true

# --- 3. runtime + GL extension ---------------------------------------------------
say "Installing the freedesktop runtime ${RUNTIME_VER} (a few hundred MB, one time)..."
flatpak install --user --noninteractive flathub "org.freedesktop.Platform//${RUNTIME_VER}"
flatpak install --user --noninteractive flathub "org.freedesktop.Platform.GL.default//${RUNTIME_VER}" >/dev/null 2>&1 || true

# --- 4. the app bundle -----------------------------------------------------------
say "Fetching the latest AnkiTov release..."
TAG=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n1)
[ -n "$TAG" ] || TAG="v0.6.0"
say "Release ${TAG}"
DL="https://github.com/${REPO}/releases/download/${TAG}/AnkiTov-linux-x86_64.flatpak"
TMP="${TMPDIR:-/tmp}/ankitov-install"
mkdir -p "$TMP"
if command -v curl >/dev/null 2>&1; then curl -fsSL -o "$TMP/AnkiTov-linux-x86_64.flatpak" "$DL"
elif command -v wget >/dev/null 2>&1; then wget -qO "$TMP/AnkiTov-linux-x86_64.flatpak" "$DL"
else die "need curl or wget to download the bundle"; fi
[ "$(wc -c < "$TMP/AnkiTov-linux-x86_64.flatpak")" -gt 1000000 ] || die "download failed (got a stub — check your network and re-run)."

say "Installing the app (existing install is replaced; your class data is kept)..."
flatpak uninstall --user --noninteractive "$APP_ID" >/dev/null 2>&1 || true
flatpak install --user --noninteractive "$TMP/AnkiTov-linux-x86_64.flatpak"

# --- 5. verify -------------------------------------------------------------------
# (flatpak info's Location line is version-dependent — check the standard deploy
# paths directly instead.)
VERIFY=""
for LOC in "$HOME/.local/share/flatpak" "/var/lib/flatpak"; do
  if [ -f "$LOC/app/$APP_ID/current/active/files/bin/config/production.yaml" ]; then
    VERIFY="$LOC"; break
  fi
done
[ -n "$VERIFY" ] || die "install verification failed (config missing) — please report this."

say "Done. Start AnkiTov with:"
printf '  flatpak run %s classroom      # students on this network join via the printed URL\n' "$APP_ID"
printf '  flatpak run %s                # solo (teacher only)\n' "$APP_ID"
printf '  flatpak run %s stop           # stop a background instance\n' "$APP_ID"
