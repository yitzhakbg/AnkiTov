//! Migration — P2 minimal deck-track binding.
//!
//! Adds a nullable `track_id` to `decks` so decks can be bound to a Track
//! Library track (tracks.id is a String UUID). NULL means the deck is
//! unassigned. No hard DB-level FK: the bind route validates track existence
//! at the application layer, keeping SQLite ALTER TABLE simple.

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Decks {
    Table,
    TrackId,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Decks::Table)
                    .add_column(string_null(Decks::TrackId))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Decks::Table)
                    .drop_column(Decks::TrackId)
                    .to_owned(),
            )
            .await
    }
}
