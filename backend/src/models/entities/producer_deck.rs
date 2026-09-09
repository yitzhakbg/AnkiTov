//! ProducerDeck entity — producer-facing deck catalog row with verification
//! status lifecycle (P1: Producer upload + verification, spec:
//! retention-period-plan §Phase 1).
//!
//! Status lifecycle: `pending → verified | rejected` (plus
//! `published`/`delisted` for later phases). String column matching house
//! style (`capsule_session.status` precedent).

use sea_orm::entity::prelude::*;

/// ProducerDeck status values (`pub const STATUS_*` set per P1 spec).
pub const STATUS_PENDING: &str = "pending";
pub const STATUS_VERIFIED: &str = "verified";
pub const STATUS_REJECTED: &str = "rejected";
pub const STATUS_PUBLISHED: &str = "published";
pub const STATUS_DELISTED: &str = "delisted";

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "producer_decks")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Foreign key to Producers.id (i32 pk_auto).
    pub producer_id: i32,
    /// Foreign key to Decks.id (i32 pk_auto).
    pub deck_id: i32,
    /// One of the `STATUS_*` constants above.
    pub status: String,
    /// Human-readable reason set when `status == rejected`.
    pub reject_reason: Option<String>,
    /// Unix timestamp in seconds (UTC) when verification completed.
    pub verified_at: Option<i64>,
    /// Unix timestamp in seconds (UTC).
    pub created_at: i64,
    /// Unix timestamp in seconds (UTC).
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
