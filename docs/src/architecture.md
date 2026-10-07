# AnkiTov Architecture

> **Status note (2026-10):** The Two-Tier Ecosystem, streaming/DRM, and Clearing
> House sections below describe the long-term *vision*. The implemented core
> today is the backend + IMP pipeline — `project-knowledge/prong-progress.md`
> is the re-baselined source of truth.

## Two-Tier Ecosystem

| Tier | License | Scope |
|------|---------|-------|
| **AnkiTov Core** | AGPL-3.0 | Multi-tenant directory, anki-cloud sync protocol, libSQL WAL forensic aggregation |
| **AnkiTov Commerce Gateway** | Proprietary | Enterprise streaming, premium marketplace, consumption-based monetization |

## 5-Layer System Isolation

```
Layer 1: Cloud Gateway        → Delivers ONLY interactive low-latency video feed
Layer 2: Unmodified Anki Core → Stock Anki Desktop (AGPL-3.0, 100% pristine)
Layer 3: AnkiTov DRM Add-on   → Python hook interceptors, in-memory crypto processor
Layer 4: HTML5 Stream (PWA)   → Single touchpoint exposed to the student
```

- **Streaming via Selkies:** students interact through a browser/PWA video feed;
  no access to `.anki2` databases, add-on source, or developer tools.
- **DRM:** AES-256 encrypted premium content; in-memory decryption via
  `gui_hooks.card_will_render`; exports/copy/edit suppressed for premium content.
- **Clearing House:** pay-by-use billing driven by `gui_hooks.reviewer_did_answer_card`
  telemetry → immutable SeaORM audit ledger → automated revenue distribution.

## Product Stack

| Layer | Technology | Boundary |
|-------|-----------|----------|
| Backend | Rust 2021, Loco.rs, Axum | `backend/` |
| Persistence | SeaORM, SQLite/libSQL (WAL) | Product runtime |
| IMP | Tracks, profiles, capsule generation, FSRS, telemetry, compliance | Product runtime |
| Optional NLU | Backend Rig adapter + DeepSeek fallback | `backend/src/services/rig_nlu.rs` |
| Anki integration | Supported add-on/runtime assets | `addons/` |

## Interleaved Mastery Pipeline (IMP)

The current development focus. Every student receives a single server-side
"Remediation Capsule" of dynamically mixed prerequisite tracks.

- **Track:** tag-based sub-collection of cards with prerequisite metadata
- **TrackProfile:** name, track list, target retention, N-value
- **CapsuleSession:** student + profile + N, generated with FSRS urgency sort
  (lowest R first) and min per-track presence guarantee
- **N-Lever:** global control, per-cohort override
- **Compliance:** sessions completed / N per student per week

Full spec: [`specs/oss/2026-09-03-ankitov-core-boundary-map.md`](specs/oss/2026-09-03-ankitov-core-boundary-map.md)

## Key Architectural Decisions

- **No gRPC** — all inter-service comms via stdio, HTTP, or MCP stdio.
- **Schema changes are reversible** — `NoOpMigrator` pattern.
- **Rate limiting** — per-student/day, enforced in the backend.
- **Model routing** — cheapest capable model for heavy code-gen (DeepSeek v4 Flash);
  manual selection for difficult Rust tasks. No automatic cost-aware routing.