//! Capsule Session controller — listing, inspection, and generation endpoints.
//!
//! Capsule sessions are **generated server-side** by the Capsule Generation Service
//! (Phase 3). This controller provides:
//! - **POST /management/capsule-sessions/generate** — trigger capsule generation
//! - **GET /management/capsule-sessions** — list sessions with filters
//! - **GET /management/capsule-sessions/{session_id}** — inspect a single session

use crate::models::entities::capsule_session as cs_entity;
use crate::models::entities::track_profile as tp_entity;
use crate::models::management::{
    ApiResponse, CapsuleSessionInfo, CapsuleSessionQuery, GenerateCapsuleRequest,
};
use crate::services::capsule_generator::{self, GenerateRequest};
use crate::services::capsule_delivery::CapsuleDelivery;
use axum::extract::{Path, Query, State, Json, Extension};
use loco_rs::{app::AppContext, prelude::*};
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait, PaginatorTrait, QueryOrder};
use crate::middleware::auth::AuthUser;
use crate::middleware::auth::ensure_role;

const TAG: &str = "Management Console";

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management/capsule-sessions")
        .add("/generate", post(generate_session))
        .add("/", get(list_sessions))
        .add("/:session_id", get(get_session))
        .add("/:session_id/deliver", post(deliver_session))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn entity_to_info(s: &cs_entity::Model) -> CapsuleSessionInfo {
    CapsuleSessionInfo {
        id: s.id.clone(),
        student_id: s.student_id.clone(),
        track_profile_id: s.track_profile_id.clone(),
        n_value: s.n_value,
        session_duration_minutes: s.session_duration_minutes,
        card_ids: s.card_ids.clone(),
        status: s.status.clone(),
        session_week: s.session_week.clone(),
        cards_completed: s.cards_completed,
        cards_total: s.cards_total,
        created_at: s.created_at,
        completed_at: s.completed_at,
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// List capsule sessions with optional filters (student, status, week, pagination).
#[utoipa::path(
    get,
    path = "/management/capsule-sessions",
    params(
        ("student_id" = Option<String>, Query, description = "Filter by student"),
        ("status" = Option<String>, Query, description = "Filter: pending | in_progress | completed | expired"),
        ("session_week" = Option<String>, Query, description = "ISO week (e.g., 2026-W27)"),
        ("page" = Option<u32>, Query, description = "Page number"),
        ("per_page" = Option<u32>, Query, description = "Items per page")
    ),
    responses(
        (status = 200, description = "Paginated list of capsule sessions", body = Vec<CapsuleSessionInfo>)
    ),
    tag = TAG
)]
pub async fn list_sessions(
    State(ctx): State<AppContext>,
    Query(params): Query<CapsuleSessionQuery>
) -> Result<Response> {
    tracing::info!(
        "Listing capsule sessions — student={:?}, status={:?}, week={:?}",
        params.student_id, params.status, params.session_week
    );

    let mut query = cs_entity::Entity::find();

    if let Some(ref sid) = params.student_id {
        query = query.filter(cs_entity::Column::StudentId.eq(sid.clone()));
    }
    if let Some(ref status) = params.status {
        query = query.filter(cs_entity::Column::Status.eq(status.clone()));
    }
    if let Some(ref week) = params.session_week {
        query = query.filter(cs_entity::Column::SessionWeek.eq(week.clone()));
    }

    query = query.order_by_desc(cs_entity::Column::CreatedAt);

    let page = params.page.max(1);
    let per_page = params.per_page.max(1);

    let paginator = query.paginate(&ctx.db, per_page as u64);
    let sessions = paginator
        .fetch_page(page.saturating_sub(1) as u64)
        .await
        .map_err(|e| {
            tracing::error!("list_sessions DB error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    let infos: Vec<CapsuleSessionInfo> = sessions.iter().map(entity_to_info).collect();
    format::json(infos)
}

/// Get a single capsule session by ID.
#[utoipa::path(
    get,
    path = "/management/capsule-sessions/{session_id}",
    params(("session_id" = String, Path, description = "Session ID (UUID)")),
    responses(
        (status = 200, description = "Capsule session details", body = CapsuleSessionInfo),
        (status = 404, description = "Session not found")
    ),
    tag = TAG
)]
pub async fn get_session(
    State(ctx): State<AppContext>,
    Path(session_id): Path<String>
) -> Result<Response> {
    tracing::info!("Fetching capsule session: {session_id}");

    let session = cs_entity::Entity::find_by_id(&session_id)
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("get_session DB error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    match session {
        Some(s) => format::json(entity_to_info(&s)),
        None => format::json(ApiResponse {
            success: false,
            message: format!("Capsule session {} not found", session_id),
        }),
    }
}

/// Generate a new remediation capsule for a student.
///
/// This triggers the full Interleaved Mastery Pipeline:
/// resolve profile → query due cards → FSRS sort → slice → persist.
#[utoipa::path(
    post,
    path = "/management/capsule-sessions/generate",
    request_body = GenerateCapsuleRequest,
    responses(
        (status = 201, description = "Capsule generated successfully"),
        (status = 404, description = "No profile or no due cards"),
        (status = 429, description = "Rate limited (already generated recently)")
    ),
    tag = TAG
)]
pub async fn generate_session(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Json(payload): Json<GenerateCapsuleRequest>,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    tracing::info!(
        "Generating capsule for student {} (week: {})",
        payload.student_id,
        payload.session_week
    );

    let request = GenerateRequest {
        student_id: payload.student_id,
        anki2_path: payload.anki2_path,
        track_profile_id: payload.track_profile_id,
        session_week: payload.session_week,
        n_value: payload.n_value,
        session_duration_minutes: payload.session_duration_minutes,
    };

    match capsule_generator::generate_capsule(&ctx.db, request).await {
        Ok(response) => {
            tracing::info!(
                "Capsule generated: session={}, cards={}",
                response.session_id,
                response.capsule_size
            );
            format::json(response)
        }
        Err(e) => {
            tracing::error!("Capsule generation failed: {:?}", e);
            let message = match &e {
                capsule_generator::GeneratorError::NoProfileForStudent(id) => {
                    format!("No track profile assigned for student {}", id)
                }
                capsule_generator::GeneratorError::NoDueCards => {
                    "No due cards found for the assigned tracks".to_string()
                }
                capsule_generator::GeneratorError::RateLimited(id) => {
                    format!("Student {} already had a session generated this week", id)
                }
                _ => format!("Capsule generation failed: {}", e),
            };
            format::json(ApiResponse {
                success: false,
                message,
            })
        }
    }
}

/// Deliver a generated capsule to the AnkiTov Session Driver addon.
///
/// Takes an existing capsule session (status: pending), sends the card IDs
/// to the addon's `/session` endpoint, and updates the session status to
/// `in_progress`.
///
/// The addon will create a filtered deck, tag the capsule cards, and open
/// Anki's reviewer so the student can begin their interleaved session.
#[utoipa::path(
    post,
    path = "/management/capsule-sessions/{session_id}/deliver",
    params(("session_id" = String, Path, description = "Session ID (UUID)")),
    responses(
        (status = 200, description = "Capsule delivered to addon"),
        (status = 404, description = "Session not found"),
        (status = 409, description = "Session already delivered"),
        (status = 502, description = "Addon unreachable")
    ),
    tag = TAG
)]
pub async fn deliver_session(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(session_id): Path<String>,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    let db = &ctx.db;

    // Find the capsule session
    let session = cs_entity::Entity::find_by_id(&session_id)
        .one(db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?
        .ok_or(loco_rs::Error::NotFound)?;

    // Only "pending" sessions can be delivered
    if session.status != "pending" {
        return format::json(ApiResponse {
            success: false,
            message: format!(
                "session {} is already {} — cannot deliver",
                session_id, session.status
            ),
        });
    }

    // Parse card IDs from JSON
    let card_ids: Vec<i64> = serde_json::from_str(&session.card_ids)
        .map_err(|e| {
            tracing::error!("Failed to parse card_ids for session {}: {}", session_id, e);
            loco_rs::Error::InternalServerError
        })?;

    // Resolve track profile name for the addon
    let profile_name = tp_entity::Entity::find_by_id(&session.track_profile_id)
        .one(db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?
        .map(|p| p.name)
        .unwrap_or_else(|| "Unknown".to_string());

    let capsule_size = card_ids.len();

    // Deliver to addon
    let delivery = CapsuleDelivery::from_env();
    match delivery
        .start_session(&session_id, &card_ids, capsule_size, &profile_name)
        .await
    {
        Ok(response) => {
            tracing::info!(
                "Capsule delivered: session={}, status={}, cards={}",
                session_id,
                response.status,
                capsule_size
            );

            // Update session status to in_progress
            let mut active: cs_entity::ActiveModel = session.into();
            active.status = ActiveValue::Set("in_progress".to_string());
            active
                .update(db)
                .await
                .map_err(|_| loco_rs::Error::InternalServerError)?;

            format::json(serde_json::json!({
                "status": "delivered",
                "session_id": session_id,
                "addon_response": response,
                "capsule_size": capsule_size,
            }))
        }
        Err(e) => {
            tracing::error!("Delivery failed for session {}: {}", session_id, e);
            format::json(ApiResponse {
                success: false,
                message: format!("Addon delivery failed: {}", e),
            })
        }
    }
}