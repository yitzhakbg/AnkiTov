//! ProfileAssignment entity — binds a student or cohort to a track profile.
//!
//! Capsule generation resolves a student's effective track profile from the
//! assignment that targets the student directly (`target_type = "student"`)
//! or, failing that, their cohort (`target_id` = the student's
//! `subsection_id`). The create endpoint upserts on duplicate
//! (target, profile) combinations, so re-posting the same assignment is
//! idempotent.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "profile_assignments")]
pub struct Model {
    /// Auto-increment primary key.
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Assignment target kind: `"student"` or `"cohort"`.
    pub target_type: String,
    /// Recipient id: `users.id` when `target_type = "student"`, or
    /// `users.subsection_id` when `"cohort"`.
    pub target_id: String,
    /// FK to `track_profiles.id` — the track mix and retention settings to
    /// apply.
    pub track_profile_id: String,
    /// Resolution priority; lower wins. The controller sets 1 for student
    /// targets and 10 for cohort targets, so a direct assignment overrides
    /// the cohort default.
    pub priority: i32,
    /// Admin/actor that created the assignment (for audit).
    pub assigned_by: String,
    /// Unix timestamp in seconds (UTC).
    pub assigned_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
