# Developing with Goose

AnkiTov development is driven by [Goose](https://block.github.io/goose/), an
open-source AI coding agent. The
[AnkiTov-Goose](https://github.com/yitzhakbg/AnkiTov-Goose) repository is
**public** and carries the complete Goose environment for this project —
configuration templates, recipes, subagents, skills, apps, and bootstrap
tooling — so anyone who wants to develop with Goose can reconstruct the same
harness on a new machine in minutes.

## What is in AnkiTov-Goose

| Path | Purpose |
|------|---------|
| `bootstrap.sh` | Arch-aware master setup (Apple Silicon / Intel): links the global Goose config, installs recipes, scheduled tasks and summon subagents, registers apps, restores the scheduled-job registry |
| `config/` | Templated `goosehints` and `gooseignore` used by the monorepo |
| `recipes/` | Project recipes (`ankitov-design`, `ankitov-plan`, `ankitov-research`, …) and session-utility recipes (`bootstrap-session`, `compact-context`, `debrief-session`) |
| `scheduled_recipes/` | Cron-triggered recipe jobs |
| `agents/` | Summon subagents (`ankitov-rust-engineer`, `ankitov-code-reviewer`, `ankitov-minimal-change`, …) |
| `skills/` | Reusable skills such as `code-review` |
| `apps/` | Goose HTML apps, including the AnkiTov management console |
| `scripts/` | `env-deploy.sh` (environment capture/deploy), `sync-sessions.sh`, `setup-linux-goose.sh`, `deploy-headroom.sh` |

See the repository's [README](https://github.com/yitzhakbg/AnkiTov-Goose) and
[ARCHITECTURE.md](https://github.com/yitzhakbg/AnkiTov-Goose/blob/main/ARCHITECTURE.md)
for the full cross-platform portability audit.

## Quick start

```sh
git clone https://github.com/yitzhakbg/AnkiTov-Goose.git ~/ankitov-goose
cd ~/ankitov-goose
ANKITOV_REPO=/path/to/AnkiTov-C ./bootstrap.sh   # links config, recipes, agents, apps
cd "$ANKITOV_REPO" && cargo check && goose session -r
```

`bootstrap.sh` resolves architecture-dependent paths (Homebrew prefix, Rust
target, budget-gate rebuild) between ARM64 and x86_64 hosts.

## How the monorepo wires into it

In the AnkiTov monorepo, `.goosehints` and `.gooseignore` are symlinks into
the AnkiTov-Goose checkout (`config/goosehints.template` and
`config/gooseignore.template`), so hint/ignore rules are versioned with the
Goose environment rather than the source tree. `scripts/bootstrap-new-machine.sh`
re-creates those symlinks on a fresh clone and expects `~/ankitov-goose` to
exist. The budget-gate crate is rebuilt locally on each target machine; Goose
recipes consult it for spend routing and rate limiting.

## Secrets policy

Machine-specific configuration is deliberately **not distributed**:
`config/global-config.yaml`, `config/project-config.yaml`,
`custom_providers/`, and `goose_config.yaml` are gitignored in
AnkiTov-Goose, and API keys are referenced by environment-variable name
(`api_key_env`) rather than committed values. Each host supplies its own
provider keys when running `bootstrap.sh`. Session databases and snapshots
are likewise local-only.
