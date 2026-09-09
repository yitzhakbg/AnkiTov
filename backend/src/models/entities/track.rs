//! Track entity — a tag-based sub-collection of cards in the Track Library.
//!
//! Tracks represent atomic subject areas (e.g., "Decimals & Percentages - 7th Grade Math")
//! that can be mixed into remediation capsules. Each track carries a `tag` used for
//! FSRS parameter aggregation and per-subject analytics.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "tracks")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// Subject area classification (e.g., "Math", "Literacy", "Science").
    pub subject_area: Option<String>,
    /// Tag used for FSRS grouping and analytics aggregation.
    pub tag: String,
    /// Default target retention rate (0.0–1.0), typically 0.78–0.80.
    pub target_retention: f64,
    /// Default sessions per week for this track.
    pub n_value: i32,
    /// Unix timestamp in seconds (UTC).
    pub created_at: i64,
    /// Unix timestamp in seconds (UTC).
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}