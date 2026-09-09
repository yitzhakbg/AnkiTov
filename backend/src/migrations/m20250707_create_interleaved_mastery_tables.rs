//! Migration — creates Interleaved Mastery Pipeline tables.
//!
//! Tables:
//!   - tracks             — Track Library (tag-based sub-collections)
//!   - track_profiles     — Named profiles grouping tracks for assignment
//!   - track_profile_tracks — Many-to-many junction (profiles ↔ tracks)
//!   - capsule_sessions   — Per-student remediation capsule snapshots

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // -----------------------------------------------------------------------
        // TRACKS — Central Track Library
        // -----------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(Tracks::Table)
                    .col(ColumnDef::new(Tracks::Id)
                        .string()
                        .not_null()
                        .primary_key())
                    .col(ColumnDef::new(Tracks::Name)
                        .string()
                        .not_null())
                    .col(text_null(Tracks::Description))
                    .col(string_null(Tracks::SubjectArea))
                    .col(ColumnDef::new(Tracks::Tag)
                        .string()
                        .not_null())
                    .col(ColumnDef::new(Tracks::TargetRetention)
                        .double()
                        .not_null()
                        .default(0.80))
                    .col(ColumnDef::new(Tracks::NValue)
                        .integer()
                        .not_null()
                        .default(3))
                    .col(big_integer(Tracks::CreatedAt).not_null())
                    .col(big_integer(Tracks::UpdatedAt).not_null())
                    .to_owned(),
            )
            .await?;

        // -----------------------------------------------------------------------
        // TRACK PROFILES — Named assignment profiles
        // -----------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(TrackProfiles::Table)
                    .col(ColumnDef::new(TrackProfiles::Id)
                        .string()
                        .not_null()
                        .primary_key())
                    .col(ColumnDef::new(TrackProfiles::Name)
                        .string()
                        .not_null()
                        .unique_key())
                    .col(text_null(TrackProfiles::Description))
                    .col(double_null(TrackProfiles::TargetRetention))
                    .col(integer_null(TrackProfiles::NValue))
                    .col(big_integer(TrackProfiles::CreatedAt).not_null())
                    .col(big_integer(TrackProfiles::UpdatedAt).not_null())
                    .to_owned(),
            )
            .await?;

        // -----------------------------------------------------------------------
        // TRACK PROFILE TRACKS — Junction table (many-to-many)
        // -----------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(TrackProfileTracks::Table)
                    .col(pk_auto(TrackProfileTracks::Id))
                    .col(ColumnDef::new(TrackProfileTracks::TrackProfileId)
                        .string()
                        .not_null())
                    .col(ColumnDef::new(TrackProfileTracks::TrackId)
                        .string()
                        .not_null())
                    .col(ColumnDef::new(TrackProfileTracks::SortOrder)
                        .integer()
                        .not_null()
                        .default(0))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tpt_track_profile")
                            .from(TrackProfileTracks::Table, TrackProfileTracks::TrackProfileId)
                            .to(TrackProfiles::Table, TrackProfiles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tpt_track")
                            .from(TrackProfileTracks::Table, TrackProfileTracks::TrackId)
                            .to(Tracks::Table, Tracks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // -----------------------------------------------------------------------
        // CAPSULE SESSIONS — Per-student remediation slice
        // -----------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(CapsuleSessions::Table)
                    .col(ColumnDef::new(CapsuleSessions::Id)
                        .string()
                        .not_null()
                        .primary_key())
                    .col(ColumnDef::new(CapsuleSessions::StudentId)
                        .string()
                        .not_null())
                    .col(ColumnDef::new(CapsuleSessions::TrackProfileId)
                        .string()
                        .not_null())
                    .col(ColumnDef::new(CapsuleSessions::NValue)
                        .integer()
                        .not_null())
                    .col(ColumnDef::new(CapsuleSessions::CardIds)
                        .text()
                        .not_null())
                    .col(ColumnDef::new(CapsuleSessions::Status)
                        .string()
                        .not_null()
                        .default("pending"))
                    .col(ColumnDef::new(CapsuleSessions::SessionWeek)
                        .string()
                        .not_null())
                    .col(ColumnDef::new(CapsuleSessions::CardsCompleted)
                        .integer()
                        .not_null()
                        .default(0))
                    .col(ColumnDef::new(CapsuleSessions::CardsTotal)
                        .integer()
                        .not_null())
                    .col(big_integer(CapsuleSessions::CreatedAt).not_null())
                    .col(big_integer_null(CapsuleSessions::CompletedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_cs_track_profile")
                            .from(CapsuleSessions::Table, CapsuleSessions::TrackProfileId)
                            .to(TrackProfiles::Table, TrackProfiles::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CapsuleSessions::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(TrackProfileTracks::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(TrackProfiles::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Tracks::Table).to_owned())
            .await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Iden enums
// ---------------------------------------------------------------------------

#[derive(Iden)]
enum Tracks {
    Table,
    Id,
    Name,
    Description,
    SubjectArea,
    Tag,
    TargetRetention,
    NValue,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum TrackProfiles {
    Table,
    Id,
    Name,
    Description,
    TargetRetention,
    NValue,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum TrackProfileTracks {
    Table,
    Id,
    TrackProfileId,
    TrackId,
    SortOrder,
}

#[derive(Iden)]
enum CapsuleSessions {
    Table,
    Id,
    StudentId,
    TrackProfileId,
    NValue,
    CardIds,
    Status,
    SessionWeek,
    CardsCompleted,
    CardsTotal,
    CreatedAt,
    CompletedAt,
}