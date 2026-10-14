//! FSRS MemoryState derivation + persistence (T-116 fsrs-rs production swap).
//!
//! The `.anki2` collection is opened **read-only** (immutable mode), so the
//! FSRS-6 memory states that the fsrs-rs scheduler ranks by cannot be written
//! back into the student's collection the way Anki itself does. Instead, the
//! states are *derived* from the snapshot and persisted in the backend
//! database (`fsrs_memory_states` table, one row per `(student_id, card_id)`).
//!
//! ## Derivation chain (per card, first success wins)
//!
//! 1. **revlog replay** — replay the card's review history through
//!    `FSRS::memory_state` (ratings from `revlog.ease`, day gaps from
//!    `revlog.id` timestamps; cram reviews and out-of-range ease values are
//!    skipped, matching Anki's FSRS handling). Freshest and most faithful.
//! 2. **SM-2 approximation** — for review cards with sane `cards.factor` /
//!    `cards.ivl` but no usable revlog (e.g. imported without history):
//!    `FSRS::memory_state_from_sm2`.
//! 3. **First-transition defaults** — terminal fallback: the state after a
//!    first "good" review from no prior state (computed live from the crate,
//!    consistent with the gate-test anchors).
//!
//! New cards (`type = 0`) are **not** derived or persisted: the ranking layer
//! scores them with the new-card sentinel recall, identical to the in-house
//! model's tier, so new-card ordering is the same under both schedulers.
//!
//! ## Cache-aside refresh
//!
//! Each persisted row records `revlog_head` — the newest `revlog.id` (ms
//! epoch) observed when the state was derived. A later snapshot whose revlog
//! contains newer entries for the card (the student reviewed it in Anki and
//! re-synced) triggers re-derivation from the full history. Rows whose
//! `revlog_head` still matches the snapshot are cache hits and are not
//! recomputed. Derivation is deterministic, so concurrent generations for the
//! same student converge to identical rows (last write wins, same values).
//!
//! ## Send-safety
//!
//! `rusqlite::Connection` is `!Sync`, so no future may hold it across an
//! `.await`. The API is therefore phased: [`extract_snapshot`] does **all**
//! snapshot reads synchronously and returns owned plain data
//! ([`SnapshotData`]); [`ensure_states`] then works purely against the
//! backend database and that owned data. Callers must drop the connection
//! before awaiting.
//!
//! All operations are idempotent; a partially-persisted batch self-heals on
//! the next generation (missing rows are re-derived).

use std::collections::HashMap;
use std::sync::LazyLock;

use fsrs::{FSRS, FSRSItem, FSRSReview, MemoryState};
use rusqlite::Connection;
use sea_orm::{
    ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    sea_query::OnConflict,
};

use crate::models::entities::fsrs_memory_state::{ActiveModel, Column, Entity};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// `source` value for states derived by revlog replay.
pub const SOURCE_REVIEW_LOG: &str = "revlog";
/// `source` value for states approximated from SM-2 data.
pub const SOURCE_SM2: &str = "sm2";
/// `source` value for first-transition default states.
pub const SOURCE_FIRST_TRANSITION: &str = "first_transition";

/// Desired retention used for SM-2 approximation and first-transition
/// seeding. Matches the in-house model's 0.90 base recall and the FSRS
/// default desired retention.
pub const DESIRED_RETENTION: f32 = 0.9;

/// Sane SM-2 ease-factor window (permille). Anki's default is 2500; anything
/// outside [1000, 5000] is treated as missing/garbage data.
const SM2_FACTOR_MIN: i64 = 1000;
const SM2_FACTOR_MAX: i64 = 5000;

/// Max card ids per SQL `IN (...)` batch (SQLite's default host-parameter
/// limit is 999; stay well under it).
const SQL_CHUNK: usize = 500;

/// Max rows per INSERT batch for the upsert.
const UPSERT_CHUNK: usize = 100;

/// Shared FSRS model (default FSRS-6 parameters). `FSRS` is a small
/// parameter array; the shared instance keeps the derivation path
/// allocation-free after first use.
static FSRS_MODEL: LazyLock<FSRS> = LazyLock::new(FSRS::default);

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors from memory-state derivation and persistence: snapshot reads
/// (`.anki2`) and backend-DB store operations.
#[derive(Debug, thiserror::Error)]
pub enum FsrsStateError {
    /// Any failure reading the `.anki2` snapshot (revlog heads, revlog rows,
    /// or card scheduling fields). The context string names the query.
    #[error("snapshot query failed: {0}")]
    Snapshot(String, #[source] rusqlite::Error),

    /// Failure reading/writing the `fsrs_memory_states` table.
    #[error("memory-state store error: {0}")]
    Store(#[from] sea_orm::DbErr),
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// How a memory state was derived (persisted as `source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateSource {
    /// Replayed from the snapshot's revlog history.
    Revlog,
    /// Approximated from SM-2 `factor`/`ivl` (no usable revlog).
    Sm2,
    /// First-transition defaults (no usable history at all).
    FirstTransition,
}

impl StateSource {
    /// Canonical DB string for the `source` column.
    pub fn as_str(self) -> &'static str {
        match self {
            StateSource::Revlog => SOURCE_REVIEW_LOG,
            StateSource::Sm2 => SOURCE_SM2,
            StateSource::FirstTransition => SOURCE_FIRST_TRANSITION,
        }
    }

    /// Reconstruct from the persisted `source` string. Unknown values
    /// (written by a future version) degrade to
    /// [`StateSource::FirstTransition`] — `source` is informational, never
    /// load-bearing.
    fn from_db_str(s: &str) -> StateSource {
        match s {
            SOURCE_REVIEW_LOG => StateSource::Revlog,
            SOURCE_SM2 => StateSource::Sm2,
            _ => StateSource::FirstTransition,
        }
    }
}

/// A derived FSRS-6 memory state plus the metadata needed for cache-aside
/// refresh and ranking.
#[derive(Debug, Clone, Copy)]
pub struct DerivedState {
    /// FSRS-6 stability (days until recall drops to the target retention).
    pub stability: f64,
    /// FSRS-6 difficulty (1.0 easy … 10.0 hard).
    pub difficulty: f64,
    /// Unix timestamp (seconds) of the card's most recent review, when known
    /// (revlog-derived only). `None` → the ranking layer falls back to
    /// day-precision elapsed estimation from `due`/`ivl`.
    pub last_review_secs: Option<i64>,
    /// Newest `revlog.id` (ms epoch) observed at derivation time; the
    /// cache-freshness marker. `None` for sm2/first-transition states.
    pub revlog_head: Option<i64>,
    /// How this state was derived.
    pub source: StateSource,
}

/// Scheduling fields needed for the SM-2 fallback, read from `cards`.
#[derive(Debug, Clone, Copy)]
pub struct CardSchedFacts {
    /// Anki card type: 0 = new, 1 = learning, 2 = review.
    pub card_type: i64,
    /// Current interval in days (review cards).
    pub ivl: i64,
    /// SM-2 ease factor in permille (Anki default 2500).
    pub factor: i64,
}

/// Owned plain-data extract of everything the derivation needs from one
/// `.anki2` snapshot. Produced synchronously by [`extract_snapshot`] so no
/// `rusqlite` type is held across an `.await` anywhere downstream.
#[derive(Debug, Clone, Default)]
pub struct SnapshotData {
    /// The card ids this snapshot covers.
    pub card_ids: Vec<i64>,
    /// Newest `revlog.id` (ms epoch) per card (cards with no revlog rows are
    /// absent).
    pub revlog_heads: HashMap<i64, i64>,
    /// `cards` scheduling fields per card (cards missing from the table are
    /// absent).
    pub sched_facts: HashMap<i64, CardSchedFacts>,
    /// Per-card revlog rows `(id, ease, type)` ordered by `id` ascending.
    pub revlog_rows: HashMap<i64, Vec<(i64, i64, i64)>>,
}

// ---------------------------------------------------------------------------
// Public API — snapshot extraction (sync)
// ---------------------------------------------------------------------------

/// Extract all snapshot data the derivation needs for the given cards.
///
/// Synchronous and connection-bound: callers must complete (and drop the
/// connection) before any `.await`.
pub fn extract_snapshot(
    conn: &Connection,
    card_ids: &[i64],
) -> Result<SnapshotData, FsrsStateError> {
    if card_ids.is_empty() {
        return Ok(SnapshotData::default());
    }
    Ok(SnapshotData {
        card_ids: card_ids.to_vec(),
        revlog_heads: revlog_heads(conn, card_ids)?,
        sched_facts: card_scheduling_facts(conn, card_ids)?,
        revlog_rows: revlog_rows_for(conn, card_ids)?,
    })
}

// ---------------------------------------------------------------------------
// Public API — derivation (pure, sync)
// ---------------------------------------------------------------------------

/// Derive memory states for every card in the snapshot, using the derivation
/// chain (revlog → SM-2 → first transition). Pure function of the snapshot
/// data — no I/O.
///
/// Cards with no usable history at all are omitted only if even the
/// first-transition fallback fails (practically impossible); the caller
/// applies its own terminal fallback for omitted cards.
pub fn derive_states_from_snapshot(snapshot: &SnapshotData) -> HashMap<i64, DerivedState> {
    let mut states = HashMap::with_capacity(snapshot.card_ids.len());
    for &card_id in &snapshot.card_ids {
        let head = snapshot.revlog_heads.get(&card_id).copied();

        // 1. revlog replay
        if let Some(rows) = snapshot.revlog_rows.get(&card_id) {
            if let Some(ms) = state_from_revlog_rows(rows) {
                states.insert(
                    card_id,
                    DerivedState {
                        stability: ms.stability as f64,
                        difficulty: ms.difficulty as f64,
                        last_review_secs: head.map(|h| h / 1000),
                        revlog_head: head,
                        source: StateSource::Revlog,
                    },
                );
                continue;
            }
        }

        // 2. SM-2 approximation (review cards with sane factor/ivl only —
        //    learning-card `ivl` is not a day count, and new cards have
        //    neither).
        if let Some(facts) = snapshot.sched_facts.get(&card_id) {
            if facts.card_type == 2
                && facts.ivl >= 1
                && (SM2_FACTOR_MIN..=SM2_FACTOR_MAX).contains(&facts.factor)
            {
                if let Some(st) = state_from_sm2(facts.factor, facts.ivl) {
                    states.insert(card_id, st);
                    continue;
                }
            }
        }

        // 3. First-transition defaults (terminal fallback).
        if let Some(st) = first_transition_state() {
            states.insert(card_id, st);
        }
        // else: omitted — caller applies its own terminal fallback.
    }
    states
}

/// Derive a memory state by replaying one card's revlog rows through the
/// FSRS model. `rows` are `(revlog.id, ease, type)` ordered by `id`
/// ascending. Returns `None` when no usable reviews remain after filtering
/// or when the model rejects the sequence.
fn state_from_revlog_rows(rows: &[(i64, i64, i64)]) -> Option<MemoryState> {
    // Anki's FSRS ignores cram reviews (type 3) and ease values outside the
    // 1..=4 rating scale (e.g. cram rows carry ease 0).
    let usable: Vec<(i64, i64)> = rows
        .iter()
        .copied()
        .filter(|&(_, ease, rtype)| rtype != 3 && (1..=4).contains(&ease))
        .map(|(id, ease, _)| (id, ease))
        .collect();
    if usable.is_empty() {
        return None;
    }

    let mut reviews = Vec::with_capacity(usable.len());
    let mut prev_id: Option<i64> = None;
    for &(id, ease) in &usable {
        // delta_t = whole days since the previous *usable* review; the first
        // review must carry delta_t = 0 (fsrs-rs requirement). Saturate —
        // real revlog ids are epoch milliseconds, so gaps fit u32 easily.
        let delta_t = match prev_id {
            None => 0u32,
            Some(prev) => ((id - prev) / 86_400_000).clamp(0, u32::MAX as i64) as u32,
        };
        reviews.push(FSRSReview {
            rating: ease as u32,
            delta_t,
        });
        prev_id = Some(id);
    }

    let state = FSRS_MODEL
        .memory_state(FSRSItem { reviews }, None)
        .ok()?;
    if !is_usable_state(&state) {
        return None;
    }
    Some(state)
}

/// Derive a memory state from SM-2 data (`cards.factor` permille,
/// `cards.ivl` days). Returns `None` when the model rejects the inputs or
/// produces a non-usable state.
fn state_from_sm2(factor_permille: i64, interval_days: i64) -> Option<DerivedState> {
    let state = FSRS_MODEL
        .memory_state_from_sm2(
            factor_permille as f32 / 1000.0,
            interval_days as f32,
            DESIRED_RETENTION,
        )
        .ok()?;
    if !is_usable_state(&state) {
        return None;
    }
    Some(DerivedState {
        stability: state.stability as f64,
        difficulty: state.difficulty as f64,
        last_review_secs: None,
        revlog_head: None,
        source: StateSource::Sm2,
    })
}

/// Terminal fallback: the state after a first "good" review from no prior
/// state, computed live from the crate (consistent with the gate-test doc
/// anchors). Returns `None` only if the crate itself errors — practically
/// impossible with default parameters, but handled without panicking.
pub fn first_transition_state() -> Option<DerivedState> {
    let next = FSRS_MODEL.next_states(None, DESIRED_RETENTION, 0).ok()?;
    let memory = next.good.memory;
    if !is_usable_state(&memory) {
        return None;
    }
    Some(DerivedState {
        stability: memory.stability as f64,
        difficulty: memory.difficulty as f64,
        last_review_secs: None,
        revlog_head: None,
        source: StateSource::FirstTransition,
    })
}

/// A state is usable for ranking only if both components are finite and
/// stability is positive (`current_retrievability` divides by it).
fn is_usable_state(state: &MemoryState) -> bool {
    state.stability.is_finite() && state.difficulty.is_finite() && state.stability > 0.0
}

// ---------------------------------------------------------------------------
// Public API — cache-aside persistence (async, DB only)
// ---------------------------------------------------------------------------

/// Cache-aside state resolution for the production scheduler: returns a
/// memory state for every card in `snapshot`, using persisted rows when
/// fresh and deriving + persisting fresh states otherwise.
///
/// Takes the owned [`SnapshotData`] (never a `rusqlite` connection) so the
/// returned future is `Send`.
///
/// Freshness rule: a cached row is fresh iff its `revlog_head` equals the
/// snapshot's current revlog head for that card (both `None` counts as
/// equal — no revlog on either side). Any mismatch re-derives from the full
/// history.
pub async fn ensure_states(
    db: &DatabaseConnection,
    student_id: &str,
    snapshot: &SnapshotData,
) -> Result<HashMap<i64, DerivedState>, FsrsStateError> {
    if snapshot.card_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let cached = load_cached_states(db, student_id, &snapshot.card_ids).await?;

    let mut states: HashMap<i64, DerivedState> = HashMap::with_capacity(snapshot.card_ids.len());
    let mut stale_ids: Vec<i64> = Vec::new();
    for &card_id in &snapshot.card_ids {
        let head = snapshot.revlog_heads.get(&card_id).copied();
        if let Some(row) = cached.get(&card_id) {
            if row.revlog_head == head {
                states.insert(
                    card_id,
                    DerivedState {
                        stability: row.stability,
                        difficulty: row.difficulty,
                        last_review_secs: row.last_review_secs,
                        revlog_head: row.revlog_head,
                        source: StateSource::from_db_str(&row.source),
                    },
                );
                continue;
            }
        }
        stale_ids.push(card_id);
    }

    if stale_ids.is_empty() {
        return Ok(states);
    }

    // Derive only the stale/missing cards (sub-snapshot view).
    let sub = SnapshotData {
        card_ids: stale_ids.clone(),
        revlog_heads: snapshot
            .revlog_heads
            .iter()
            .filter(|(cid, _)| stale_ids.contains(cid))
            .map(|(&cid, &head)| (cid, head))
            .collect(),
        sched_facts: snapshot
            .sched_facts
            .iter()
            .filter(|(cid, _)| stale_ids.contains(cid))
            .map(|(&cid, &facts)| (cid, facts))
            .collect(),
        revlog_rows: snapshot
            .revlog_rows
            .iter()
            .filter(|(cid, _)| stale_ids.contains(cid))
            .map(|(&cid, rows)| (cid, rows.clone()))
            .collect(),
    };
    let derived = derive_states_from_snapshot(&sub);
    let fresh: Vec<(i64, DerivedState)> = stale_ids
        .iter()
        .filter_map(|id| derived.get(id).copied().map(|st| (*id, st)))
        .collect();

    upsert_states(db, student_id, &fresh).await?;
    for (card_id, st) in fresh {
        states.insert(card_id, st);
    }
    Ok(states)
}

/// Load persisted memory-state rows for one student, keyed by card id.
async fn load_cached_states(
    db: &DatabaseConnection,
    student_id: &str,
    card_ids: &[i64],
) -> Result<HashMap<i64, crate::models::entities::fsrs_memory_state::Model>, sea_orm::DbErr> {
    let mut out = HashMap::new();
    for chunk in card_ids.chunks(SQL_CHUNK) {
        let rows = Entity::find()
            .filter(Column::StudentId.eq(student_id))
            .filter(Column::CardId.is_in(chunk.to_vec()))
            .all(db)
            .await?;
        for row in rows {
            out.insert(row.card_id, row);
        }
    }
    Ok(out)
}

/// Upsert derived states, batched. Conflict target is the unique
/// `(student_id, card_id)` index; conflicting rows get every state field
/// updated (row identity — `id` — is preserved).
async fn upsert_states(
    db: &DatabaseConnection,
    student_id: &str,
    entries: &[(i64, DerivedState)],
) -> Result<(), sea_orm::DbErr> {
    if entries.is_empty() {
        return Ok(());
    }
    let now = unix_now();
    for chunk in entries.chunks(UPSERT_CHUNK) {
        let models: Vec<ActiveModel> = chunk
            .iter()
            .map(|&(card_id, st)| ActiveModel {
                id: Set(uuid::Uuid::new_v4().to_string()),
                student_id: Set(student_id.to_string()),
                card_id: Set(card_id),
                stability: Set(st.stability),
                difficulty: Set(st.difficulty),
                last_review_secs: Set(st.last_review_secs),
                source: Set(st.source.as_str().to_string()),
                revlog_head: Set(st.revlog_head),
                updated_at: Set(now),
            })
            .collect();

        Entity::insert_many(models)
            .on_conflict(
                OnConflict::columns([Column::StudentId, Column::CardId])
                    .update_columns([
                        Column::Stability,
                        Column::Difficulty,
                        Column::LastReviewSecs,
                        Column::Source,
                        Column::RevlogHead,
                        Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(db)
            .await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Snapshot queries (sync, connection-bound)
// ---------------------------------------------------------------------------

/// Newest `revlog.id` (ms epoch) per card, for cards that have revlog rows.
fn revlog_heads(conn: &Connection, card_ids: &[i64]) -> Result<HashMap<i64, i64>, FsrsStateError> {
    let mut out = HashMap::new();
    for chunk in card_ids.chunks(SQL_CHUNK) {
        let sql = format!(
            "SELECT cid, MAX(id) FROM revlog WHERE cid IN ({}) GROUP BY cid",
            placeholders(chunk.len())
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| FsrsStateError::Snapshot("revlog heads prep".into(), e))?;
        let mut rows = stmt
            .query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|e| FsrsStateError::Snapshot("revlog heads exec".into(), e))?;
        while let Some(row) = rows.next() {
            let (cid, head) =
                row.map_err(|e| FsrsStateError::Snapshot("revlog heads row".into(), e))?;
            out.insert(cid, head);
        }
    }
    Ok(out)
}

/// Per-card revlog rows `(id, ease, type)` ordered by `id` ascending.
fn revlog_rows_for(
    conn: &Connection,
    card_ids: &[i64],
) -> Result<HashMap<i64, Vec<(i64, i64, i64)>>, FsrsStateError> {
    let mut out: HashMap<i64, Vec<(i64, i64, i64)>> = HashMap::new();
    for chunk in card_ids.chunks(SQL_CHUNK) {
        let sql = format!(
            "SELECT id, cid, ease, type FROM revlog WHERE cid IN ({}) ORDER BY cid, id",
            placeholders(chunk.len())
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| FsrsStateError::Snapshot("revlog rows prep".into(), e))?;
        let mut rows = stmt
            .query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|e| FsrsStateError::Snapshot("revlog rows exec".into(), e))?;
        while let Some(row) = rows.next() {
            let (id, cid, ease, rtype) =
                row.map_err(|e| FsrsStateError::Snapshot("revlog rows row".into(), e))?;
            out.entry(cid).or_default().push((id, ease, rtype));
        }
    }
    Ok(out)
}

/// Scheduling fields (`type`, `ivl`, `factor`) per card, for the SM-2
/// fallback.
fn card_scheduling_facts(
    conn: &Connection,
    card_ids: &[i64],
) -> Result<HashMap<i64, CardSchedFacts>, FsrsStateError> {
    let mut out = HashMap::new();
    for chunk in card_ids.chunks(SQL_CHUNK) {
        let sql = format!(
            "SELECT id, type, ivl, factor FROM cards WHERE id IN ({})",
            placeholders(chunk.len())
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| FsrsStateError::Snapshot("cards facts prep".into(), e))?;
        let mut rows = stmt
            .query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|e| FsrsStateError::Snapshot("cards facts exec".into(), e))?;
        while let Some(row) = rows.next() {
            let (id, card_type, ivl, factor) =
                row.map_err(|e| FsrsStateError::Snapshot("cards facts row".into(), e))?;
            out.insert(id, CardSchedFacts { card_type, ivl, factor });
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(", ")
}

/// Unix timestamp in seconds; 0 if the clock is somehow before the epoch
/// (defensive — never expected).
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
