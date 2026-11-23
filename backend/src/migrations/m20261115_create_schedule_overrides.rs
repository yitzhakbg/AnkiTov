//! Migration — B-0035 sporadic-continuity scheduler: `schedule_overrides`.
//!
//! One row per scheduling override for a `(student, card)` pair. The backlog
//! triage deferral overlay writes one active row per deferred card carrying
//! `deferred_until` — the epoch-ms instant when the card re-enters the normal
//! due flow. Deferral is a *scheduling decision*, not a memory-state change:
//! the `.anki2` collection is never written and FSRS states stay untouched.
//!
//! The unique **partial** index on `(student_id, card_id) WHERE cleared_at IS
//! NULL` guarantees at most one active override per pair. sea-query's index
//! builder cannot express a partial-index `WHERE` clause, so that index is
//! created via raw SQL (SQLite-compatible; the backend runs on SQLite).
//! Answering a card sets `cleared_at`, retiring the override while keeping
//! the row as an audit trail.
//!
//! Purely additive — no existing table is touched, so rollback cost is zero.

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ---------------------------------------------------------------------
        // Create schedule_overrides table
        // ---------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(ScheduleOverrides::Table)
                    .col(string(ScheduleOverrides::Id).not_null().primary_key())
                    .col(string(ScheduleOverrides::StudentId).not_null()) // FK → users.id
                    .col(big_integer(ScheduleOverrides::CardId).not_null()) // Anki card id
                    .col(big_integer(ScheduleOverrides::DeferredUntil).not_null()) // epoch ms
                    .col(string_null(ScheduleOverrides::Reason))
                    .col(integer(ScheduleOverrides::KSessions).not_null())
                    .col(big_integer(ScheduleOverrides::CreatedAt).not_null()) // epoch ms
                    .col(big_integer_null(ScheduleOverrides::ClearedAt)) // epoch ms
                    .to_owned(),
            )
            .await?;

        // ---------------------------------------------------------------------
        // Unique partial index: at most one ACTIVE override per (student, card).
        // sea-query has no partial-index builder support → raw SQL.
        // ---------------------------------------------------------------------
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE UNIQUE INDEX idx_schedule_overrides_active \
                 ON schedule_overrides (student_id, card_id) \
                 WHERE cleared_at IS NULL",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_schedule_overrides_active")
                    .table(ScheduleOverrides::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(ScheduleOverrides::Table).to_owned())
            .await
    }
}

// ---------------------------------------------------------------------------
// Idens — column names for migrations
// ---------------------------------------------------------------------------

#[derive(Iden)]
enum ScheduleOverrides {
    Table,
    Id,
    StudentId,
    CardId,
    DeferredUntil,
    Reason,
    KSessions,
    CreatedAt,
    ClearedAt,
}
