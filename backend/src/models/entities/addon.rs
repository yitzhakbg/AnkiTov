//! Addon entity — represents an Anki add-on package.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "addons")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub file_size_bytes: i64,
    pub checksum_sha256: String,
    pub config_json: Option<String>,
    /// Stored as lowercase UUID string.
    pub installed_by: String,
    /// Unix timestamp in seconds (UTC).
    pub created_at: i64,
    /// Unix timestamp in seconds (UTC).
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
