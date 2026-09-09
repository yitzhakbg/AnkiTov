//! Migration — Step 1 & 2 schema additions.
//!
//! Adds:
//!   - `profile_assignments` table implementing our Hybrid Assignment Model.
//!   - `generation_jobs` table for storing sequential batch background automation.
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
        // Create profile_assignments table
        // -----------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(ProfileAssignments::Table)
                    .col(pk_auto(ProfileAssignments::Id))
                    .col(string(ProfileAssignments::TargetType).not_null()) // "student" or "cohort"
                    .col(string(ProfileAssignments::TargetId).not_null())   // user.id or subsection_id
                    .col(string(ProfileAssignments::TrackProfileId).not_null())
                    .col(integer(ProfileAssignments::Priority).not_null().default(1))
                    .col(string(ProfileAssignments::AssignedBy).not_null())
                    .col(big_integer(ProfileAssignments::AssignedAt).not_null())
                    // Ensure target_type + target_id is unique so we don't have multiple assignments for the same target
                    .index(
                        Index::create()
                            .unique()
                            .name("idx-profile-assignment-target")
                            .table(ProfileAssignments::Table)
                            .col(ProfileAssignments::TargetType)
                            .col(ProfileAssignments::TargetId),
                    )
                    .to_owned(),
            )
            .await?;

        // -----------------------------------------------------------------------
        // Create generation_jobs table
        // -----------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(GenerationJobs::Table)
                    .col(string(GenerationJobs::Id).not_null().primary_key())
                    .col(string(GenerationJobs::Status).not_null())         // "pending", "processing", "completed", "failed"
                    .col(integer(GenerationJobs::Progress).not_null().default(0))
                    .col(integer(GenerationJobs::TotalStudents).not_null().default(0))
                    .col(integer(GenerationJobs::CompletedStudents).not_null().default(0))
                    .col(text_null(GenerationJobs::Anomalies))             // JSON structured log of anomalies
                    .col(big_integer_null(GenerationJobs::CompletedAt))
                    .col(big_integer(GenerationJobs::CreatedAt).not_null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ProfileAssignments::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(GenerationJobs::Table).to_owned())
            .await?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Idens — column names for migrations
// ---------------------------------------------------------------------------

#[derive(Iden)]
enum ProfileAssignments {
    Table,
    Id,
    TargetType,
    TargetId,
    TrackProfileId,
    Priority,
    AssignedBy,
    AssignedAt,
}

#[derive(Iden)]
enum GenerationJobs {
    Table,
    Id,
    Status,
    Progress,
    TotalStudents,
    CompletedStudents,
    Anomalies,
    CompletedAt,
    CreatedAt,
}
