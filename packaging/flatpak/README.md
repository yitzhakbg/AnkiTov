# Flatpak packaging (org.ankitov.AnkiTov)

Owner decisions (2026-11-18): app ID `org.ankitov.AnkiTov`; direct-download
bundles first, Flathub later; signing deferred but on the table; flatpak
REPLACES the AppImage intent.

## What this is
A wrapper around the static-musl `tar.gz` the release already builds — no new
compilation. `flatpak-builder` unpacks the tarball into `/app/bin` and installs
the desktop entry.

## One-command install (end users)

    curl -fsSL https://github.com/yitzhakbg/AnkiTov/releases/download/v0.6.0/install-ankitov.sh | sh

Installs flatpak (if missing) → flathub remote → the freedesktop runtime →
the AnkiTov bundle, verifies the deploy, and prints the run commands
(`classroom` / solo / `stop`). Ships as a release asset (`install-ankitov.sh`);
safe to re-run to upgrade — class data is kept.

## Build (CI, ubuntu-24.04)
    sudo apt-get install -y flatpak flatpak-builder
    flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo --system
    flatpak-builder --force-clean --repo=repo --arch=x86_64 build-dir packaging/flatpak/org.ankitov.AnkiTov.json
    flatpak build-bundle repo ankitov-linux-x86_64.flatpak org.ankitov.AnkiTov

## Gate (in release.yml)
1. `ldd` on the musl backend must report "not a dynamic executable" — if it
   links glibc, the freedesktop 25.08 runtime is wrong and the job FAILS loudly.
2. The bundle must install (`flatpak install --user ./bundle`) in the job.
3. The site download page emits flatpak links ONLY when the asset exists in the
   release (the 404 class of bug may never recur).

## Launcher (shipped inside the artifact)
The bundled `ankitov-launch` is flatpak-aware: it prints `flatpak run ...`
commands (never sandbox paths like /app/bin), and has a classroom mode:

    flatpak run org.ankitov.AnkiTov classroom

binds 0.0.0.0, auto-stops a running instance, prints the student join URL
(`http://<LAN-IP>:5150/reviewer`), and pre-checks the port with a human-readable
message instead of a raw `AddrInUse` (catches native-install/flatpak port fights).

## Not yet
- **Icon/logo — NEEDED.** The app currently surfaces the generic loco.rs icon.
  We need a real AnkiTov logo: an SVG (desktop `Icon=` + metainfo) plus PNG
  hicolor sizes (128x128 minimum, 256 preferred) for the window icon. This is a
  blocker for Flathub review and the main visual-polish gap in the flatpak.
- **Flathub ease bar:** when we ship to Flathub it must be super easy —
  `flatpak install flathub org.ankitov.AnkiTov` resolves the runtime
  automatically (no manual runtime install like the bundle lane needs), and the
  listing needs metainfo (description, screenshots, releases) + the icon above.
  The direct-download bundle lane stays for offline/no-store users.
- Signing: deferred (unsigned bundles install with a warning).
- Flathub submission: later; requires the signed repo + review.
