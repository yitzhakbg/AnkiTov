# Flatpak packaging (org.ankitov.AnkiTov)

Owner decisions (2026-11-18): app ID `org.ankitov.AnkiTov`; direct-download
bundles first, Flathub later; signing deferred but on the table; flatpak
REPLACES the AppImage intent.

## What this is
A wrapper around the static-musl `tar.gz` the release already builds — no new
compilation. `flatpak-builder` unpacks the tarball into `/app/bin` and installs
the desktop entry.

## Build (CI, ubuntu-24.04)
    sudo apt-get install -y flatpak flatpak-builder
    flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo --system
    flatpak-builder --force-clean --repo=repo --arch=x86_64 build-dir packaging/flatpak/org.ankitov.AnkiTov.json
    flatpak build-bundle repo ankitov-linux-x86_64.flatpak org.ankitov.AnkiTov

## Gate (in release.yml)
1. `ldd` on the musl backend must report "not a dynamic executable" — if it
   links glibc, the freedesktop 24.08 runtime is wrong and the job FAILS loudly.
2. The bundle must install (`flatpak install --user ./bundle`) in the job.
3. The site download page emits flatpak links ONLY when the asset exists in the
   release (the 404 class of bug may never recur).

## Not yet
- Icon: no AnkiTov SVG exists in the repo; the manifest installs no icon until
  one is added (flatpak shows a generic icon, harmless).
- Signing: deferred (unsigned bundles install with a warning).
- Flathub: later; requires the signed repo + review.
