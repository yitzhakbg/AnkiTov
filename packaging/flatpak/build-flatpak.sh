#!/bin/sh
# Assemble an AnkiTov Flatpak bundle (T-123 packaging, the no-apt lane).
#
# Why this exists: .deb needs dpkg. Fedora, Arch, openSUSE, NixOS, most immutable
# distros, and any machine where the user cannot run apt have no path to the .deb.
# A .flatpak bundle is a single file that `flatpak install ./ankitov.flatpak`
# consumes on any distro with flatpak installed — no root, no package manager.
#
# Usage:
#   build-flatpak.sh --arch <x86_64|aarch64> --version <X.Y.Z> \
#                    --stage <dir> --out <dist-dir>
#
# --stage is the layout packaging/assemble.sh produces for the Linux tarball:
#   backend, ankitov-launch, config/production.yaml, content/seed/packages
# (CI extracts the uploaded tarball; locally, extract the tarball assemble.sh built.)
#
# Output: <out>/ankitov-<version>-linux-<arch>.flatpak
#
# The stage is re-laid into the Flatpak /app tree:
#   /app/bin/backend, /app/bin/ankitov-launch, /app/bin/ankitov-run
#   /app/bin/config/production.yaml, /app/bin/content/seed/packages
#   /app/share/applications/org.ankitov.AnkiTov.desktop
#   /app/share/icons/hicolor/scalable/apps/org.ankitov.AnkiTov.svg
#   /app/share/metainfo/org.ankitov.AnkiTov.appdata.xml
#
# Tooling notes:
#   * Needs flatpak + flatpak-builder on PATH and the org.freedesktop.Platform
#     runtime reachable from a remote (CI adds flathub). Skipped with a warning
#     otherwise, so a missing toolchain never breaks the release.
#   * The binary is copied, not compiled inside the sandbox: the backend is a
#     static musl build (rustls-only stack, no glibc floor), so the runtime is
#     only the sandbox shell. A Flathub submission would need a source-building
#     module instead — see packaging/README.md.
set -eu

usage() {
  echo "usage: $0 --arch <x86_64|aarch64> --version <X.Y.Z> --stage <dir> --out <dist-dir>" >&2
  exit 1
}

ARCH="" VERSION="" STAGE="" OUT="" STAGE_ONLY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --arch)    ARCH=${2:?}; shift 2 ;;
    --version) VERSION=${2:?}; shift 2 ;;
    --stage)   STAGE=${2:?}; shift 2 ;;
    --out)     OUT=${2:?}; shift 2 ;;
    --stage-only) STAGE_ONLY=1; shift ;;
    *) usage ;;
  esac
done
[ -n "$ARCH" ] && [ -n "$VERSION" ] && [ -n "$STAGE" ] && [ -n "$OUT" ] || usage
[ -d "$STAGE" ] || { echo "build-flatpak: stage dir not found: $STAGE" >&2; exit 1; }

PKG_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$PKG_DIR/../.." && pwd)
APP_ID=org.ankitov.AnkiTov
mkdir -p "$OUT"

note() { printf 'build-flatpak: %s\n' "$1"; }
warn() { printf 'build-flatpak: WARNING: %s\n' "$1" >&2; }

if [ ! -f "$PKG_DIR/$APP_ID.json" ]; then
  echo "build-flatpak: manifest not found: $PKG_DIR/$APP_ID.json" >&2
  exit 1
fi

# --- Flatpak stage tree ------------------------------------------------------
FLAT_STAGE="$OUT/flatpak-stage-$ARCH"
rm -rf "$FLAT_STAGE"
mkdir -p "$FLAT_STAGE/bin" "$FLAT_STAGE/share/applications" \
         "$FLAT_STAGE/share/icons/hicolor/scalable/apps" "$FLAT_STAGE/share/metainfo"

[ -f "$STAGE/backend" ] || { echo "build-flatpak: no backend binary in stage" >&2; exit 1; }
cp "$STAGE/backend" "$FLAT_STAGE/bin/backend"
chmod +x "$FLAT_STAGE/bin/backend"

# Launcher: prefer the one already in the stage (assemble.sh copied it); fall
# back to the repo copy so a stage built straight from `cargo build` also works.
if [ -f "$STAGE/ankitov-launch" ]; then
  cp "$STAGE/ankitov-launch" "$FLAT_STAGE/bin/ankitov-launch"
else
  cp "$PKG_DIR/../launch/ankitov-launch.sh" "$FLAT_STAGE/bin/ankitov-launch"
fi
chmod +x "$FLAT_STAGE/bin/ankitov-launch"
cp "$PKG_DIR/ankitov-run" "$FLAT_STAGE/bin/ankitov-run"
chmod +x "$FLAT_STAGE/bin/ankitov-run"

if [ -d "$STAGE/config" ]; then
  # bin/ exists, bin/config does not: cp -R lands the dir at bin/config.
  cp -R "$STAGE/config" "$FLAT_STAGE/bin/config"
else
  mkdir -p "$FLAT_STAGE/bin/config"
  cp "$REPO_ROOT/backend/config/production.yaml" "$FLAT_STAGE/bin/config/production.yaml"
fi

if [ -d "$STAGE/content/seed/packages" ]; then
  mkdir -p "$FLAT_STAGE/bin/content/seed"
  cp -R "$STAGE/content/seed/packages" "$FLAT_STAGE/bin/content/seed/packages"
elif [ -d "$REPO_ROOT/content/seed/packages" ]; then
  mkdir -p "$FLAT_STAGE/bin/content/seed"
  cp -R "$REPO_ROOT/content/seed/packages" "$FLAT_STAGE/bin/content/seed/packages"
else
  warn "no seed packs in stage; bundle ships without them"
fi

cp "$PKG_DIR/$APP_ID.desktop" "$FLAT_STAGE/share/applications/$APP_ID.desktop"
ICON_SRC="$REPO_ROOT/specs/launch/site-org/favicon.svg"
[ -f "$ICON_SRC" ] && cp "$ICON_SRC" "$FLAT_STAGE/share/icons/hicolor/scalable/apps/$APP_ID.svg"

# appdata: pin the release version/date so Flathub's appcheck sees a real release.
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$(date -u +%Y-%m-%d)/g" \
  "$PKG_DIR/$APP_ID.appdata.xml" > "$FLAT_STAGE/share/metainfo/$APP_ID.appdata.xml"

# --- Build + bundle ----------------------------------------------------------
if ! command -v flatpak-builder >/dev/null 2>&1; then
  warn "flatpak-builder not found; staged tree left at $FLAT_STAGE (bundle skipped)"
  exit 0
fi

# Environment guard, not a manifest guard: a missing runtime means the runner
# could not fetch it (no network / no user namespaces), which should warn rather
# than fail the release. A manifest error still fails loudly.
if ! flatpak list --runtime 2>/dev/null | grep -q 'org.freedesktop.Platform//' &&
   ! flatpak list --user --runtime 2>/dev/null | grep -q 'org.freedesktop.Platform//'; then
  warn "runtime org.freedesktop.Platform not installed on this host; bundle skipped"
  exit 0
fi

BUILD_DIR="$OUT/flatpak-build-$ARCH"
rm -rf "$BUILD_DIR"

# Sources are relative to the manifest dir, so the stage tarball goes there.
tar -czf "$PKG_DIR/ankitov-stage.tar.gz" -C "$FLAT_STAGE" .

if [ "$STAGE_ONLY" = 1 ]; then
  note "staged tree + $PKG_DIR/ankitov-stage.tar.gz written; build left to the caller"
  exit 0
fi

flatpak-builder --user --force-clean --no-cache "$BUILD_DIR" "$PKG_DIR/$APP_ID.json" >/dev/null

BUNDLE="$OUT/ankitov-$VERSION-linux-$ARCH.flatpak"
flatpak build-bundle "$BUILD_DIR" "$BUNDLE" "$APP_ID" >/dev/null

rm -rf "$BUILD_DIR" "$FLAT_STAGE" "$PKG_DIR/ankitov-stage.tar.gz"
note "built $BUNDLE"
