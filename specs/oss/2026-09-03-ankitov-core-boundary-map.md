# AnkiTov `ankitov-core` OSS Boundary Map

**Date:** 2026-09-03
**Status:** Owner-approved direction (G3, two-repo split). This document is the operative
boundary spec for the `ankitov-core` export. It supersedes the conflicting parts of
`specs/design/2026-08-26-boundary-map.md` (see §2).
**Scope of this doc:** (1) directory/file-level boundary, (2) license application per layer
+ trademark note, (3) repo mechanics for the export + sync discipline, (4) execution
checklist for the orchestrator.

---

## 1. Decision Summary

Two repos:

| Repo | Visibility | License | Content |
|---|---|---|---|
| `AnkiTov` (this monorepo) | **Private, upstream source of truth** | mixed (see §4) | everything |
| `ankitov-core` (new) | Public | AGPL-3.0-only (code/docs) + CC0-1.0 (seed decks) | the **pipe**: backend, session driver, seed content, self-host docs & deployment |

Rule of thumb (inherited from 2026-08-26, still valid):

> If it's the pipe that makes the open-source feature work, it's open.
> If it's the lock (paywall, commerce, ops policy, go-to-market) — or any internal
> reasoning about the business — it's closed.

**What the community gets:** full Loco.rs backend (IMP pipeline, FSRS sort, capsule
generation, compliance tracking, dashboard), Session Driver Anki addon, CC0 seed decks +
generator, self-hosting docs and deployment (Docker/Caddy), i18n locales, contributing
docs. **What they never get:** budget gate, factory tooling, launch/marketing, editorial
calendar, video scripts, internal specs/knowledge, ops runbooks, student data, the
`AnkiTov` trademark.

---

## 2. Boundary at Directory/File Level

### 2.1 Divergence from the 2026-08-26 map (audit trail)

| Item | 2026-08-26 map said | Now (owner, 2026-09-03) | Why |
|---|---|---|---|
| `ankitov-budget-gate/` | OSS ("commodity pipe") | **Proprietary** | Owner decision; it encodes ops/spend policy, not product plumbing |
| `toolchains/` (factory-harness) | OSS | **Proprietary** | Factory/internal tooling |
| `specs/` | All specs OSS | **Curated subset only** (§2.2) | Most specs are internal reasoning/roadmap |
| Seed deck license | `TBC-OWNER-DECISION` | **CC0-1.0** | Owner decision; `content/seed/manifest.json` must be updated |
| Session Driver addon | OSS | OSS (unchanged) | no encryption/paywall inside |

### 2.2 EXPORT → `ankitov-core`

Target tree:

```
ankitov-core/
├── backend/                  # full Loco.rs crate (AGPL-3.0-only)
│   ├── src/                  # controllers, models, services, migrations, views
│   ├── config/               # development.yaml, test.yaml, production.yaml (see §6 tripwire)
│   ├── i18n/                 # all locales (user-facing translations)
│   ├── resources/            # dashboard assets
│   └── tests/                # integration + harness tests
├── session-driver/           # from addons/ankitov_session_driver/ (AGPL-3.0-only)
├── content/seed/             # seed decks: packages/, sources/, generate_decks.py, manifest.json (CC0-1.0)
│   └── tools/import_check.sh # copied from scripts/seed/import_check.sh
├── docs/                     # SELECTIVE export — see exclusion note below
├── specs/                    # SELECTIVE export: this boundary map + specs/audits/*.txt invariants
├── tools/                    # i18n tooling: validate_i18n.py, propagate_locale_keys.py
│                             #   (translate_ux_keys.py ONLY after owner review — may embed API endpoints/keys)
├── Dockerfile.backend
├── docker-compose.yml        # secrets via ${ENV} indirection — verified OK
├── Caddyfile                 # verified clean: domain via {$ANKITOV_DOMAIN:localhost}
├── rust-toolchain.toml
├── Cargo.toml                # NEW workspace: members = ["backend"] ONLY (budget-gate member dropped)
├── LICENSE                   # AGPL-3.0-only full text
├── content/seed/LICENSE      # CC0-1.0 full text (seed deck content)
├── TRADEMARKS.md             # §5 trademark note
└── README.md                 # rewritten: self-host quickstart, disclaimer, no monorepo paths
```

**docs/ exclusion note:** export `overview.md`, `architecture.md` (system overview only),
`contributing.md`, `development/*`, `imp/*`, `reference/*`, `custom.css`, `book.toml`,
`SUMMARY.md` (pruned). **Do NOT export** `docs/src/architecture/strategic-blueprint.md`,
`streaming-drm.md`, `clearing-house.md` (go-to-market/enterprise strategy → keep private),
nor `docs/deployment.md`, `docs/ops/*` until owner reviews them.

**Root-level questions flagged for owner (default = exclude):** `inject_db.py`,
`seed_data.py`, `seed_users.py` (synthetic but PII-shaped — fake names/emails; if wanted
as fixtures, move under `backend/tests/fixtures/` with SYNTHETIC labeling first),
`VERSION`, `CHANGELOG.md`.

### 2.3 KEEP PRIVATE (monorepo only — never exported)

| Path | What it is |
|---|---|
| `ankitov-budget-gate/` | MCP ops/spend gate — proprietary |
| `toolchains/factory-harness/` | Build/factory automation — proprietary |
| `buzz-hive/` | Vendored Buzz agent platform (Apache-2.0, upstream `block/buzz`). Separate project + license — **never re-license or copy into AGPL tree**; factory use only |
| `.goose/`, `/Volumes/YBG1TB4Mac/AnkiTov-goose` | Buzz agent factory config — internal |
| `recipes/` | Goose subrecipes/factory — internal |
| `scripts/` (root) | bootstrap, jj tooling, sync-server, harness2 — internal ops. Exception: `scripts/seed/import_check.sh` is copied **out** into `content/seed/tools/` |
| `project-knowledge/` | internal audits, standups, positioning doctrine |
| `docs-repository/` | internal runbooks — `anki-operations-access.md` is access architecture; never export |
| `workspace/` | strategic blueprint, RPI workflow, launch/video/presentation drafts |
| `content/editorial/` | editorial calendar + posts — proprietary |
| `blog/` | marketing blog content — proprietary |
| `specs/` except the §2.2 subset | design/plans/research (internal reasoning), `launch/` (+ deployed `launch/site/` — **public but NOT OSS**, stays here), `media/` (video scripts), `outreach/`, `content/` (content plans), `templates/` |
| `addons/addons.db`, `*.sqlite`, `*.db` at root | data files — never |
| `backend/data/` | **live student database** (`ankitov.db` + `-shm`/`-wal`) — never |
| `AnkiPlayGround/` | Anki profile/collection data — never |
| `node_modules/`, `.code-graph/`, `.headroom/`, `__pycache__/` | build/tool caches — never |
| `goose_config.yaml.bak.*`, `--help` (junk file) | cleanup candidates — never export |

---

## 3. (reserved for reference) Upstream facts this map relies on

- Backend is Loco.rs 0.11 / Axum / SeaORM / SQLite (`backend/Cargo.toml` — **no `license`
  field today**; checklist adds one).
- Root `Cargo.toml` is a workspace whose members include `ankitov-budget-gate` → the OSS
  repo needs its own workspace file with only `backend`.
- `content/seed/manifest.json` currently carries `"license":"TBC-OWNER-DECISION"` and a
  pinned `sha256` for `math-definitions-en.apkg` → license flip + regeneration invalidates
  hashes; regenerate then re-pin.
- Deployment secrets already use env indirection (`${ANKITOV_JWT_SECRET}`); one dev-only
  default (`ankitov-dev-auth` for SQLD) is present and acceptable.

---

## 4. License Application Per Layer

| Layer | License | Mechanics |
|---|---|---|
| Backend code, session driver, i18n locales, deployment manifests, exported docs & specs | **AGPL-3.0-only** | `LICENSE` (AGPL-3.0 full text) at repo root; `license = "AGPL-3.0-only"` in `backend/Cargo.toml` + new root `Cargo.toml` `[workspace.package]`; SPDX identifier line (`// SPDX-License-Identifier: AGPL-3.0-only`) required in new files going forward; enforce in CI |
| Seed deck content (`content/seed/`: `.apkg`, `.csv`, generator script is code → dual-rule below) | **CC0-1.0** for deck content | `content/seed/LICENSE` (CC0-1.0 text); set `manifest.json → "license":"CC0-1.0"`; regenerate `math-definitions-en.apkg` and re-pin `sha256` in manifest; note that `generate_decks.py` and `tools/import_check.sh` are **code → AGPL-3.0-only**, sitting inside the CC0 content dir (state both in `content/seed/README.md`) |
| Trademark (`AnkiTov` name/logo) | **Proprietary — no open license** | see §5 |

Rationale notes: AGPL-3.0-only keeps parity with upstream Anki (itself AGPL-3.0) and
closes the SaaS loophole for a self-host education product. "Only" (not "or-later") is the
owner's call for maximum copyleft certainty. CC0-1.0 makes seed decks friction-free for
schools and downstream deck authors.

## 5. Trademark Note

- The **AnkiTov name and logo remain proprietary**; they are NOT licensed under AGPL or
  CC0. Add `TRADEMARKS.md` to `ankitov-core` stating: redistribution of the code is
  permitted under AGPL-3.0-only, but use of the *AnkiTov* name/branding to distribute
  modified builds is not; forks must rename (distributions like Debian have the same
  convention).
- Required disclaimer in the OSS `README.md` (visible, near the top):
  > AnkiTov is an independent project and is **not affiliated with, endorsed by, or
  > connected to Anki, AnkiMobile, AnkiDesktop, or AnkiWeb**. "Anki" is a trademark of its
  > respective owner (Damien Elmes). AnkiTov is built to interoperate with the open Anki
  > ecosystem.
- Do not use the Anki logo in OSS branding; textual "works with Anki" interop statements
  are fine under the disclaimer.

---

## 6. Repo Mechanics

**Model:** private monorepo = single source of truth; `ankitov-core` = **one-way filtered
export** (monorepo → public). No direct commits to public that don't land upstream first;
community PRs are fetched as patches and re-landed in the monorepo, then re-exported.

**Export mechanism:** an allowlist-driven export script (`scripts/export-ankitov-core.sh`,
new, monorepo-side) that:
1. `rsync`s only allowlisted paths (§2.2) into `../ankitov-core-staging/` — **allowlist,
   not denylist** (new private files can never leak by accident);
2. materializes the OSS-only root files (new `Cargo.toml`, `LICENSE`, `TRADEMARKS.md`,
   rewritten `README.md`) from templates in `scripts/oss-templates/`;
3. runs the **secret tripwire** before any commit: deny patterns for
   `backend/data/**`, `*.sqlite`, `*.db`, `ANKITOV_JWT_SECRET=`, `BEGIN.*PRIVATE KEY`,
   `@school.edu`, `school.edu` emails, `project-knowledge`, `internal-runbooks`,
   `budget-gate`, `Strategic_Blueprint`, `school.toml`-style creds; plus a `gitleaks` run;
4. emits `SYNC-MANIFEST.md` recording monorepo ref → export timestamp + file checksums
   (audit trail for "what shipped when").

**Sync discipline:**
- Changes land in the monorepo first; export is a deliberate, tagged act
  (`core-sync/<monorepo-ref>`).
- Public-only hotfixes (e.g., README typos) are allowed in `ankitov-core` but **must be
  back-ported** into the monorepo within the next sync, or they will be clobbered.
- Cadence: sync at every release + at minimum weekly; dry-run diff (`--dry-run` mode)
  reviewed before publish.

**What must NEVER leak (hard list, enforced by tripwire + allowlist):** secrets/tokens
(`ANKITOV_JWT_SECRET` values, JWT signing keys, SMTP creds, `.env` files), **student data**
(`backend/data/ankitov.db*`, any `.sqlite`/`.db`, even synthetic-PII-shaped
`seed_users.py`/`seed_data.py` by default), internal specs & knowledge
(`project-knowledge/`, `docs-repository/` — esp. `anki-operations-access.md` —,
`workspace/`), go-to-market (`specs/launch|media|outreach|content/`, `blog/`,
`content/editorial/`, launch site source), factory tooling (`toolchains/`, `recipes/`,
`.goose/`, `buzz-hive/`, budget gate), tool caches, and monorepo-internal paths in docs.

**CI / done-gate implications:**
- Monorepo `just ci` remains the pre-merge gate — the export inherits its guarantees;
  exporting is *not* a test step.
- New `ankitov-core` CI (GitHub Actions): `cargo fmt`/`clippy`/`test -p backend`, deck
  regeneration check (`generate_decks.py` output sha256 == `manifest.json`), gitleaks,
  license-header/SPDX check, `LICENSES` presence check, docs build (pruned mdBook).
- Monorepo done-gate additions: any PR touching an allowlisted path runs the export
  **dry-run** and attaches the diff; any PR adding a *new* top-level dir must state in the
  PR body whether it is OSS-boundary or private (keeps §2.2/§2.3 from rotting).
- The spec-done rule for future boundary changes: amend this file first, get owner
  sign-off, then change the allowlist — never the reverse.

---

## 7. Execution Checklist (orchestrator)

Phase 0 — owner confirmations (blockers):
- [ ] Confirm `tools/translate_ux_keys.py` export eligibility (check for embedded API endpoints/keys).
- [ ] Confirm docs pruned-SUMMARY set (esp. exclusion of strategic-blueprint/streaming-drm/clearing-house).
- [ ] Confirm default-exclude of `seed_users.py`/`seed_data.py`/`inject_db.py`.
- [ ] Confirm `specs/audits/*.txt` invariants may ship publicly.

Phase 1 — monorepo-side prep (in-monorepo edits, small):
- [ ] Flip `content/seed/manifest.json` license `TBC-OWNER-DECISION` → `CC0-1.0`; regenerate `math-definitions-en.apkg`; re-pin `sha256`.
- [ ] Add `license = "AGPL-3.0-only"` to `backend/Cargo.toml` (+ `[workspace.package]` license).
- [ ] Write `scripts/export-ankitov-core.sh` (allowlist rsync + templates + tripwire + `SYNC-MANIFEST.md`) and `scripts/oss-templates/` (OSS `Cargo.toml` w/ members=`["backend"]`, `LICENSE`, `content/seed/LICENSE`, `TRADEMARKS.md`, `README.md`).
- [ ] Copy `scripts/seed/import_check.sh` reference into the export allowlist as `content/seed/tools/import_check.sh`.
- [ ] Cleanup junk that could confuse the export: `--help`, `goose_config.yaml.bak.*`.

Phase 2 — first export:
- [ ] Run export script in dry-run; owner reviews the staged tree + diff.
- [ ] Run tripwire + `gitleaks` on staging; zero findings required.
- [ ] Verify staged tree: `cargo check` inside staging workspace (proves budget-gate removal is clean), `bash content/seed/tools/import_check.sh`, docs build.
- [ ] `git init` + signed initial commit in the new repo (`ankitov-core`); push to GitHub as **private** first.
- [ ] Owner reviews private repo → flip to public.

Phase 3 — CI + governance:
- [ ] Stand up `ankitov-core` GitHub Actions per §6; require green before public flip.
- [ ] Add PR-template/done-gate rule in monorepo: boundary-adjacent PRs run export dry-run; new top-level dirs declare boundary.
- [ ] Record the decision + this file's path in `project-knowledge/key-decisions.md`; add `specs/oss/` note that this doc is the canonical boundary reference (supersedes `specs/design/2026-08-26-boundary-map.md` where they conflict).

Phase 4 — steady state:
- [ ] Tag first sync `core-sync/<monorepo-ref>`; schedule recurring sync; back-port rule for public-only fixes.

---

*End of boundary map. This file is the single spec deliverable for G3; it was produced
from a read-only survey (tree + targeted file reads) on 2026-09-03.*
