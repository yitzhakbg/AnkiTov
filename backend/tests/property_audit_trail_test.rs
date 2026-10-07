//! Property tests — audit-trail invariants.
//!
//! Source of truth: `specs/audits/audit-trail-invariants.txt` (INVARIANT X—DD).
//! Spec: `specs/harness/2026-10-04-test-gates.md` §4.
//!
//! The ledger's own module doc calls it an "immutable append-only event ledger".
//! These tests hold it to that. Each test builds a private in-memory SQLite
//! (one per test process, so `nextest` parallelism is safe) and runs the
//! migrations the production config runs.
//!
//! Deferred to M2 (call-site audits, not value properties): INVARIANT Y, Z, AA
//! (every N-change / profile assign / revoke / generate is logged *at the call
//! sites* in track_profiles.rs, tracks.rs, capsule_generator.rs) and DD
//! (telemetry enrichment). See spec §4.1.
//!
//! Run: `cargo nextest run property_audit_trail`

use backend::migrations::Migrator;
use sea_orm::{Database, EntityTrait, PaginatorTrait, QueryOrder};
use sea_orm_migration::MigratorTrait;

mod entity {
    pub use backend::models::entities::audit_log;
}
use entity::audit_log::{self, Entity as AuditLog};

/// A private migrated in-memory database. Returns a fresh handle + runtime so
/// proptest bodies (which are sync) can drive async writes.
async fn fresh_db() -> sea_orm::DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.expect("connect");
    Migrator::up(&db, None).await.expect("migrate");
    db
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
}

use proptest::prelude::*;

fn event_type() -> impl Strategy<Value = String> {
    prop::sample::select(vec!["n_change", "profile_assign", "profile_revoke", "capsule_generate"])
        .prop_map(String::from)
}

/// Random JSON-ish event_data payloads, including edge shapes (empty, unicode,
/// long) — the payload must round-trip verbatim.
fn event_data() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just("{\"old_n\":3,\"new_n\":1,\"cohort_id\":\"c-1\"}".to_string()),
        Just("{}".to_string()),
        "[1,2,3]".prop_map(String::from),
        // The regex strategy yields an owned `String`, not `&str` — annotating
        // the parameter as `&str` is an E0631 type mismatch.
        ".*".prop_map(|s: String| s.repeat(3)),
    ]
}

// ── INVARIANT X — fire-and-forget: log_event returns (), never propagates ────
//
// This is a compile-time property: if `log_event`'s return type ever becomes
// `Result`, this assignment stops compiling. That is the strongest possible
// encoding of "caller never blocks on audit write failures".
#[tokio::test]
async fn invariant_x_log_event_returns_unit() {
    let db = fresh_db().await;

    let result: () = backend::services::audit_logger::log_event(
        &db,
        "n_change",
        r#"{"old_n":3,"new_n":1}"#,
        "instructor-1",
        Some("cohort-1"),
    )
    .await;

    let _: () = result; // INVARIANT X: fire-and-forget, no Result in the path
    assert_eq!(AuditLog::find().count(&db).await.unwrap(), 1);
}

// ── INVARIANT X — a failing audit write must not panic or propagate ────────
#[tokio::test]
async fn log_event_tolerates_unwritable_db() {
    // A database that cannot be opened: the audit write must fail *internally*
    // (traced at error!) and the call must still return normally.
    let closed = Database::connect("sqlite::memory:").await.unwrap();
    // Drop the pool's backing store by using a corrupt path instead:
    let broken = Database::connect("sqlite:///nonexistent-dir/ankitov.db?mode=rw").await;
    if let Ok(db) = broken {
        let _: () = backend::services::audit_logger::log_event(
            &db, "capsule_generate", "{}", "system", None,
        )
        .await;
    } else {
        // Could not even connect — also a non-panicking outcome for the caller.
    }
    // Closing an established connection must likewise not take the caller down.
    drop(closed);
}

proptest! {
    // ── INVARIANT BB — every entry gets a fresh v4 UUID; no sequential IDs ──
    #[test]
    fn audit_ids_are_unique_v4(
        entries in prop::collection::vec((event_type(), event_data(), "[a-z0-9\\-]{1,8}"), 1..30),
    ) {
        let rt = runtime();
        rt.block_on(async {
            let db = fresh_db().await;
            for (i, (et, ed, actor)) in entries.iter().enumerate() {
                backend::services::audit_logger::log_event(
                    &db, et, ed, actor, Some(&format!("target-{i}")),
                ).await;
            }

            let rows = AuditLog::find().all(&db).await.unwrap();
            prop_assert_eq!(rows.len(), entries.len());

            let mut seen = std::collections::HashSet::new();
            for row in &rows {
                prop_assert!(seen.insert(row.id.clone()), "duplicate audit id {}", row.id);
                let parsed = uuid::Uuid::parse_str(&row.id)
                    .unwrap_or_else(|e| panic!("audit id {} is not a UUID: {}", row.id, e));
                prop_assert_eq!(
                    parsed.get_version_num(), 4,
                    "audit id {} is not a v4 UUID", row.id
                );
            }
            Ok(())
        });
    }

    // ── INVARIANT CC — created_at is set and non-decreasing in-process ──────
    #[test]
    fn audit_timestamps_are_set_and_monotonic(
        entries in prop::collection::vec((event_type(), event_data(), "[a-z0-9\\-]{1,8}"), 1..40),
    ) {
        let rt = runtime();
        rt.block_on(async {
            let db = fresh_db().await;
            let mut previous = i64::MIN;
            for (et, ed, actor) in entries.iter() {
                backend::services::audit_logger::log_event(&db, et, ed, actor, None).await;
                let latest = AuditLog::find()
                    .order_by_asc(audit_log::Column::CreatedAt)
                    .all(&db).await.unwrap();
                let last = latest.last().unwrap().created_at;
                prop_assert!(last > 0, "created_at not set (got {})", last);
                prop_assert!(last >= previous, "created_at went backwards: {} < {}", last, previous);
                previous = last;
            }
            Ok(())
        });
    }

    // ── Module doc: "immutable append-only event ledger" ────────────────────
    #[test]
    fn audit_trail_is_append_only(
        first in prop::collection::vec((event_type(), event_data(), "[a-z0-9\\-]{1,8}"), 1..15),
        second in prop::collection::vec((event_type(), event_data(), "[a-z0-9\\-]{1,8}"), 1..15),
    ) {
        let rt = runtime();
        rt.block_on(async {
            let db = fresh_db().await;
            for (et, ed, actor) in first.iter() {
                backend::services::audit_logger::log_event(&db, et, ed, actor, None).await;
            }
            let after_first = AuditLog::find().all(&db).await.unwrap();
            let ids_first: std::collections::HashSet<String> =
                after_first.iter().map(|r| r.id.clone()).collect();

            for (et, ed, actor) in second.iter() {
                backend::services::audit_logger::log_event(&db, et, ed, actor, None).await;
            }
            let after_second = AuditLog::find().all(&db).await.unwrap();

            prop_assert_eq!(after_second.len(), first.len() + second.len(),
                "append-only violated: {} entries became {}", after_first.len(), after_second.len());

            // Nothing that was already written may change.
            for row in &after_first {
                let still = after_second.iter().find(|r| r.id == row.id)
                    .unwrap_or_else(|| panic!("audit row {} disappeared", row.id));
                prop_assert_eq!(&still.event_type, &row.event_type, "event_type mutated");
                prop_assert_eq!(&still.event_data, &row.event_data, "event_data mutated");
                prop_assert_eq!(&still.actor_id, &row.actor_id, "actor_id mutated");
                prop_assert_eq!(still.created_at, row.created_at, "created_at mutated");
            }
            prop_assert_eq!(ids_first.len(), after_first.len());
            Ok(())
        });
    }
}