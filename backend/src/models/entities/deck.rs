//! Deck entity — represents an Anki deck uploaded to the management system.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "decks")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub card_count: i32,
    pub file_size_bytes: i64,
    pub checksum_sha256: String,
    /// Stored as lowercase UUID string.
    pub uploaded_by: String,
    /// Optional foreign key to `tracks.id` — P2 minimal deck-track binding.
    /// NULL means the deck is unassigned (library-wide).
    pub track_id: Option<String>,
    /// Unix timestamp in seconds (UTC).
    pub created_at: i64,
    /// Unix timestamp in seconds (UTC).
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::track::Entity",
        from = "Column::TrackId",
        to = "super::track::Column::Id"
    )]
    Track,
}

impl ActiveModelBehavior for ActiveModel {}
