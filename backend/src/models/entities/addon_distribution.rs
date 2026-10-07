//! AddonDistribution entity — tracks addon installations across targets.
//!
//! Created when a teacher/admin distributes an addon to a user, group, or
//! class. Powers the management console's "which targets have this addon"
//! reporting and per-target rollout checks.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "addon_distributions")]
pub struct Model {
    /// Primary key (UUID string; auto-generated on insert).
    #[sea_orm(primary_key)]
    pub id: String,
    /// FK to `addons.id` — the package being distributed.
    pub addon_id: String,
    /// Distribution scope: `"user"`, `"group"`, or `"class"` (validated
    /// against this set by the distribute endpoints).
    pub target_type: String,
    /// UUID of the distribution target (a user id, group id, or class id,
    /// depending on `target_type`).
    pub target_id: String,
    /// Lowercase UUID string of the teacher/admin who distributed the addon.
    pub distributed_by: String,
    /// Unix timestamp in seconds (UTC) of the distribution event.
    pub distributed_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
