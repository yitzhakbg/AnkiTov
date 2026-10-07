//! ClassDisplayBoard entity — per-class leaderboard display configuration.
//!
//! One row per class, created on demand when a teacher enables (or rotates)
//! the projector board for that class. Carries:
//!
//! - `token`   — the unguessable, rotatable display token (spec §8.4).
//! - `enabled` — whether the board is currently live (disabled → routes 404).
//! - `consent_by` / `consent_at` — the teacher who consented, and when.
//!
//! Read-only from the student and display surfaces; only the staff (teacher)
//! controller writes to it.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "class_display_boards")]
pub struct Model {
    /// FK → `classes.id`.
    #[sea_orm(primary_key)]
    pub class_id: String,
    /// Unguessable, rotatable display token (spec §8.4). Unique.
    pub token: String,
    /// Live board? false → token routes 404 (same code for unknown/rotated).
    pub enabled: bool,
    /// Email of the teacher who enabled / consented (spec §7.2).
    pub consent_by: Option<String>,
    /// Unix timestamp (UTC) when consent was recorded.
    pub consent_at: i64,
    /// Unix timestamp (UTC) when this row was first created.
    pub created_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::class::Entity",
        from = "Column::ClassId",
        to = "super::class::Column::Id"
    )]
    Class,
}

impl ActiveModelBehavior for ActiveModel {}
