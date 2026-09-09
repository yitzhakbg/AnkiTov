//! Track Profile controller — CRUD endpoints for track profiles and assignments.
//!
//! Instructors create profiles (e.g., "Math Rescue Mix") that blend multiple
//! tracks into a single remediation capsule. Profiles can override target
//! retention and N on a per-cohort or per-student basis.

use crate::models::entities::track as track_entity;
use crate::models::entities::track_profile as tp_entity;
use crate::models::entities::track_profile_track as tpt_entity;
use crate::models::management::{
    ApiResponse, AssignTrackRequest, AssignedTrackEntry, CreateTrackProfileRequest,
    TrackProfileDetail, TrackProfileInfo, UpdateTrackProfileRequest,
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
        .prefix("/management/track-profiles")
        .add("/", get(list_profiles))
        .add("/", post(create_profile))
        .add("/:profile_id", get(get_profile))
        .add("/:profile_id", put(update_profile))
        .add("/:profile_id", delete(delete_profile))
        .add("/:profile_id/tracks", post(assign_track))
        .add("/:profile_id/tracks/:track_id", delete(unassign_track))
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

fn entity_to_info(p: &tp_entity::Model, track_count: u32) -> TrackProfileInfo {
    TrackProfileInfo {
        id: p.id.clone(),
        name: p.name.clone(),
        description: p.description.clone(),
        target_retention: p.target_retention,
        n_value: p.n_value,
        session_duration_minutes: p.session_duration_minutes,
        track_count,
        created_at: p.created_at,
        updated_at: p.updated_at,
    }
}

async fn fetch_profile_detail(
    db: &impl sea_orm::ConnectionTrait,
    profile: &tp_entity::Model
) -> Result<TrackProfileDetail, loco_rs::Error> {
    let junctions = tpt_entity::Entity::find()
        .filter(tpt_entity::Column::TrackProfileId.eq(&profile.id))
        .order_by_asc(tpt_entity::Column::SortOrder)
        .all(db)
        .await
        .map_err(|e| {
            tracing::error!("fetch_profile_detail junctions error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    let track_ids: Vec<String> = junctions.iter().map(|j| j.track_id.clone()).collect();
    let tracks = if track_ids.is_empty() {
        Vec::new()
    } else {
        track_entity::Entity::find()
            .filter(track_entity::Column::Id.is_in(track_ids.clone()))
            .all(db)
            .await
            .map_err(|e| {
                tracing::error!("fetch_profile_detail tracks error: {:?}", e);
                loco_rs::Error::InternalServerError
            })?
    };

    let track_map: std::collections::HashMap<String, &track_entity::Model> =
        tracks.iter().map(|t| (t.id.clone(), t)).collect();

    let assigned_tracks: Vec<AssignedTrackEntry> = junctions
        .iter()
        .filter_map(|j| {
            track_map.get(&j.track_id).map(|t| AssignedTrackEntry {
                track_id: j.track_id.clone(),
                track_name: t.name.clone(),
                subject_area: t.subject_area.clone(),
                sort_order: j.sort_order,
            })
        })
        .collect();

    Ok(TrackProfileDetail {
        id: profile.id.clone(),
        name: profile.name.clone(),
        description: profile.description.clone(),
        target_retention: profile.target_retention,
        n_value: profile.n_value,
        session_duration_minutes: profile.session_duration_minutes,
        tracks: assigned_tracks,
        created_at: profile.created_at,
        updated_at: profile.updated_at,
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// List all track profiles with assignment counts.
#[utoipa::path(
    get,
    path = "/management/track-profiles",
    responses(
        (status = 200, description = "List of track profiles", body = Vec<TrackProfileInfo>)
    ),
    tag = TAG
)]
pub async fn list_profiles(State(ctx): State<AppContext>) -> Result<Response> {
    tracing::info!("Listing all track profiles");
    let profiles = tp_entity::Entity::find()
        .order_by_asc(tp_entity::Column::Name)
        .all(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("list_profiles DB error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    // Count tracks per profile in one pass
    let mut infos = Vec::new();
    for p in &profiles {
        let count = tpt_entity::Entity::find()
            .filter(tpt_entity::Column::TrackProfileId.eq(&p.id))
            .count(&ctx.db)
            .await
            .map_err(|e| {
                tracing::error!("list_profiles count error: {:?}", e);
                loco_rs::Error::InternalServerError
            })?;
        infos.push(entity_to_info(p, count as u32));
    }
    format::json(infos)
}

/// Get a single track profile with full track assignments.
#[utoipa::path(
    get,
    path = "/management/track-profiles/{profile_id}",
    params(("profile_id" = String, Path, description = "Profile ID (UUID)")),
    responses(
        (status = 200, description = "Profile detail with track assignments", body = TrackProfileDetail),
        (status = 404, description = "Profile not found")
    ),
    tag = TAG
)]
pub async fn get_profile(
    State(ctx): State<AppContext>,
    Path(profile_id): Path<String>
) -> Result<Response> {
    tracing::info!("Fetching profile: {profile_id}");
    let profile = tp_entity::Entity::find_by_id(&profile_id)
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("get_profile DB error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    let Some(p) = profile else {
        return format::json(ApiResponse {
            success: false,
            message: format!("Profile {} not found", profile_id),
        });
    };

    let detail = fetch_profile_detail(&ctx.db, &p).await?;
    format::json(detail)
}

/// Create a new track profile.
#[utoipa::path(
    post,
    path = "/management/track-profiles",
    request_body = CreateTrackProfileRequest,
    responses(
        (status = 201, description = "Profile created", body = TrackProfileDetail),
        (status = 400, description = "Invalid input")
    ),
    tag = TAG
)]
pub async fn create_profile(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Json(payload): Json<CreateTrackProfileRequest>)
     -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    tracing::info!("Creating track profile: {}", payload.name);
    let ts = now_ts();
    let id = uuid::Uuid::new_v4().to_string();

    let profile = tp_entity::ActiveModel {
        id: ActiveValue::Set(id),
        name: ActiveValue::Set(payload.name),
        description: ActiveValue::Set(payload.description),
        target_retention: ActiveValue::Set(payload.target_retention),
        n_value: ActiveValue::Set(payload.n_value),
        session_duration_minutes: ActiveValue::Set(payload.session_duration_minutes),
        created_at: ActiveValue::Set(ts),
        updated_at: ActiveValue::Set(ts),
    }
    .insert(&ctx.db)
    .await
    .map_err(|e| {
        tracing::error!("create_profile DB error: {:?}", e);
        loco_rs::Error::InternalServerError
    })?;

    tracing::info!("Profile created: id={}, name={}", profile.id, profile.name);

    // Audit: initial N-value
    if let Some(n) = profile.n_value {
        audit_logger::log_n_change(
            &ctx.db,
            "system",
            None,
            0,  // old_n = 0 (no previous value)
            n
        )
        .await;
    }

    let detail = fetch_profile_detail(&ctx.db, &profile).await?;
    format::json(detail)
}

/// Update a track profile.
#[utoipa::path(
    put,
    path = "/management/track-profiles/{profile_id}",
    params(("profile_id" = String, Path, description = "Profile ID (UUID)")),
    request_body = UpdateTrackProfileRequest,
    responses(
        (status = 200, description = "Profile updated", body = TrackProfileDetail),
        (status = 404, description = "Profile not found")
    ),
    tag = TAG
)]
pub async fn update_profile(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(profile_id): Path<String>,
    Json(payload): Json<UpdateTrackProfileRequest>)
     -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    tracing::info!("Updating profile: {profile_id}");

    let existing = tp_entity::Entity::find_by_id(&profile_id)
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("update_profile find error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    let Some(profile) = existing else {
        return format::json(ApiResponse {
            success: false,
            message: format!("Profile {} not found", profile_id),
        });
    };

    let ts = now_ts();
    // Capture old_n before moving profile into ActiveModel
    let old_n = profile.n_value.unwrap_or(0);
    let mut active: tp_entity::ActiveModel = profile.into();
    active.updated_at = ActiveValue::Set(ts);

    if let Some(v) = payload.name { active.name = ActiveValue::Set(v); }
    if let Some(v) = payload.description { active.description = ActiveValue::Set(Some(v)); }
    if let Some(v) = payload.target_retention { active.target_retention = ActiveValue::Set(Some(v)); }
    if let Some(v) = payload.n_value {
        if v != old_n {
            audit_logger::log_n_change(&ctx.db, "system", None, old_n, v).await;
        }
        active.n_value = ActiveValue::Set(Some(v));
    }
    if let Some(v) = payload.session_duration_minutes { active.session_duration_minutes = ActiveValue::Set(v); }

    let updated = active.update(&ctx.db).await.map_err(|e| {
        tracing::error!("update_profile save error: {:?}", e);
        loco_rs::Error::InternalServerError
    })?;

    let detail = fetch_profile_detail(&ctx.db, &updated).await?;
    format::json(detail)
}

/// Delete a track profile (also removes all track assignments via cascade).
#[utoipa::path(
    delete,
    path = "/management/track-profiles/{profile_id}",
    params(("profile_id" = String, Path, description = "Profile ID (UUID)")),
    responses(
        (status = 200, description = "Profile deleted", body = ApiResponse),
        (status = 404, description = "Profile not found")
    ),
    tag = TAG
)]
pub async fn delete_profile(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(profile_id): Path<String>)
     -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    tracing::info!("Deleting profile: {profile_id}");
    let res = tp_entity::Entity::delete_by_id(&profile_id)
        .exec(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("delete_profile DB error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    if res.rows_affected == 0 {
        return format::json(ApiResponse {
            success: false,
            message: format!("Profile {} not found", profile_id),
        });
    }

    format::json(ApiResponse {
        success: true,
        message: format!("Profile {} deleted", profile_id),
    })
}

/// Assign a track to a profile (or update sort order if already assigned).
#[utoipa::path(
    post,
    path = "/management/track-profiles/{profile_id}/tracks",
    params(("profile_id" = String, Path, description = "Profile ID (UUID)")),
    request_body = AssignTrackRequest,
    responses(
        (status = 201, description = "Track assigned to profile"),
        (status = 404, description = "Profile or track not found")
    ),
    tag = TAG
)]
pub async fn assign_track(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(profile_id): Path<String>,
    Json(payload): Json<AssignTrackRequest>)
     -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    tracing::info!("Assigning track {} to profile {}", payload.track_id, profile_id);

    // Verify profile exists
    let profile = tp_entity::Entity::find_by_id(&profile_id)
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("assign_track find profile error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;
    if profile.is_none() {
        return format::json(ApiResponse {
            success: false,
            message: format!("Profile {} not found", profile_id),
        });
    }

    // Verify track exists
    let track = track_entity::Entity::find_by_id(&payload.track_id)
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("assign_track find track error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;
    if track.is_none() {
        return format::json(ApiResponse {
            success: false,
            message: format!("Track {} not found", payload.track_id),
        });
    }

    // Upsert: check if assignment already exists
    let existing = tpt_entity::Entity::find()
        .filter(tpt_entity::Column::TrackProfileId.eq(&profile_id))
        .filter(tpt_entity::Column::TrackId.eq(&payload.track_id))
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("assign_track find junction error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    if let Some(junction) = existing {
        // Update sort order only
        let mut active: tpt_entity::ActiveModel = junction.into();
        active.sort_order = ActiveValue::Set(payload.sort_order);
        active.update(&ctx.db).await.map_err(|e| {
            tracing::error!("assign_track update junction error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;
    } else {
        // Insert new junction
        tpt_entity::ActiveModel {
            id: ActiveValue::NotSet,
            track_profile_id: ActiveValue::Set(profile_id.clone()),
            track_id: ActiveValue::Set(payload.track_id.clone()),
            sort_order: ActiveValue::Set(payload.sort_order),
        }
        .insert(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("assign_track insert junction error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;
    }

    tracing::info!("Track {} assigned to profile {}", payload.track_id, profile_id);
    format::json(ApiResponse {
        success: true,
        message: format!(
            "Track {} assigned to profile {} (sort_order={})",
            payload.track_id, profile_id, payload.sort_order
        ),
    })
}

/// Remove a track assignment from a profile.
#[utoipa::path(
    delete,
    path = "/management/track-profiles/{profile_id}/tracks/{track_id}",
    params(
        ("profile_id" = String, Path, description = "Profile ID (UUID)"),
        ("track_id" = String, Path, description = "Track ID (UUID)")
    ),
    responses(
        (status = 200, description = "Track unassigned from profile"),
        (status = 404, description = "Assignment not found")
    ),
    tag = TAG
)]
pub async fn unassign_track(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path((profile_id, track_id)): Path<(String, String)>)
     -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    tracing::info!("Unassigning track {} from profile {}", track_id, profile_id);

    let res = tpt_entity::Entity::delete_many()
        .filter(tpt_entity::Column::TrackProfileId.eq(&profile_id))
        .filter(tpt_entity::Column::TrackId.eq(&track_id))
        .exec(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("unassign_track DB error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    if res.rows_affected == 0 {
        return format::json(ApiResponse {
            success: false,
            message: format!("No assignment found: profile={profile_id}, track={track_id}"),
        });
    }

    format::json(ApiResponse {
        success: true,
        message: format!("Track {} removed from profile {}", track_id, profile_id),
    })
}