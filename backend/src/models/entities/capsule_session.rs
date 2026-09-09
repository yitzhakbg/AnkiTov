//! CapsuleSession entity — a single remediation capsule generated for a student.
//!
//! Each capsule is a fixed-size slice of interleaved cards compiled from the student's
//! assigned track profile. Capsules are generated server-side and track the student's
//! progress through the session.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "capsule_sessions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// FK to the user (student) this capsule was generated for.
    pub student_id: String,
    /// FK to the track profile used for generation.
    pub track_profile_id: String,
    /// Snapshot of N-value (sessions per week) at generation time.
    pub n_value: i32,
    /// Snapshot of session duration (minutes) at generation time.
    pub session_duration_minutes: i32,
    /// JSON array of card IDs included in this capsule.
    pub card_ids: String,
    /// Status: "pending", "in_progress", "completed", "expired".
    pub status: String,
    /// ISO week identifier (e.g., "2026-W27") for compliance tracking.
    pub session_week: String,
    /// Number of cards reviewed so far.
    pub cards_completed: i32,
    /// Total number of cards in this capsule.
    pub cards_total: i32,
    /// Unix timestamp in seconds (UTC) when generated.
    pub created_at: i64,
    /// Unix timestamp in seconds (UTC) when completed.
    pub completed_at: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::track_profile::Entity",
        from = "Column::TrackProfileId",
        to = "super::track_profile::Column::Id"
    )]
    TrackProfile,
}

impl ActiveModelBehavior for ActiveModel {}