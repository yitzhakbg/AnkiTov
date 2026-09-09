# AnkiTov

AnkiTov is the product being developed: institutional spaced-repetition
infrastructure with a backend, management console, IMP services, and supported
Anki integration assets.

## Product stack

| Layer | Technology | Boundary |
|---|---|---|
| Backend | Rust 2021 + Loco.rs + Axum | `backend/` |
| Persistence | SeaORM with SQLite/libSQL-compatible storage | Product runtime |
| IMP | Tracks, profiles, capsule generation, telemetry, compliance | Product runtime |
| Optional NLU | Backend NLU Rig + DeepSeek adapter with fallback | Product runtime, optional |
| Auxiliary binaries | Factory-harness Rig and budget gate | AnkiTov components; deployment is product-scoped |
| Documentation | mdBook + rustdoc + OpenAPI/Scalar | Product documentation |

## Development boundary

Goose is the separate development operator, not an AnkiTov runtime component.
Terax, Jujutsu, Goose recipes/extensions, TaskLite, codebase-memory-mcp,
LanceDB/Librarian, and DeepSeek Code Review are Goose-side development tools.

Rig belongs to AnkiTov, not Goose:

- `backend/src/services/rig_nlu.rs` is the optional backend NLU path.
- `toolchains/factory-harness/` is the auxiliary factory-harness Rig component.

These are separate AnkiTov code paths. See
[`development/tooling.md`](development/tooling.md) for the authoritative
boundary.
