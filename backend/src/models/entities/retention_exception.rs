//! RetentionException entity — flags users/decks with anomalous retention rates.
//!
//! Written by the retention analytics pipeline when a student's measured rate
//! falls below the configured threshold. The ask engine and management
//! dashboards read these rows to surface at-risk learners, and list active
//! exceptions by filtering on a null `resolved_at`.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "retention_exceptions")]
pub struct Model {
    /// Primary key (UUID string).
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// FK to `users.id` — the student whose retention triggered the anomaly.
    pub user_id: String,
    /// Deck identifier the anomaly was detected on (displayed in ask/stats
    /// detail rows, e.g. "Deck: `<deck_id>` — threshold X%").
    pub deck_id: String,
    /// Category label of the anomaly (rendered as "Type: …" in the ask
    /// engine's exception listing).
    pub exception_type: String,
    /// Measured retention rate as a fraction 0.0–1.0; the ask engine maps
    /// < 0.40 to "danger" and < 0.60 to "warn".
    pub retention_rate: f64,
    /// Expected retention floor (fraction 0.0–1.0) that `retention_rate`
    /// fell below when the exception fired.
    pub threshold: f64,
    /// Unix timestamp in seconds (UTC) when the anomaly was detected;
    /// exception listings order by this descending.
    pub detected_at: i64,
    /// Unix timestamp in seconds (UTC); `None` while the exception is open —
    /// all dashboards filter `ResolvedAt IS NULL` to list active ones.
    pub resolved_at: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
