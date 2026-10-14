//! Migration — T-116 fsrs-rs production swap: MemoryState persistence.
//!
//! Creates the `fsrs_memory_states` table: one row per (student, card) holding
//! the FSRS-6 memory state (`stability`, `difficulty`) that the fsrs-rs
//! scheduler ranks by. The `.anki2` collection is opened read-only (immutable
//! mode), so derived states are persisted here instead of written back into
//! the student's collection.
//!
//! Rows are keyed by `(student_id, card_id)` — a unique composite index, not a
//! plain card index: teacher-distributed decks give every student's collection
//! the *same* Anki card IDs, so card IDs are only unique within one student's
//! collection.
//!
//! `revlog_head` records the newest `revlog.id` (ms epoch) observed when the
//! state was derived; a later snapshot whose revlog contains newer entries
//! triggers re-derivation (cache refresh). `source` records how the state was
//! derived (`revlog` replay, `sm2` approximation, or `first_transition`
//! defaults) for observability.
//!
//! Purely additive — it does not touch any existing table, so rollback cost
//! is zero. All operations are reversible.

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ---------------------------------------------------------------------
        // Create fsrs_memory_states table
        // ---------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(FsrsMemoryStates::Table)
                    .col(string(FsrsMemoryStates::Id).not_null().primary_key())
                    .col(string(FsrsMemoryStates::StudentId).not_null()) // FK → users.id
                    .col(big_integer(FsrsMemoryStates::CardId).not_null()) // Anki card id (ms epoch)
                    .col(double(FsrsMemoryStates::Stability).not_null())
                    .col(double(FsrsMemoryStates::Difficulty).not_null())
                    .col(big_integer_null(FsrsMemoryStates::LastReviewSecs))
                    .col(string(FsrsMemoryStates::Source).not_null())
                    .col(big_integer_null(FsrsMemoryStates::RevlogHead))
                    .col(big_integer(FsrsMemoryStates::UpdatedAt).not_null())
                    .to_owned(),
            )
            .await?;

        // ---------------------------------------------------------------------
        // Indexes
        // ---------------------------------------------------------------------
        // Unique per (student, card): the scheduler's cache-aside lookup key.
        // Also serves student-prefix lookups (leftmost column) in SQLite.
        manager
            .create_index(
                Index::create()
                    .unique()
                    .name("idx-fsrs-memory-states-student-card")
                    .table(FsrsMemoryStates::Table)
                    .col(FsrsMemoryStates::StudentId)
                    .col(FsrsMemoryStates::CardId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx-fsrs-memory-states-student-card")
                    .table(FsrsMemoryStates::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(FsrsMemoryStates::Table).to_owned())
            .await
    }
}

// ---------------------------------------------------------------------------
// Idens — column names for migrations
// ---------------------------------------------------------------------------

#[derive(Iden)]
enum FsrsMemoryStates {
    Table,
    Id,
    StudentId,
    CardId,
    Stability,
    Difficulty,
    LastReviewSecs,
    Source,
    RevlogHead,
    UpdatedAt,
}
