//! Schedule triage — B-0035 sporadic-continuity scheduler
//! (spec `specs/design/2026-11-14-sporadic-continuity-scheduler.md`).
//!
//! Two moves, applied at session assembly in `capsule_generator` (which has a
//! DB handle in BOTH scheduler branches, so the in-house rollback path stays
//! behaviour-identical — spec §4):
//!
//! **Move #1 — backlog triage (deferral overlay).** After the due set is
//! ranked (R ascending) and the slicer has selected the session budget, the
//! overflow is re-spaced deterministically: the i-th overflow card (0-based,
//! R-ascending order) is deferred to `now + ceil(i / budget) × tranche_days`
//! — one budget-sized tranche per upcoming day across `K = ceil(overflow /
//! budget)` sessions. Deferrals are rows in `schedule_overrides` (AnkiTov's
//! own DB — **no `.anki2` writes, ever**; deferral is a scheduling decision,
//! not a memory-state change). The due-filter drops cards whose active
//! override has not elapsed yet. The plan is idempotent and self-healing:
//! every session assembly recomputes and upserts; answering a card clears its
//! override (`clear_override`).
//!
//! **Move #2 — early-review fill.** When the slicer selected fewer cards than
//! the budget, backfill from not-yet-due review cards with R ≥ `fill.r_min`,
//! ranked R descending, restricted to cards whose elapsed time since their
//! last review (merged `.anki2` revlog + `web_review_log`, via the derived
//! memory states) is ≥ `fill.interval_fraction` of their scheduled interval —
//! no same-day re-reviews. Fillers are ordinary early reviews; no overlay
//! rows are involved.
//!
//! Budget semantics: the session budget is computed from the **pre-filter**
//! due pool (all ranked due cards, deferred ones included). Deferred cards
//! must not shrink the budget, or the tranche plan (budget-sized sessions)
//! would collapse. A consequence: with triage disabled the budget always
//! equals the selected count, so fill never fires — the kill-switch restores
//! exactly the pre-B-0035 behaviour.
//!
//! Rollback: `ANKITOV_SCHEDULER_TRIAGE=off` (env kill-switch) or the
//! `triage.enabled` app setting disables both moves instantly.

use std::collections::HashMap;
use std::path::Path;

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
};

use crate::models::entities::schedule_override;
use crate::services::{audit_logger, capsule_slicer, fsrs_sort, fsrs_state, settings};

// ---------------------------------------------------------------------------
// Configuration (app_settings + env kill-switch)
// ---------------------------------------------------------------------------

pub const KEY_TRIAGE_ENABLED: &str = "triage.enabled";
pub const KEY_TRIAGE_TRANCHE_DAYS: &str = "triage.tranche_days";
pub const KEY_FILL_R_MIN: &str = "fill.r_min";
pub const KEY_FILL_INTERVAL_FRACTION: &str = "fill.interval_fraction";

pub const DEFAULT_TRIAGE_ENABLED: bool = true;
pub const DEFAULT_TRANCHE_DAYS: i64 = 1;
pub const DEFAULT_FILL_R_MIN: f64 = 0.85;
pub const DEFAULT_FILL_INTERVAL_FRACTION: f64 = 0.5;

/// Env kill-switch: `ANKITOV_SCHEDULER_TRIAGE=off` disables both moves
/// instantly (mirrors the `ANKITOV_SCHEDULER` pattern — unknown values are
/// ignored, never an error).
pub const ENV_KILL_SWITCH: &str = "ANKITOV_SCHEDULER_TRIAGE";

/// Reason stamped on deferral rows written by the backlog triage.
pub const REASON_BACKLOG_TRIAGE: &str = "backlog-triage";

const DAY_MS: i64 = 86_400_000;
const DAY_SECS: f64 = 86_400.0;

/// Resolved triage/fill configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct TriageConfig {
    pub enabled: bool,
    pub tranche_days: i64,
    pub fill_r_min: f64,
    pub fill_interval_fraction: f64,
}

impl Default for TriageConfig {
    fn default() -> Self {
        Self {
            enabled: DEFAULT_TRIAGE_ENABLED,
            tranche_days: DEFAULT_TRANCHE_DAYS,
            fill_r_min: DEFAULT_FILL_R_MIN,
            fill_interval_fraction: DEFAULT_FILL_INTERVAL_FRACTION,
        }
    }
}

/// Load the triage config from `app_settings`. Every key is independent:
/// a missing row, an unreadable value, or a DB error falls back to the
/// default for that key (never fails the session).
pub async fn load_config(db: &DatabaseConnection) -> TriageConfig {
    let mut cfg = TriageConfig::default();

    if let Ok(Some(v)) = settings::get(db, KEY_TRIAGE_ENABLED).await {
        if let Some(parsed) = parse_bool(&v) {
            cfg.enabled = parsed;
        }
    }
    if let Ok(Some(v)) = settings::get(db, KEY_TRIAGE_TRANCHE_DAYS).await {
        if let Ok(n) = v.trim().parse::<i64>() {
            if n >= 1 {
                cfg.tranche_days = n;
            }
        }
    }
    if let Ok(Some(v)) = settings::get(db, KEY_FILL_R_MIN).await {
        if let Ok(x) = v.trim().parse::<f64>() {
            if (0.0..=1.0).contains(&x) {
                cfg.fill_r_min = x;
            }
        }
    }
    if let Ok(Some(v)) = settings::get(db, KEY_FILL_INTERVAL_FRACTION).await {
        if let Ok(x) = v.trim().parse::<f64>() {
            if (0.0..=1.0).contains(&x) {
                cfg.fill_interval_fraction = x;
            }
        }
    }

    cfg
}

/// True when both moves are active: app setting on AND kill-switch not off.
pub fn feature_enabled(config: &TriageConfig) -> bool {
    config.enabled && !kill_switch_off()
}

/// True when the env kill-switch disables triage (`=off`). Any other value
/// (unset, garbage) keeps the feature governed by `triage.enabled`.
pub fn kill_switch_off() -> bool {
    matches!(std::env::var(ENV_KILL_SWITCH).as_deref(), Ok("off"))
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "on" | "yes" => Some(true),
        "false" | "0" | "off" | "no" => Some(false),
        _ => None,
    }
}

/// Unix milliseconds; 0 if the clock is somehow before the epoch (defensive —
/// never expected).
pub(crate) fn unix_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum TriageError {
    #[error("schedule triage database error: {0}")]
    Database(#[from] sea_orm::DbErr),
    #[error("schedule triage sort error: {0}")]
    Sort(#[from] fsrs_sort::FsrsSortError),
    #[error("schedule triage state error: {0}")]
    State(#[from] fsrs_state::FsrsStateError),
}

// ---------------------------------------------------------------------------
// Move #1 — deferral overlay
// ---------------------------------------------------------------------------

/// A planned deferral for one overflow card.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedDeferral {
    pub card_id: i64,
    /// Epoch-ms instant when the card re-enters the normal due flow.
    pub deferred_until: i64,
    /// Number of tranche sessions the plan spans (`K = ceil(len/budget)`).
    pub k_sessions: i32,
}

/// Pure tranche math (spec §2 step 3): the i-th overflow card (0-based, in
/// R-ascending order) is deferred to `now + ceil((i + 1) / budget) ×
/// tranche_days` days. Tranche 1 (`i = 0..budget-1`) lands `+tranche_days`
/// out, tranche 2 `+2·tranche_days`, and so on up to
/// `K = ceil(len / budget)` tranches — one budget-sized tranche per
/// upcoming day.
///
/// Note on the spec formula: the spec writes `now + ceil(i / capsule_budget)
/// days (0-based i)`. Read with a 0-based `i`, that puts a *single* card in
/// tranche 0 (eligible again at the very next assembly, including a
/// same-day one) and needs `K + 1` distinct days — contradicting the spec's
/// own prose, "one budget-sized tranche per upcoming day across
/// `K = ceil(overflow / budget)` sessions". The formula's `i` is therefore
/// read as the 1-based card ordinal, i.e. `ceil((i + 1) / budget)` for a
/// 0-based `i`: exactly K budget-sized tranches on days 1..K and a hard cap
/// of `budget` re-entries per calendar day.
pub fn plan_deferral(
    overflow: &[i64],
    budget: usize,
    tranche_days: i64,
    now_ms: i64,
) -> Vec<PlannedDeferral> {
    if overflow.is_empty() || budget == 0 {
        return Vec::new();
    }
    let budget = budget as i64;
    let tranche_days = tranche_days.max(1);
    let k = overflow.len().div_ceil(budget as usize) as i32;

    overflow
        .iter()
        .enumerate()
        .map(|(i, &card_id)| {
            // ceil((i + 1) / budget) via floor division (i ≥ 0, budget > 0;
            // i64::div_ceil is not stable on this toolchain).
            let days_ahead = (i as i64 / budget + 1) * tranche_days;
            PlannedDeferral {
                card_id,
                deferred_until: now_ms + days_ahead * DAY_MS,
                k_sessions: k,
            }
        })
        .collect()
}

/// Load the student's active overrides as `card_id → deferred_until (ms)`.
pub async fn load_active_overrides(
    db: &DatabaseConnection,
    student_id: &str,
) -> Result<HashMap<i64, i64>, sea_orm::DbErr> {
    let rows = schedule_override::Entity::find()
        .filter(schedule_override::Column::StudentId.eq(student_id))
        .filter(schedule_override::Column::ClearedAt.is_null())
        .all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.card_id, r.deferred_until))
        .collect())
}

/// Due-filter (spec §2 step 4): keep a card when it has no active override,
/// or when its `deferred_until` has elapsed (`now_ms >= deferred_until`).
/// Pure — the ranked order (R ascending) is preserved.
pub fn filter_by_overrides(
    ranked: &[fsrs_sort::RankedCard],
    overrides: &HashMap<i64, i64>,
    now_ms: i64,
) -> Vec<fsrs_sort::RankedCard> {
    ranked
        .iter()
        .filter(|c| match overrides.get(&c.card_id) {
            None => true,
            Some(&until) => now_ms >= until,
        })
        .cloned()
        .collect()
}

/// Recompute per-track counts from the post-filter eligible list.
///
/// The slicer's pass-2 gate iterates `track_counts.keys()` — a stale key for
/// a track whose eligible cards were all deferred would block the overflow
/// pass and under-select. Always feed the slicer counts recomputed from the
/// exact list it slices.
pub fn recompute_track_counts(eligible: &[fsrs_sort::RankedCard]) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for card in eligible {
        *counts.entry(card.track_tag.clone()).or_insert(0usize) += 1;
    }
    counts
}

/// Upsert the deferral plan: one active row per `(student, card)` — the
/// unique partial index on `(student_id, card_id) WHERE cleared_at IS NULL`
/// guarantees at most one active row per pair. Existing active rows are
/// updated in place (stale `deferred_until`/`k_sessions` replaced); missing
/// rows are inserted. Idempotent: re-planning the same tranche is a no-op
/// beyond refreshing the timestamps.
pub async fn upsert_deferrals(
    db: &DatabaseConnection,
    student_id: &str,
    plan: &[PlannedDeferral],
) -> Result<(), TriageError> {
    let now_ms = unix_now_ms();
    for item in plan {
        let existing = schedule_override::Entity::find()
            .filter(schedule_override::Column::StudentId.eq(student_id))
            .filter(schedule_override::Column::CardId.eq(item.card_id))
            .filter(schedule_override::Column::ClearedAt.is_null())
            .one(db)
            .await?;

        match existing {
            Some(row) => {
                let mut am: schedule_override::ActiveModel = row.into();
                am.deferred_until = ActiveValue::Set(item.deferred_until);
                am.k_sessions = ActiveValue::Set(item.k_sessions);
                am.reason = ActiveValue::Set(Some(REASON_BACKLOG_TRIAGE.to_string()));
                am.update(db).await?;
            }
            None => {
                let am = schedule_override::ActiveModel {
                    id: ActiveValue::Set(uuid::Uuid::new_v4().to_string()),
                    student_id: ActiveValue::Set(student_id.to_string()),
                    card_id: ActiveValue::Set(item.card_id),
                    deferred_until: ActiveValue::Set(item.deferred_until),
                    reason: ActiveValue::Set(Some(REASON_BACKLOG_TRIAGE.to_string())),
                    k_sessions: ActiveValue::Set(item.k_sessions),
                    created_at: ActiveValue::Set(now_ms),
                    cleared_at: ActiveValue::Set(None),
                };
                am.insert(db).await?;
            }
        }
    }
    Ok(())
}

/// Clear the active override for `(student, card)` — called when the student
/// answers the card (review state recorded). The row is retired, not deleted:
/// `cleared_at` is stamped and the row stays as an audit trail.
pub async fn clear_override(
    db: &DatabaseConnection,
    student_id: &str,
    card_id: i64,
) -> Result<(), sea_orm::DbErr> {
    let existing = schedule_override::Entity::find()
        .filter(schedule_override::Column::StudentId.eq(student_id))
        .filter(schedule_override::Column::CardId.eq(card_id))
        .filter(schedule_override::Column::ClearedAt.is_null())
        .one(db)
        .await?;

    if let Some(row) = existing {
        let mut am: schedule_override::ActiveModel = row.into();
        am.cleared_at = ActiveValue::Set(Some(unix_now_ms()));
        am.update(db).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Move #2 — early-review fill
// ---------------------------------------------------------------------------

/// A fill candidate with its computed recall and elapsed time.
#[derive(Debug, Clone, PartialEq)]
pub struct FillCandidate {
    pub card_id: i64,
    pub recall: f64,
    /// Fractional days since the card's last review (merged revlog + web).
    pub elapsed_days: f64,
    /// Scheduled interval in days (Anki `cards.ivl`).
    pub ivl: i64,
}

/// Pure fill selection (spec §3): keep candidates with
/// `recall >= r_min` AND `elapsed_days >= interval_fraction × ivl`
/// (no same-day re-reviews), rank by R descending with `card_id` ascending
/// as the deterministic tie-break, take at most `shortfall`.
pub fn select_fill(
    candidates: &[FillCandidate],
    shortfall: usize,
    r_min: f64,
    interval_fraction: f64,
) -> Vec<i64> {
    if shortfall == 0 {
        return Vec::new();
    }

    let mut eligible: Vec<&FillCandidate> = candidates
        .iter()
        .filter(|c| c.recall >= r_min && c.elapsed_days >= interval_fraction * c.ivl as f64)
        .collect();

    // R descending; card_id ascending breaks ties deterministically. The
    // in-house recall model returns a constant 0.95 for every not-yet-due
    // card, so the tie-break is what makes the in-house fill order stable.
    eligible.sort_by(|a, b| {
        b.recall
            .partial_cmp(&a.recall)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.card_id.cmp(&b.card_id))
    });

    eligible
        .into_iter()
        .take(shortfall)
        .map(|c| c.card_id)
        .collect()
}

/// Query not-yet-due cards, derive their memory states (merged revlog +
/// `web_review_log` — the same states both scheduler modes rank with), and
/// select up to `shortfall` early-review fillers.
///
/// The rusqlite connection is confined to a sync block and dropped before any
/// `.await` (Send requirement, same pattern as `fetch_and_sort_cached`).
/// `ensure_states` runs in BOTH scheduler modes: the interval-fraction gate
/// must see web-reviewer answers on the in-house path too, or the two modes
/// would diverge on which cards are fill-eligible (spec §4 parity).
pub(crate) async fn fill_session(
    db: &DatabaseConnection,
    student_id: &str,
    anki2_path: impl AsRef<Path>,
    track_tags: &[String],
    shortfall: usize,
    config: &TriageConfig,
) -> Result<Vec<i64>, TriageError> {
    if shortfall == 0 {
        return Ok(Vec::new());
    }

    // ── Sync phase: open collection, query candidates, extract snapshot ──
    let (facts, mut snapshot, ctx) = {
        let (conn, ctx) = fsrs_sort::open_collection(anki2_path)?;
        let facts = fsrs_sort::query_fill_candidates(&conn, track_tags, ctx)?;
        let card_ids: Vec<i64> = facts.iter().map(|f| f.card_id).collect();
        let snapshot = fsrs_state::extract_snapshot(&conn, &card_ids)?;
        (facts, snapshot, ctx)
    }; // conn dropped — never held across .await
    if facts.is_empty() {
        return Ok(Vec::new());
    }

    let mode = fsrs_sort::scheduler_mode();
    let states = fsrs_state::ensure_states(db, student_id, &mut snapshot).await?;

    let candidates: Vec<FillCandidate> = facts
        .iter()
        .filter_map(|f| {
            // Cards with no known last review cannot prove the interval
            // fraction — excluded (gate unprovable).
            let last_review_secs = states.get(&f.card_id)?.last_review_secs?;

            let recall = match mode {
                fsrs_sort::SchedulerMode::FsrsRs => fsrs_sort::fsrs_recall(
                    &fsrs_sort::CardFacts {
                        card_id: f.card_id,
                        note_id: f.note_id,
                        note_tags: f.note_tags.clone(),
                        ivl: f.ivl,
                        card_type: 2,
                        due: f.due,
                        factor: 0,
                    },
                    &ctx,
                    &states,
                ),
                // In-house recall is constant 0.95 for not-yet-due cards
                // (overdue_days <= 0 branch) — the card_id tie-break in
                // `select_fill` provides the deterministic order.
                fsrs_sort::SchedulerMode::InHouse => {
                    fsrs_sort::compute_approximate_recall(-1, f.ivl)
                }
            };

            let elapsed_days = (ctx.now_secs - last_review_secs) as f64 / DAY_SECS;
            Some(FillCandidate {
                card_id: f.card_id,
                recall,
                elapsed_days,
                ivl: f.ivl,
            })
        })
        .collect();

    Ok(select_fill(
        &candidates,
        shortfall,
        config.fill_r_min,
        config.fill_interval_fraction,
    ))
}

// ---------------------------------------------------------------------------
// Orchestrator — called from capsule_generator after the slicer
// ---------------------------------------------------------------------------

/// Outcome of the post-slice triage, consumed by `capsule_generator` for the
/// session payload and the telemetry event.
#[derive(Debug, Clone, PartialEq)]
pub struct TriageOutcome {
    /// Due cards ranked by the scheduler (pre-filter pool size).
    pub due_count: usize,
    /// Session budget the slicer targeted (pre-filter pool).
    pub budget: usize,
    /// Overflow cards deferred by this assembly.
    pub deferred_count: usize,
    /// Tranche count `K = ceil(overflow / budget)` (0 when nothing deferred).
    pub k: usize,
    /// Early-review fillers appended to the session.
    pub filled_card_ids: Vec<i64>,
}

/// Apply both triage moves after the slicer ran on the eligible list:
/// defer the overflow into daily tranches, then fill the shortfall with
/// early reviews.
pub async fn post_slice_triage(
    db: &DatabaseConnection,
    student_id: &str,
    anki2_path: impl AsRef<Path>,
    track_tags: &[String],
    sort_result: &fsrs_sort::SortResult,
    slice: &capsule_slicer::SliceResult,
    budget: usize,
    config: &TriageConfig,
) -> Result<TriageOutcome, TriageError> {
    let now_ms = unix_now_ms();

    // ── Move #1: defer the overflow into budget-sized daily tranches ────
    let plan = plan_deferral(&slice.remaining, budget, config.tranche_days, now_ms);
    if !plan.is_empty() {
        upsert_deferrals(db, student_id, &plan).await?;
    }

    // ── Move #2: early-review fill for thin sessions ────────────────────
    let shortfall = budget.saturating_sub(slice.actual_size);
    let filled = fill_session(
        db,
        student_id,
        anki2_path,
        track_tags,
        shortfall,
        config,
    )
    .await?;

    Ok(TriageOutcome {
        due_count: sort_result.ranked.len(),
        budget,
        deferred_count: plan.len(),
        k: plan.first().map(|p| p.k_sessions as usize).unwrap_or(0),
        filled_card_ids: filled,
    })
}

/// Telemetry payload for one session assembly (spec: one triage event per
/// assembly, on the existing audit path). Emitted even when the feature is
/// disabled — the disabled events are the A/B baseline for the Testing Army
/// evaluation.
pub fn telemetry_payload(
    student_id: &str,
    outcome: &TriageOutcome,
    triage_enabled: bool,
) -> String {
    serde_json::json!({
        "student": student_id,
        "due_count": outcome.due_count,
        "budget": outcome.budget,
        "deferred_count": outcome.deferred_count,
        "k": outcome.k,
        "filled_count": outcome.filled_card_ids.len(),
        "triage_enabled": triage_enabled,
    })
    .to_string()
}

/// Audit event type for the triage telemetry event.
pub const AUDIT_EVENT_TYPE: &str = "schedule_triage";

/// Convenience wrapper so callers do not import `audit_logger` just for this.
pub async fn log_triage_event(
    db: &DatabaseConnection,
    student_id: &str,
    session_id: &str,
    outcome: &TriageOutcome,
    triage_enabled: bool,
) {
    let payload = telemetry_payload(student_id, outcome, triage_enabled);
    audit_logger::log_event(
        db,
        AUDIT_EVENT_TYPE,
        &payload,
        student_id,
        Some(session_id),
    )
    .await;
}

// ---------------------------------------------------------------------------
// Tests — pure tranche math, override filter, fill selection, kill-switch
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ranked(card_id: i64, track: &str) -> fsrs_sort::RankedCard {
        fsrs_sort::RankedCard {
            card_id,
            note_id: card_id,
            track_tag: track.to_string(),
            approximate_recall: 0.5,
            overdue_days: 1,
        }
    }

    // ── Tranche math ─────────────────────────────────────────────────────

    #[test]
    fn tranche_math_budget_24_overflow_100() {
        let now = 1_700_000_000_000i64;
        let overflow: Vec<i64> = (1..=100).collect();
        let plan = plan_deferral(&overflow, 24, 1, now);
        assert_eq!(plan.len(), 100);
        // Tranche 1 (i=0..23) → +1d (first budget-sized tranche per upcoming
        // day); tranche 2 (i=24..47) → +2d; … tranche 5 (i=96..99) → +5d.
        // K = ceil(100/24) = 5 budget-sized tranches on days 1..5.
        assert_eq!(plan[0].deferred_until, now + DAY_MS);
        assert_eq!(plan[23].deferred_until, now + DAY_MS);
        assert_eq!(plan[24].deferred_until, now + 2 * DAY_MS);
        assert_eq!(plan[47].deferred_until, now + 2 * DAY_MS);
        assert_eq!(plan[48].deferred_until, now + 3 * DAY_MS);
        assert_eq!(plan[71].deferred_until, now + 3 * DAY_MS);
        assert_eq!(plan[72].deferred_until, now + 4 * DAY_MS);
        assert_eq!(plan[95].deferred_until, now + 4 * DAY_MS);
        assert_eq!(plan[96].deferred_until, now + 5 * DAY_MS);
        assert_eq!(plan[99].deferred_until, now + 5 * DAY_MS);
        assert_eq!(plan[0].k_sessions, 5);
        assert_eq!(plan[99].k_sessions, 5);
    }

    #[test]
    fn tranche_math_custom_tranche_days() {
        let now = 1_700_000_000_000i64;
        let plan = plan_deferral(&[10, 11, 12], 2, 3, now);
        // Tranche 1 (i=0,1) → +3d; tranche 2 (i=2) → +6d.
        assert_eq!(plan[0].deferred_until, now + 3 * DAY_MS);
        assert_eq!(plan[1].deferred_until, now + 3 * DAY_MS);
        assert_eq!(plan[2].deferred_until, now + 6 * DAY_MS);
        assert_eq!(plan[0].k_sessions, 2); // ceil(3/2)
    }

    #[test]
    fn tranche_math_empty_overflow_and_zero_budget() {
        assert!(plan_deferral(&[], 24, 1, 0).is_empty());
        assert!(plan_deferral(&[1, 2, 3], 0, 1, 0).is_empty());
    }

    // ── Override filter ──────────────────────────────────────────────────

    #[test]
    fn override_filter_keeps_unoverridden_and_expired() {
        let now = 1_700_000_000_000i64;
        let ranked = vec![ranked(1, "math"), ranked(2, "math"), ranked(3, "vocab")];
        let overrides: HashMap<i64, i64> = [(2, now + DAY_MS), (3, now - 1)].into_iter().collect();
        let eligible = filter_by_overrides(&ranked, &overrides, now);
        // Card 1: no override → keep. Card 2: future deferral → drop.
        // Card 3: deferral elapsed → keep.
        assert_eq!(eligible.iter().map(|c| c.card_id).collect::<Vec<_>>(), [1, 3]);
    }

    #[test]
    fn override_filter_boundary_now_equals_until_is_eligible() {
        let now = 1_700_000_000_000i64;
        let ranked = vec![ranked(7, "math")];
        let overrides: HashMap<i64, i64> = [(7, now)].into_iter().collect();
        let eligible = filter_by_overrides(&ranked, &overrides, now);
        assert_eq!(eligible.len(), 1, "deferred_until == now must re-enter");
    }

    #[test]
    fn recompute_track_counts_drops_empty_tracks() {
        let eligible = vec![ranked(1, "math"), ranked(2, "math"), ranked(3, "vocab")];
        let counts = recompute_track_counts(&eligible);
        assert_eq!(counts.get("math"), Some(&2));
        assert_eq!(counts.get("vocab"), Some(&1));
        assert_eq!(counts.len(), 2);
        // A track absent from the eligible list must be absent from the map
        // (stale keys would block the slicer's overflow pass).
        assert!(counts.get("history").is_none());
    }

    // ── Fill selection ───────────────────────────────────────────────────

    fn candidate(card_id: i64, recall: f64, elapsed_days: f64, ivl: i64) -> FillCandidate {
        FillCandidate {
            card_id,
            recall,
            elapsed_days,
            ivl,
        }
    }

    #[test]
    fn fill_respects_r_floor_and_interval_fraction() {
        let candidates = vec![
            candidate(1, 0.90, 6.0, 10), // passes both gates
            candidate(2, 0.80, 6.0, 10), // below R floor
            candidate(3, 0.90, 4.0, 10), // elapsed < 0.5 × ivl (same-day-ish)
            candidate(4, 0.86, 5.0, 10), // passes both (boundary R)
        ];
        let picked = select_fill(&candidates, 10, 0.85, 0.5);
        assert_eq!(picked, vec![1, 4], "R descending order");
    }

    #[test]
    fn fill_caps_at_shortfall_and_tie_breaks_by_card_id() {
        let candidates = vec![
            candidate(9, 0.95, 6.0, 10),
            candidate(2, 0.95, 6.0, 10),
            candidate(7, 0.95, 6.0, 10),
        ];
        // Equal recall (the in-house constant-0.95 case) → card_id ascending.
        let picked = select_fill(&candidates, 2, 0.85, 0.5);
        assert_eq!(picked, vec![2, 7]);
    }

    #[test]
    fn fill_zero_shortfall_is_empty() {
        let candidates = vec![candidate(1, 0.95, 6.0, 10)];
        assert!(select_fill(&candidates, 0, 0.85, 0.5).is_empty());
    }

    // ── Kill-switch ──────────────────────────────────────────────────────

    #[test]
    fn kill_switch_only_off_value_disables() {
        std::env::remove_var(ENV_KILL_SWITCH);
        assert!(!kill_switch_off());
        std::env::set_var(ENV_KILL_SWITCH, "off");
        assert!(kill_switch_off());
        std::env::set_var(ENV_KILL_SWITCH, "on");
        assert!(!kill_switch_off(), "only the exact value 'off' disables");
        std::env::set_var(ENV_KILL_SWITCH, "garbage");
        assert!(!kill_switch_off());
        std::env::remove_var(ENV_KILL_SWITCH);
    }

    #[test]
    fn feature_enabled_requires_config_and_no_kill_switch() {
        std::env::remove_var(ENV_KILL_SWITCH);
        let cfg = TriageConfig::default();
        assert!(feature_enabled(&cfg));
        let cfg_off = TriageConfig {
            enabled: false,
            ..cfg.clone()
        };
        assert!(!feature_enabled(&cfg_off));
        std::env::set_var(ENV_KILL_SWITCH, "off");
        assert!(!feature_enabled(&cfg), "kill-switch overrides enabled=true");
        std::env::remove_var(ENV_KILL_SWITCH);
    }

    // ── Telemetry payload ────────────────────────────────────────────────

    #[test]
    fn telemetry_payload_shape() {
        let outcome = TriageOutcome {
            due_count: 60,
            budget: 25,
            deferred_count: 35,
            k: 2,
            filled_card_ids: vec![1, 2, 3],
        };
        let payload = telemetry_payload("student-1", &outcome, true);
        let v: serde_json::Value = serde_json::from_str(&payload).expect("json");
        assert_eq!(v["student"], "student-1");
        assert_eq!(v["due_count"], 60);
        assert_eq!(v["budget"], 25);
        assert_eq!(v["deferred_count"], 35);
        assert_eq!(v["k"], 2);
        assert_eq!(v["filled_count"], 3);
        assert_eq!(v["triage_enabled"], true);
    }
}
