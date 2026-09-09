//! Addon management endpoints.
//!
//! Handles installation, configuration, and distribution of Anki addons
//! across managed user profiles.

use crate::models::management::{AddonInfo, ApiResponse};
use axum::{body::Bytes, extract::{Path, State}, Extension, Json};
use loco_rs::{app::AppContext, prelude::*};
use crate::middleware::auth::AuthUser;
use crate::middleware::auth::ensure_role;

const TAG: &str = "Management Console";

/// Routes for addon management.
pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management/addons")
        .add("/", get(list_addons))
        .add("/install", post(install_addon))
        .add("/:addon_id/distribute", post(distribute_addon))
        .add("/:addon_id/config", get(get_addon_config))
        .add("/:addon_id/config", put(update_addon_config))
}

/// List all installed addons with distribution counts.
#[utoipa::path(
    get,
    path = "/management/addons",
    responses(
        (status = 200, description = "List of installed addons", body = Vec<AddonInfo>)
    ),
    tag = TAG
)]
pub async fn list_addons(_ctx: State<AppContext>) -> Result<Response> {
    tracing::info!("Listing all installed Anki addons");

    // TODO: SeaORM query against addons table
    let addons: Vec<AddonInfo> = vec![];
    format::json(addons)
}

/// Install a new addon from a .ankiaddonp package.
#[utoipa::path(
    post,
    path = "/management/addons/install",
    request_body = Vec<u8>,
    responses(
        (status = 201, description = "Addon installed successfully", body = AddonInfo),
        (status = 400, description = "Invalid addon package"),
        (status = 409, description = "Addon already installed")
    ),
    tag = TAG
)]
pub async fn install_addon(
    Extension(user): Extension<AuthUser>,
    _ctx: State<AppContext>,
    body: Bytes,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    tracing::info!("Installing addon — size={} bytes", body.len());

    // TODO: Validate .ankiaddonp, extract metadata, store addon blob
    let addon = AddonInfo {
        id: uuid::Uuid::new_v4().to_string(),
        name: "Unnamed Addon".to_string(),
        version: "0.0.1".to_string(),
        installed_at: chrono::Utc::now().timestamp(),
        distribution_count: 0,
    };

    tracing::info!("Addon installed: {}", addon.id);
    format::json(addon)
}

/// Distribute an installed addon to a user, group, or class.
#[utoipa::path(
    post,
    path = "/management/addons/{addon_id}/distribute",
    params(
        ("addon_id" = String, Path, description = "Addon ID")
    ),
    request_body = crate::models::management::DistributeAddonRequest,
    responses(
        (status = 201, description = "Addon distributed successfully", body = ApiResponse),
        (status = 404, description = "Addon not found"),
        (status = 400, description = "Invalid target")
    ),
    tag = TAG
)]
pub async fn distribute_addon(
    Extension(user): Extension<AuthUser>,
    _ctx: State<AppContext>,
    Path(addon_id): Path<String>,
    Json(payload): Json<crate::models::management::DistributeAddonRequest>,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    tracing::info!(
        "Distributing addon {} to {}:{}",
        addon_id, payload.target_type, payload.target_id
    );

    // TODO: Validate target, create distribution record, push to anki-cloud
    format::json(ApiResponse {
        success: true,
        message: format!("Addon {} distributed to {}:{}", addon_id, payload.target_type, payload.target_id),
    })
}

/// Get configuration for a specific addon.
#[utoipa::path(
    get,
    path = "/management/addons/{addon_id}/config",
    params(
        ("addon_id" = String, Path, description = "Addon ID")
    ),
    responses(
        (status = 200, description = "Addon configuration as JSON"),
        (status = 404, description = "Addon not found")
    ),
    tag = TAG
)]
pub async fn get_addon_config(_ctx: State<AppContext>, Path(addon_id): Path<String>) -> Result<Response> {
    tracing::info!("Fetching config for addon: {}", addon_id);

    // TODO: SeaORM query for addon config JSON
    format::json(serde_json::json!({ "addon_id": addon_id, "settings": {} }))
}

/// Update configuration for a specific addon.
#[utoipa::path(
    put,
    path = "/management/addons/{addon_id}/config",
    params(
        ("addon_id" = String, Path, description = "Addon ID")
    ),
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Configuration updated", body = ApiResponse),
        (status = 404, description = "Addon not found")
    ),
    tag = TAG
)]
pub async fn update_addon_config(
    Extension(user): Extension<AuthUser>,
    _ctx: State<AppContext>,
    Path(addon_id): Path<String>,
    Json(_config): Json<serde_json::Value>,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    tracing::info!("Updating config for addon: {}", addon_id);

    // TODO: Validate config schema, persist to SeaORM
    format::json(ApiResponse {
        success: true,
        message: format!("Addon {} configuration updated", addon_id),
    })
}