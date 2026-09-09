//! AddonDistribution entity — tracks addon installations across targets.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "addon_distributions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: String,
    pub addon_id: String,
    pub target_type: String,
    pub target_id: String,
    pub distributed_by: String,
    pub distributed_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
