# Streamed Anki (single-app, Selkies) — setup guide

Run **Anki in a browser** — no install on the student's device. One Selkies
container serves Anki as the *only* application, opens straight into the review
session on a chosen deck, and streams it over HTTPS.

- **Upstream:** [`selkies-project/selkies`](https://github.com/selkies-project/selkies), v2.0.0 (MPL-2.0)
- **Base image:** `ghcr.io/selkies-project/selkies/base:latest-ubuntu26.04` (desktop-less)
- **Transport:** WebSockets (default) — *TURN/WebRTC is not in the default path*

This is the **app-only** variant: no LXQt, no panel, no window manager in the X11
path, no terminal. The student sees only Anki's pixels.

---

## 1. What you need

- A Linux host with Docker.
- **One** Anki for Linux, unpacked (e.g. Anki `26.09.2` `linux-x86_64`), at
  `/srv/anki-linux` on the host. It is mounted **read-only** into the container —
  every container shares the same Anki binary.
- A pre-seeded Anki data dir (`Anki2/`) with the pilot decks and the
  `ankitov_autostart` + `anki_connect` add-ons.

## 2. Run it

```bash
export PASSWD='choose-a-real-password'   # compose refuses to start without it
docker build -t ankitov-anki-base:1 -f Dockerfile.anki-base .
docker compose -f docker-compose.anki.yml up -d
```

Open `https://<host>:8091` — user **`ubuntu`**, password = the `PASSWD` value you exported (self-signed
cert; accept the warning). The stream opens **directly in the review session** on
`AnkiTov Pilot — English Vocabulary`.

Verified health check (inside the container):

```bash
curl -s -X POST http://127.0.0.1:8765 -d '{"action":"guiCurrentCard","version":6}'
# -> {"result":{"cardId":..., "deckName":"AnkiTov Pilot — English Vocabulary", ...}}
```

## 3. How the pieces fit

```
browser ──HTTPS/WebSocket──► selkies (s6 service) ──► [ capture compositor ]
                                                         │
   Wayland: labwc (session compositor)  ────────────────┘   X11: Xvfb framebuffer
        │                                                        │
        └───────────── Anki  (the only client) ──────────────────┘
```

| File | Role |
|---|---|
| `Dockerfile.anki-base` | Upstream base **+** the Qt/xcb runtime libs the base lacks (`libxcb-cursor0` …) **+** `grim` |
| `svc/anki/run` | s6 service. Waits for the session, launches Anki as the sole client, reaps it cleanly on stop |
| `svc/labwc/rc.xml` | Wayland-only: labwc window rule that **maximises Anki** to the streamed surface |
| `boot.py` | Launches Anki headless-safely (see §4) |
| `sitecustomize.py` | Watchdog that pushes into the review state (belt-and-braces; see §5) |
| `docker-compose.anki.yml` | The run: ports, `PASSWD`, `SELKIES_WAYLAND`, cap-drop, mounts |

## 4. The two gotchas that must stay fixed

1. **First-run language modal.** On a fresh profile Anki opens a *modal*
   language picker (`aqt/profiles.py::setDefaultLang` → `d.exec()`) **before**
   the profile loads. Headless, nothing clicks it and startup hangs forever.
   `boot.py` monkeypatches `setDefaultLang` to `setLang("en")`.
2. **`app/anki` shim shadows the real package.** `/opt/anki/app/anki/__init__.pyc`
   is a regular-package shim that hides the `app_packages/anki` *namespace*
   package → `ModuleNotFoundError: No module named 'anki.lang'`. `boot.py`
   removes `/opt/anki/app` from `sys.path` and uses
   `PYTHONPATH=/stream:/opt/anki/app_packages:/opt/anki/python/lib/python3.13`.

## 5. Backends: Wayland (default here) vs X11

`SELKIES_WAYLAND=true` selects the **Wayland** path:

- Selkies' **Smithay capture compositor** streams the root surface.
- **labwc** nests inside it to provide window management + XWayland; it is what
  runs `svc/labwc/rc.xml`.
- Anki runs **Wayland-native** (`QT_QPA_PLATFORM=wayland`) on labwc's socket
  (`WAYLAND_DISPLAY=wayland-0`; the capture compositor owns `wayland-1`).
- `svc/anki/run` discovers labwc's socket by *excluding* the recorded capture
  socket (`$XDG_RUNTIME_DIR/selkies-capture-display`), connect-probing each to
  skip stale ones.
- The `xvfb` service **parks itself** when `SELKIES_WAYLAND=true`.

Set `SELKIES_WAYLAND=false` for the **X11** path: Xvfb framebuffer, no window
manager, and `svc/anki/run` sizes the window itself with `xdotool` (matching the
real main window — `WM_CLASS` *Anki* + `_NET_WM_WINDOW_TYPE_NORMAL`, never the
Qt popup/selection-owner windows).

> **Why Wayland is preferred here:** it is the upstream default direction and
> keeps a single window-management story (labwc). The trade-off is that the X11
> automation tools (`xdotool`, `wmctrl`, `xwininfo`, `ffmpeg -f x11grab`) do not
> apply to Wayland-native clients; `grim` replaces `ffmpeg` for screenshots.

### Verifying a Wayland session

```bash
docker exec anki-stream bash -lc '
  export XDG_RUNTIME_DIR=/tmp/runtime-ubuntu WAYLAND_DISPLAY=wayland-0
  grim /tmp/wl.png'
# then confirm Anki is drawing / maximised (PIL): ~98% non-background pixels,
# content bbox ≈ the whole output.
```

## 6. Hardening

- `--cap-drop=ALL`, `--security-opt no-new-privileges`.
- No shell/terminal/panel/file-manager in the image.
- Anki is the *only* client; the window manager (labwc) is present only to place
  it.

## 7. Known-benign log lines

- `Could not open any dma-buf provider` — software encode path (no GPU passthrough).
- `QtWebEngine … kTransientFailure` — no GPU for the webview.

## 8. Transport notes

- WebSockets is the **default** `SELKIES_MODE`; coTURN runs internally but
  **3478 is not published**, so it is dormant unless `SELKIES_MODE=webrtc`.
- On a LAN, WebSockets talks straight to the host — **no TURN**.
