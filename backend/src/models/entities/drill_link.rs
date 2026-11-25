use sea_orm::entity::prelude::*;

/// A shareable Anki-less drill link (T-215 First-60-Seconds): clicking
/// `GET /api/v1/drill/<id>` spawns an ephemeral preview seat with this
/// link's pack imported and its deck auto-opened (kiosk 0), then 302s the
/// browser to the Selkies stream. Design record:
/// `specs/design/2026-11-23-first-60-seconds-drill-link.md`.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "drill_links")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    /// The random urlsafe token — it IS the `/api/v1/drill/<id>` path segment.
    pub id: String,
    /// Seed `.apkg` file name the seat imports (ANKITOV_SEAT_PACKS value).
    pub pack: String,
    /// Anki deck name the kiosk auto-opens (ANKITOV_SEAT_DECK value).
    pub deck: String,
    /// Landing/busy-page title shown pre-redirect.
    pub title: String,
    /// Creating user id (teacher/admin).
    pub created_by: String,
    /// Maximum times the link may grant a seat; null = unlimited.
    pub max_uses: Option<i32>,
    /// Seats granted so far (incremented per successful claim).
    pub use_count: i32,
    /// Unix seconds after which the link answers a uniform 404; null = never.
    pub expires_at: Option<i64>,
    /// Unix seconds when the link was created.
    pub created_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
