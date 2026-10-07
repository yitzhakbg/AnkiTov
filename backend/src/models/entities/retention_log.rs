//! RetentionLog entity — immutable telemetry for each card review event.
//!
//! One row per graded review, ingested from Anki sync. Aggregations over
//! these rows compute per-user/per-deck retention rates and drive the
//! anomaly detection that produces `retention_exceptions` entries.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "retention_logs")]
pub struct Model {
    /// Primary key (UUID string).
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// FK to `users.id` — the student who performed the review.
    pub user_id: String,
    /// Deck identifier the reviewed card belongs to.
    pub deck_id: String,
    /// Anki card identifier within the deck.
    pub card_id: String,
    /// Review grade / ease rating; `1` marks a lapse (Anki "again") and is
    /// what retention computations treat as a failed recall.
    pub grade: i32,
    /// Scheduled interval after this review, in seconds (SM-2/FSRS).
    pub interval_seconds: i64,
    /// SM-2 ease factor after this review (Anki stores the value ×1000).
    pub ease_factor: f64,
    /// Unix timestamp in seconds (UTC) of the review event.
    pub review_timestamp: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
