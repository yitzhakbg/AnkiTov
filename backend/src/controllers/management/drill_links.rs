//! Drill-link management (T-215 — First-60-Seconds front door).
//!
//! Teachers/admins mint shareable links that open an anonymous visitor
//! directly on Card #1 of a pre-loaded pack (public side: `controllers::drill`).
//! This controller is the teacher surface: create (with on-disk pack
//! validation), list, revoke.
//!
//! Design: `specs/design/2026-11-23-first-60-seconds-drill-link.md` §3.2.

use axum::Extension;
use loco_rs::prelude::*;
use rand::Rng;
use sea_orm::entity::prelude::EntityTrait;
use sea_orm::{ActiveModelTrait, QueryOrder, QuerySelect, Set};

use crate::middleware::auth::{ensure_role, AuthUser};
use crate::models::entities::drill_link;

use super::deck_library::{seed_packages_dir, valid_package_name};

const TAG: &str = "Management Console";
/// Drill links are class-trial-adjacent: 30 days is a generous ceiling, and
/// expires_in_secs beyond it is clamped rather than rejected (predictable).
const MAX_EXPIRY_SECS: i64 = 30 * 24 * 3600;
/// List page size (drill links are few; a hard page keeps queries bounded).
const LIST_LIMIT: u64 = 100;

/// Map a validation message onto a 400 with the message in `description`
/// (settings.rs pattern — loco's Error::BadRequest renders a generic body
/// and drops the message).
fn bad_request(msg: String) -> loco_rs::Error {
    loco_rs::Error::CustomError(
        axum::http::StatusCode::BAD_REQUEST,
        loco_rs::controller::ErrorDetail {
            error: Some("bad_request".into()),
            description: Some(msg),
        },
    )
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management")
        .add("/drill-links", get(list))
        .add("/drill-links", post(create))
        .add("/drill-links/:id", delete(revoke))
}

// ── request/response payloads ───────────────────────────────────────────────

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct CreateDrillLinkRequest {
    /// Seed pack file name (e.g. `english-vocab-he.apkg`) — must exist on disk.
    pub pack: String,
    /// Deck name auto-opened on the seat (passed to the session driver).
    pub deck: String,
    /// Human label shown in the teacher's link list.
    pub title: String,
    /// Optional click budget (each click that lands on a seat counts).
    pub max_uses: Option<i32>,
    /// Optional lifetime in seconds (clamped to 30 days).
    pub expires_in_secs: Option<i64>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct DrillLinkResponse {
    pub id: String,
    /// Full shareable URL (built from `ANKITOV_PUBLIC_URL`).
    pub url: String,
    pub pack: String,
    pub deck: String,
    pub title: String,
    pub created_by: String,
    pub max_uses: Option<i32>,
    pub use_count: i32,
    pub expires_at: Option<i64>,
    pub created_at: i64,
}

// ── handlers ────────────────────────────────────────────────────────────────

/// POST /management/drill-links — mint a drill link.
#[utoipa::path(
    post,
    path = "/api/v1/management/drill-links",
    request_body = CreateDrillLinkRequest,
    responses((status = 201, description = "Created", body = DrillLinkResponse)),
    tag = TAG
)]
pub async fn create(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Json(payload): Json<CreateDrillLinkRequest>,
) -> Result<Response> {
    ensure_role(&user, &["teacher", "admin"])?;

    // Pack must be a sane file name AND actually exist in the seed dir — a
    // link minted against a missing pack would degrade to an empty seat.
    if !valid_package_name(&payload.pack) {
        return Err(bad_request(
            "pack must be an existing seed .apkg file name (letters, digits, dot, dash, underscore)"
                .to_string(),
        ));
    }
    if !seed_packages_dir().join(&payload.pack).is_file() {
        return Err(bad_request(format!(
            "pack '{}' not found in the seed library",
            payload.pack
        )));
    }
    let deck = payload.deck.trim().to_string();
    if deck.is_empty() || deck.len() > 128 {
        return Err(bad_request("deck must be 1-128 characters".to_string()));
    }
    let title = payload.title.trim().to_string();
    if title.is_empty() || title.len() > 200 {
        return Err(bad_request("title must be 1-200 characters".to_string()));
    }
    if let Some(m) = payload.max_uses {
        if m <= 0 {
            return Err(bad_request("max_uses must be positive".to_string()));
        }
    }
    let expires_at = payload
        .expires_in_secs
        .map(|s| s.clamp(1, MAX_EXPIRY_SECS))
        .map(|s| chrono::Utc::now().timestamp() + s);

    let id = new_drill_token();
    let now = chrono::Utc::now().timestamp();
    let model = drill_link::ActiveModel {
        id: Set(id.clone()),
        pack: Set(payload.pack),
        deck: Set(deck),
        title: Set(title),
        created_by: Set(user.email.clone()),
        max_uses: Set(payload.max_uses),
        use_count: Set(0),
        expires_at: Set(expires_at),
        created_at: Set(now),
    };
    let saved = model.insert(&ctx.db).await?;

    // Codebase convention: management creates answer 200 (no controller uses
    // 201; format::json((status, body)) would serialize the tuple as JSON).
    format::json(drill_link_response(&saved))
}

/// GET /management/drill-links — latest links.
#[utoipa::path(
    get,
    path = "/api/v1/management/drill-links",
    responses((status = 200, description = "Drill links", body = [DrillLinkResponse])),
    tag = TAG
)]
pub async fn list(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    ensure_role(&user, &["teacher", "admin"])?;
    let links = drill_link::Entity::find()
        .order_by_desc(drill_link::Column::CreatedAt)
        .limit(LIST_LIMIT)
        .all(&ctx.db)
        .await?;
    let out: Vec<DrillLinkResponse> = links.iter().map(drill_link_response).collect();
    format::json(out)
}

/// DELETE /management/drill-links/:id — revoke (delete) a link.
#[utoipa::path(
    delete,
    path = "/api/v1/management/drill-links/{id}",
    params(("id" = String, Path, description = "Drill-link token")),
    responses((status = 200, description = "Revoked")),
    tag = TAG
)]
pub async fn revoke(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Response> {
    ensure_role(&user, &["teacher", "admin"])?;
    let res = drill_link::Entity::delete_by_id(&id).exec(&ctx.db).await?;
    let deleted = res.rows_affected > 0;
    format::json(serde_json::json!({ "revoked": deleted }))
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn drill_link_response(link: &drill_link::Model) -> DrillLinkResponse {
    DrillLinkResponse {
        id: link.id.clone(),
        url: drill_url(&link.id),
        pack: link.pack.clone(),
        deck: link.deck.clone(),
        title: link.title.clone(),
        created_by: link.created_by.clone(),
        max_uses: link.max_uses,
        use_count: link.use_count,
        expires_at: link.expires_at,
        created_at: link.created_at,
    }
}

/// Full shareable URL from the same `ANKITOV_PUBLIC_URL` that builds
/// `SeatInfo::stream_url` — the drill link must live under the SAME public
/// host the seat stream is served from.
fn drill_url(id: &str) -> String {
    let base = std::env::var("ANKITOV_PUBLIC_URL").unwrap_or_else(|_| "http://127.0.0.1".into());
    format!("{}/drill/{}", base.trim_end_matches('/'), id)
}

/// Mint a 192-bit urlsafe token: 32 chars from an exactly-64-symbol
/// alphabet (A-Za-z0-9, `-`, `_`) — 6 bits per char, no padding, URL-safe.
fn new_drill_token() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}
