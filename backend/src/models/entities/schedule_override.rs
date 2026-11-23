//! `schedule_overrides` entity — B-0035 sporadic-continuity scheduler.
//!
//! One row per scheduling override for a `(student, card)` pair. Active rows
//! (`cleared_at IS NULL`) carry the deferral overlay written by the backlog
//! triage (`schedule_triage::plan_deferral` → `upsert_deferrals`):
//! `deferred_until` is the epoch-ms instant when the card re-enters the
//! normal due flow. Answering a card sets `cleared_at`
//! (`schedule_triage::clear_override`), retiring the override while keeping
//! the row as an audit trail.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "schedule_overrides")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// Student identifier (users.id as string).
    pub student_id: String,
    /// Anki card id (unique per student's collection).
    pub card_id: i64,
    /// Epoch-ms instant when the deferred card re-enters the due flow.
    pub deferred_until: i64,
    /// Why the override exists (e.g. `backlog-triage`).
    pub reason: Option<String>,
    /// Number of tranche sessions the deferral plan spans.
    pub k_sessions: i32,
    /// Epoch-ms creation time.
    pub created_at: i64,
    /// Epoch-ms clear time; `NULL` while the override is active.
    pub cleared_at: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
