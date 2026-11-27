//! Migration — Add `package_file` column to the decks table.
//!
//! T-234 (drill links bind uploaded packages): the upload flow persists
//! the `.apkg` under the upload dir as `<checksum8>-<sanitized stem>.apkg`,
//! but until now the row recorded only the checksum — the sanitized stem
//! of the original filename was lost, so the on-disk package could not be
//! recovered from the row alone. The new nullable column stores the
//! persisted BARE file name (no directory — never a host path).
//!
//! NULL for seed-library decks (resolvable by name against the seed dir)
//! and for rows created before this migration (resolved by the
//! checksum-prefix glob fallback in `management::decks::resolve_uploaded_package`).

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Decks {
    Table,
    PackageFile,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Decks::Table)
                    .add_column(string_null(Decks::PackageFile))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Decks::Table)
                    .drop_column(Decks::PackageFile)
                    .to_owned(),
            )
            .await
    }
}
