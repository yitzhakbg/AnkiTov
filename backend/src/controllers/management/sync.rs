//! Sync server control endpoints.
//!
//! Manages the anki-cloud sync server for all users, provides
//! per-user sync status, and allows manual sync triggering.

use crate::models::management::{ApiResponse, SyncStatusQuery, SyncStatusResponse, UserSyncStatus};
use axum::extract::{Path, Query, State, Extension};
use loco_rs::{app::AppContext, prelude::*};
use crate::middleware::auth::AuthUser;
use crate::middleware::auth::ensure_role;

const TAG: &str = "Management Console";

/// Routes for sync server control.
pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management/sync")
        .add("/status", get(status))
        .add("/users", get(list_user_statuses))
        .add("/users/:user_id", get(user_status))
        .add("/users/:user_id/trigger", post(trigger_sync))
        .add("/full", post(full_sync))
}

/// Get overall sync server status.
#[utoipa::path(
    get,
    path = "/management/sync/status",
    responses(
        (status = 200, description = "Sync server status", body = SyncStatusResponse)
    ),
    tag = TAG
)]
pub async fn status(_ctx: State<AppContext>) -> Result<Response> {
    tracing::info!("Fetching sync server status");

    // TODO: Query anki-cloud status endpoint or internal state
    let sync_status = SyncStatusResponse {
        server_version: "0.0.1".to_string(),
        connected_users: 0,
        pending_syncs: 0,
        last_full_sync: chrono::Utc::now().timestamp(),
        status: "operational".to_string(),
    };

    format::json(sync_status)
}

/// List sync status for all users with optional status filter.
#[utoipa::path(
    get,
    path = "/management/sync/users",
    params(
        ("status" = Option<String>, Query, description = "Filter by status: active | idle | error")
    ),
    responses(
        (status = 200, description = "User sync statuses", body = Vec<UserSyncStatus>)
    ),
    tag = TAG
)]
pub async fn list_user_statuses(
    State(ctx): State<AppContext>,
    Query(params): Query<SyncStatusQuery>
) -> Result<Response> {
    tracing::info!("Listing user sync statuses — filter={:?}", params.status);

    use crate::models::entities::user as user_entity;
    
    // Query users from the DB
    let mut query = user_entity::Entity::find();
    
    // Filter out teachers/admins if we only want student sync statuses
    query = query.filter(user_entity::Column::Role.eq("student".to_string()));

    let users = query.all(&ctx.db).await?;

    let user_statuses: Vec<UserSyncStatus> = users
        .into_iter()
        .map(|u| UserSyncStatus {
            user_id: u.id.to_string(),
            full_name: u.full_name.clone(),
            cohort_id: u.subsection_id.clone(),
            last_sync: u.updated_at,
            pending_changes: 0,
            status: "idle".to_string(),
        })
        .collect();

    format::json(user_statuses)
}

/// Get sync status for a specific user.
#[utoipa::path(
    get,
    path = "/management/sync/users/{user_id}",
    params(
        ("user_id" = String, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "User sync status", body = UserSyncStatus),
        (status = 404, description = "User not found")
    ),
    tag = TAG
)]
pub async fn user_status(_ctx: State<AppContext>, Path(user_id): Path<String>) -> Result<Response> {
    tracing::info!("Fetching sync status for user: {}", user_id);

    // TODO: SeaORM query for user_id in sync_log
    let status = UserSyncStatus {
        user_id,
        full_name: None,
        cohort_id: None,
        last_sync: chrono::Utc::now().timestamp(),
        pending_changes: 0,
        status: "idle".to_string(),
    };

    format::json(status)
}

/// Manually trigger a sync for a specific user.
#[utoipa::path(
    post,
    path = "/management/sync/users/{user_id}/trigger",
    params(
        ("user_id" = String, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "Sync triggered", body = ApiResponse),
        (status = 404, description = "User not found")
    ),
    tag = TAG
)]
pub async fn trigger_sync(
    Extension(user): Extension<AuthUser>,
    _ctx: State<AppContext>,
    Path(user_id): Path<String>,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    tracing::info!("Manually triggering sync for user: {}", user_id);

    // TODO: Call anki-cloud sync API or enqueue sync job
    format::json(ApiResponse {
        success: true,
        message: format!("Sync triggered for user {}", user_id),
    })
}

/// Force a full sync for all users (admin only).
#[utoipa::path(
    post,
    path = "/management/sync/full",
    responses(
        (status = 200, description = "Full sync initiated", body = ApiResponse)
    ),
    tag = TAG
)]
pub async fn full_sync(
    Extension(user): Extension<AuthUser>,
    _ctx: State<AppContext>,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    tracing::warn!("Admin triggered full sync for all users");

    // TODO: Enqueue full-sync job across all user profiles
    format::json(ApiResponse {
        success: true,
        message: "Full sync initiated for all users".to_string(),
    })
}