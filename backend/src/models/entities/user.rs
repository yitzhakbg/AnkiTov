//! User entity — represents a student, teacher, or admin in the system.
//!
//! The `role` field drives authorization across the app (`ensure_role`
//! accepts `"teacher"`/`"admin"` for management routes), and batch capsule
//! generation targets rows where `role = "student"` only.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "users")]
pub struct Model {
    /// Surrogate primary key; referenced by invites, classes, enrollments,
    /// and used as the Anki sync credential id (see `services::sync_user`).
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Unique account name; duplicate usernames are rejected at registration.
    pub username: String,
    /// Unique email — the login identity embedded in issued JWTs.
    pub email: String,
    /// Display name shown in the console (optional; may be derived on import).
    pub full_name: Option<String>,
    /// Role string: `"student"`, `"teacher"`, or `"admin"`.
    pub role: String,
    /// Cohort / subsection identifier — the `"cohort"` target for track
    /// profile assignments (`target_id` = this value).
    pub subsection_id: Option<String>,
    /// Password hash (bcrypt); `None` until activation — invite-enrolled
    /// teachers set it at first login, imported students may never have one.
    pub password_hash: Option<String>,
    /// Unix timestamp in seconds (UTC).
    pub created_at: i64,
    /// Unix timestamp in seconds (UTC).
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
