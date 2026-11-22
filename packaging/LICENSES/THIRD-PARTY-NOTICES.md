# Third-Party Notices — AnkiTov server artifacts

AnkiTov is licensed under the Functional Source License, Version 1.1,
ALv2 Future License (FSL-1.1-ALv2) — see the LICENSE file next to this
directory. This file covers the third-party components aggregated into
AnkiTov packages and the `seat-rootfs` seat runtime.

AnkiTov talks to every component below across OS process boundaries
(CLI/HTTP/IPC); none of them are linked into the AnkiTov binary.

| Component | License | Role | Source |
|-----------|---------|------|--------|
| Anki (desktop, incl. `anki --syncserver`) | AGPL-3.0 | Student practice surface inside each seat | https://github.com/ankitects/anki |
| Selkies (v2, incl. Smithay capture compositor) | MPL-2.0 | Wayland capture + WebRTC/WebSocket streaming | https://github.com/selkies-project/selkies |
| labwc | GPL-2.0 | Wayland window manager inside each seat | https://github.com/labwc/labwc |
| bubblewrap (`bwrap`) | LGPL-2.0+ | Seat sandbox (namespace isolation) | https://github.com/containers/bubblewrap |
| Rust crates (axum, loco-rs, SeaORM, tokio, …) | MIT / Apache-2.0 | Backend dependencies | see `Cargo.lock` |

## AGPL-3.0 Corresponding Source (Anki)

The `seat-rootfs` bundle distributes compiled Anki binaries (AGPL-3.0).
The complete Corresponding Source — including the exact build recipe — is
provided in the same release as this artifact:

* `Dockerfile.anki-base` — the image build that compiles/assembles the seat
  runtime (Anki + Selkies + labwc + kiosk add-ons).
* `SOURCES.md` — the pinned upstream commit hashes / tarball URLs for every
  component in the rootfs.

Both ship in the `stream/seat-rootfs/` directory of the source repository
(https://github.com/yitzhakbg/AnkiTov) and inside `/usr/share/doc/ankitov/`
of packaged artifacts.

## Anki add-ons

The AnkiTov session-driver and kiosk add-ons (`addons/ankitov_session_driver/`
and the kiosk add-on set) are licensed Apache-2.0 and run inside Anki's
Python interpreter via public add-on hooks only. Anki core files are never
modified.
