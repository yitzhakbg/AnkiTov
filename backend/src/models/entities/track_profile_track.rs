//! TrackProfileTrack junction entity — maps tracks to track profiles (many-to-many).
//!
//! Each row links one track to one profile. `sort_order` defines the relative
//! priority or display sequence within the profile.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "track_profile_tracks")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Foreign key to `track_profiles.id`.
    pub track_profile_id: String,
    /// Foreign key to `tracks.id`.
    pub track_id: String,
    /// Display / priority order within the profile.
    pub sort_order: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::track_profile::Entity",
        from = "Column::TrackProfileId",
        to = "super::track_profile::Column::Id"
    )]
    TrackProfile,
    #[sea_orm(
        belongs_to = "super::track::Entity",
        from = "Column::TrackId",
        to = "super::track::Column::Id"
    )]
    Track,
}

impl ActiveModelBehavior for ActiveModel {}