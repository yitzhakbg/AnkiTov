//! Migration — Solo Web Reviewer (T-PWA, spec 2026-10-25 §2.1): the backend-side
//! review ledger.
//!
//! `web_review_log` is the revlog for answers given in the browser reviewer.
//! The backend never writes a student's `.anki2`, so web answers need their
//! own append-only ledger; `fsrs_memory_states` derivation gains a replay step
//! over this table AFTER the snapshot revlog (§2.2 — replay lands in a later
//! slice; the ledger is recorded from day one so no history is lost).
//!
//! One row per answer, keyed by a client-generated UUID for idempotent retries
//! (flaky mobile networks retry POSTs; the ledger must not double-count).
//!
//! Purely additive — rollback cost is zero.

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(Iden)]
pub enum WebReviewLog {
    Table,
    Id,
    StudentId,
    CardId,
    SessionId,
    Ease,
    TimeMs,
    ReviewedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(WebReviewLog::Table)
                    .col(string_len(WebReviewLog::Id, 36).not_null().primary_key())
                    .col(string(WebReviewLog::StudentId).not_null())
                    .col(big_integer(WebReviewLog::CardId).not_null())
                    .col(string(WebReviewLog::SessionId).null())
                    .col(integer(WebReviewLog::Ease).not_null())
                    .col(big_integer(WebReviewLog::TimeMs).not_null())
                    .col(big_integer(WebReviewLog::ReviewedAt).not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_web_review_log_student_card")
                    .table(WebReviewLog::Table)
                    .col(WebReviewLog::StudentId)
                    .col(WebReviewLog::CardId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(WebReviewLog::Table).to_owned())
            .await
    }
}
