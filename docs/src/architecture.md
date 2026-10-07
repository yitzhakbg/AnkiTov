# AnkiTov Architecture

AnkiTov Core is a multi-tenant spaced-repetition platform: a Rust backend
(Loco.rs 0.11 + Axum + SeaORM on SQLite/libSQL) that serves class management,
card sync, and the Interleaved Mastery Pipeline, with an integration layer for
Anki add-ons and runtime assets.

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

Full spec: [`specs/oss/2026-09-03-ankitov-core-boundary-map.md`](https://github.com/yitzhakbg/AnkiTov/blob/main/specs/oss/2026-09-03-ankitov-core-boundary-map.md)

## Key Architectural Decisions

- **No gRPC** — all inter-service comms via stdio, HTTP, or MCP stdio.
- **Schema changes are reversible** — `NoOpMigrator` pattern.
- **Rate limiting** — per-student/day, enforced in the backend.
- **Model routing** — cheapest capable model for heavy code-gen (DeepSeek v4 Flash);
  manual selection for difficult Rust tasks. No automatic cost-aware routing.
