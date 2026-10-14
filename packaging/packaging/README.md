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
- **Windows ships unsigned** in Phase 1 with SmartScreen guidance in the zip's
  README; Authenticode signing is a later phase.

## CI

`.github/workflows/release.yml` builds all five targets, smoke-gates each
binary, assembles artifacts, generates `sha256sums.txt`, and publishes the
GitHub Release on `v*` tags. Jobs run only on the public mirror
(`yitzhakbg/AnkiTov`) — the private repo's Actions minutes are exhausted until
2026-11-01, and the guard makes a stray private-repo tag a no-op instead of a
quota burn. `workflow_dispatch` runs the full build + smoke as a rehearsal but
does not publish.
