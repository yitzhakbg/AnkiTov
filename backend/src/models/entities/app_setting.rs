//! AppSetting entity — school-level configuration key/value store (T-126).
//!
//! One row per setting key. Written by the management Settings view and the
//! first-run setup wizard; read by `services::settings` and the settings
//! controller. Values are plain strings — interpretation lives in
//! `services::settings`, so adding a setting never requires a migration.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "app_settings")]
pub struct Model {
    /// Setting key (primary key), e.g. `school_name`, `locale`, `sync_url`.
    #[sea_orm(primary_key, auto_increment = false)]
    pub key: String,
    /// Setting value as a string; readers parse/validate per key.
    pub value: String,
    /// Unix timestamp in seconds (UTC) of the last write.
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
