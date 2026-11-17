# packaging/ — T-123 Phase 1 native artifacts

Everything needed to turn a built `backend` binary into a downloadable,
double-clickable AnkiTov artifact. The CI pipeline (`.github/workflows/release.yml`)
runs this end-to-end on tag pushes; locally you can drive the same steps by hand.

## Layout

| Path | Purpose |
|------|---------|
| `launch/ankitov-launch.sh` | POSIX launcher used by macOS/Linux artifacts (`start`/`run`/`stop`/`status`) |
| `launch/start-ankitov.bat` | Windows launcher (foreground console run; `stop`/`status` subcommands) |
| `macos/Info.plist` | Minimal `.app` bundle metadata (`@VERSION@` substituted at assemble time) |
| `linux/ankitov.desktop` | Desktop entry template (`@EXEC@` substituted per install layout) |
| `flatpak/` | Flatpak lane: `org.ankitov.AnkiTov.json` manifest, `build-flatpak.sh`, desktop entry, appdata, entry script |
| `assemble.sh` | Builds the artifacts: `macos` (.app → .dmg), `linux` (.deb/.AppImage/.tar.gz), `windows` (.zip) |
| `smoke.sh` / `smoke.ps1` | Per-artifact gate: boot the binary cold, require `GET /` 200 + `register` 200 |

## Local end-to-end (macOS, arm64)

```bash
cargo build --release -p backend
bash packaging/smoke.sh target/release/backend
bash packaging/assemble.sh macos --arch aarch64 --version 1.0.0 \
  --backend target/release/backend --out /tmp/ankitov-dist
# Mount the dmg, copy AnkiTov.app somewhere, launch it:
ANKITOV_NO_BROWSER=1 /path/to/AnkiTov.app/Contents/MacOS/ankitov-launch start
curl -s http://localhost:5150/ -o /dev/null -w '%{http_code}\n'   # 200
ANKITOV_NO_BROWSER=1 /path/to/AnkiTov.app/Contents/MacOS/ankitov-launch stop
```

## Local end-to-end (Flatpak, on a Linux host with flatpak)

```bash
bash packaging/assemble.sh linux --arch x86_64 --version 1.1.0 \
  --backend target/x86_64-unknown-linux-musl/release/backend --out /tmp/ankitov-dist
mkdir -p /tmp/stage && tar -xzf /tmp/ankitov-dist/ankitov-*-linux-x86_64.tar.gz -C /tmp/stage --strip-components=1
bash packaging/flatpak/build-flatpak.sh --arch x86_64 --version 1.1.0 \
  --stage /tmp/stage --out /tmp/ankitov-dist
flatpak install -y /tmp/ankitov-dist/ankitov-1.1.0-linux-x86_64.flatpak
flatpak run org.ankitov.AnkiTov          # foreground; Ctrl-C stops
```

## Design notes

- **Relative SQLite URL.** The launcher `cd`s into the data directory and sets
  `DATABASE_URL=sqlite://ankitov.db?mode=rwc`. A relative URL sidesteps two
  real bugs: macOS paths with spaces (`Application Support`) breaking URL
  parsing, and Windows drive letters (`C:\...`) being read as URL schemes.
- **Secret management.** First run generates a 32-byte random secret
  (`openssl rand -hex 32`, `od` fallback; PowerShell RNG on Windows), persisted
  with owner-only permissions so logins survive restarts.
- **No Dock icon on macOS** (`LSUIElement`): AnkiTov is a background server
  whose UI is the browser; the launcher opens it automatically.
- **Ad-hoc signing on macOS** (`codesign -s -`): required for the binary to
  pass Gatekeeper's structure checks; notarization is a later phase.
- **Static musl on Linux**: the stack is rustls-only (no OpenSSL), so Linux
  artifacts have no glibc floor and run on any distro.
- **AppImage is x86_64-only** because appimagetool is x86_64-only upstream;
  aarch64 Linux ships `.deb` + `.tar.gz`.
- **Flatpak is the no-apt lane.** A `.deb` needs dpkg, so Fedora, Arch, openSUSE, NixOS and
  immutable distros have no path in; a `.flatpak` bundle installs on any distro with flatpak,
  no root, no package manager. App id `org.ankitov.AnkiTov` (reverse DNS of ankitov.org — the
  domain the project controls, which is what Flathub requires). The static musl binary is
  *copied*, not compiled inside the sandbox, so the runtime is only the sandbox shell; a Flathub
  submission would need a source-building module instead (cargo cannot fetch crates inside the
  build sandbox). The entry point runs in the **foreground** (`ankitov-run` → `ankitov-launch
  run`): a backgrounded launcher would exit and bubblewrap tears the sandbox down with its main
  process. The launcher honours `XDG_DATA_HOME`, which flatpak points at
  `~/.var/app/org.ankitov.AnkiTov/data`, so no filesystem permissions are needed;
  `--share=network` shares the host network namespace, so the host browser still reaches
  `http://localhost:5150`. `xdg-open` inside the sandbox may not reach the host browser, so the
  launcher prints the URL as well.
- **Windows ships unsigned** in Phase 1 with SmartScreen guidance in the zip's
  README; Authenticode signing is a later phase.

## CI

`.github/workflows/release.yml` builds all five targets, smoke-gates each
binary, assembles artifacts, generates `sha256sums.txt`, and publishes the
GitHub Release on `v*` tags. The Flatpak job reuses the Linux build artifacts
(it copies files rather than compiling) and is best-effort: a runner without
user namespaces, or a flathub fetch failure, warns and exits 0 so the release
still publishes — but then the download page's `.flatpak` link 404s for that
release, so a skipped build is a signal, not a silent no-op. Jobs run only on the public mirror
(`yitzhakbg/AnkiTov`) — the private repo's Actions minutes are exhausted until
2026-11-01, and the guard makes a stray private-repo tag a no-op instead of a
quota burn. `workflow_dispatch` runs the full build + smoke as a rehearsal but
does not publish.
