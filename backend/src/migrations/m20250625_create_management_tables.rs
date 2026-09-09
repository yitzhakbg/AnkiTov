//! Initial migration — creates all Management Console tables.
//!
//! All timestamps are stored as BIGINT (Unix seconds) for maximum SQLite portability.
//! The application layer converts to/from DateTime for API serialization.

#![allow(clippy::redundant_else)]

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ---------------------------------------------------------------------------
        // USERS
        // ---------------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .col(pk_auto(Users::Id))
                    .col(string_uniq(Users::Username))
                    .col(string_uniq(Users::Email))
                    .col(string_null(Users::FullName))
                    .col(
                        ColumnDef::new(Users::Role)
                            .string()
                            .not_null()
                            .default("student"),
                    )
                    .col(uuid_null(Users::SubsectionId))
                    .col(big_integer(Users::CreatedAt).not_null())
                    .col(big_integer(Users::UpdatedAt).not_null())
                    .to_owned(),
            )
            .await?;

        // ---------------------------------------------------------------------------
        // DECKS
        // ---------------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(Decks::Table)
                    .col(pk_auto(Decks::Id))
                    .col(ColumnDef::new(Decks::Name).string().not_null())
                    .col(text_null(Decks::Description))
                    .col(ColumnDef::new(Decks::CardCount).integer().not_null().default(0))
                    .col(ColumnDef::new(Decks::FileSizeBytes).big_integer().not_null().default(0))
                    .col(ColumnDef::new(Decks::ChecksumSha256).string().not_null())
                    .col(string(Decks::UploadedBy))
                    .col(big_integer(Decks::CreatedAt).not_null())
                    .col(big_integer(Decks::UpdatedAt).not_null())
                    .to_owned(),
            )
            .await?;

        // ---------------------------------------------------------------------------
        // DECK DISTRIBUTIONS
        // ---------------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(DeckDistributions::Table)
                    .col(pk_auto(DeckDistributions::Id))
                    .col(integer(DeckDistributions::DeckId))
                    .col(ColumnDef::new(DeckDistributions::TargetType).string().not_null())
                    .col(string(DeckDistributions::TargetId))
                    .col(string(DeckDistributions::DistributedBy))
                    .col(big_integer(DeckDistributions::DistributedAt).not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(DeckDistributions::Table, DeckDistributions::DeckId)
                            .to(Decks::Table, Decks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // ---------------------------------------------------------------------------
        // ADDONS
        // ---------------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(Addons::Table)
                    .col(ColumnDef::new(Addons::Id).string().primary_key().not_null())
                    .col(ColumnDef::new(Addons::Name).string().not_null())
                    .col(ColumnDef::new(Addons::Version).string().not_null())
                    .col(text_null(Addons::Description))
                    .col(ColumnDef::new(Addons::FileSizeBytes).big_integer().not_null().default(0))
                    .col(ColumnDef::new(Addons::ChecksumSha256).string().not_null())
                    .col(text_null(Addons::ConfigJson))
                    .col(string(Addons::InstalledBy))
                    .col(big_integer(Addons::CreatedAt).not_null())
                    .col(big_integer(Addons::UpdatedAt).not_null())
                    .to_owned(),
            )
            .await?;

        // ---------------------------------------------------------------------------
        // ADDON DISTRIBUTIONS
        // ---------------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(AddonDistributions::Table)
                    .col(ColumnDef::new(AddonDistributions::Id).string().primary_key().not_null())
                    .col(string(AddonDistributions::AddonId))
                    .col(ColumnDef::new(AddonDistributions::TargetType).string().not_null())
                    .col(string(AddonDistributions::TargetId))
                    .col(string(AddonDistributions::DistributedBy))
                    .col(big_integer(AddonDistributions::DistributedAt).not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(AddonDistributions::Table, AddonDistributions::AddonId)
                            .to(Addons::Table, Addons::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // ---------------------------------------------------------------------------
        // SYNC STATUSES
        // ---------------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(SyncStatuses::Table)
                    .col(ColumnDef::new(SyncStatuses::Id).string().primary_key().not_null())
                    .col(string(SyncStatuses::UserId))
                    .col(
                        ColumnDef::new(SyncStatuses::Status)
                            .string()
                            .not_null()
                            .default("idle"),
                    )
                    .col(big_integer_null(SyncStatuses::LastSync))
                    .col(ColumnDef::new(SyncStatuses::PendingChanges).integer().not_null().default(0))
                    .col(text_null(SyncStatuses::ErrorMessage))
                    .to_owned(),
            )
            .await?;

        // ---------------------------------------------------------------------------
        // RETENTION LOGS
        // ---------------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(RetentionLogs::Table)
                    .col(ColumnDef::new(RetentionLogs::Id).string().primary_key().not_null())
                    .col(string(RetentionLogs::UserId))
                    .col(string(RetentionLogs::DeckId))
                    .col(ColumnDef::new(RetentionLogs::CardId).string().not_null())
                    .col(ColumnDef::new(RetentionLogs::Grade).integer().not_null())
                    .col(ColumnDef::new(RetentionLogs::IntervalSeconds).big_integer().not_null())
                    .col(double(RetentionLogs::EaseFactor))
                    .col(big_integer(RetentionLogs::ReviewTimestamp).not_null())
                    .to_owned(),
            )
            .await?;

        // ---------------------------------------------------------------------------
        // RETENTION EXCEPTIONS
        // ---------------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(RetentionExceptions::Table)
                    .col(ColumnDef::new(RetentionExceptions::Id).string().primary_key().not_null())
                    .col(string(RetentionExceptions::UserId))
                    .col(string(RetentionExceptions::DeckId))
                    .col(ColumnDef::new(RetentionExceptions::ExceptionType).string().not_null())
                    .col(double(RetentionExceptions::RetentionRate))
                    .col(double(RetentionExceptions::Threshold))
                    .col(big_integer(RetentionExceptions::DetectedAt).not_null())
                    .col(big_integer_null(RetentionExceptions::ResolvedAt))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(RetentionExceptions::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(RetentionLogs::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(SyncStatuses::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(AddonDistributions::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Addons::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(DeckDistributions::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Decks::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Users::Table).to_owned())
            .await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Table identifiers
// ---------------------------------------------------------------------------

#[derive(Iden)]
enum Users {
    Table,
    Id,
    Username,
    Email,
    FullName,
    Role,
    SubsectionId,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum Decks {
    Table,
    Id,
    Name,
    Description,
    CardCount,
    FileSizeBytes,
    ChecksumSha256,
    UploadedBy,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum DeckDistributions {
    Table,
    Id,
    DeckId,
    TargetType,
    TargetId,
    DistributedBy,
    DistributedAt,
}

#[derive(Iden)]
enum Addons {
    Table,
    Id,
    Name,
    Version,
    Description,
    FileSizeBytes,
    ChecksumSha256,
    ConfigJson,
    InstalledBy,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum AddonDistributions {
    Table,
    Id,
    AddonId,
    TargetType,
    TargetId,
    DistributedBy,
    DistributedAt,
}

#[derive(Iden)]
enum SyncStatuses {
    Table,
    Id,
    UserId,
    Status,
    LastSync,
    PendingChanges,
    ErrorMessage,
}

#[derive(Iden)]
enum RetentionLogs {
    Table,
    Id,
    UserId,
    DeckId,
    CardId,
    Grade,
    IntervalSeconds,
    EaseFactor,
    ReviewTimestamp,
}

#[derive(Iden)]
enum RetentionExceptions {
    Table,
    Id,
    UserId,
    DeckId,
    ExceptionType,
    RetentionRate,
    Threshold,
    DetectedAt,
    ResolvedAt,
}
