//! Class entity — represents an instructional group (e.g., "Period 1 — Algebra I").
//!
//! Classes are the primary organizational unit for students. Decks, profiles,
//! and capsules can be distributed to classes. Each class has one assigned teacher.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "classes")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// Human-readable name (e.g., "Period 1 — Algebra I").
    pub name: String,
    /// Denormalized subject area for display/filtering convenience.
    pub subject_area: Option<String>,
    /// FK to users.id for the assigned teacher.
    pub teacher_id: String,
    /// Free-form period label (e.g., "1", "Morning Block", "3rd Hour").
    pub period: Option<String>,
    /// Academic year (e.g., "2026-2027") for clean year-over-year rollover.
    pub academic_year: Option<String>,
    /// 6-character alphanumeric code for student self-enrollment.
    pub class_code: Option<String>,
    /// Unix timestamp in seconds (UTC).
    pub created_at: i64,
    /// Unix timestamp in seconds (UTC).
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
