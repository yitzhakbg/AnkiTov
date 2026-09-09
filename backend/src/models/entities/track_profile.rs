//! TrackProfile entity — a named profile that groups tracks for assignment.
//!
//! Instructors create profiles (e.g., "Math Rescue Mix") that blend multiple tracks
//! into a single remediation capsule. Profiles can override target retention and N
//! on a per-cohort or per-student basis.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "track_profiles")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// Human-readable profile name (unique).
    pub name: String,
    pub description: Option<String>,
    /// Optional per-profile target retention override.
    pub target_retention: Option<f64>,
    /// Optional per-profile N-value override (sessions per week).
    pub n_value: Option<i32>,
    /// Session duration in minutes (default 10, min 1, max 60).
    /// Used by the capsule slicer for time-bound sizing.
    pub session_duration_minutes: i32,
    /// Unix timestamp in seconds (UTC).
    pub created_at: i64,
    /// Unix timestamp in seconds (UTC).
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}