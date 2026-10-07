# Coding Standards

## Rust

- Follow idiomatic Rust conventions.
- Annotate controller routes with `#[utoipa::path]` where the API contract requires it.
- Use SeaORM for product database operations; document justified raw-SQL exceptions.
- Run `cargo check` after Rust changes and `cargo nextest run` for the relevant suite.
- Keep development-only tooling out of the product runtime unless explicitly designed as a product dependency.

## Python

- Use type hints.
- Do not use bare `except` clauses.
- Python is currently limited to supported Anki add-on/runtime assets and scripts.

## Shell

- Use `bash`, not zsh.
- Use `set -euo pipefail` in scripts.
- Do not place API tokens or local review configuration in the repository.

## VCS and review

- Use Jujutsu (`jj`) for repository history and commits.
- Prefer atomic, descriptive commits.
- Normal commits go through `scripts/review-jj-commit.sh`, which runs the local
  `deepseek-review` CLI against the current patch before `jj commit`.
- DeepSeek review is advisory and does not replace human review, tests, security
  checks, or product validation.

## Communication and architecture

- No gRPC: use stdio, HTTP, or MCP stdio where appropriate.
- Do not describe Goose development tools as AnkiTov product components.
- Qualify Rig references as **factory-harness Rig** or **backend NLU Rig**.

## Documentation timeliness

Documentation is part of done: a change that lands without its docs is incomplete.

- Every new or changed public route carries a `#[utoipa::path]` annotation (aggregated in `backend/src/openapi.rs`).
- New capabilities get or update a chapter in `docs/src/` in the same change.
- `scripts/check-docs.sh` must pass before recording the commit; CI re-checks and republishes on push to `main` (`.github/workflows/docs.yml`).
