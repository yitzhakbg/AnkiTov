//! FSRS Card Sort — retrieve due cards from Anki SQLite and rank by urgency.
//!
//! Opens the `.anki2` database read-only (immutable mode) to query overdue
//! review and learning cards filtered by note tags, then sorts **most urgent
//! first** (lowest recall probability). Also queries `revlog.time` to compute
//! per-track average seconds-per-card for the capsule slicer's time-bound
//! calculation.
//!
//! ## Two schedulers (T-116 fsrs-rs production swap)
//!
//! * **fsrs-rs** (production default) — ranks by FSRS-6 retrievability
//!   `R = current_retrievability(MemoryState, elapsed_days)` with memory
//!   states derived and persisted by [`super::fsrs_state`] (cache-aside,
//!   keyed by `(student_id, card_id)`). Entry point:
//!   [`fetch_and_sort_cached`].
//! * **in-house** (rollback + property-suite anchor) — the original
//!   half-life decay model, unchanged. Entry point: [`fetch_and_sort`].
//!   Selected at runtime with `ANKITOV_SCHEDULER=in-house`.
//!
//! Both paths share the same pipeline (due-filter → per-card recall →
//! urgency sort → per-track time estimates); only the recall function
//! differs. New cards are scored with the same sentinel recall under both
//! schedulers, so new-card ordering is identical.
//!
//! ## In-house Approximate Recall Model (fallback)
//!
//! For relative ordering (not exact FSRS prediction):
//!
//! ```text
//! overdue_days = today - day_due
//! if overdue_days <= 0:  R ≈ 0.95  (not yet due; already stable)
//! else:                  R ≈ 0.90 × 0.5^(overdue_days / max(ivl, 1))
//! ```
//!
//! This half-life decay model yields correct **relative** ordering:
//! cards further past due with shorter intervals are most urgent.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use sea_orm::DatabaseConnection;

use crate::services::fsrs_state::{self, DerivedState};

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors from the scheduling pipeline: `.anki2` access, query failures,
/// missing collection metadata, and memory-state derivation/persistence.
#[derive(Debug, thiserror::Error)]
pub enum FsrsSortError {
    #[error("cannot open {0}: {1}")]
    CannotOpen(String, #[source] rusqlite::Error),

    #[error("SQL query failed: {0}")]
    QueryFailed(String, #[source] rusqlite::Error),

    #[error("no collection creation time found")]
    NoCollectionCrt,

    #[error("memory-state derivation/persistence failed: {0}")]
    State(#[from] fsrs_state::FsrsStateError),
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A due card with its track tag and computed urgency.
#[derive(Debug, Clone)]
pub struct RankedCard {
    pub card_id: i64,
    pub note_id: i64,
    /// The track tag this card matches (first matching tag).
    pub track_tag: String,
    /// Approximate probability of recall (0.0–1.0).
    /// Lower = more urgent.
    pub approximate_recall: f64,
    /// Days overdue (0 = due today, positive = past due).
    pub overdue_days: i32,
}

/// Result of the full fetch-and-sort operation.
#[derive(Debug, Clone)]
pub struct SortResult {
    /// Cards sorted by ascending approximate_recall (most urgent first).
    pub ranked: Vec<RankedCard>,
    /// Total due cards per track tag.
    pub track_counts: HashMap<String, usize>,
    /// Estimated average seconds per card (cold-start or from revlog).
    pub avg_seconds_per_card: f64,
}

/// Per-track time estimates from revlog history.
pub type TrackTimeEstimates = HashMap<String, f64>;

/// Per-card facts extracted from the `.anki2` cards/notes join, shared by
/// both recall functions.
#[derive(Debug, Clone)]
pub(crate) struct CardFacts {
    pub card_id: i64,
    pub note_id: i64,
    pub note_tags: String,
    /// Current interval in days (review cards); not a day count for
    /// learning cards.
    pub ivl: i64,
    /// Anki card type: 0 = new, 1 = learning, 2 = review.
    pub card_type: i64,
    /// Due marker: day number (review), Unix seconds (learning), 0 (new).
    pub due: i64,
    /// SM-2 ease factor in permille (Anki default 2500; 0 for new cards).
    pub factor: i64,
}

/// Snapshot clock context for recall computation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RecallCtx {
    /// Unix seconds at query time.
    pub now_secs: i64,
    /// Days since collection creation (`col.crt`).
    pub today_day: i64,
    /// Collection creation timestamp (Unix seconds).
    pub crt: i64,
}

/// Recall assigned to new cards (type 0). Matches the in-house model's
/// not-yet-due tier so new-card ordering is identical under both schedulers.
pub(crate) const NEW_CARD_SENTINEL_RECALL: f64 = 0.95;

/// Recall function: maps a card's facts + snapshot clock to a probability of
/// recall (0.0–1.0, lower = more urgent).
type RecallFn<'a> = &'a dyn Fn(&CardFacts, &RecallCtx) -> f64;

// ---------------------------------------------------------------------------
// Scheduler selection
// ---------------------------------------------------------------------------

/// Which recall model the production path uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerMode {
    /// fsrs-rs FSRS-6 retrievability with persisted memory states (default).
    FsrsRs,
    /// In-house half-life model (rollback path).
    InHouse,
}

/// Read the scheduler selection from `ANKITOV_SCHEDULER`.
///
/// * `in-house` / `in_house` / `legacy` → [`SchedulerMode::InHouse`]
/// * unset, empty, or any other value → [`SchedulerMode::FsrsRs`] (default)
///
/// Unknown values fall back to the default rather than erroring: a config
/// typo must not take scheduling down, and the rollback switch only needs
/// its explicit spellings to work.
pub fn scheduler_mode() -> SchedulerMode {
    match std::env::var("ANKITOV_SCHEDULER").as_deref() {
        Ok("in-house") | Ok("in_house") | Ok("legacy") => SchedulerMode::InHouse,
        _ => SchedulerMode::FsrsRs,
    }
}

// ---------------------------------------------------------------------------
// Public API — in-house scheduler (rollback + property-suite anchor)
// ---------------------------------------------------------------------------

/// Open the `.anki2` file and retrieve all due cards matching the given
/// tags, sorted by urgency (lowest approximate recall first), using the
/// **in-house** half-life recall model.
///
/// This is the rollback path (`ANKITOV_SCHEDULER=in-house`) and the anchor
/// for the property suites (`property_scheduling_invariants_test`,
/// `property_fsrs_sort_test`, `profile_scenario_matrix_test`), which pin its
/// pipeline semantics. Production uses [`fetch_and_sort_cached`].
///
/// # Arguments
/// * `anki2_path` — Path to the `.anki2` collection file.
/// * `tags` — Track tags to filter cards by (matched via notes.tags LIKE).
/// * `lookback_days` — Number of days to look back in revlog for per-card
///   timing estimates (30-day window per spec).
pub fn fetch_and_sort(
    anki2_path: impl AsRef<Path>,
    tags: &[String],
    lookback_days: u32,
) -> Result<SortResult, FsrsSortError> {
    if tags.is_empty() {
        return Ok(SortResult {
            ranked: Vec::new(),
            track_counts: HashMap::new(),
            avg_seconds_per_card: crate::services::capsule_slicer::COLD_START_SECONDS_PER_CARD,
        });
    }

    let (conn, ctx) = open_collection(anki2_path)?;
    let cards = query_due_cards(&conn, tags, ctx)?;
    let avg_seconds = compute_avg_time_per_card(&conn, tags, lookback_days)
        .unwrap_or(crate::services::capsule_slicer::COLD_START_SECONDS_PER_CARD);
    let recall = |facts: &CardFacts, ctx: &RecallCtx| in_house_recall(facts, ctx);
    rank_and_sort(cards, ctx, tags, avg_seconds, &recall)
}

// ---------------------------------------------------------------------------
// Public API — fsrs-rs scheduler (production)
// ---------------------------------------------------------------------------

/// Rank due cards with the **fsrs-rs** FSRS-6 retrievability model over an
/// already-open snapshot, using the provided memory states.
///
/// Cards missing from `states` fall back to the in-house recall (defensive —
/// [`fetch_and_sort_cached`] guarantees complete coverage for non-new cards).
/// New cards (type 0) are scored with the new-card sentinel, matching the
/// in-house tier.
///
/// Testable without a database: pass an empty map to exercise the fallback
/// paths, or inject synthetic states to exercise the retrievability path.
pub fn fetch_and_sort_fsrs(
    conn: &Connection,
    tags: &[String],
    lookback_days: u32,
    states: &HashMap<i64, DerivedState>,
) -> Result<SortResult, FsrsSortError> {
    if tags.is_empty() {
        return Ok(SortResult {
            ranked: Vec::new(),
            track_counts: HashMap::new(),
            avg_seconds_per_card: crate::services::capsule_slicer::COLD_START_SECONDS_PER_CARD,
        });
    }

    let ctx = snapshot_ctx(conn)?;
    let cards = query_due_cards(conn, tags, ctx)?;
    let avg_seconds = compute_avg_time_per_card(conn, tags, lookback_days)
        .unwrap_or(crate::services::capsule_slicer::COLD_START_SECONDS_PER_CARD);
    let recall = |facts: &CardFacts, ctx: &RecallCtx| fsrs_recall(facts, ctx, states);
    rank_and_sort(cards, ctx, tags, avg_seconds, &recall)
}

/// Production entry point: fsrs-rs ranking with cache-aside memory-state
/// persistence.
///
/// Opens the snapshot read-only, resolves memory states for every due
/// non-new card via [`fsrs_state::ensure_states`] (persisted rows when
/// fresh, derivation + upsert otherwise), then ranks by FSRS-6
/// retrievability. The in-house pipeline semantics (due-filter, tag
/// matching, urgency sort, tie-breaks, per-track time estimates) are
/// identical to [`fetch_and_sort`].
///
/// # Arguments
/// * `db` — Backend database handle (`fsrs_memory_states` table).
/// * `student_id` — Owning student (`users.id`); states are scoped
///   per-student because shared decks reuse the same Anki card ids.
/// * `anki2_path` — Path to the `.anki2` collection file.
/// * `tags` — Track tags to filter cards by.
/// * `lookback_days` — Revlog lookback for per-card timing estimates.
pub async fn fetch_and_sort_cached(
    db: &DatabaseConnection,
    student_id: &str,
    anki2_path: impl AsRef<Path>,
    tags: &[String],
    lookback_days: u32,
) -> Result<SortResult, FsrsSortError> {
    if tags.is_empty() {
        return Ok(SortResult {
            ranked: Vec::new(),
            track_counts: HashMap::new(),
            avg_seconds_per_card: crate::services::capsule_slicer::COLD_START_SECONDS_PER_CARD,
        });
    }

    // ── Sync phase: every snapshot touch happens inside this block so the
    //    rusqlite connection is never held across an `.await` (rusqlite is
    //    !Sync; the generation future must stay Send for tokio::spawn).
    let mut prepared = {
        let (conn, ctx) = open_collection(anki2_path)?;
        let cards = query_due_cards(&conn, tags, ctx)?;
        // New cards are sentinel-scored; only learning/review cards need
        // states.
        let state_ids: Vec<i64> = cards
            .iter()
            .filter(|c| c.card_type != 0)
            .map(|c| c.card_id)
            .collect();
        let snapshot = fsrs_state::extract_snapshot(&conn, &state_ids)?;
        let avg_seconds = compute_avg_time_per_card(&conn, tags, lookback_days)
            .unwrap_or(crate::services::capsule_slicer::COLD_START_SECONDS_PER_CARD);
        PreparedSort {
            cards,
            ctx,
            snapshot,
            avg_seconds,
        }
    }; // connection dropped here

    // ── Async phase: backend DB only (cache-aside state resolution).
    let states = fsrs_state::ensure_states(db, student_id, &mut prepared.snapshot).await?;

    // ── Sync phase: pure ranking + sort.
    let recall = |facts: &CardFacts, ctx: &RecallCtx| fsrs_recall(facts, ctx, &states);
    rank_and_sort(prepared.cards, prepared.ctx, tags, prepared.avg_seconds, &recall)
}

/// Everything extracted from the snapshot before the async phase.
struct PreparedSort {
    cards: Vec<CardFacts>,
    ctx: RecallCtx,
    snapshot: fsrs_state::SnapshotData,
    avg_seconds: f64,
}

// ---------------------------------------------------------------------------
// Pipeline internals (shared by both schedulers)
// ---------------------------------------------------------------------------

/// Open the `.anki2` collection read-only (immutable mode) and read the
/// snapshot clock context.
fn open_collection(
    anki2_path: impl AsRef<Path>,
) -> Result<(Connection, RecallCtx), FsrsSortError> {
    let path_str = anki2_path.as_ref().display().to_string();
    let uri = format!("file:{}?immutable=1", path_str);
    let conn = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| FsrsSortError::CannotOpen(path_str, e))?;

    let ctx = snapshot_ctx(&conn)?;
    Ok((conn, ctx))
}

/// Read the snapshot clock context from an open collection.
fn snapshot_ctx(conn: &Connection) -> Result<RecallCtx, FsrsSortError> {
    let crt: i64 = conn
        .query_row("SELECT crt FROM col LIMIT 1", [], |row| row.get(0))
        .map_err(|e| FsrsSortError::QueryFailed("col.crt".into(), e))?;

    let now_secs = unix_now_secs();
    let today_day = (now_secs - crt) / 86400;

    Ok(RecallCtx {
        now_secs,
        today_day,
        crt,
    })
}

/// Query due cards matching the given tags (active cards only), preserving
/// the in-house due-filter semantics:
///
/// Anki queue semantics: -1 = suspended, -2 = user-buried, -3 = sched-buried,
/// 0 = new, 1 = learning, 2 = review, 3 = day-learn.
/// Only admit active cards (queue >= 0).
/// type=2 (review): due is a day number → due if due <= today_day
/// type=1 (learning): due is a Unix timestamp → due if due <= now_secs
/// type=0 (new): always included
fn query_due_cards(
    conn: &Connection,
    tags: &[String],
    ctx: RecallCtx,
) -> Result<Vec<CardFacts>, FsrsSortError> {
    // ── Build tag filter ──────────────────────────────────────────────
    // Anki stores tags space-separated in notes.tags.
    // We escape single quotes and use LIKE for substring matching.
    let escaped_tags: Vec<String> = tags
        .iter()
        .map(|t| format!("n.tags LIKE '%{}%'", t.replace('\'', "''")))
        .collect();
    let tag_filter = escaped_tags.join(" OR ");

    let sql = format!(
        "SELECT c.id, c.nid, n.tags, c.ivl, c.type, c.due, c.factor \
         FROM cards c \
         JOIN notes n ON c.nid = n.id \
         WHERE c.queue >= 0 \
           AND ({tag_filter}) \
           AND ( \
             (c.type = 2 AND c.due <= {today_day}) \
             OR (c.type = 1 AND c.due <= {now_secs}) \
             OR (c.type = 0) \
           ) \
         ORDER BY c.id",
        today_day = ctx.today_day,
        now_secs = ctx.now_secs,
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| FsrsSortError::QueryFailed("cards query prep".into(), e))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,    // c.id
                row.get::<_, i64>(1)?,    // c.nid
                row.get::<_, String>(2)?, // n.tags
                row.get::<_, i64>(3)?,    // c.ivl
                row.get::<_, i64>(4)?,    // c.type
                row.get::<_, i64>(5)?,    // c.due
                row.get::<_, i64>(6)?,    // c.factor
            ))
        })
        .map_err(|e| FsrsSortError::QueryFailed("cards query exec".into(), e))?;

    let mut cards = Vec::new();
    for row in rows {
        let (card_id, note_id, note_tags, ivl, card_type, due, factor) = row
            .map_err(|e| FsrsSortError::QueryFailed("row read".into(), e))?;
        cards.push(CardFacts {
            card_id,
            note_id,
            note_tags,
            ivl,
            card_type,
            due,
            factor,
        });
    }
    Ok(cards)
}

/// Apply the recall function, assign track tags, and sort by urgency.
/// Pure — no I/O; the per-track time estimate is passed in precomputed.
fn rank_and_sort(
    cards: Vec<CardFacts>,
    ctx: RecallCtx,
    tags: &[String],
    avg_seconds: f64,
    recall: RecallFn<'_>,
) -> Result<SortResult, FsrsSortError> {
    let mut track_counts: HashMap<String, usize> = HashMap::new();
    let mut ranked: Vec<RankedCard> = Vec::with_capacity(cards.len());

    for facts in &cards {
        // Find which of our track tags matches this note
        let matched_tag = match find_matching_tag(&facts.note_tags, tags) {
            Some(t) => t,
            None => continue, // shouldn't happen with our filter, but be safe
        };

        let overdue_days = compute_overdue_days(
            facts.card_type,
            facts.due,
            ctx.today_day,
            ctx.now_secs,
            ctx.crt,
        );
        let approximate_recall = recall(facts, &ctx);

        *track_counts.entry(matched_tag.clone()).or_insert(0) += 1;

        ranked.push(RankedCard {
            card_id: facts.card_id,
            note_id: facts.note_id,
            track_tag: matched_tag,
            approximate_recall,
            overdue_days,
        });
    }

    // ── Sort by urgency (lowest recall first), then overdue_days desc,
    //     then card_id ascending (deterministic tie-break) ──
    ranked.sort_by(|a, b| {
        a.approximate_recall
            .partial_cmp(&b.approximate_recall)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.overdue_days.cmp(&a.overdue_days))
            .then_with(|| a.card_id.cmp(&b.card_id))
    });

    Ok(SortResult {
        ranked,
        track_counts,
        avg_seconds_per_card: avg_seconds,
    })
}

// ---------------------------------------------------------------------------
// Recall functions
// ---------------------------------------------------------------------------

/// In-house half-life recall (rollback path + property-suite anchor).
fn in_house_recall(facts: &CardFacts, ctx: &RecallCtx) -> f64 {
    let overdue_days = compute_overdue_days(
        facts.card_type,
        facts.due,
        ctx.today_day,
        ctx.now_secs,
        ctx.crt,
    );
    compute_approximate_recall(overdue_days, facts.ivl)
}

/// fsrs-rs FSRS-6 recall (production path).
///
/// * New cards (type 0) → sentinel recall (same tier as in-house).
/// * Cards with a derived/persisted state → FSRS-6 retrievability with
///   elapsed = days since last review (fractional when the review timestamp
///   is known, day-precision otherwise).
/// * Cards missing from `states` → in-house recall (defensive fallback).
fn fsrs_recall(facts: &CardFacts, ctx: &RecallCtx, states: &HashMap<i64, DerivedState>) -> f64 {
    if facts.card_type == 0 {
        return NEW_CARD_SENTINEL_RECALL;
    }

    let Some(state) = states.get(&facts.card_id) else {
        // Should not happen in production (ensure_states covers all non-new
        // due cards); degrade to the in-house model rather than guessing.
        return in_house_recall(facts, ctx);
    };

    let overdue_days = compute_overdue_days(
        facts.card_type,
        facts.due,
        ctx.today_day,
        ctx.now_secs,
        ctx.crt,
    );

    let elapsed_days = match state.last_review_secs {
        Some(last_review_secs) => {
            // Fractional days since the last review — the exact quantity
            // FSRS retrievability is defined over.
            ((ctx.now_secs - last_review_secs) as f64 / 86_400.0).max(0.0)
        }
        None => match facts.card_type {
            // Review card: due = last-review day + ivl, so
            // elapsed = today - last_review = overdue + ivl (day precision).
            2 => (i64::from(overdue_days.max(0)) + facts.ivl.max(0)) as f64,
            // Learning card without a known review time: reviewed (or
            // introduced) effectively now.
            _ => 0.0,
        },
    };

    let memory_state = fsrs::MemoryState {
        stability: state.stability as f32,
        difficulty: state.difficulty as f32,
    };
    let retrievability = fsrs::current_retrievability(
        memory_state,
        elapsed_days as f32,
        fsrs::FSRS6_DEFAULT_DECAY,
    );
    (retrievability as f64).clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Find the first track tag that appears in the note's space-separated tags.
fn find_matching_tag(note_tags: &str, track_tags: &[String]) -> Option<String> {
    // Anki tags are space-separated. Split and check for membership.
    let note_tag_set: std::collections::HashSet<&str> =
        note_tags.split_whitespace().collect();

    for tag in track_tags {
        if note_tag_set.contains(tag.as_str()) {
            return Some(tag.clone());
        }
    }
    None
}

/// Compute days overdue relative to the due date.
fn compute_overdue_days(
    card_type: i64,
    due: i64,
    today_day: i64,
    now_secs: i64,
    _crt: i64,
) -> i32 {
    match card_type {
        2 => {
            // Review card: `due` is a day number
            (today_day - due) as i32
        }
        1 => {
            // Learning card: `due` is a Unix timestamp in seconds
            let seconds_overdue = now_secs - due;
            (seconds_overdue / 86400) as i32
        }
        _ => {
            // New cards (type=0): treat as "due now" → overdue_days = 0
            0
        }
    }
}

/// Compute approximate probability of recall using a half-life decay model.
///
/// ```text
/// R ≈ 0.90 × 0.5^(overdue_days / ivl)
/// ```
///
/// This is a simplification of the FSRS retrievability formula:
/// `R = (1 + 19 × t / S)^(-1)` where S ≈ ivl and t = elapsed_days.
///
/// For relative ordering, half-life decay is sufficient. Used by the
/// in-house rollback path and as the defensive fallback when a card has no
/// usable memory state.
pub fn compute_approximate_recall(overdue_days: i32, ivl: i64) -> f64 {
    if overdue_days <= 0 {
        // Card is not yet due — high recall
        return 0.95;
    }

    let effective_ivl = ivl.max(1) as f64;
    let decay_exponent = overdue_days as f64 / effective_ivl;

    // Base recall at due time: 0.90 (FSRS default desired retention)
    0.90 * (0.5_f64).powf(decay_exponent)
}

/// Compute average seconds per card from revlog history, grouped by track tags.
///
/// Returns the overall average across all matching tracks, or `None` if no
/// historical data exists (caller should fall back to cold-start default).
fn compute_avg_time_per_card(
    conn: &Connection,
    tags: &[String],
    lookback_days: u32,
) -> Option<f64> {
    let cutoff_ms = unix_now_millis() - (lookback_days as i64 * 86_400_000); // milliseconds

    // Build tag filter for revlog query
    let escaped_tags: Vec<String> = tags
        .iter()
        .map(|t| format!("n.tags LIKE '%{}%'", t.replace('\'', "''")))
        .collect();
    let tag_filter = escaped_tags.join(" OR ");

    let sql = format!(
        "SELECT AVG(r.time) / 1000.0 \
         FROM revlog r \
         JOIN cards c ON r.cid = c.id \
         JOIN notes n ON c.nid = n.id \
         WHERE r.id >= {cutoff_ms} \
           AND ({tag_filter}) \
           AND r.time > 0"
    );

    let result: Option<f64> = conn
        .query_row(&sql, [], |row| row.get(0))
        .ok()
        .flatten();

    // If we have data, return it; otherwise None triggers cold-start
    result.filter(|&t| t > 0.0 && t.is_finite())
}

/// Unix seconds; 0 if the clock is somehow before the epoch (defensive —
/// never expected).
fn unix_now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Unix milliseconds; 0 if the clock is somehow before the epoch.
fn unix_now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_approximate_recall_at_due() {
        // Card is due today (overdue_days = 0)
        let r = compute_approximate_recall(0, 30);
        assert!((r - 0.95).abs() < 0.01);
    }

    #[test]
    fn test_compute_approximate_recall_overdue_one_interval() {
        // Card is overdue by exactly its interval (overdue_days = ivl)
        let r = compute_approximate_recall(10, 10);
        // 0.90 × 0.5¹ = 0.45
        assert!((r - 0.45).abs() < 0.01);
    }

    #[test]
    fn test_compute_approximate_recall_very_overdue() {
        // Card is 3× overdue relative to interval
        let r = compute_approximate_recall(30, 10);
        // 0.90 × 0.5³ = 0.1125
        assert!((r - 0.1125).abs() < 0.01);
    }

    #[test]
    fn test_compute_approximate_recall_ordering() {
        // More overdue + shorter interval = more urgent (lower R)
        let r1 = compute_approximate_recall(5, 5);  // overdue 5, ivl 5
        let r2 = compute_approximate_recall(5, 50); // overdue 5, ivl 50
        let r3 = compute_approximate_recall(20, 50); // overdue 20, ivl 50

        // r1 (0.45) < r2 (0.84) < r3 (0.68)
        assert!(r1 < r2); // shorter interval → lower recall
        assert!(r3 < r2); // more overdue → lower recall
    }

    #[test]
    fn test_find_matching_tag_exact() {
        let note_tags = "math::fractions vocab science::biology";
        let track_tags: Vec<String> = vec!["vocab".into(), "science::biology".into()];
        assert_eq!(find_matching_tag(note_tags, &track_tags), Some("vocab".into()));
    }

    #[test]
    fn test_find_matching_tag_no_match() {
        let note_tags = "math::fractions vocab";
        let track_tags: Vec<String> = vec!["science".into()];
        assert_eq!(find_matching_tag(note_tags, &track_tags), None);
    }

    #[test]
    fn test_find_matching_tag_hierarchy() {
        let note_tags = "ankitov::math::grade7";
        let track_tags: Vec<String> = vec!["ankitov::math::grade7".into(), "other".into()];
        assert_eq!(
            find_matching_tag(note_tags, &track_tags),
            Some("ankitov::math::grade7".into())
        );
    }

    #[test]
    fn test_scheduler_mode_default_and_rollback() {
        // Default (unset/garbage) → fsrs-rs
        std::env::remove_var("ANKITOV_SCHEDULER");
        assert_eq!(scheduler_mode(), SchedulerMode::FsrsRs);
        // Rollback spellings → in-house
        std::env::set_var("ANKITOV_SCHEDULER", "in-house");
        assert_eq!(scheduler_mode(), SchedulerMode::InHouse);
        std::env::set_var("ANKITOV_SCHEDULER", "legacy");
        assert_eq!(scheduler_mode(), SchedulerMode::InHouse);
        // Unknown → default
        std::env::set_var("ANKITOV_SCHEDULER", "wat");
        assert_eq!(scheduler_mode(), SchedulerMode::FsrsRs);
        std::env::remove_var("ANKITOV_SCHEDULER");
    }

    /// Integration test: fetch_and_sort with a real SQLite fixture.
    #[test]
    fn test_fetch_and_sort_with_fixture() {
        let tmp = std::env::temp_dir().join("ankitov_fsrs_test_000.anki2");
        let _ = std::fs::remove_file(&tmp);
        let conn = rusqlite::Connection::open(&tmp).expect("open");
        let now = chrono::Utc::now().timestamp();

        conn.execute_batch(
            "CREATE TABLE col (id INTEGER PRIMARY KEY, crt INTEGER NOT NULL, mod INTEGER, scm INTEGER, ver INTEGER, dty INTEGER, usn INTEGER, ls INTEGER, conf TEXT, models TEXT, decks TEXT, dconf TEXT, tags TEXT);
            CREATE TABLE cards (id INTEGER PRIMARY KEY, nid INTEGER NOT NULL, did INTEGER NOT NULL DEFAULT 1, ord INTEGER NOT NULL DEFAULT 0, mod INTEGER NOT NULL DEFAULT 0, usn INTEGER NOT NULL DEFAULT -1, type INTEGER NOT NULL DEFAULT 0, queue INTEGER NOT NULL DEFAULT 0, due INTEGER NOT NULL DEFAULT 0, ivl INTEGER NOT NULL DEFAULT 0, factor INTEGER NOT NULL DEFAULT 0, reps INTEGER NOT NULL DEFAULT 0, lapses INTEGER NOT NULL DEFAULT 0, left INTEGER NOT NULL DEFAULT 0, odue INTEGER NOT NULL DEFAULT 0, odid INTEGER NOT NULL DEFAULT 0, flags INTEGER NOT NULL DEFAULT 0, data TEXT NOT NULL DEFAULT '');
            CREATE TABLE notes (id INTEGER PRIMARY KEY, guid TEXT NOT NULL DEFAULT '', mid INTEGER NOT NULL DEFAULT 0, mod INTEGER NOT NULL DEFAULT 0, usn INTEGER NOT NULL DEFAULT -1, tags TEXT NOT NULL DEFAULT '', flds TEXT NOT NULL DEFAULT '', sfld TEXT NOT NULL DEFAULT '', csum INTEGER NOT NULL DEFAULT 0, flags INTEGER NOT NULL DEFAULT 0, data TEXT NOT NULL DEFAULT '');"
        ).expect("create schema");
        conn.execute("INSERT INTO col (id, crt) VALUES (1, ?)", rusqlite::params![now]).expect("col");
        conn.execute("INSERT INTO notes (id, tags) VALUES (1, 'math')", []).expect("note1");
        conn.execute("INSERT INTO notes (id, tags) VALUES (2, 'vocab')", []).expect("note2");
        // Active review cards — due = -5 (past), queue = 2
        conn.execute("INSERT INTO cards (id,nid,type,queue,due,ivl) VALUES (1,1,2,2,-5,10)", []).expect("card1");
        conn.execute("INSERT INTO cards (id,nid,type,queue,due,ivl) VALUES (2,2,2,2,-5,50)", []).expect("card2");
        // Suspended (queue=-1) -> excluded
        conn.execute("INSERT INTO cards (id,nid,type,queue,due,ivl) VALUES (3,1,2,-1,-5,5)", []).expect("card3");
        drop(conn);

        let result = super::fetch_and_sort(
            tmp.to_str().unwrap(),
            &["math".to_string(), "vocab".to_string()],
            30,
        );
        let _ = std::fs::remove_file(&tmp);
        let sr = result.expect("fetch_and_sort should succeed");

        assert_eq!(sr.ranked.len(), 2, "expected 2 active cards, got {}", sr.ranked.len());
        assert!(sr.ranked.iter().all(|c| c.card_id != 3), "suspended card leaked");
        assert!(sr.track_counts.values().sum::<usize>() == sr.ranked.len());
    }
}
