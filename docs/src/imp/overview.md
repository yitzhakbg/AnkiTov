---
type: note
title: Interleaved Mastery Pipeline — Design Overview
---

# Interleaved Mastery Pipeline — Design Overview

The Interleaved Mastery Pipeline is the foundational requirement for Prong 1 (Management Console). Every student receives a single, server-side generated **Remediation Capsule** composed of dynamically mixed prerequisite tracks from a centralized track library.

_See the full spec at [`specs/design/2026-07-01-interleaved-mastery-pipeline.md`](https://github.com/yitzhakbg/AnkiTov/blob/main/specs/design/2026-07-01-interleaved-mastery-pipeline.md)._

## Core Concepts

| Concept | Description |
|---|---|
| **Track** | A tagged sub-collection of cards (e.g., "7th Grade Math — Fractions") |
| **Track Profile** | Named collection of tracks + N-value + target retention |
| **N-Lever** | Global/per-cohort sessions-per-week setting |
| **Capsule** | A fixed-size study session slice from the pool |
| **Compliance** | Sessions completed vs. N-value target per week |

## Pipeline Flow

```
Track Library (curated) → Track Profile (assigned) → Card Pool (overdue query)
  → FSRS Urgency Sort → N-Lever Slice → Capsule Session → Student Study → Telemetry
```
