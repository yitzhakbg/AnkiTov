//! User entity — represents a student, teacher, or admin in the system.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub username: String,
    pub email: String,
    pub full_name: Option<String>,
    pub role: String,
    pub subsection_id: Option<String>,
    pub password_hash: Option<String>,
    /// Seat kiosk level (owner decision 2026-11-23): 0 = practice-only
    /// kiosk, 1 = guided, 2 = full Anki. SeatManager maps student → level
    /// at seat start; see `addons/ankitov_session_driver/kiosk.py`.
    pub kiosk_level: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
