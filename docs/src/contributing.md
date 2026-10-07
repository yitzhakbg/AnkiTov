---
type: note
title: Contributing to AnkiTov
---

# Contributing to AnkiTov

## Quick Start

```bash
bash scripts/bootstrap.sh
```

That's it. The script checks tooling, builds the workspace, and runs tests.
See [`README.md`](../README.md#quick-start) for what you need installed.

## Coding Standards

### Rust

- Idiomatic Rust 2021 edition.
- Controller routes should carry `#[utoipa::path]` annotations for API docs.
- Use SeaORM for database operations. Document raw-SQL exceptions.
- Run `cargo check --workspace` before committing.
- Run `cargo nextest run` to verify tests.

### Python

- Type hints required. No bare `except` clauses.
- Limited to Anki add-on/runtime assets and scripts.

### Shell

- `bash` only (never zsh). `set -euo pipefail` in scripts.
- No API tokens or review configuration in the repository.

## VCS: Jujutsu

All commits use `jj` (Jujutsu), not `git`. Atomic commits with descriptive messages.

```bash
# Normal commit flow
scripts/review-jj-commit.sh    # DeepSeek review + commit

# Or skip review for minor changes
jj commit -m "message"

# Rollback by describing the issue (no need to remember bookmark names)
scripts/jj-rollback-by-log.sh "the i18n changes broke the dashboard"
scripts/jj-rollback-by-log.sh "before the IMP pipeline" --list
scripts/jj-rollback-by-log.sh --to-bookmark main --kind bookmark-back
```

The `scripts/jj-rollback-by-log.sh` tool searches jj commit history by
plain-language description, lets you pick the matching commit interactively,
then rolls back to before it. Four rollback kinds:
- `new` (default): `jj new <rev>` — create a clean working copy from before
  the change
- `restore`: `jj restore --from <rev>` — restore file content without moving
  history
- `abandon-through`: `jj abandon <rev>..` — abandon the range (SKYNET,
  destructive)
- `bookmark-back`: move a bookmark back to a prior state

Always run with `--list` first to preview. See `--help` for full usage.

The DeepSeek review adapter consumes a Git-format patch; repository history
remains in Jujutsu. The review is advisory — it does not replace human review
or test validation.
The DeepSeek review adapter consumes a Git-format patch; repository history
remains in Jujutsu. The review is advisory — it does not replace human review
or test validation.

## Build & Test

```bash
# Check everything
cargo check --workspace

# Run tests
cargo nextest run

# Start dev server (localhost:5150)
cd backend && cargo run start
```

## Tooling Boundary

Goose development tools are separate from AnkiTov product components.

| Tool | Role | Side |
|------|------|------|
| Goose, Terax, Jujutsu, TaskLite, codebase-memory-mcp, LanceDB | Dev environment | Goose-side |
| Backend, addons | Product | AnkiTov-side |

Goose operates on the repository; it does not define the product's runtime
architecture. See [`docs/src/architecture.md`](src/architecture.md) for the product
stack.

## Docs

- **mdBook** at `docs/` — architecture, development guide, IMP reference
- **rustdoc** — `cd backend && cargo doc --no-deps --open`
- **OpenAPI/Scalar** — `http://localhost:5150/scalar` (dev server running)

## PR Workflow

1. Understand the task. Read relevant specs in `specs/`.
2. Implement with atomic `jj` commits.
3. Each commit: `cargo check --workspace` + `cargo nextest run` + `scripts/review-jj-commit.sh`.
4. Push the `jj` stack for review.

## Reference

- Coding standards: [`docs/src/development/standards.md`](./src/development/standards.md)
- Tooling boundary: [`docs/src/development/tooling.md`](./src/development/tooling.md)
- HITL Gate: [`docs/src/development/hitl-gate.md`](./src/development/hitl-gate.md)
