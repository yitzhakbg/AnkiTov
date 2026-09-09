//! Migration — Add class_code column to classes table.
//!
//! Class codes are 6-character alphanumeric tokens that students use
//! to self-enroll. Generated on class creation, unique per class.

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Classes {
    Table,
    ClassCode,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Classes::Table)
                    .add_column(string_null(Classes::ClassCode))
                    .to_owned(),
            )
            .await?;

        // Add unique index on class_code for fast lookup
        manager
            .create_index(
                Index::create()
                    .unique()
                    .name("idx-classes-class-code")
                    .table(Classes::Table)
                    .col(Classes::ClassCode)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx-classes-class-code")
                    .table(Classes::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Classes::Table)
                    .drop_column(Classes::ClassCode)
                    .to_owned(),
            )
            .await
    }
}