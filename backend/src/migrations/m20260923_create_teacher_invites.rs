//! Migration — Prong 8 (Teacher Registration).
//!
//! Creates the `teacher_invites` table: one row per pending teacher invite.
//! Created on admin enrollment, consumed on first-login. Purely additive —
//! it does not touch `users` or any existing table, so rollback cost is zero.
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
        // Create teacher_invites table
        // -----------------------------------------------------------------------
        manager
            .create_table(
                Table::create()
                    .table(TeacherInvites::Table)
                    .col(string(TeacherInvites::Id).not_null().primary_key())
                    .col(big_integer(TeacherInvites::UserId).not_null())          // FK → users.id
                    .col(string(TeacherInvites::School).not_null())
                    .col(string_null(TeacherInvites::Community))
                    .col(string_null(TeacherInvites::Role))
                    .col(string(TeacherInvites::Locale).not_null())
                    .col(string_null(TeacherInvites::Email))
                    .col(string(TeacherInvites::InviteToken).not_null())
                    .col(big_integer(TeacherInvites::CreatedAt).not_null())
                    .col(big_integer(TeacherInvites::ExpiresAt).not_null())
                    .col(big_integer_null(TeacherInvites::UsedAt))
                    .col(big_integer_null(TeacherInvites::RevokedAt))
                    .to_owned(),
            )
            .await?;

        // -----------------------------------------------------------------------
        // Indexes
        // -----------------------------------------------------------------------
        // Unique index on invite_token for fast single-use lookup.
        manager
            .create_index(
                Index::create()
                    .unique()
                    .name("idx-teacher-invites-token")
                    .table(TeacherInvites::Table)
                    .col(TeacherInvites::InviteToken)
                    .to_owned(),
            )
            .await?;

        // Regular index on user_id (one pending invite per teacher).
        manager
            .create_index(
                Index::create()
                    .name("idx-teacher-invites-user")
                    .table(TeacherInvites::Table)
                    .col(TeacherInvites::UserId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx-teacher-invites-token")
                    .table(TeacherInvites::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx-teacher-invites-user")
                    .table(TeacherInvites::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(TeacherInvites::Table).to_owned())
            .await
    }
}

// ---------------------------------------------------------------------------
// Idens — column names for migrations
// ---------------------------------------------------------------------------

#[derive(Iden)]
enum TeacherInvites {
    Table,
    Id,
    UserId,
    School,
    Community,
    Role,
    Locale,
    Email,
    InviteToken,
    CreatedAt,
    ExpiresAt,
    UsedAt,
    RevokedAt,
}
