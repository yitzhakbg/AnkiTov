---
type: note
title: Model Appraisal Logs
---

# Model Appraisal Logs

This page distinguishes models used by Goose from AI features that may run in
AnkiTov.

| Concern | Model/component | Boundary |
|---|---|---|
| Goose development sessions | Configured Goose provider/model | Goose-side development |
| DeepSeek code review | `hustcer/deepseek-review` local CLI | Goose-side quality gate |
| Dashboard NLU | Rig + configured DeepSeek in `backend/src/services/rig_nlu.rs` | Optional AnkiTov product path |

The factory-harness component and backend NLU adapter are separate AnkiTov code
paths. Goose is the separate development environment, not a product runtime
component and not the owner of Rig.

Model choice, endpoint, and token configuration must remain externalized and
must never be committed as secrets.
