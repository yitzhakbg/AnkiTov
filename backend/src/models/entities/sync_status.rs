//! SyncStatus entity — tracks Anki sync state per user.
//!
//! One row per user, updated by the sync pipeline. The management console
//! lists these rows for connectivity views, and the ask engine reads
//! `last_sync` / `pending_changes` for missed-practice and stale-client
//! detection.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "sync_statuses")]
pub struct Model {
    /// Primary key (UUID string).
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// FK to `users.id` — the user this sync state row belongs to.
    pub user_id: String,
    /// Classification of the sync state ("active" | "idle" | "error"; the
    /// management console's status query filters on this string).
    pub status: String,
    /// Unix timestamp in seconds (UTC) of the last successful sync;
    /// `None` means never synced (counted as missed practice by the ask
    /// engine's missed-practice query).
    pub last_sync: Option<i64>,
    /// Number of local changes queued for the next push to Anki Cloud.
    pub pending_changes: i32,
    /// Detail of the last sync failure; `None` while syncing is healthy.
    pub error_message: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
