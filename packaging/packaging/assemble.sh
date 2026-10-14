#!/bin/sh
# Assemble AnkiTov Phase-1 distribution artifacts (T-123).
#
# Usage:
#   assemble.sh <macos|linux|windows> --arch <aarch64|x86_64> --version <X.Y.Z> \
#               --backend <path-to-built-binary> --out <dist-dir>
#
# Outputs (into --out):
#   macos   → ankitov-<v>-macos-<arch>.dmg   (AnkiTov.app: launcher + backend,
#                                             ad-hoc signed)
#   linux   → ankitov-<v>-linux-<arch>.deb   (/opt/ankitov + .desktop entry)
#             ankitov-<v>-linux-<arch>.AppImage  (x86_64 only — appimagetool is
#                                             x86_64-only upstream; skipped with
#                                             a warning elsewhere)
#             ankitov-<v>-linux-<arch>.tar.gz    (fallback, always produced)
#   windows → ankitov-<v>-windows-<arch>.zip (backend.exe + start-ankitov.bat)
#
# Tooling notes:
#   * macOS needs hdiutil + codesign (present on macOS runners and hosts).
#   * Linux .deb needs dpkg-deb (present on Debian/Ubuntu runners); if absent
#     the .deb is skipped with a warning and the tarball still ships.
#   * AppImage needs appimagetool on PATH (CI downloads it); skipped otherwise.
#   * Windows zip uses PowerShell Compress-Archive (present on Windows runners;
#     the script itself runs under git-bash).
set -eu

usage() {
  echo "usage: $0 <macos|linux|windows> --arch <aarch64|x86_64> --version <X.Y.Z> --backend <binary> --out <dir>" >&2
  exit 1
}

[ $# -ge 1 ] || usage
OS=$1
shift

ARCH="" VERSION="" BACKEND="" OUT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --arch)    ARCH=${2:?}; shift 2 ;;
    --version) VERSION=${2:?}; shift 2 ;;
    --backend) BACKEND=${2:?}; shift 2 ;;
    --out)     OUT=${2:?}; shift 2 ;;
    *) usage ;;
  esac
done
[ -n "$ARCH" ] && [ -n "$VERSION" ] && [ -n "$BACKEND" ] && [ -n "$OUT" ] || usage
[ -f "$BACKEND" ] || { echo "assemble: backend binary not found: $BACKEND" >&2; exit 1; }

PKG_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$PKG_DIR/.." && pwd)
mkdir -p "$OUT"

note() { printf 'assemble: %s\n' "$1"; }
warn() { printf 'assemble: WARNING: %s\n' "$1" >&2; }

# ---------------------------------------------------------------------------
# macOS: AnkiTov.app + .dmg
# ---------------------------------------------------------------------------
assemble_macos() {
  STAGE="$OUT/stage-macos-$ARCH"
  APP="$STAGE/AnkiTov.app"
  rm -rf "$STAGE"
  mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

  sed "s/@VERSION@/$VERSION/g" "$PKG_DIR/macos/Info.plist" > "$APP/Contents/Info.plist"
  cp "$PKG_DIR/launch/ankitov-launch.sh" "$APP/Contents/MacOS/ankitov-launch"
  cp "$BACKEND" "$APP/Contents/MacOS/backend"
  chmod +x "$APP/Contents/MacOS/ankitov-launch" "$APP/Contents/MacOS/backend"
  # Loco resolves config/ relative to CWD; the launcher cds to the data dir and
  # bootstraps its config copy from this bundled one on first run.
  mkdir -p "$APP/Contents/MacOS/config"
  cp "$REPO_ROOT/backend/config/production.yaml" "$APP/Contents/MacOS/config/production.yaml"

  # Phase 1 signing: ad-hoc (no Developer ID yet). Gatekeeper guidance lives in
  # the download page; Developer ID + notarytool is Phase 2.
  if command -v codesign >/dev/null 2>&1; then
    codesign --force --deep -s - "$APP" 2>/dev/null || warn "ad-hoc codesign failed (continuing)"
  else
    warn "codesign not found; shipping unsigned .app"
  fi

  DMG_STAGE="$OUT/dmg-macos-$ARCH"
  rm -rf "$DMG_STAGE"
  mkdir -p "$DMG_STAGE"
  cp -R "$APP" "$DMG_STAGE/"
  ln -s /Applications "$DMG_STAGE/Applications"
  DMG="$OUT/ankitov-$VERSION-macos-$ARCH.dmg"
  hdiutil create -volname "AnkiTov $VERSION" -srcfolder "$DMG_STAGE" -ov -format UDZO "$DMG" >/dev/null
  rm -rf "$DMG_STAGE" "$STAGE"
  note "built $DMG"
}

# ---------------------------------------------------------------------------
# Linux: .deb + optional .AppImage + .tar.gz fallback
# ---------------------------------------------------------------------------
assemble_linux() {
  case "$ARCH" in
    x86_64) DEB_ARCH=amd64 ;;
    aarch64) DEB_ARCH=arm64 ;;
    *) echo "assemble: unsupported linux arch $ARCH" >&2; exit 1 ;;
  esac

  # --- .deb ---
  ROOT="$OUT/deb-$ARCH"
  rm -rf "$ROOT"
  mkdir -p "$ROOT/DEBIAN" "$ROOT/opt/ankitov" \
           "$ROOT/usr/share/applications" "$ROOT/usr/share/doc/ankitov"
  cat > "$ROOT/DEBIAN/control" <<EOF
Package: ankitov
Version: $VERSION
Section: education
Priority: optional
Architecture: $DEB_ARCH
Maintainer: AnkiTov <yitzhakbargeva@gmail.com>
Description: AnkiTov — classroom spaced-repetition practice server
 Local AnkiTov backend (dashboard + API + SQLite). Start it with
 /opt/ankitov/ankitov-launch start or from the desktop entry.
Homepage: https://github.com/yitzhakbg/AnkiTov
EOF
  cp "$PKG_DIR/launch/ankitov-launch.sh" "$ROOT/opt/ankitov/ankitov-launch"
  cp "$BACKEND" "$ROOT/opt/ankitov/backend"
  chmod +x "$ROOT/opt/ankitov/ankitov-launch" "$ROOT/opt/ankitov/backend"
  mkdir -p "$ROOT/opt/ankitov/config"
  cp "$REPO_ROOT/backend/config/production.yaml" "$ROOT/opt/ankitov/config/production.yaml"
  sed "s|@EXEC@|/opt/ankitov/ankitov-launch start|" "$PKG_DIR/linux/ankitov.desktop" \
    > "$ROOT/usr/share/applications/ankitov.desktop"
  for f in LICENSE COPYRIGHT; do
    [ -f "$PKG_DIR/../$f" ] && cp "$PKG_DIR/../$f" "$ROOT/usr/share/doc/ankitov/" && break
  done
  [ -f "$ROOT/usr/share/doc/ankitov/LICENSE" ] || \
    printf 'AnkiTov — see the project repository for license terms.\n' \
      > "$ROOT/usr/share/doc/ankitov/copyright"
  if command -v dpkg-deb >/dev/null 2>&1; then
    DEB="$OUT/ankitov-$VERSION-linux-$ARCH.deb"
    dpkg-deb --build --root-owner-group "$ROOT" "$DEB" >/dev/null
    note "built $DEB"
  else
    warn "dpkg-deb not found; skipping .deb (tarball still produced)"
  fi
  rm -rf "$ROOT"

  # --- .AppImage (x86_64 only; appimagetool is x86_64-only upstream) ---
  if [ "$ARCH" = "x86_64" ] && command -v appimagetool >/dev/null 2>&1; then
    APPDIR="$OUT/AppDir"
    rm -rf "$APPDIR"
    mkdir -p "$APPDIR/usr/bin"
    cp "$PKG_DIR/launch/ankitov-launch.sh" "$APPDIR/usr/bin/ankitov-launch"
    cp "$BACKEND" "$APPDIR/usr/bin/backend"
    chmod +x "$APPDIR/usr/bin/ankitov-launch" "$APPDIR/usr/bin/backend"
    mkdir -p "$APPDIR/usr/bin/config"
    cp "$REPO_ROOT/backend/config/production.yaml" "$APPDIR/usr/bin/config/production.yaml"
    sed "s|@EXEC@|ankitov-launch start|" "$PKG_DIR/linux/ankitov.desktop" \
      > "$APPDIR/ankitov.desktop"
    printf '#!/bin/sh\nexec "$APPDIR/usr/bin/ankitov-launch" "$@"\n' > "$APPDIR/AppRun"
    chmod +x "$APPDIR/AppRun"
    APPIMAGE="$OUT/ankitov-$VERSION-linux-$ARCH.AppImage"
    if ARCH="$ARCH" appimagetool --appimage-extract-and-run "$APPDIR" "$APPIMAGE" >/dev/null 2>&1; then
      note "built $APPIMAGE"
    else
      warn "appimagetool failed; skipping .AppImage (tarball still produced)"
    fi
    rm -rf "$APPDIR"
  elif [ "$ARCH" != "x86_64" ]; then
    note "no .AppImage for $ARCH (appimagetool is x86_64-only upstream); tarball covers it"
  fi

  # --- .tar.gz fallback (always) ---
  TARBALL_STAGE="$OUT/tarball-linux-$ARCH"
  rm -rf "$TARBALL_STAGE"
  mkdir -p "$TARBALL_STAGE/ankitov-$VERSION"
  cp "$PKG_DIR/launch/ankitov-launch.sh" "$TARBALL_STAGE/ankitov-$VERSION/ankitov-launch"
  cp "$BACKEND" "$TARBALL_STAGE/ankitov-$VERSION/backend"
  chmod +x "$TARBALL_STAGE/ankitov-$VERSION/ankitov-launch" "$TARBALL_STAGE/ankitov-$VERSION/backend"
  mkdir -p "$TARBALL_STAGE/ankitov-$VERSION/config"
  cp "$REPO_ROOT/backend/config/production.yaml" "$TARBALL_STAGE/ankitov-$VERSION/config/production.yaml"
  TAR="$OUT/ankitov-$VERSION-linux-$ARCH.tar.gz"
  tar -czf "$TAR" -C "$TARBALL_STAGE" "ankitov-$VERSION"
  rm -rf "$TARBALL_STAGE"
  note "built $TAR"
}

# ---------------------------------------------------------------------------
# Windows: .zip (backend.exe + start-ankitov.bat)
# ---------------------------------------------------------------------------
assemble_windows() {
  STAGE="$OUT/ankitov-$VERSION-windows-$ARCH"
  rm -rf "$STAGE"
  mkdir -p "$STAGE"
  cp "$BACKEND" "$STAGE/backend.exe"
  cp "$PKG_DIR/launch/start-ankitov.bat" "$STAGE/start-ankitov.bat"
  mkdir -p "$STAGE/config"
  cp "$REPO_ROOT/backend/config/production.yaml" "$STAGE/config/production.yaml"
  cat > "$STAGE/README.txt" <<EOF
AnkiTov $VERSION (Windows)

1. Double-click start-ankitov.bat
2. A console window opens and the dashboard opens in your browser
   (http://localhost:5150)
3. Keep the console window open while using AnkiTov; close it to stop.

Data lives in %LOCALAPPDATA%\AnkiTov (database + generated secret).
Windows may show a SmartScreen warning for unsigned apps: click
"More info" -> "Run anyway".
EOF
  ZIP="$OUT/ankitov-$VERSION-windows-$ARCH.zip"
  if command -v powershell.exe >/dev/null 2>&1; then
    powershell.exe -NoProfile -Command "Compress-Archive -Path '$STAGE/*' -DestinationPath '$ZIP' -Force" >/dev/null
  elif command -v zip >/dev/null 2>&1; then
    (cd "$OUT" && zip -qr "$(basename "$ZIP")" "$(basename "$STAGE")")
  else
    warn "neither powershell.exe nor zip found; leaving staged dir at $STAGE"
    return 0
  fi
  rm -rf "$STAGE"
  note "built $ZIP"
}

case "$OS" in
  macos)   assemble_macos ;;
  linux)   assemble_linux ;;
  windows) assemble_windows ;;
  *) usage ;;
esac
