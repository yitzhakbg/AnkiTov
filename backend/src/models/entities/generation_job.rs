//! GenerationJob entity — one row per batch capsule-generation run.
//!
//! The sequential background runner (`services::capsule_generator_job`)
//! drives the row's lifecycle: `pending` at trigger time, `processing` once
//! the batch starts, then `completed` (or `failed` on a run-level abort).
//! Progress is rewritten after each student so the console can poll it.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "generation_jobs")]
pub struct Model {
    /// UUID primary key — returned by the trigger endpoint and handed to
    /// the background runner.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// Lifecycle stage: `"pending"`, `"processing"`, `"completed"`, or
    /// `"failed"`.
    pub status: String,
    /// Completion percentage 0–100, recomputed after each student as
    /// `completed * 100 / total`.
    pub progress: i32,
    /// Snapshot of active students (`role = "student"`) at job start.
    pub total_students: i32,
    /// Students processed so far (including those that needed fallbacks).
    pub completed_students: i32,
    /// JSON-serialized list of per-student anomalies (`StudentAnomaly`);
    /// left untouched when no student hit a fallback.
    pub anomalies: Option<String>,
    /// Unix timestamp in seconds (UTC); set when the run finishes, `None`
    /// while the job is still running.
    pub completed_at: Option<i64>,
    /// Unix timestamp in seconds (UTC); creation time, also the ordering
    /// key for the "latest job" query.
    pub created_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
