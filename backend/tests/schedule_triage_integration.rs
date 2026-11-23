//! B-0035 — Sporadic-continuity scheduler: backlog triage + early-review fill.
//!
//! End-to-end coverage of the two Phase-1 spec moves through the real
//! assembly path (`generate_capsule`):
//!
//! 1. Deferral-overlay backlog triage — after a long absence the due pile
//!    overflows the session budget; overflow cards are spread into
//!    budget-sized tranches, one per upcoming day, across
//!    `K = ceil(overflow / budget)` sessions.
//! 2. Early-review fill — when the eligible pool is thinner than the budget,
//!    the session is backfilled from not-yet-due cards whose elapsed time is
//!    at least `fill.interval_fraction × ivl` and whose recall is at least
//!    `fill.r_min`.
//!
//! Uses the synthetic schema-v18 `.anki2` fixture (same pattern as
//! `fsrs_state_persistence_test.rs`) + a migrated in-memory backend DB.
//!
//! Spec: specs/design/2026-11-14-sporadic-continuity-scheduler.md
//!
//! Run: `cargo nextest run -p backend --test schedule_triage_integration`
//! (the env-var tests rely on nextest running each test in its own process).

use backend::models::entities::audit_log::{Column as AuditLogCol, Entity as AuditLog};
use backend::models::entities::capsule_session::{self, Entity as CapsuleSession};
use backend::models::entities::schedule_override::{self, ActiveModel as ScheduleOverrideActiveModel, Column as OverrideCol, Entity as ScheduleOverride};
use backend::models::entities::track::ActiveModel as TrackActiveModel;
use backend::models::entities::track_profile::ActiveModel as TrackProfileActiveModel;
use backend::models::entities::track_profile_track::ActiveModel as TrackProfileTrackActiveModel;
use backend::models::entities::web_review_log::{self};
use backend::services::capsule_generator::{generate_capsule, GenerateRequest};
use backend::services::review_ingest::{ingest_answer, ReviewAnswer};
use backend::services::schedule_triage;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Database, DatabaseConnection, EntityTrait, QueryFilter, Set,
};
use sea_orm_migration::MigratorTrait;
use std::collections::{HashMap, HashSet};

const DAY_MS: i64 = 86_400_000;
const DAY_SECS: i64 = 86_400;
/// The capsule path carries the student id as a string while the web-review
/// ingest path carries it as an i64; the override overlay keys on the
/// decimal string on both paths (web_review_log stores `i64.to_string()`),
/// so the fixture student id is a decimal string.
const STUDENT: &str = "777001";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn fresh_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.expect("connect");
    backend::migrations::Migrator::up(&db, None)
        .await
        .expect("migrate");
    db
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis() as i64
}

/// Build a minimal schema-v18 `.anki2` with cards + revlog rows.
/// Card spec: (id, tags, ivl, type, due, queue). Revlog spec:
/// (cid, id(epoch-ms), ease, type). Copied verbatim from
/// `fsrs_state_persistence_test.rs`.
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

/// Synthetic `.anki2` for the triage scenarios: `due_count` overdue review
/// cards (ids `1..=due_count`, 5 days overdue, ivl `10 + id % 90`, one
/// 20-day-old revlog row each) plus `future_count` not-yet-due review cards
/// (ids `200..`, due today+30, ivl 30). `col.crt` is anchored so
/// `today_day == 100`.
///
/// The revlog `time` column stays 0 → the per-card time estimate falls back
/// to `COLD_START_SECONDS_PER_CARD` (25 s), so with the profile below
/// (n=1, 10-minute sessions) the budget is
/// `min(25, ceil(pool / 1), floor(600 / 25)) = min(pool, 24)`.
fn build_fixture(
    dir: &std::path::Path,
    due_count: usize,
    future_count: usize,
) -> (std::path::PathBuf, i64) {
    let now = now_secs();
    let crt = now - 100 * DAY_SECS;
    let today_day = 100i64;

    let mut cards: Vec<(i64, &str, i64, i64, i64, i64)> = Vec::new();
    for id in 1..=due_count as i64 {
        cards.push((id, "math", 10 + id % 90, 2, today_day - 5, 2));
    }
    for i in 0..future_count as i64 {
        let id = 200 + i;
        cards.push((id, "math", 30, 2, today_day + 30, 2));
    }

    // One well-inside-lookback review each (derivations succeed; the avg-time
    // probe only looks at `r.time > 0`, which stays 0 → cold-start fallback).
    let head_ms = now_ms() - 20 * DAY_MS;
    // revlog.id must be unique per row (it IS the PK): offset by card id.
    let revlog: Vec<(i64, i64, i64, i64)> = (1..=due_count as i64)
        .map(|id| (id, head_ms + id, 3, 0))
        .collect();

    (build_collection(dir, crt, &cards, &revlog), today_day)
}

/// One profile ("prof-1") with a single "math" track; n=1 + 10-minute
/// sessions pin the budget math above.
async fn make_profile(db: &DatabaseConnection) {
    let now = now_secs();
    TrackProfileActiveModel {
        id: Set("prof-1".into()),
        name: Set("triage-profile".into()),
        description: Set(None),
        target_retention: Set(None),
        n_value: Set(Some(1)),
        session_duration_minutes: Set(10),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .expect("insert profile");
    TrackActiveModel {
        id: Set("trk-1".into()),
        name: Set("math-track".into()),
        description: Set(None),
        subject_area: Set(None),
        tag: Set("math".into()),
        target_retention: Set(0.8),
        n_value: Set(3),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .expect("insert track");
    TrackProfileTrackActiveModel {
        track_profile_id: Set("prof-1".into()),
        track_id: Set("trk-1".into()),
        sort_order: Set(1),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert junction");
}

fn gen_request(path: &std::path::Path) -> GenerateRequest {
    GenerateRequest {
        student_id: STUDENT.into(),
        anki2_path: path.display().to_string(),
        track_profile_id: "prof-1".into(),
        session_week: "2026-W47".into(),
        n_value: None,
        session_duration_minutes: None,
    }
}

async fn seed_override(db: &DatabaseConnection, card_id: i64, until_ms: i64) {
    ScheduleOverrideActiveModel {
        id: Set(format!("ovr-seed-{card_id}")),
        student_id: Set(STUDENT.into()),
        card_id: Set(card_id),
        deferred_until: Set(until_ms),
        reason: Set(Some("backlog-triage".into())),
        k_sessions: Set(1),
        created_at: Set(now_ms()),
        cleared_at: Set(None),
    }
    .insert(db)
    .await
    .expect("seed override");
}

async fn insert_web_row(db: &DatabaseConnection, id: String, card_id: i64, reviewed_at: i64) {
    web_review_log::ActiveModel {
        id: Set(id),
        student_id: Set(STUDENT.into()),
        card_id: Set(card_id),
        // NOTE: sea-orm 1.1.20 emits `NOT NULL NULL` for `.null()` columns on
        // SQLite, so session_id is effectively NOT NULL in every deployed DB
        // (pre-existing schema quirk, outside B-0035 scope) — the fixture
        // always supplies a session id.
        session_id: Set(Some("sess-fixture".into())),
        ease: Set(3),
        time_ms: Set(5_000),
        reviewed_at: Set(reviewed_at),
    }
    .insert(db)
    .await
    .expect("web row");
}

async fn active_overrides(
    db: &DatabaseConnection,
) -> Vec<backend::models::entities::schedule_override::Model> {
    ScheduleOverride::find()
        .filter(OverrideCol::StudentId.eq(STUDENT))
        .filter(OverrideCol::ClearedAt.is_null())
        .all(db)
        .await
        .expect("overrides")
}

async fn triage_events(db: &DatabaseConnection) -> Vec<serde_json::Value> {
    let rows = AuditLog::find()
        .filter(AuditLogCol::EventType.eq(schedule_triage::AUDIT_EVENT_TYPE))
        .all(db)
        .await
        .expect("audit rows");
    rows.into_iter()
        .map(|r| serde_json::from_str(&r.event_data).expect("payload json"))
        .collect()
}

// ---------------------------------------------------------------------------
// 1. Absence backlog — budget-sized session + K deferral tranches
// ---------------------------------------------------------------------------

#[tokio::test]
async fn absence_backlog_defers_across_k_tranches() {
    let db = fresh_db().await;
    make_profile(&db).await;
    let dir = tempfile::TempDir::new().expect("tempdir");
    let now0 = now_ms();
    let (path, _) = build_fixture(dir.path(), 100, 0);

    let resp = generate_capsule(&db, gen_request(&path))
        .await
        .expect("generate session 1");
    assert_eq!(resp.card_ids.len(), 24, "session = exactly the budget");
    assert_eq!(resp.capsule_size, 24);

    // 76 active overrides spread across K = ceil(76/24) = 4 budget-sized
    // tranches, one per upcoming day (days 1..4).
    let rows = active_overrides(&db).await;
    assert_eq!(rows.len(), 76);
    let selected: HashSet<i64> = resp.card_ids.iter().copied().collect();
    assert!(
        rows.iter().all(|r| !selected.contains(&r.card_id)),
        "selected cards must not be deferred"
    );

    let mut by_day: HashMap<i64, usize> = HashMap::new();
    for r in &rows {
        let days = ((r.deferred_until - now0) as f64 / DAY_MS as f64).round() as i64;
        *by_day.entry(days).or_default() += 1;
    }
    assert_eq!(by_day.len(), 4, "one tranche per upcoming day: {by_day:?}");
    assert_eq!(by_day.get(&1), Some(&24));
    assert_eq!(by_day.get(&2), Some(&24));
    assert_eq!(by_day.get(&3), Some(&24));
    assert_eq!(by_day.get(&4), Some(&4));

    // Telemetry: exactly one schedule_triage audit event for session 1.
    let events = triage_events(&db).await;
    assert_eq!(events.len(), 1);
    let p = &events[0];
    assert_eq!(p["student"], STUDENT);
    assert_eq!(p["due_count"], 100);
    assert_eq!(p["budget"], 24);
    assert_eq!(p["deferred_count"], 76);
    assert_eq!(p["k"], 4);
    assert_eq!(p["filled_count"], 0);
    assert_eq!(p["triage_enabled"], true);
}

// ---------------------------------------------------------------------------
// 2. Re-entry — an elapsed tranche rejoins the due flow on schedule
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reentry_after_tranche_elapses() {
    let db = fresh_db().await;
    make_profile(&db).await;
    let dir = tempfile::TempDir::new().expect("tempdir");
    let (path, _) = build_fixture(dir.path(), 100, 0);

    let s1 = generate_capsule(&db, gen_request(&path))
        .await
        .expect("session 1");
    assert_eq!(s1.card_ids.len(), 24);

    // Tranche 1 = the earliest deferred_until bucket (24 cards).
    let rows = active_overrides(&db).await;
    assert_eq!(rows.len(), 76);
    let min_until = rows.iter().map(|r| r.deferred_until).min().expect("rows");
    let tranche1: Vec<i64> = rows
        .iter()
        .filter(|r| r.deferred_until == min_until)
        .map(|r| r.card_id)
        .collect();
    assert_eq!(tranche1.len(), 24);

    // Complete session 1 (so the 30-minute rate limit does not fire), answer
    // its 24 cards through the web-reviewer ledger (derived states see
    // elapsed ≈ 0 → recall ≈ 1 → they drop to the back of the R-ascending
    // sort), and backdate tranche 1 so it is eligible again.
    let session = CapsuleSession::find_by_id(&s1.session_id)
        .one(&db)
        .await
        .expect("find session")
        .expect("session row exists");
    let mut completed: capsule_session::ActiveModel = session.into();
    completed.status = Set("completed".to_string());
    completed.completed_at = Set(Some(now_secs()));
    completed.update(&db).await.expect("complete session 1");

    for id in &s1.card_ids {
        insert_web_row(
            &db,
            format!("ans-1-{id}"),
            *id,
            now_secs(),
        )
        .await;
    }

    let backdate = now_ms() - 1_000;
    for r in rows.iter().filter(|r| r.deferred_until == min_until) {
        let mut am: schedule_override::ActiveModel = Default::default();
        am.id = Set(r.id.clone());
        am.deferred_until = Set(backdate);
        am.update(&db).await.expect("backdate tranche");
    }

    let s2 = generate_capsule(&db, gen_request(&path))
        .await
        .expect("session 2");
    assert_eq!(s2.card_ids.len(), 24);
    let s1_set: HashSet<i64> = s1.card_ids.iter().copied().collect();
    let s2_set: HashSet<i64> = s2.card_ids.iter().copied().collect();
    assert!(s1_set.is_disjoint(&s2_set), "no same cards back-to-back");
    let want: HashSet<i64> = tranche1.iter().copied().collect();
    assert_eq!(s2_set, want, "session 2 picks up the elapsed tranche");

    // Telemetry: session 2 re-plans the 24 answered cards (overflow) into one
    // future tranche; session 1's event is the 76/4 baseline.
    let events = triage_events(&db).await;
    assert_eq!(events.len(), 2);
    let s2_payload = events
        .iter()
        .find(|p| p["deferred_count"] == 24 && p["k"] == 1)
        .expect("session 2 telemetry");
    assert_eq!(s2_payload["budget"], 24);
    assert_eq!(s2_payload["due_count"], 100);
    assert_eq!(s2_payload["filled_count"], 0);
    let s1_payload = events
        .iter()
        .find(|p| p["deferred_count"] == 76 && p["k"] == 4)
        .expect("session 1 telemetry");
    assert_eq!(s1_payload["budget"], 24);
}

// ---------------------------------------------------------------------------
// 3. Answering a card clears its active override (web-reviewer hook)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn web_answer_clears_active_override() {
    let db = fresh_db().await;
    make_profile(&db).await;
    let dir = tempfile::TempDir::new().expect("tempdir");
    let (path, _) = build_fixture(dir.path(), 30, 0);

    let resp = generate_capsule(&db, gen_request(&path))
        .await
        .expect("session 1");
    assert_eq!(resp.card_ids.len(), 24);
    let rows = active_overrides(&db).await;
    assert_eq!(rows.len(), 6, "30 due, budget 24 → 6 deferred");
    let cleared_target = rows[0].card_id;

    ingest_answer(
        &db,
        777_001,
        "student-triage@example.test",
        &ReviewAnswer {
            answer_id: "clear-hook-test-answer-0001".into(),
            card_id: cleared_target,
            ease: 3,
            time_ms: 4_000,
            session_id: Some("clear-hook-sess".into()),
        },
    )
    .await
    .expect("ingest");

    assert_eq!(active_overrides(&db).await.len(), 5);
    // The row is retired (audit trail), not deleted.
    let cleared = ScheduleOverride::find()
        .filter(OverrideCol::StudentId.eq(STUDENT))
        .filter(OverrideCol::CardId.eq(cleared_target))
        .all(&db)
        .await
        .expect("query cleared row");
    assert_eq!(cleared.len(), 1);
    assert!(cleared[0].cleared_at.is_some(), "row retired, not deleted");

    // Duplicate ingest (same answer_id) is idempotent and harmless.
    ingest_answer(
        &db,
        777_001,
        "student-triage@example.test",
        &ReviewAnswer {
            answer_id: "clear-hook-test-answer-0001".into(),
            card_id: cleared_target,
            ease: 3,
            time_ms: 4_000,
            session_id: Some("clear-hook-sess".into()),
        },
    )
    .await
    .expect("duplicate ingest");
    assert_eq!(active_overrides(&db).await.len(), 5);
}

// ---------------------------------------------------------------------------
// 4. Thin-day early fill — gates: r_min floor + interval fraction + state
// ---------------------------------------------------------------------------

#[tokio::test]
async fn thin_day_early_fill_respects_gates() {
    // In-house scheduler: recall for not-yet-due cards is the constant 0.95,
    // so fill selection is the deterministic card_id-ascending tie-break.
    std::env::set_var("ANKITOV_SCHEDULER", "in-house");
    let db = fresh_db().await;
    make_profile(&db).await;
    let dir = tempfile::TempDir::new().expect("tempdir");
    let (path, _) = build_fixture(dir.path(), 30, 40);

    // Simulate prior sessions: 20 of the 30 due cards sit under future
    // overrides → the eligible pool (10) is thinner than the budget (24,
    // computed from the PRE-filter pool of 30).
    let now0 = now_ms();
    for id in 11..=30i64 {
        seed_override(&db, id, now0 + DAY_MS).await;
    }
    // The interval-fraction gate: elapsed ≥ 0.5 × ivl(30) = 15 days.
    // Cards 200..236 were last seen 20 days ago (pass), 237/238 5 days ago
    // (blocked), 239 has no history at all (blocked).
    for id in 200..=236i64 {
        insert_web_row(&db, format!("fill-ans-{id}"), id, now_secs() - 20 * DAY_SECS).await;
    }
    for id in [237, 238] {
        insert_web_row(&db, format!("fill-ans-{id}"), id, now_secs() - 5 * DAY_SECS).await;
    }

    let resp = generate_capsule(&db, gen_request(&path))
        .await
        .expect("generate");
    assert_eq!(resp.card_ids.len(), 24, "10 due + 14 filled = budget");
    let got: HashSet<i64> = resp.card_ids.iter().copied().collect();
    for id in 1..=10i64 {
        assert!(got.contains(&id), "due card {id} missing");
    }
    for id in 200..=213i64 {
        assert!(got.contains(&id), "fill card {id} missing (card_id asc)");
    }
    for id in [237, 238, 239] {
        assert!(!got.contains(&id), "gate-excluded card {id} selected");
    }
    for id in 11..=30i64 {
        assert!(!got.contains(&id), "deferred card {id} leaked into session");
    }

    // No new deferrals (overflow was empty) and no overrides for filled cards.
    let rows = active_overrides(&db).await;
    assert_eq!(rows.len(), 20);
    assert!(rows.iter().all(|r| !(200..=239).contains(&r.card_id)));

    let events = triage_events(&db).await;
    assert_eq!(events.len(), 1);
    let p = &events[0];
    assert_eq!(p["due_count"], 30);
    assert_eq!(p["budget"], 24);
    assert_eq!(p["deferred_count"], 0);
    assert_eq!(p["k"], 0);
    assert_eq!(p["filled_count"], 14);
    assert_eq!(p["triage_enabled"], true);

    std::env::remove_var("ANKITOV_SCHEDULER");
}

// ---------------------------------------------------------------------------
// 5. Kill-switch — ANKITOV_SCHEDULER_TRIAGE=off → exactly pre-B-0035 behavior
// ---------------------------------------------------------------------------

#[tokio::test]
async fn kill_switch_disables_triage_and_fill() {
    std::env::set_var("ANKITOV_SCHEDULER_TRIAGE", "off");
    let db = fresh_db().await;
    make_profile(&db).await;
    let dir = tempfile::TempDir::new().expect("tempdir");
    let (path, _) = build_fixture(dir.path(), 5, 40);
    // 40 eligible fill candidates would exist with triage on — the
    // kill-switch must block the fill as well as the overlay.
    for id in 200..=239i64 {
        insert_web_row(&db, format!("fill-ans-{id}"), id, now_secs() - 20 * DAY_SECS).await;
    }

    let resp = generate_capsule(&db, gen_request(&path))
        .await
        .expect("generate");
    assert_eq!(resp.card_ids.len(), 5, "kill-switch: no fill backfill");
    assert!(
        active_overrides(&db).await.is_empty(),
        "kill-switch: no deferrals"
    );

    // Telemetry is still emitted (the disabled events are the A/B baseline).
    let events = triage_events(&db).await;
    assert_eq!(events.len(), 1);
    let p = &events[0];
    assert_eq!(p["due_count"], 5);
    assert_eq!(p["budget"], 5);
    assert_eq!(p["deferred_count"], 0);
    assert_eq!(p["filled_count"], 0);
    assert_eq!(p["triage_enabled"], false);

    std::env::remove_var("ANKITOV_SCHEDULER_TRIAGE");
}

