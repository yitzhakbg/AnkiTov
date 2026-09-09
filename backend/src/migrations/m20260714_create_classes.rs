//! Migration — Create classes and class_enrollments tables.
//!
//! Adds:
//!   - `classes` table for organizing students into instructional groups.
//!   - `class_enrollments` junction table with soft-delete via `active` flag.
//!   - `class_id` FK columns on profile_assignments, deck_distributions,
//!     and addon_distributions for class-level targeting.
//!
//! All operations are reversible.

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Classes {
    Table,
    Id,
    Name,
    SubjectArea,
    TeacherId,
    Period,
    AcademicYear,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum ClassEnrollments {
    Table,
    Id,
    ClassId,
    StudentId,
    EnrolledAt,
    Active,
}

#[derive(DeriveIden)]
enum ProfileAssignments {
    Table,
    ClassId,
}

#[derive(DeriveIden)]
enum DeckDistributions {
    Table,
    ClassId,
}

#[derive(DeriveIden)]
enum AddonDistributions {
    Table,
    ClassId,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── Create classes table ──
        manager
            .create_table(
                Table::create()
                    .table(Classes::Table)
                    .col(string(Classes::Id).not_null().primary_key())
                    .col(string(Classes::Name).not_null())
                    .col(string_null(Classes::SubjectArea))
                    .col(string(Classes::TeacherId).not_null())
                    .col(string_null(Classes::Period))
                    .col(string_null(Classes::AcademicYear))
                    .col(big_integer(Classes::CreatedAt).not_null())
                    .col(big_integer(Classes::UpdatedAt).not_null())
                    .to_owned(),
            )
            .await?;

        // ── Create class_enrollments junction table ──
        manager
            .create_table(
                Table::create()
                    .table(ClassEnrollments::Table)
                    .col(pk_auto(ClassEnrollments::Id))
                    .col(string(ClassEnrollments::ClassId).not_null())
                    .col(string(ClassEnrollments::StudentId).not_null())
                    .col(big_integer(ClassEnrollments::EnrolledAt).not_null())
                    .col(
                        boolean(ClassEnrollments::Active)
                            .not_null()
                            .default(true),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-class-enrollments-class")
                            .from(ClassEnrollments::Table, ClassEnrollments::ClassId)
                            .to(Classes::Table, Classes::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .index(
                        Index::create()
                            .unique()
                            .name("idx-class-enrollment-active")
                            .table(ClassEnrollments::Table)
                            .col(ClassEnrollments::ClassId)
                            .col(ClassEnrollments::StudentId)
                            .col(ClassEnrollments::Active),
                    )
                    .to_owned(),
            )
            .await?;

        // ── Add class_id to profile_assignments ──
        manager
            .alter_table(
                Table::alter()
                    .table(ProfileAssignments::Table)
                    .add_column(string_null(ProfileAssignments::ClassId))
                    .to_owned(),
            )
            .await?;

        // ── Add class_id to deck_distributions ──
        manager
            .alter_table(
                Table::alter()
                    .table(DeckDistributions::Table)
                    .add_column(string_null(DeckDistributions::ClassId))
                    .to_owned(),
            )
            .await?;

        // ── Add class_id to addon_distributions ──
        manager
            .alter_table(
                Table::alter()
                    .table(AddonDistributions::Table)
                    .add_column(string_null(AddonDistributions::ClassId))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── Remove class_id columns ──
        manager
            .alter_table(
                Table::alter()
                    .table(AddonDistributions::Table)
                    .drop_column(AddonDistributions::ClassId)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(DeckDistributions::Table)
                    .drop_column(DeckDistributions::ClassId)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(ProfileAssignments::Table)
                    .drop_column(ProfileAssignments::ClassId)
                    .to_owned(),
            )
            .await?;

        // ── Drop tables ──
        manager
            .drop_table(Table::drop().table(ClassEnrollments::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(Classes::Table).to_owned())
            .await?;

        Ok(())
    }
}
