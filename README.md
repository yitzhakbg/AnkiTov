# AnkiTov Core (`ankitov-core`)

AnkiTov Core is an open-source spaced repetition engine, retention management system, and Anki addon bridge designed for schools, educators, and independent learners.

> **Disclaimer:** AnkiTov is an independent project and is **not affiliated with, endorsed by, or connected to Anki, AnkiMobile, AnkiDesktop, or AnkiWeb**. "Anki" is a registered trademark of Damien Elmes. AnkiTov is built to interoperate with the open Anki ecosystem.

---

## Features

- **Loco.rs Backend:** High-performance Rust backend providing student retention tracking, FSRS-based scheduling sort, and session telemetry.
- **Session Driver Addon:** Open Anki Desktop addon for seamless review synchronization and session lifecycle management.
- **Seed Content:** High-quality, CC0-licensed starter decks and generation scripts across foundational subjects (Math, Science, History, Vocabulary).
- **Self-Hosting Ready:** Simple deployment via Docker Compose and Caddy with automatic TLS.

---

## Quick Start (Self-Hosting)

### Prerequisites
- Docker and Docker Compose
- Rust toolchain (for building from source)

### Running with Docker Compose
```bash
docker compose up -d
```
Access the management console at `http://localhost:3000` (or your configured domain).

---

## Licenses

- Code, documentation, and configuration files are licensed under the **GNU Affero General Public License v3.0 (AGPL-3.0-only)**. See [LICENSE](LICENSE) for details.
- Seed decks and source cards in `content/seed/` are dedicated to the public domain under the **Creative Commons CC0 1.0 Universal (CC0-1.0)** license. See [content/seed/LICENSE](content/seed/LICENSE).
- The AnkiTov name and branding are proprietary trademarks. See [TRADEMARKS.md](TRADEMARKS.md) for usage policies.
