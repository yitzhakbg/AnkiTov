//! Migration — Create producers and producer_decks tables (P1: Producer
//! upload + verification, spec: retention-period-plan §Phase 1).
//!
//! Adds:
//!   - `producers` — producer identity (name, email, optional contact note).
//!   - `producer_decks` — producer-facing deck catalog row with verification
//!     status lifecycle `pending → verified | rejected` (plus
//!     `published`/`delisted` for later phases).
//!
//! NoOpMigrator pattern: hardcoded reversible Rust migration registered in
//! `backend/src/migrations/mod.rs`. All operations are reversible.

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Producers {
    Table,
    Id,
    Name,
    Email,
    ContactNote,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum ProducerDecks {
    Table,
    Id,
    ProducerId,
    DeckId,
    Status,
    RejectReason,
    VerifiedAt,
    CreatedAt,
    UpdatedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── Create producers table ──
        manager
            .create_table(
                Table::create()
                    .table(Producers::Table)
                    .col(integer(Producers::Id).not_null().auto_increment().primary_key())
                    .col(string(Producers::Name).not_null())
                    .col(string(Producers::Email).not_null())
                    .col(string_null(Producers::ContactNote))
                    .col(big_integer(Producers::CreatedAt).not_null())
                    .col(big_integer(Producers::UpdatedAt).not_null())
                    .to_owned(),
            )
            .await?;

        // ── Create producer_decks catalog table ──
        // Status is a string column matching house style
        // (capsule_session.status precedent): 'pending' | 'verified' |
        // 'rejected' | 'published' | 'delisted'.
        manager
            .create_table(
                Table::create()
                    .table(ProducerDecks::Table)
                    .col(integer(ProducerDecks::Id).not_null().auto_increment().primary_key())
                    .col(integer(ProducerDecks::ProducerId).not_null())
                    .col(integer(ProducerDecks::DeckId).not_null())
                    .col(string(ProducerDecks::Status).not_null().default("pending"))
                    .col(string_null(ProducerDecks::RejectReason))
                    .col(big_integer_null(ProducerDecks::VerifiedAt))
                    .col(big_integer(ProducerDecks::CreatedAt).not_null())
                    .col(big_integer(ProducerDecks::UpdatedAt).not_null())
                    .to_owned(),
            )
            .await?;

        // Index for catalog lookups by producer.
        manager
            .create_index(
                Index::create()
                    .name("idx_producer_decks_producer_id")
                    .table(ProducerDecks::Table)
                    .col(ProducerDecks::ProducerId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ProducerDecks::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Producers::Table).to_owned())
            .await?;
        Ok(())
    }
}
