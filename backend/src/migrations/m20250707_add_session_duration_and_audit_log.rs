//! Migration — Phase 4 schema additions (IMP-10 through IMP-14).
//!
//! Adds:
//!   - session_duration_minutes to track_profiles
//!   - session_duration_minutes to capsule_sessions
//!   - audit_logs table (immutable event ledger)
//!
//! All operations are reversible.

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // -----------------------------------------------------------------------
        // Add session_duration_minutes to track_profiles
        // -----------------------------------------------------------------------
        manager
            .alter_table(
                Table::alter()
                    .table(TrackProfiles::Table)
                    .add_column(
                        ColumnDef::new(TrackProfiles::SessionDurationMinutes)
                            .integer()
                            .not_null()
                            .default(10),
                    )
                    .to_owned(),
            )
            .await?;

        // -----------------------------------------------------------------------
        // Add session_duration_minutes to capsule_sessions
        // -----------------------------------------------------------------------
        manager
            .alter_table(
                Table::alter()
                    .table(CapsuleSessions::Table)
                    .add_column(
                        ColumnDef::new(CapsuleSessions::SessionDurationMinutes)
                            .integer()
                            .not_null()
                            .default(10),
                    )
                    .to_owned(),
            )
            .await?;

        // -----------------------------------------------------------------------
        // Create audit_logs table
        // -----------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(AuditLogs::Table)
                    .col(ColumnDef::new(AuditLogs::Id)
                        .string()
                        .not_null()
                        .primary_key())
                    .col(ColumnDef::new(AuditLogs::EventType)
                        .string()
                        .not_null())
                    .col(ColumnDef::new(AuditLogs::EventData)
                        .text()
                        .not_null())
                    .col(ColumnDef::new(AuditLogs::ActorId)
                        .string()
                        .not_null())
                    .col(string_null(AuditLogs::TargetId))
                    .col(big_integer(AuditLogs::CreatedAt).not_null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(TrackProfiles::Table)
                    .drop_column(TrackProfiles::SessionDurationMinutes)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(CapsuleSessions::Table)
                    .drop_column(CapsuleSessions::SessionDurationMinutes)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(AuditLogs::Table).to_owned())
            .await?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Iden — column names for ALTER TABLE operations
// ---------------------------------------------------------------------------

#[derive(Iden)]
enum TrackProfiles {
    Table,
    SessionDurationMinutes,
}

#[derive(Iden)]
enum CapsuleSessions {
    Table,
    SessionDurationMinutes,
}

#[derive(Iden)]
enum AuditLogs {
    Table,
    Id,
    EventType,
    EventData,
    ActorId,
    TargetId,
    CreatedAt,
}