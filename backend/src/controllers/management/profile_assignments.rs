//! Profile Assignments and Background Capsule Generation controllers.

use axum::{
    extract::{Path, State, Extension},
    Json,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use loco_rs::prelude::*;
use crate::middleware::auth::AuthUser;
use crate::middleware::auth::ensure_role;

use crate::{
    models::entities::{
        profile_assignment as pa_entity,
        generation_job as gj_entity,
    },
    services::audit_logger,
    services::capsule_generator_job,
};

use super::TAG;

// ---------------------------------------------------------------------------
// Schemas for API requests & responses
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct CreateAssignmentRequest {
    pub target_type: String,       // "student" or "cohort"
    pub target_id: String,         // user.id or subsection_id
    pub track_profile_id: String,
    pub assigned_by: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AssignmentResponse {
    pub id: i32,
    pub target_type: String,
    pub target_id: String,
    pub track_profile_id: String,
    pub priority: i32,
    pub assigned_by: String,
    pub assigned_at: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GenerationJobResponse {
    pub id: String,
    pub status: String,
    pub progress: i32,
    pub total_students: i32,
    pub completed_students: i32,
    pub anomalies: Option<serde_json::Value>,
    pub completed_at: Option<i64>,
    pub created_at: i64,
}

// ---------------------------------------------------------------------------
// Routes Mapping
// ---------------------------------------------------------------------------

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management")
        .add("/profile-assignments", get(list_assignments))
        .add("/profile-assignments", post(create_assignment))
        .add("/profile-assignments/:id", delete(delete_assignment))
        .add("/capsule-sessions/generate-all", post(generate_all_sessions))
        .add("/generation-jobs/latest", get(latest_generation_job))
}

// ---------------------------------------------------------------------------
// Controllers
// ---------------------------------------------------------------------------

/// List all Profile Assignments.
#[utoipa::path(
    get,
    path = "/api/v1/management/profile-assignments",
    responses(
        (status = 200, description = "Success", body = [AssignmentResponse])
    ),
    tag = TAG
)]
pub async fn list_assignments(State(ctx): State<AppContext>) -> Result<Response> {
    let assignments = pa_entity::Entity::find()
        .order_by_desc(pa_entity::Column::AssignedAt)
        .all(&ctx.db)
        .await?;

    let res: Vec<AssignmentResponse> = assignments
        .into_iter()
        .map(|m| AssignmentResponse {
            id: m.id,
            target_type: m.target_type,
            target_id: m.target_id,
            track_profile_id: m.track_profile_id,
            priority: m.priority,
            assigned_by: m.assigned_by,
            assigned_at: m.assigned_at,
        })
        .collect();

    format::json(res)
}

/// Create or Upsert a Profile Assignment (duplication-conflict upserts).
#[utoipa::path(
    post,
    path = "/api/v1/management/profile-assignments",
    request_body = CreateAssignmentRequest,
    responses(
        (status = 201, description = "Created", body = AssignmentResponse)
    ),
    tag = TAG
)]
pub async fn create_assignment(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Json(payload): Json<CreateAssignmentRequest>,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    // Determine priority
    let priority = if payload.target_type == "student" { 1 } else { 10 };

    // Check if duplicate assignment exists for target
    let existing = pa_entity::Entity::find()
        .filter(pa_entity::Column::TargetType.eq(&payload.target_type))
        .filter(pa_entity::Column::TargetId.eq(&payload.target_id))
        .one(&ctx.db)
        .await?;

    let model = if let Some(m) = existing {
        // Update
        let mut active: pa_entity::ActiveModel = m.into();
        active.track_profile_id = ActiveValue::Set(payload.track_profile_id);
        active.assigned_by = ActiveValue::Set(payload.assigned_by);
        active.assigned_at = ActiveValue::Set(chrono::Utc::now().timestamp());
        active.update(&ctx.db).await?
    } else {
        // Create new
        let active = pa_entity::ActiveModel {
            id: ActiveValue::NotSet,
            target_type: ActiveValue::Set(payload.target_type),
            target_id: ActiveValue::Set(payload.target_id),
            track_profile_id: ActiveValue::Set(payload.track_profile_id),
            priority: ActiveValue::Set(priority),
            assigned_by: ActiveValue::Set(payload.assigned_by),
            assigned_at: ActiveValue::Set(chrono::Utc::now().timestamp()),
        };
        active.insert(&ctx.db).await?
    };

    // Capture audit fields before model is consumed by res
    let audit_actor = model.assigned_by.clone();
    let audit_profile_id = model.track_profile_id.clone();
    let audit_student_id = if model.target_type == "student" {
        model.target_id.clone()
    } else {
        "cohort".to_string()
    };

    // Audit: log profile assignment
    audit_logger::log_profile_assign(
        &ctx.db,
        &audit_actor,
        &audit_student_id,
        &audit_profile_id,
        &audit_profile_id
    )
    .await;

    let res = AssignmentResponse {
        id: model.id,
        target_type: model.target_type,
        target_id: model.target_id,
        track_profile_id: model.track_profile_id,
        priority: model.priority,
        assigned_by: model.assigned_by,
        assigned_at: model.assigned_at,
    };

    format::json(res)
}

/// Delete a Profile Assignment override.
#[utoipa::path(
    delete,
    path = "/api/v1/management/profile-assignments/{id}",
    responses(
        (status = 200, description = "Deleted")
    ),
    params(
        ("id" = i32, Path, description = "Assignment ID")
    ),
    tag = TAG
)]
pub async fn delete_assignment(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    // Fetch assignment before deleting for audit trail
    if let Some(assignment) = pa_entity::Entity::find_by_id(id).one(&ctx.db).await? {
        let student_id = if assignment.target_type == "student" {
            &assignment.target_id
        } else {
            "cohort"
        };
        audit_logger::log_profile_revoke(
            &ctx.db,
            "system",
            student_id,
            &assignment.track_profile_id,
            &assignment.track_profile_id
        )
        .await;
    }

    pa_entity::Entity::delete_by_id(id).exec(&ctx.db).await?;
    format::json(serde_json::json!({ "status": "deleted" }))
}

/// Trigger sequential batch-generation of capsules for all active students (Background mode).
#[utoipa::path(
    post,
    path = "/api/v1/management/capsule-sessions/generate-all",
    responses(
        (status = 202, description = "Batch Generation Triggered", body = GenerationJobResponse)
    ),
    tag = TAG
)]
pub async fn generate_all_sessions(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    let job_id = Uuid::new_v4().to_string();

    // Persist a pending job
    let active_job = gj_entity::ActiveModel {
        id: ActiveValue::Set(job_id.clone()),
        status: ActiveValue::Set("pending".to_string()),
        progress: ActiveValue::Set(0),
        total_students: ActiveValue::Set(0),
        completed_students: ActiveValue::Set(0),
        anomalies: ActiveValue::NotSet,
        completed_at: ActiveValue::NotSet,
        created_at: ActiveValue::Set(chrono::Utc::now().timestamp()),
    };
    active_job.insert(&ctx.db).await?;

    // Spawn sequential worker in background
    let db_clone = ctx.db.clone();
    let job_id_clone = job_id.clone();
    
    // Resolve playground base directory path
    let playground_dir = std::env::var("ANKIPLAYGROUND_PATH")
        .unwrap_or_else(|_| "/Volumes/YBG1TB4Mac/AnkiTov/AnkiPlayGround".to_string());

    tokio::spawn(async move {
        let _ = capsule_generator_job::run_batch_generation(&db_clone, &job_id_clone, &playground_dir).await;
    });

    // Emits 202 Accepted status check instantly
    let res = GenerationJobResponse {
        id: job_id,
        status: "pending".to_string(),
        progress: 0,
        total_students: 0,
        completed_students: 0,
        anomalies: None,
        completed_at: None,
        created_at: chrono::Utc::now().timestamp(),
    };

    format::json(res)
}

/// Poll the latest Background Generation Job.
#[utoipa::path(
    get,
    path = "/api/v1/management/generation-jobs/latest",
    responses(
        (status = 200, description = "Success", body = GenerationJobResponse)
    ),
    tag = TAG
)]
pub async fn latest_generation_job(State(ctx): State<AppContext>) -> Result<Response> {
    let latest = gj_entity::Entity::find()
        .order_by_desc(gj_entity::Column::CreatedAt)
        .one(&ctx.db)
        .await?;

    let res = match latest {
        Some(m) => {
            let anomalies_json: Option<serde_json::Value> = m.anomalies
                .and_then(|s| serde_json::from_str(&s).ok());

            Some(GenerationJobResponse {
                id: m.id,
                status: m.status,
                progress: m.progress,
                total_students: m.total_students,
                completed_students: m.completed_students,
                anomalies: anomalies_json,
                completed_at: m.completed_at,
                created_at: m.created_at,
            })
        }
        None => None,
    };

    format::json(res)
}