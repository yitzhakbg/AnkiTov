//! T-116 — fsrs-rs production swap: MemoryState derivation + persistence.
//!
//! Covers the derivation chain (revlog replay → SM-2 → first transition),
//! the cache-aside `ensure_states` store (round-trip, stale refresh,
//! cross-student isolation), and the production `fetch_and_sort_cached`
//! entry point end-to-end against a synthetic `.anki2` + migrated in-memory
//! backend DB.
//!
//! Spec: `docs-repository/technical/scheduling-core.md` §4 (swap boundary)
//! and §6 (MemoryState persistence is the gating item).
//!
//! Run: `cargo nextest run -p backend --test fsrs_state_persistence_test`

use backend::models::entities::fsrs_memory_state::{self, Entity as FsrsMemoryState};
use backend::services::fsrs_sort::{fetch_and_sort, fetch_and_sort_cached};
use backend::services::fsrs_state::{
    CardSchedFacts, SnapshotData, StateSource, derive_states_from_snapshot, ensure_states,
    extract_snapshot,
};
use sea_orm::{ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter};
use sea_orm_migration::MigratorTrait;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A private migrated in-memory database (same pattern as
/// `property_audit_trail_test.rs` — one per test, safe under nextest).
async fn fresh_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.expect("connect");
    backend::migrations::Migrator::up(&db, None).await.expect("migrate");
    db
}

const DAY_MS: i64 = 86_400_000;

/// Revlog row ids are epoch milliseconds; anchor the fixture to a fixed
/// recent-ish instant so derivations are fully deterministic.
fn review_ms(days_ago: i64) -> i64 {
    // Any fixed epoch works — FSRS only sees day gaps between rows.
    1_700_000_000_000 - days_ago * DAY_MS
}

/// Hand-built snapshot for one card with the given revlog rows
/// `(id, ease, type)`.
fn snapshot_with_revlog(card_id: i64, rows: &[(i64, i64, i64)]) -> SnapshotData {
    let head = rows.iter().map(|r| r.0).max();
    SnapshotData {
        card_ids: vec![card_id],
        revlog_heads: head.map(|h| HashMap::from([(card_id, h)])).unwrap_or_default(),
        sched_facts: HashMap::from([(
            card_id,
            CardSchedFacts { card_type: 2, ivl: 10, factor: 2500 },
        )]),
        revlog_rows: HashMap::from([(card_id, rows.to_vec())]),
    }
}

// ---------------------------------------------------------------------------
// Derivation chain (pure)
// ---------------------------------------------------------------------------

#[test]
fn revlog_replay_derives_state_with_head_and_last_review() {
    let rows = vec![
        (review_ms(30), 3, 0), // good, 30 days ago
        (review_ms(20), 3, 0), // good, 20 days ago
        (review_ms(5), 4, 0),  // easy, 5 days ago
    ];
    let states = derive_states_from_snapshot(&snapshot_with_revlog(101, &rows));
    let state = states.get(&101).expect("state derived");

    assert_eq!(state.source, StateSource::Revlog);
    assert_eq!(state.revlog_head, Some(review_ms(5)));
    assert_eq!(state.last_review_secs, Some(review_ms(5) / 1000));
    assert!(
        state.stability.is_finite() && state.stability > 0.0,
        "stability must be usable: {:?}",
        state.stability
    );
    assert!(
        (1.0..=10.0).contains(&state.difficulty),
        "difficulty must be in the FSRS range: {:?}",
        state.difficulty
    );
}

#[test]
fn more_successful_reviews_grow_stability() {
    let one = vec![(review_ms(10), 3, 0)];
    let four = vec![
        (review_ms(40), 3, 0),
        (review_ms(30), 3, 0),
        (review_ms(20), 3, 0),
        (review_ms(10), 3, 0),
    ];
    let states = derive_states_from_snapshot(&snapshot_with_revlog(1, &one)
        .merged_with(&snapshot_with_revlog(2, &four)));
    let s1 = states.get(&1).expect("one-review state").stability;
    let s4 = states.get(&2).expect("four-review state").stability;
    assert!(
        s4 > s1,
        "four spaced good reviews must stabilize more than one: {s4} vs {s1}"
    );
}

#[test]
fn cram_rows_and_out_of_range_ease_are_skipped() {
    // Only cram rows (type 3) and ease-0 rows → nothing usable → the card
    // falls through to the SM-2 fallback (its facts are review-shaped).
    let cram_only = vec![
        (review_ms(10), 0, 3),
        (review_ms(5), 3, 3),
    ];
    let states = derive_states_from_snapshot(&snapshot_with_revlog(7, &cram_only));
    let state = states.get(&7).expect("fallback state");
    assert_eq!(state.source, StateSource::Sm2, "cram-only history → SM-2");
    assert_eq!(state.revlog_head, None, "no usable revlog → no head");

    // Ease outside 1..=4 (e.g. 0 or 5) is likewise unusable.
    let bad_ease = vec![(review_ms(9), 0, 0), (review_ms(4), 5, 0)];
    let states = derive_states_from_snapshot(&snapshot_with_revlog(8, &bad_ease));
    assert_eq!(
        states.get(&8).expect("fallback state").source,
        StateSource::Sm2
    );
}

#[test]
fn sm2_fallback_uses_review_card_facts() {
    // Review card, no revlog: ivl=10 days, factor 2500 → SM-2 derivation.
    let snapshot = SnapshotData {
        card_ids: vec![42],
        revlog_heads: HashMap::new(),
        sched_facts: HashMap::from([(
            42,
            CardSchedFacts { card_type: 2, ivl: 10, factor: 2500 },
        )]),
        revlog_rows: HashMap::new(),
    };
    let states = derive_states_from_snapshot(&snapshot);
    let state = states.get(&42).expect("sm2 state");
    assert_eq!(state.source, StateSource::Sm2);
    assert!(state.stability > 0.0, "sm2 stability must be positive");
    assert_eq!(state.last_review_secs, None);
    assert_eq!(state.revlog_head, None);
}

#[test]
fn sm2_fallback_rejects_out_of_range_factors() {
    // factor 100 (permille) is outside the sane window → not SM-2-derivable
    // → first-transition fallback.
    let snapshot = SnapshotData {
        card_ids: vec![43],
        revlog_heads: HashMap::new(),
        sched_facts: HashMap::from([(
            43,
            CardSchedFacts { card_type: 2, ivl: 10, factor: 100 },
        )]),
        revlog_rows: HashMap::new(),
    };
    let states = derive_states_from_snapshot(&snapshot);
    assert_eq!(
        states.get(&43).expect("state").source,
        StateSource::FirstTransition
    );
}

#[test]
fn first_transition_for_learning_cards() {
    let snapshot = SnapshotData {
        card_ids: vec![44],
        revlog_heads: HashMap::new(),
        sched_facts: HashMap::from([(
            44,
            CardSchedFacts { card_type: 1, ivl: 0, factor: 0 },
        )]),
        revlog_rows: HashMap::new(),
    };
    let states = derive_states_from_snapshot(&snapshot);
    let state = states.get(&44).expect("first-transition state");
    assert_eq!(state.source, StateSource::FirstTransition);
    assert!(state.stability > 0.0);
}

// ---------------------------------------------------------------------------
// Cache-aside persistence (async, DB)
// ---------------------------------------------------------------------------

/// Insert a persisted row directly, with a sentinel `updated_at` so tests
/// can observe whether `ensure_states` rewrote it.
async fn seed_row(db: &DatabaseConnection, student: &str, card: i64, head: Option<i64>) {
    let model = fsrs_memory_state::ActiveModel {
        id: sea_orm::Set(uuid::Uuid::new_v4().to_string()),
        student_id: sea_orm::Set(student.to_string()),
        card_id: sea_orm::Set(card),
        stability: sea_orm::Set(3.25),
        difficulty: sea_orm::Set(5.5),
        last_review_secs: sea_orm::Set(head.map(|h| h / 1000)),
        source: sea_orm::Set("revlog".to_string()),
        revlog_head: sea_orm::Set(head),
        updated_at: sea_orm::Set(12_345),
    };
    FsrsMemoryState::insert(model).exec(db).await.expect("seed row");
}

async fn row_for(
    db: &DatabaseConnection,
    student: &str,
    card: i64,
) -> Option<fsrs_memory_state::Model> {
    FsrsMemoryState::find()
        .filter(fsrs_memory_state::Column::StudentId.eq(student))
        .filter(fsrs_memory_state::Column::CardId.eq(card))
        .one(db)
        .await
        .expect("query row")
}

#[tokio::test]
async fn ensure_states_reuses_fresh_cache_without_rewrite() {
    let db = fresh_db().await;
    let head = review_ms(5);
    let snapshot = snapshot_with_revlog(101, &[(review_ms(30), 3, 0), (head, 3, 0)]);

    // Pre-seed a row that is already fresh (revlog_head matches the
    // snapshot). ensure_states must serve it from cache and NOT rewrite it
    // (sentinel updated_at survives).
    seed_row(&db, "alice", 101, Some(head)).await;

    let states = ensure_states(&db, "alice", &snapshot).await.expect("ensure");
    let state = states.get(&101).expect("cached state served");
    assert_eq!(state.stability, 3.25);
    assert_eq!(state.difficulty, 5.5);
    assert_eq!(state.source, StateSource::Revlog);

    let row = row_for(&db, "alice", 101).await.expect("row exists");
    assert_eq!(row.updated_at, 12_345, "fresh cache hit must not rewrite");
}

#[tokio::test]
async fn ensure_states_refreshes_stale_rows() {
    let db = fresh_db().await;
    let head = review_ms(5);
    let snapshot = snapshot_with_revlog(101, &[(review_ms(30), 3, 0), (head, 3, 0)]);

    // Stale: persisted head is one review behind the snapshot.
    seed_row(&db, "alice", 101, Some(head - DAY_MS)).await;

    let states = ensure_states(&db, "alice", &snapshot).await.expect("ensure");
    let state = states.get(&101).expect("refreshed state");
    assert_eq!(state.revlog_head, Some(head), "stale row must re-derive");
    assert_eq!(state.source, StateSource::Revlog);
    assert_eq!(state.last_review_secs, Some(head / 1000));

    let row = row_for(&db, "alice", 101).await.expect("row exists");
    assert_eq!(row.revlog_head, Some(head), "row must be updated");
    assert!(row.updated_at > 12_345, "refresh must bump updated_at");
}

#[tokio::test]
async fn ensure_states_isolates_students() {
    let db = fresh_db().await;
    let head = review_ms(5);
    let snapshot = snapshot_with_revlog(101, &[(review_ms(30), 3, 0), (head, 3, 0)]);

    // Student A has a fresh cached row; student B has nothing.
    seed_row(&db, "alice", 101, Some(head)).await;

    let states_b = ensure_states(&db, "bob", &snapshot).await.expect("ensure B");
    let state_b = states_b.get(&101).expect("B state derived fresh");
    assert_eq!(state_b.source, StateSource::Revlog);
    assert_ne!(
        state_b.stability, 3.25,
        "B must get a derived state, not A's seeded values"
    );

    // A's row is untouched by B's resolution.
    let row_a = row_for(&db, "alice", 101).await.expect("A row exists");
    assert_eq!(row_a.updated_at, 12_345, "A's fresh row must not be rewritten");
    assert_eq!(row_a.stability, 3.25);

    // And B now has exactly one row of their own.
    let row_b = row_for(&db, "bob", 101).await.expect("B row exists");
    assert_eq!(row_b.revlog_head, Some(head));
}

#[tokio::test]
async fn ensure_states_derives_and_persists_missing_cards() {
    let db = fresh_db().await;
    let head = review_ms(5);
    let snapshot = snapshot_with_revlog(101, &[(review_ms(30), 3, 0), (head, 3, 0)]);

    let states = ensure_states(&db, "alice", &snapshot).await.expect("ensure");
    let state = states.get(&101).expect("derived state");
    assert_eq!(state.source, StateSource::Revlog);
    assert_eq!(state.revlog_head, Some(head));

    let row = row_for(&db, "alice", 101).await.expect("persisted");
    assert_eq!(row.revlog_head, Some(head));
    assert!((row.stability - state.stability).abs() < 1e-9);
}

// ---------------------------------------------------------------------------
// Production entry point end-to-end
// ---------------------------------------------------------------------------

/// Build a minimal schema-v18 `.anki2` with cards + revlog rows.
/// Card spec: (id, tags, ivl, type, due, queue). Revlog spec:
/// (cid, id, ease, type).
fn build_collection(
    dir: &std::path::Path,
    crt: i64,
    cards: &[(i64, &str, i64, i64, i64, i64)],
    revlog: &[(i64, i64, i64, i64)],
) -> std::path::PathBuf {
    let path = dir.join("collection.anki2");
    let conn = rusqlite::Connection::open(&path).expect("open synthetic collection");
    conn.execute_batch(
        "CREATE TABLE col (
            id INTEGER PRIMARY KEY, crt INTEGER NOT NULL DEFAULT 0,
            mod INTEGER NOT NULL DEFAULT 0, scm INTEGER NOT NULL DEFAULT 0,
            ver INTEGER NOT NULL DEFAULT 0, dty INTEGER NOT NULL DEFAULT 0,
            usn INTEGER NOT NULL DEFAULT 0, ls INTEGER NOT NULL DEFAULT 0,
            conf TEXT NOT NULL DEFAULT '{}', models TEXT NOT NULL DEFAULT '{}',
            decks TEXT NOT NULL DEFAULT '{}', dconf TEXT NOT NULL DEFAULT '{}',
            tags TEXT NOT NULL DEFAULT '');
         CREATE TABLE notes (
            id INTEGER PRIMARY KEY, guid TEXT NOT NULL DEFAULT '',
            mid INTEGER NOT NULL DEFAULT 0, mod INTEGER NOT NULL DEFAULT 0,
            usn INTEGER NOT NULL DEFAULT 0, tags TEXT NOT NULL DEFAULT '',
            flds TEXT NOT NULL DEFAULT '', sfld TEXT NOT NULL DEFAULT '',
            csum INTEGER NOT NULL DEFAULT 0, flags INTEGER NOT NULL DEFAULT 0,
            data TEXT NOT NULL DEFAULT '');
         CREATE TABLE cards (
            id INTEGER PRIMARY KEY, nid INTEGER NOT NULL DEFAULT 0,
            did INTEGER NOT NULL DEFAULT 0, ord INTEGER NOT NULL DEFAULT 0,
            mod INTEGER NOT NULL DEFAULT 0, usn INTEGER NOT NULL DEFAULT 0,
            type INTEGER NOT NULL DEFAULT 0, queue INTEGER NOT NULL DEFAULT 0,
            due INTEGER NOT NULL DEFAULT 0, ivl INTEGER NOT NULL DEFAULT 0,
            factor INTEGER NOT NULL DEFAULT 0, reps INTEGER NOT NULL DEFAULT 0,
            lapses INTEGER NOT NULL DEFAULT 0, left INTEGER NOT NULL DEFAULT 0,
            odue INTEGER NOT NULL DEFAULT 0, odid INTEGER NOT NULL DEFAULT 0,
            flags INTEGER NOT NULL DEFAULT 0, data TEXT NOT NULL DEFAULT '');
         CREATE TABLE revlog (
            id INTEGER PRIMARY KEY, cid INTEGER NOT NULL DEFAULT 0,
            usn INTEGER NOT NULL DEFAULT 0, ease INTEGER NOT NULL DEFAULT 0,
            ivl INTEGER NOT NULL DEFAULT 0, last_ivl INTEGER NOT NULL DEFAULT 0,
            factor INTEGER NOT NULL DEFAULT 0, time INTEGER NOT NULL DEFAULT 0,
            type INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE decks (
            id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT '',
            mtime_secs INTEGER NOT NULL DEFAULT 0, usn INTEGER NOT NULL DEFAULT 0,
            common TEXT NOT NULL DEFAULT '{}', kind TEXT NOT NULL DEFAULT '{}');
         CREATE TABLE notetypes (
            id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT '',
            mtime_secs INTEGER NOT NULL DEFAULT 0, usn INTEGER NOT NULL DEFAULT 0,
            config TEXT NOT NULL DEFAULT '{}', fields TEXT NOT NULL DEFAULT '[]',
            templates TEXT NOT NULL DEFAULT '[]', type INTEGER NOT NULL DEFAULT 0);",
    )
    .expect("create synthetic schema");

    conn.execute(
        "INSERT INTO col (id, crt, mod, scm, ver, conf) VALUES (1, ?1, 0, 0, 18, '{}')",
        [crt],
    )
    .expect("insert col");
    conn.execute(
        "INSERT INTO decks (id, name, mtime_secs, usn, common, kind) VALUES (1, 'Synthetic', 0, 0, '{}', '{}')",
        [],
    )
    .expect("insert deck");

    for (cid, tags, ivl, ctype, due, queue) in cards {
        conn.execute(
            "INSERT INTO notes (id, guid, mid, mod, usn, tags, flds, sfld, csum, flags, data)
             VALUES (?1, '', 1, 0, 0, ?2, '', '', 0, 0, '')",
            rusqlite::params![cid, tags],
        )
        .expect("insert note");
        conn.execute(
            "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data)
             VALUES (?1, ?1, 1, 0, 0, 0, ?2, ?3, ?4, ?5, 2500, 0, 0, 0, 0, 0, 0, '')",
            rusqlite::params![cid, ctype, queue, due, ivl],
        )
        .expect("insert card");
    }
    for (cid, rid, ease, rtype) in revlog {
        conn.execute(
            "INSERT INTO revlog (id, cid, usn, ease, ivl, last_ivl, factor, time, type)
             VALUES (?1, ?2, 0, ?3, 0, 0, 0, 0, ?4)",
            rusqlite::params![rid, cid, ease, rtype],
        )
        .expect("insert revlog row");
    }
    path
}

#[tokio::test]
async fn fetch_and_sort_cached_end_to_end() {
    let db = fresh_db().await;
    let dir = tempfile::TempDir::new().expect("tempdir");
    // crt = 100 days ago → today_day = 100 for the service.
    let crt = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
        - 100 * 86_400;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis() as i64;

    // Card 1: new (type 0) — sentinel-scored, no state needed.
    // Card 2: review (type 2), due today, ivl 10, with a real revlog history.
    // Card 3: learning (type 1), due in the past (seconds domain).
    let path = build_collection(
        dir.path(),
        crt,
        &[
            (1, "vocab", 0, 0, 0, 0),
            (2, "vocab", 10, 2, 100, 1),
            (3, "grammar", 0, 1, crt - 86_400, 1),
        ],
        &[
            (2, now_ms - 30 * DAY_MS, 3, 0),
            (2, now_ms - 20 * DAY_MS, 3, 0),
            (2, now_ms - 5 * DAY_MS, 4, 0),
        ],
    );

    let tags = vec!["vocab".to_string(), "grammar".to_string()];
    let result =
        fetch_and_sort_cached(&db, "student-e2e", &path, &tags, 30).await.expect("cached sort");

    // All three cards admitted and ranked.
    assert_eq!(result.ranked.len(), 3, "all three cards must be admitted");
    assert_eq!(result.track_counts.get("vocab"), Some(&2));
    assert_eq!(result.track_counts.get("grammar"), Some(&1));

    // Ordering: recall ascending (the primary urgency key).
    for pair in result.ranked.windows(2) {
        assert!(
            pair[0].approximate_recall <= pair[1].approximate_recall,
            "ranked must be recall-ascending: {:?}",
            result.ranked
        );
    }

    // The two non-new cards got persisted states scoped to the student.
    let row2 = row_for(&db, "student-e2e", 2).await.expect("card 2 state");
    assert_eq!(row2.source, "revlog");
    assert!(row2.stability > 0.0);
    let row3 = row_for(&db, "student-e2e", 3).await.expect("card 3 state");
    assert_eq!(row3.source, "first_transition");

    // New card 1 must NOT have a persisted state (sentinel-scored).
    assert!(
        row_for(&db, "student-e2e", 1).await.is_none(),
        "new cards are sentinel-scored and must not be persisted"
    );

    // Second call: everything is cached now — same result, no re-derivation
    // errors, and the rows are unchanged (fresh cache hit).
    let again =
        fetch_and_sort_cached(&db, "student-e2e", &path, &tags, 30).await.expect("second sort");
    assert_eq!(again.ranked.len(), 3);
    for (a, b) in result.ranked.iter().zip(again.ranked.iter()) {
        assert_eq!(a.card_id, b.card_id, "cached pass must reproduce ordering");
        assert!((a.approximate_recall - b.approximate_recall).abs() < 1e-9);
    }
}

#[tokio::test]
async fn cached_and_in_house_schedulers_admit_the_same_cards() {
    let db = fresh_db().await;
    let dir = tempfile::TempDir::new().expect("tempdir");
    let crt = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
        - 100 * 86_400;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis() as i64;

    let path = build_collection(
        dir.path(),
        crt,
        &[
            (1, "vocab", 0, 0, 0, 0),
            (2, "vocab", 10, 2, 95, 1),
            (3, "grammar", 3, 2, 98, 1),
            (4, "grammar", 0, 1, crt - 3600, 1),
        ],
        &[
            (2, now_ms - 40 * DAY_MS, 3, 0),
            (2, now_ms - 10 * DAY_MS, 2, 0),
            (3, now_ms - 15 * DAY_MS, 4, 0),
        ],
    );

    let tags = vec!["vocab".to_string(), "grammar".to_string()];
    let in_house = fetch_and_sort(&path, &tags, 30).expect("in-house sort");
    let fsrs = fetch_and_sort_cached(&db, "student-iso", &path, &tags, 30)
        .await
        .expect("cached sort");

    // Same admission set and track counts (the due-filter is shared); the
    // recall *values* legitimately differ between models — ordering is each
    // scheduler's own contract, asserted separately.
    let ids_in_house: std::collections::BTreeSet<i64> =
        in_house.ranked.iter().map(|c| c.card_id).collect();
    let ids_fsrs: std::collections::BTreeSet<i64> = fsrs.ranked.iter().map(|c| c.card_id).collect();
    assert_eq!(ids_in_house, ids_fsrs, "schedulers must admit the same cards");
    assert_eq!(in_house.track_counts, fsrs.track_counts);
}

// ---------------------------------------------------------------------------
// Snapshot extraction against a real file
// ---------------------------------------------------------------------------

#[test]
fn extract_snapshot_reads_revlog_and_facts() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let crt = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
        - 100 * 86_400;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis() as i64;
    let path = build_collection(
        dir.path(),
        crt,
        &[(2, "vocab", 10, 2, 100, 1)],
        &[(2, now_ms - 5 * DAY_MS, 3, 0)],
    );
    let conn = rusqlite::Connection::open(&path).expect("open");

    let snapshot = extract_snapshot(&conn, &[2]).expect("extract");
    assert_eq!(snapshot.card_ids, vec![2]);
    assert_eq!(snapshot.revlog_heads.get(&2), Some(&(now_ms - 5 * DAY_MS)));
    let facts = snapshot.sched_facts.get(&2).expect("facts");
    assert_eq!((facts.card_type, facts.ivl, facts.factor), (2, 10, 2500));
    assert_eq!(
        snapshot.revlog_rows.get(&2).expect("rows"),
        &vec![(now_ms - 5 * DAY_MS, 3, 0)]
    );
}

/// Helper for the stability-growth test: merge two single-card snapshots.
trait MergeSnapshot {
    fn merged_with(self, other: &SnapshotData) -> SnapshotData;
}

impl MergeSnapshot for SnapshotData {
    fn merged_with(self, other: &SnapshotData) -> SnapshotData {
        let mut merged = self;
        merged.card_ids.extend(other.card_ids.iter().copied());
        merged.revlog_heads.extend(other.revlog_heads.clone());
        merged.sched_facts.extend(other.sched_facts.clone());
        merged.revlog_rows.extend(other.revlog_rows.clone());
        merged
    }
}
