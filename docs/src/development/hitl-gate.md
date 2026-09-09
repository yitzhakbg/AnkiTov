# HITL Gate Protocol (Human-in-the-Loop)

The approval gate belongs to Goose's **development environment**. It protects
repository changes and operator actions; it is not a product security boundary.

For product security, implement authentication, authorization, tenant/student
scoping, privacy minimization, retention, and auditability in the AnkiTov
backend. Those guarantees cannot be delegated to Goose, Rig, DeepSeek review,
or a local notification.

The normal development sequence is:

1. Goose presents the proposed change and its scope.
2. The operator reviews the relevant diff and approves or rejects it.
3. Goose runs product validation (`cd backend && cargo check` and relevant
   `cargo nextest run`).
4. `scripts/review-jj-commit.sh` obtains a DeepSeek review of the current patch
   using the upstream CLI's Git-format adapter.
5. The operator considers the findings and records the atomic change with `jj`.

DeepSeek review is a quality aid, not an automatic authorization mechanism. A
review finding can require a follow-up change; a clean review does not prove
correctness.
