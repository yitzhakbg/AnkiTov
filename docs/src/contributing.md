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

Documentation is **part of done** — a change that lands without its docs is incomplete.

- Every new or changed public route carries a `#[utoipa::path]` annotation (aggregated in `backend/src/openapi.rs`); the API reference then stays current automatically.
- A change that introduces a new capability adds or updates a chapter in `docs/src/` (mdBook) **in the same change**.
- Run `scripts/check-docs.sh` before recording the commit — it fails the build if the book doesn't build or SUMMARY links are broken.
- CI re-checks everything on push to `main` (`.github/workflows/docs.yml`, build + audit only) — so stale or broken docs turn the build red, not silent.

Where things live:

- **mdBook** at `docs/` — architecture, development guide, IMP reference; built + audited by CI and published to https://yitzhakbg.github.io/ankitov-docs/ from the public `ankitov-docs` repo
- **rustdoc** — `cd backend && cargo doc --no-deps --open`; compiled by CI as part of the docs build
- **OpenAPI/Scalar** — served live: `http://localhost:5150/scalar` and `/api/v1/openapi.json`

## PR Workflow

1. Understand the task. Read relevant specs in `specs/`.
2. Implement with atomic `jj` commits.
3. Each commit: `cargo check --workspace` + `cargo nextest run` + `scripts/review-jj-commit.sh`.
4. Push the `jj` stack for review.

## Reference

- Coding standards: [`docs/src/development/standards.md`](./src/development/standards.md)
- Tooling boundary: [`docs/src/development/tooling.md`](./src/development/tooling.md)
- HITL Gate: [`docs/src/development/hitl-gate.md`](./src/development/hitl-gate.md)

### Exporting all project context to Gemini Notebook (NotebookLM)

When you want the *entire* AnkiTov knowledge base — strategy, key decisions,
designs, ops, content, launch — in a form you can upload to
**Google Gemini Notebook** (a.k.a. NotebookLM), run:

```bash
scripts/notebook/export-for-notebooklm.sh          # → ../ankitov-notebook-export/
scripts/notebook/export-for-notebooklm.sh --dry-run # preview the plan + word counts
```

It consolidates the 300+ `.md` files into **13 themed source files** (one per
roadmap area), each with a generated table-of-contents header and a per-section
`_Source: [path](#)_` label so NotebookLM can cite answers back to the original.
`Markdown` is natively accepted by NotebookLM (no PDF/DOCX conversion). The
allowlist approach means only enumerated paths can ever be exported, and a
**secret-scrub tripwire** aborts the run if real credentials (JWTs, private keys,
`api_key=…` values, etc.) are found in the staged output — bare mentions of env-var
names in prose do *not* trigger it. A `SYNC-MANIFEST.md` with SHA-256 checksums and
the `README.md` index ship in the export folder.