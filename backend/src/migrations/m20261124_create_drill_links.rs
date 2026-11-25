//! Migration — T-215 First-60-Seconds drill links.
//!
//! Creates the `drill_links` table: one row per shareable Anki-less drill
//! link (`GET /api/v1/drill/<id>` → seat start with the link's pack/deck →
//! 302 to the seat stream). Purely additive — it does not touch any existing
//! table, so rollback cost is zero.
//!
//! Design record: `specs/design/2026-11-23-first-60-seconds-drill-link.md`.

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum DrillLinks {
    Table,
    Id,
    Pack,
    Deck,
    Title,
    CreatedBy,
    MaxUses,
    UseCount,
    ExpiresAt,
    CreatedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DrillLinks::Table)
                    // The random token IS the link path segment (192-bit class).
                    .col(string(DrillLinks::Id).not_null().primary_key())
                    .col(string(DrillLinks::Pack).not_null()) // .apkg name in the seed library
                    .col(string(DrillLinks::Deck).not_null()) // ANKITOV_SEAT_DECK value
                    .col(string(DrillLinks::Title).not_null()) // landing/busy-page title
                    .col(string(DrillLinks::CreatedBy).not_null()) // user id (teacher/admin)
                    .col(integer_null(DrillLinks::MaxUses)) // null = unlimited
                    .col(integer(DrillLinks::UseCount).not_null().default(0))
                    .col(big_integer_null(DrillLinks::ExpiresAt)) // unix secs; null = no expiry
                    .col(big_integer(DrillLinks::CreatedAt).not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(DrillLinks::Table).to_owned())
            .await
    }
}
