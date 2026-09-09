//! Track Library controller — CRUD endpoints for remediation tracks.
//!
//! Tracks are tag-based sub-collections of cards that can be mixed into
//! remediation capsules. Each track carries FSRS aggregation metadata.

use crate::models::entities::track as track_entity;
use crate::models::entities::track_profile_track as tpt_entity;
use crate::models::management::{
    ApiResponse, CreateTrackRequest, TrackInfo, UpdateTrackRequest,
};
use crate::services::audit_logger;
use axum::extract::{Path, State, Json, Extension};
use loco_rs::{app::AppContext, prelude::*};
use sea_orm::{entity::prelude::*, ActiveValue, QueryOrder};
use crate::middleware::auth::AuthUser;
use crate::middleware::auth::ensure_role;

const TAG: &str = "Management Console";

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management/tracks")
        .add("/", get(list_tracks))
        .add("/", post(create_track))
        .add("/:track_id", get(get_track))
        .add("/:track_id", put(update_track))
        .add("/:track_id", delete(delete_track))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn entity_to_info(t: &track_entity::Model) -> TrackInfo {
    TrackInfo {
        id: t.id.clone(),
        name: t.name.clone(),
        description: t.description.clone(),
        subject_area: t.subject_area.clone(),
        tag: t.tag.clone(),
        target_retention: t.target_retention,
        n_value: t.n_value,
        created_at: t.created_at,
        updated_at: t.updated_at,
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// List all tracks in the Track Library.
#[utoipa::path(
    get,
    path = "/management/tracks",
    responses(
        (status = 200, description = "List of all tracks", body = Vec<TrackInfo>)
    ),
    tag = TAG
)]
pub async fn list_tracks(State(ctx): State<AppContext>) -> Result<Response> {
    tracing::info!("Listing all tracks");
    let tracks = track_entity::Entity::find()
        .order_by_asc(track_entity::Column::Name)
        .all(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("list_tracks DB error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;
    let infos: Vec<TrackInfo> = tracks.iter().map(entity_to_info).collect();
    format::json(infos)
}

/// Get a single track by ID.
#[utoipa::path(
    get,
    path = "/management/tracks/{track_id}",
    params(("track_id" = String, Path, description = "Track ID (UUID)")),
    responses(
        (status = 200, description = "Track metadata", body = TrackInfo),
        (status = 404, description = "Track not found")
    ),
    tag = TAG
)]
pub async fn get_track(
    State(ctx): State<AppContext>,
    Path(track_id): Path<String>
) -> Result<Response> {
    tracing::info!("Fetching track: {track_id}");
    let track = track_entity::Entity::find_by_id(&track_id)
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("get_track DB error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;
    match track {
        Some(t) => format::json(entity_to_info(&t)),
        None => format::json(ApiResponse {
            success: false,
            message: format!("Track {} not found", track_id),
        }),
    }
}

/// Create a new track in the Track Library.
#[utoipa::path(
    post,
    path = "/management/tracks",
    request_body = CreateTrackRequest,
    responses(
        (status = 201, description = "Track created", body = TrackInfo),
        (status = 400, description = "Invalid input")
    ),
    tag = TAG
)]
pub async fn create_track(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Json(payload): Json<CreateTrackRequest>)
     -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    tracing::info!("Creating track: {}", payload.name);
    let ts = now_ts();
    let id = uuid::Uuid::new_v4().to_string();

    let track = track_entity::ActiveModel {
        id: ActiveValue::Set(id),
        name: ActiveValue::Set(payload.name),
        description: ActiveValue::Set(payload.description),
        subject_area: ActiveValue::Set(payload.subject_area),
        tag: ActiveValue::Set(payload.tag),
        target_retention: ActiveValue::Set(payload.target_retention),
        n_value: ActiveValue::Set(payload.n_value),
        created_at: ActiveValue::Set(ts),
        updated_at: ActiveValue::Set(ts),
    }
    .insert(&ctx.db)
    .await
    .map_err(|e| {
        tracing::error!("create_track DB error: {:?}", e);
        loco_rs::Error::InternalServerError
    })?;

    tracing::info!("Track created: id={}, name={}", track.id, track.name);
    format::json(entity_to_info(&track))
}

/// Update an existing track.
#[utoipa::path(
    put,
    path = "/management/tracks/{track_id}",
    params(("track_id" = String, Path, description = "Track ID (UUID)")),
    request_body = UpdateTrackRequest,
    responses(
        (status = 200, description = "Track updated", body = TrackInfo),
        (status = 404, description = "Track not found")
    ),
    tag = TAG
)]
pub async fn update_track(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(track_id): Path<String>,
    Json(payload): Json<UpdateTrackRequest>)
     -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    tracing::info!("Updating track: {track_id}");

    // Fetch existing track
    let existing = track_entity::Entity::find_by_id(&track_id)
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("update_track find error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    let Some(track) = existing else {
        return format::json(ApiResponse {
            success: false,
            message: format!("Track {} not found", track_id),
        });
    };

    let ts = now_ts();
    // Capture old_n before moving track into ActiveModel
    let old_n = track.n_value;
    let mut active: track_entity::ActiveModel = track.into();
    active.updated_at = ActiveValue::Set(ts);

    if let Some(v) = payload.name { active.name = ActiveValue::Set(v); }
    if let Some(v) = payload.description { active.description = ActiveValue::Set(Some(v)); }
    if let Some(v) = payload.subject_area { active.subject_area = ActiveValue::Set(Some(v)); }
    if let Some(v) = payload.tag { active.tag = ActiveValue::Set(v); }
    if let Some(v) = payload.target_retention { active.target_retention = ActiveValue::Set(v); }
    if let Some(v) = payload.n_value {
        if v != old_n {
            audit_logger::log_n_change(&ctx.db, "system", None, old_n, v).await;
        }
        active.n_value = ActiveValue::Set(v);
    }

    let updated = active.update(&ctx.db).await.map_err(|e| {
        tracing::error!("update_track save error: {:?}", e);
        loco_rs::Error::InternalServerError
    })?;

    format::json(entity_to_info(&updated))
}

/// Delete a track from the Track Library.
/// Only succeeds if the track is not assigned to any profiles.
#[utoipa::path(
    delete,
    path = "/management/tracks/{track_id}",
    params(("track_id" = String, Path, description = "Track ID (UUID)")),
    responses(
        (status = 200, description = "Track deleted", body = ApiResponse),
        (status = 409, description = "Track still assigned to profiles"),
        (status = 404, description = "Track not found")
    ),
    tag = TAG
)]
pub async fn delete_track(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(track_id): Path<String>)
     -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    tracing::info!("Deleting track: {track_id}");

    // Guard: refuse deletion if track is still assigned to any profile
    let assignment_count = tpt_entity::Entity::find()
        .filter(tpt_entity::Column::TrackId.eq(&track_id))
        .count(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("delete_track count error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    if assignment_count > 0 {
        return format::json(ApiResponse {
            success: false,
            message: format!(
                "Track {} is assigned to {} profile(s). Remove assignments before deleting.",
                track_id, assignment_count
            ),
        });
    }

    let res = track_entity::Entity::delete_by_id(&track_id)
        .exec(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("delete_track DB error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    if res.rows_affected == 0 {
        return format::json(ApiResponse {
            success: false,
            message: format!("Track {} not found", track_id),
        });
    }

    format::json(ApiResponse {
        success: true,
        message: format!("Track {} deleted", track_id),
    })
}