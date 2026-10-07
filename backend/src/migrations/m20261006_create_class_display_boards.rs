//! Migration — Leaderboard chunk 2 (§8.8 of specs/2026-10-06-leaderboard.md).
//!
//! Creates the `class_display_boards` table: one row per class with a
//! projector-board display token (spec §8.4). Token is the credential for
//! the public `/api/v1/display/class/<token>` route — no JWT required.
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
        // class_display_boards — PK is class_id (FK → classes.id).
        // -----------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(ClassDisplayBoards::Table)
                    .col(string(ClassDisplayBoards::ClassId).not_null().primary_key())
                    .col(string(ClassDisplayBoards::Token).not_null())
                    .col(boolean(ClassDisplayBoards::Enabled).not_null().default(false))
                    .col(string_null(ClassDisplayBoards::ConsentBy))
                    .col(big_integer(ClassDisplayBoards::ConsentAt).not_null().default(0))
                    .col(big_integer(ClassDisplayBoards::CreatedAt).not_null())
                    .to_owned(),
            )
            .await?;

        // -----------------------------------------------------------------------
        // Unique index on token — the display route resolves a board by token,
        // and rotations must not leave two live rows with the same token.
        // -----------------------------------------------------------------------
        manager
            .create_index(
                Index::create()
                    .unique()
                    .name("idx-class-display-boards-token")
                    .table(ClassDisplayBoards::Table)
                    .col(ClassDisplayBoards::Token)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx-class-display-boards-token")
                    .table(ClassDisplayBoards::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(ClassDisplayBoards::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum ClassDisplayBoards {
    Table,
    ClassId,
    Token,
    Enabled,
    ConsentBy,
    ConsentAt,
    CreatedAt,
}
