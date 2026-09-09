//! DeckDistribution entity — tracks which decks are distributed to which targets.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "deck_distributions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Foreign key to Decks.id (i32 pk_auto).
    pub deck_id: i32,
    pub target_type: String,
    /// Target entity ID (UUID string for user/group/class).
    pub target_id: String,
    /// Admin user who made the distribution (UUID string).
    pub distributed_by: String,
    /// Unix timestamp in seconds (UTC).
    pub distributed_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
