//! Migration — Add display_name to class_enrollments for human-readable student names.
//!
//! The existing student_id field stores machine-friendly slugs (e.g., "fatima-al-rashid").
//! display_name stores the human-readable form (e.g., "Fatima Al Rashid") that
//! teachers see in the UI. Falls back to student_id if not set.
//!
//! Also adds metadata_json for extensible student metadata (email, SIS ID, etc.).

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum ClassEnrollments {
    Table,
    DisplayName,
    MetadataJson,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ClassEnrollments::Table)
                    .add_column(string_null(ClassEnrollments::DisplayName))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(ClassEnrollments::Table)
                    .add_column(string_null(ClassEnrollments::MetadataJson))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ClassEnrollments::Table)
                    .drop_column(ClassEnrollments::MetadataJson)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(ClassEnrollments::Table)
                    .drop_column(ClassEnrollments::DisplayName)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}
