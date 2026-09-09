//! ClassEnrollment entity — junction table linking students to classes.
//!
//! Uses a soft-delete pattern: when a student transfers out of a class,
//! `active` is set to false rather than deleting the row. This preserves
//! historical capsule and compliance data. A student can be re-enrolled
//! by inserting a new row with `active = true`.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "class_enrollments")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// FK to classes.id.
    pub class_id: String,
    /// FK to users.id.
    pub student_id: String,
    /// Unix timestamp in seconds (UTC) when enrolled.
    pub enrolled_at: i64,
    /// true = currently enrolled; false = transferred out.
    pub active: bool,
    /// Human-readable display name (e.g., "Fatima Al Rashid"). Falls back to student_id.
    pub display_name: Option<String>,
    /// Extensible JSON metadata (email, SIS ID, etc.).
    pub metadata_json: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
