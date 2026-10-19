//! Migration — T-126 dashboard UX: app settings key-value store.
//!
//! Creates the `app_settings` table: a tiny key/value store for school-level
//! configuration that must survive restarts and be editable from the new
//! Settings view (school name, default locale, Anki sync-server URL).
//!
//! A KV table (rather than columns on an existing entity) keeps future
//! settings additive: a new key needs no migration, only a service constant.
//! Values are plain strings; interpretation belongs to the reader
//! (`services::settings`), so the table never needs a schema change when a
//! setting's type evolves.
//!
//! Purely additive — rollback cost is zero.

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AppSettings::Table)
                    .col(string(AppSettings::Key).not_null().primary_key())
                    .col(string(AppSettings::Value).not_null())
                    .col(big_integer(AppSettings::UpdatedAt).not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AppSettings::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum AppSettings {
    Table,
    Key,
    Value,
    UpdatedAt,
}
