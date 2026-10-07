# Interleaved Foundational Mastery Pipeline

**Phase:** Design
**Date:** 2026-07-01
**Author:** Derived from external proposal, revised for AnkiTov architecture
**Prong:** 1 (Management Console) — Foundational Requirement
**Status:** Draft

---

> **Boundary note (2026-08-02):** This is an AnkiTov product design. Goose,
> its extensions and recipes, TaskLite, codebase-memory-mcp, LanceDB/Librarian,
> and DeepSeek Code Review are Goose-side development tools. The Rig client,
> factory-harness binary, and budget gate are AnkiTov components, not Goose
> tools. Product rate limiting, authorization, and telemetry controls must be
> implemented in the backend.

## Problem Statement

Schools deploying spaced repetition face a structural tension: institution-wide
curriculum follows a uniform calendar, but individual students arrive with
wildly different prerequisite gaps. A 7th-grade math unit assumes fraction
fluency; students who failed the 6th-grade fraction diagnostic can't keep up.

Assigning separate "remediation decks" per subject creates UX fragmentation,
backlog anxiety, and administrative overhead. The student sees multiple "catch-up
piles," each growing independently, while the teacher manages a fragmented deck
tree. The cognitive benefit of **interleaving** — mixing disparate subjects to
strengthen recall discrimination — is lost.

---

## Solution: The Interleaved Mastery Pipeline

Every student receives a single, unified "Remediation Capsule." While the
interface is identical for all students, the internal composition is a
customized cocktail of independent mastery tracks, tailored to repair specific
historical learning gaps. All processing occurs **server-side** via the
Management Console, with the student interacting through a single PWA button.

### Core Principles

| Principle | Description |
|---|---|
| **The Content** | A single Master Capsule per student, dynamically compiled as a mixed matrix of prerequisite tracks. Tracks never merge into a flat deck; each card retains its track provenance for analytics. |
| **The Cadence** | Flexible sessions managed via a global instructor lever ($N$ sessions per week), completely decoupled from the active curriculum. The lever is a Management Console control. |
| **The Student Experience** | A single, low-stress daily capsule of diverse subjects, shielding students from backlog anxiety. The PWA shows one button: "Start Today's Session." |
| **The Analytics** | Per-track retention, stability, and compliance metrics are surfaced on the Instructor Dashboard — the student's compliance is the primary lifestyle metric; per-track health is the diagnostic layer. |

---

## Pillar 1: Content Architecture (Server-Side Track Cocktail)

The institution maintains a centralized library of modular, tag-based tracks in
the Management Console. Instead of assigning separate decks, the instructor
assigns a **Track Profile** to a student group. The server merges tracks into
one active pool at session generation time.

```
[Central Track Library — Management Console]
├── Track 1: Decimals & Percentages (7th Grade Math)
├── Track 2: Roots & Prefixes (6th Grade Literacy)
└── Track 3: Data Literacy & Graphs (8th Grade Science)
        │
        ▼ (Merged & Interleaved Server-Side into)
┌────────────────────────────────────────┐
│  Student's Single "Remediation Capsule" │
│  [Card 1: Math] → [Card 2: Vocab]      │
└────────────────────────────────────────┘
```

- **Asynchronous Mixes:** Student A's capsule might mix 7th-grade math with
  6th-grade English. Student B's capsule might contain only 5th-grade
  arithmetic recovery. Both interact with the same PWA button.
- **Track Provenance:** Every card retains its track ID. The FSRS scheduler
  evaluates per-card independently, and the analytics layer aggregates by
  track for per-subject diagnostic visibility.
- **The Interleaving Advantage:** The system sorts the session queue by
  lowest probability of recall ($R$), which naturally produces an interleaved
  sequence — subjects mix based on which concepts are closest to slipping
  away.

### Acceptance Scenario: Track Profile Assignment

**Given** a class of 30 students with varying prerequisite diagnostic scores
**When** the instructor assigns "Math Rescue Mix" to 5 low-scoring students
  and "General Literacy Mix" to the remaining 25
**Then** each student sees exactly one "Start Session" button in their PWA
**And** the low-scoring students' capsules contain math remediation cards
  interleaved with literacy cards
**And** the management console logs the track profile assignment to the audit
  ledger with instructor ID, timestamp, and cohort scope

---

## Pillar 2: Algorithmic Calibration (FSRS Cross-Domain Pools)

Because each capsule contains a highly diverse mix of items, the FSRS engine
must be configured to evaluate memory decay across varied domains without
causing card accumulation loops.

- **Target Retention ($R$) Optimization:** Default class-wide target retention
  of **78% – 80%**, configurable per cohort or per track profile in the
  Management Console. This range ensures cards across mixed categories are
  pushed to wider intervals early, keeping combined daily volume low.
- **Independent Item History:** FSRS naturally evaluates each card's stability
  ($S$) independently. A difficult math card will not warp scheduling intervals
  for vocabulary cards in the same capsule.
- **Track-Level Aggregation:** The analytics pipeline aggregates per-card
  parameters by track tag, enabling the instructor dashboard to display
  per-subject health even though the student sees a single capsule.
- **Dynamic Tuning:** A backend product policy monitors capsule size trends. If a
  student's daily volume exceeds the configurable threshold (default: 25 cards),
  the backend adjusts global $N$ and alerts the instructor. The development
  budget gate does not participate in this runtime decision.

### Acceptance Scenario: FSRS Cross-Domain Stability

**Given** a student with a math track (stability $S = 5$ days) and a literacy
  track (stability $S = 30$ days)
**When** the student answers a math card incorrectly
**Then** the math card's stability decreases independently
**And** the literacy card's scheduling interval remains unchanged
**And** the next session's lowest-$R$ sort includes the struggling math card
  alongside stable literacy cards, producing an interleaved mix

---

## Pillar 3: Server-Side Session Generation (Dynamic Interleaved Slicing)

Unlike the original proposal (which relied on a client-side add-on), AnkiTov
implements the slicing logic **entirely server-side** in the Management
Console backend (Loco.rs). The student's PWA simply renders the pre-computed
session and dispatches review telemetry.

### Pipeline (Loco.rs Service)

1. **Fetch Global Configurations:** On session request, the server resolves
   the current value of $N$ (target sessions per week) for the student's
   cohort and confirms their assigned track profile.

2. **Compile Overdue Pool:** The server queries the libSQL database, gathering
   all due cards across all tracks assigned to the student into a single
   virtual queue. Cards retain track provenance metadata.

3. **Calculate Card Budget (Capsule Slicing):** Instead of exposing the true
   mixed backlog total, the server computes capsule size as the minimum
   of three constraints: (a) **N-spread:** pool_size / $N$,
   (b) **Hard cap:** 25 cards (absolute ceiling), (c) **Time bound:**
   session_duration_minutes × 60 / estimated_per_card_time_seconds
   (per-student average from revlog telemetry). If cards remain after
   the slice, they spill to the next session — the student never
   sees the backlog.

4. **Sort by Urgency (Interleaved Selection):** The capsule is populated by
   selecting cards with the **lowest Probability of Recall ($R$) first.**
   Because multiple tracks are mixed, this urgency sort naturally produces an
   interleaved sequence — subjects mix based on which concepts are closest to
   slipping away. The sort respects a minimum per-track presence to prevent a
   single failing track from dominating the entire capsule.

5. **Render Session (PWA):** The PWA displays a single button:
   **"Start Today's Session."** A clean tracker bar shows progress toward the
   weekly goal: **"Sessions This Week: 2 / N."** No deck lists, no card
   counters, no backlog numbers.

6. **Telemetry Dispatch:** Each card answer triggers a telemetry ping via the
   DRM add-on layer (`gui_hooks.reviewer_did_answer_card` for streamed
   sessions, or direct HTTP dispatch for PWA-native cards). Pings are written
   to the append-only SeaORM audit ledger.

### Acceptance Scenario: Capsule Slicing

**Given** a student with $N = 3$ sessions per week and a mixed overdue pool
  of 90 cards
**When** the server generates a session capsule
**Then** the capsule contains exactly 25 cards (75 / 3, respecting the cap)
**And** the remaining 65 cards are deferred to subsequent sessions
**And** the PWA shows "Sessions This Week: 0 / 3"
**And** the student sees no indication of the deferred backlog

---

## Pillar 4: Instructor Controls & Cohort Mixing

Instructors operate the system through the Management Console dashboard.
Rather than micromanaging individual flashcards, they manage **Track Mix
Assignments** and the **Global Weekly Sessions (N-Lever)** to regulate cognitive pace.

### The Instructor Management Matrix

| Dashboard Action | Operational Impact | Classroom Context |
|---|---|---|
| **Assign Track Profiles** | Blends specific subject modules into a single student's background pool without creating new decks. | A teacher sets up "Math Rescue Mix" for five students who failed the prerequisite diagnostic, while the rest gets "General Literacy Mix." |
| **Adjust N (e.g., N = 3)** | Recalculates the active capsule sizes across all mixed decks for every student in the class. | The standard weekly cadence. Students complete their single mixed session during a 10-minute quiet slot on any three days of the week. |
| **Drop Lever to N = 1 or 0** | Shrinks session sizes to zero or maintenance levels, hiding the mixed backlog. | Deployed during high-stress school periods (e.g., standardized testing weeks) to protect students from unnecessary cognitive overhead. |
| **View Per-Track Health** | Surfaces per-track retention, stability, and volume trends for any student or cohort. | Teacher sees that Chloe's math track has 20% retention — the math interval is too aggressive — while her literacy track is healthy at 80%. |

### Streamlined Compliance Tracking

Grading remains decoupled from subject mastery. The primary dashboard metric
is: **Did the student complete their mixed capsules this week?**

```
[Classroom Analytics Dashboard — Management Console]
Student     Track Profile             Weekly Goal   Completed   Status        Track Health
──────────────────────────────────────────────────────────────────────────────────────────
Aria M.     Math Recovery + Vocab    3 Sessions    3 / 3       [Compliant]   Math:72%  Lit:85%
Benji K.    Advanced Literacy Mix    3 Sessions    3 / 3       [Compliant]   Lit:88%  Sci:79%
Chloe T.    Arithmetic Gaps Only     3 Sessions    1 / 3       [Attention]   Arith:45%
```

If Chloe drops to 1 out of 3 sessions, the teacher knows she is falling behind
on her fundamental recovery routine. The instructor can drill into Chloe's
per-track health to see whether the issue is arithmetic difficulty specifically
or general compliance. Intervention happens during homeroom — not because
Chloe is "bad at math," but simply to help her find time for her daily 8-minute
capsule.

### Acceptance Scenario: N-Lever Adjustment

**Given** 30 students all assigned $N = 3$ sessions per week
**When** the instructor drops the lever to $N = 1$ during standardized testing
  week
**Then** each student's next capsule size is recalculated to $1/3$ of the
  previous size (or the per-card cap, whichever is smaller)
**And** the PWA shows "Sessions This Week: 0 / 1"
**And** the management console logs the lever adjustment to the audit ledger
  with instructor ID, old value, new value, and timestamp
**And** after testing week ends and the lever returns to $N = 3$, overdue
  cards are gradually reintroduced (no backlog shock)

---

## Integration With AnkiTov Architecture

### Management Console (Prong 1) — Direct Integration Points

| Component | Integration |
|---|---|
| **Track Library** | Lives in the Management Console's deck management section. Tracks are tag-based sub-collections, not separate decks. |
| **Track Profile Assignment** | Uses the existing user/group management system. A track profile is a first-class entity (name, track list, target retention, N value, session_duration_minutes). |
| **N-Lever** | A global config setting in the Management Console, overridable per cohort and per track profile. |
| **Capsule Generation** | A Loco.rs background job or on-demand service. Query: libSQL overdue cards filtered by track profile → FSRS sort → slice → return. |
| **Per-Track Health** | Aggregated from the existing telemetry ledger. FSRS parameters per card → grouped by track tag → displayed in the analytics dashboard. |
| **Audit Logging** | Every N-change, profile-assignment, and capsule-generation event is written to the SeaORM append-only ledger. |

### Product Rate-Limit Integration

Product rate limits are backend concerns and must be implemented in AnkiTov
services/controllers, with tenant and student authorization. They are not
provided by Goose or by the development budget gate.

The separate `ankitov-budget-gate/` sidecar is an AnkiTov component, not a Goose
tool. Its deployment and authorization role must be defined explicitly within
product architecture; it cannot substitute for backend product policy.

### Streaming Paradigm Compatibility

This design is fully compatible with the Selkies streaming architecture:
- **Thin session-driver addon** (~200-400 lines Python, `addons/ankitov-session-driver/`)
  bridges capsule card_ids to Anki's reviewer via filtered decks. It has no
  user-facing UI and communicates with the backend via HTTP (localhost:18765).
- All track mixing, FSRS scheduling, and capsule slicing happens in Loco.rs
- The PWA is the only student-facing interface (addon operates silently)
- The container image remains read-only, tamper-proof
- All data traverses the secure HTML5 video feed

---

## Open Questions for Implementation

1. **Minimum per-track presence:** When sorting by lowest $R$, a struggling
   track can dominate. What is the minimum per-track card allocation per
   session? (Suggested: at least 1 card per assigned track, or 20% of the
   capsule, whichever is smaller.)

2. **Backlog re-entry after N-reduction:** When N drops from 3 to 1 and back,
   how should overdue cards be re-introduced? (Suggested: gradual backfill at
   the new capsule rate, capped at 1.5× the standard capsule size, to prevent
   shock.)

3. **N-Lever granularity:** Is N a whole number or can it be fractional (e.g.,
   N=0.5 for biweekly sessions)? (Suggested: whole numbers only for v1.)

4. **Track profile versioning:** If a track's content changes, what happens to
   existing students' scheduled cards? (Suggested: reassess all in-progress
   cards with new content, preserving old stability estimates as priors.)

5. **Integration with anki-mcp-server:** The existing anki-mcp-server has
   `retention_stats`, `deck_health`, `find_problems`. Should these operate on
   the interleaved capsule or on the underlying track partitions?
   (Suggested: per-track, with an aggregate view.)

6. **Per-card timing cold start:** For time-bound capsule sizing, what default
   seconds-per-card should be used for new students with no revlog history?
   (Suggested: 25 seconds as a conservative starting default, adjusted after
   3-5 sessions of per-student, per-track data accumulate.)

7. **Session driver addon deployment:** Should the addon (`ankitov-session-driver/`)
   be installed inside the Selkies container image (preferred) or mounted as a
   volume at runtime? (Suggested: baked into the container image, enabled by
   default, no configuration needed.)
