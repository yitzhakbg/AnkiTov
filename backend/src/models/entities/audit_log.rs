//! AuditLog entity — immutable append-only ledger for compliance events.
//!
//! Records every N-value change, profile assignment/revocation, and capsule
//! generation event. Used for billing verification and compliance auditing.
//!
//! This is the audit trail backing the Clearing House Model's consumption-based
//! monetization (§5 of Strategic Blueprint).

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "audit_logs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// Type of event: "n_change", "profile_assign", "profile_revoke",
    /// "capsule_generate", "capsule_complete".
    pub event_type: String,
    /// JSON blob with event-specific details.
    pub event_data: String,
    /// Who triggered the event (instructor ID, "system", or student ID).
    pub actor_id: String,
    /// Target student or cohort affected.
    pub target_id: Option<String>,
    /// Unix timestamp in seconds (UTC).
    pub created_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}