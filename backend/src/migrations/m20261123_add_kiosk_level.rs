//! Migration — Add kiosk_level column to users table.
//!
//! Per-student seat kiosk level (owner decision 2026-11-23): the backend
//! maps student → kiosk level when SeatManager spawns the student's Selkies
//! seat, instead of the previous backend-wide env default.
//!
//! Levels (see `addons/ankitov_session_driver/kiosk.py`):
//!   0 = practice-only kiosk (deck auto-chosen, escapes bounced) — default
//!   1 = guided (deck opened, UI free)
//!   2 = full Anki
//!
//! Existing users get 0 (the locked-down kiosk), which matches the behavior
//! every deployed seat has shown since the kiosk shipped.

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Users {
    Table,
    KioskLevel,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .add_column(integer(Users::KioskLevel).default(0))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .drop_column(Users::KioskLevel)
                    .to_owned(),
            )
            .await
    }
}
