---
type: note
title: Development Tooling and Product Boundary
---

# Development Tooling and Product Boundary

The tools used by Goose to develop AnkiTov are distinct from AnkiTov's own
components. Goose operates on the repository; it does not define the product's
runtime architecture.

## Goose-side development tools

| Tool | Development role | Shipped with AnkiTov? |
|---|---|---|
| **Goose** | AI-assisted sessions, edits, tests, and orchestration | No |
| **Goose extensions/recipes** | Search, analysis, subagents, and workflows | No |
| **Jujutsu (`jj`)** | Version control | No |
| **`scripts/jj-rollback-by-log.sh`** | Plain-language rollback via commit message search | No |
| **Terax** | Optional terminal/editor workspace | No |
| **TaskLite, codebase-memory-mcp, LanceDB/Librarian** | Optional planning/context aids | No |
| **DeepSeek Code Review** | Review gate for normal commits | No |

## AnkiTov components

| Component | Role | Location |
|---|---|---|
| **Backend NLU Rig** | Optional product dashboard NLU adapter | `backend/src/services/rig_nlu.rs` |
| **Backend/IMP/Anki integration** | Product application and study workflow | `backend/`, `addons/` |

Rig and backend integration are AnkiTov components; they are not Goose tools.
Goose may build or run them, but does not use them as its own agent framework.

A factory-harness build or DeepSeek review does not validate the product. Use:

```bash
cd backend
cargo check
cargo nextest run
```

See [`workspace/Tooling_AnkiTov.md`](../../workspace/Tooling_AnkiTov.md) for the
authoritative boundary and review-wrapper setup.
