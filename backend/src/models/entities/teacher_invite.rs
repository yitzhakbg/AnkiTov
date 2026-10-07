//! TeacherInvite entity — one invite per enrolled teacher (Prong 8).
//!
//! Admin enrollment creates the user row (no password yet) plus this invite.
//! The `:invite_id` + `:token` path pair is the *credential* for the public
//! first-login wizard (no JWT exists before activation). `invite_status`
//! classifies an invite as revoked → used → expired, in that order, so a
//! non-NULL `revoked_at`/`used_at` wins over the `expires_at` check.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "teacher_invites")]
pub struct Model {
    /// Invite identifier — the public `:invite_id` path segment paired with
    /// `invite_token`.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// FK to `users.id` of the not-yet-activated teacher account.
    pub user_id: i64,
    /// School captured at enrollment; displayed by the first-login wizard.
    pub school: String,
    /// Optional school community/grouping captured at enrollment.
    pub community: Option<String>,
    /// Optional intended role (server-side default applies when `None`).
    pub role: Option<String>,
    /// UI locale for the teacher's first-login wizard.
    pub locale: String,
    /// Optional contact email captured at enrollment (the user row's email
    /// remains the authoritative login identity).
    pub email: Option<String>,
    /// Secret path credential — must match `:token` in first-login URLs.
    /// Rotated by the resend endpoint, which also resets the TTL window.
    pub invite_token: String,
    /// Unix timestamp in seconds (UTC); reset on resend together with
    /// `expires_at`.
    pub created_at: i64,
    /// Expiry (`created_at` + 14-day TTL); invites past this are invalid
    /// unless already revoked/used.
    pub expires_at: i64,
    /// Unix timestamp in seconds (UTC); set when first login activates the
    /// account — non-`None` means the invite is consumed.
    pub used_at: Option<i64>,
    /// Unix timestamp in seconds (UTC); non-`None` marks the invite revoked.
    pub revoked_at: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
