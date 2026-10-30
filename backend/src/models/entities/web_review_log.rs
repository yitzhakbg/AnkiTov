//! WebReviewLog entity — the backend-side revlog for Solo Web Reviewer answers.
//!
//! One row per browser-reviewer answer (spec 2026-10-25 §2.1). The primary key
//! is a client-generated UUID so flaky-mobile-network retries are idempotent.
//! `fsrs_memory_states` derivation replays this table after the snapshot
//! revlog (§2.2) — implemented in `services/fsrs_state.rs`
//! (`load_web_reviews` + merged-timeline replay), so web answers shape
//! scheduling exactly like Anki reviews.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "web_review_log")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, string_len = 36)]
    pub id: String,
    /// FK to the user (student) who answered.
    pub student_id: String,
    /// Anki card id (as in the collection `.anki2`).
    pub card_id: i64,
    /// Capsule session this answer belonged to, when any.
    pub session_id: Option<String>,
    /// FSRS rating: 1=Again, 2=Hard, 3=Good, 4=Easy.
    pub ease: i32,
    /// Milliseconds spent on the card.
    pub time_ms: i64,
    /// Unix timestamp in seconds (UTC).
    pub reviewed_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
