//! Class management endpoints.
//!
//! Provides CRUD for classes (instructional groups) and enrollment management.
//! Classes are the primary organizational unit — decks, profiles, and capsules
//! can be distributed to classes rather than individual students.

use axum::extract::{Path, Query, State};
use axum::{Extension, Json};
use loco_rs::{app::AppContext, prelude::*};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use crate::middleware::auth::AuthUser;
use crate::middleware::auth::ensure_role;

use crate::models::entities::{class, class_enrollment};

/// Characters used for class codes (no ambiguous 0/O, 1/I/L).
const CODE_CHARS: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
const CODE_LENGTH: usize = 6;

const TAG: &str = "Management Console";

// ── Request / Response types ──

/// Query params for class listing.
#[derive(Debug, Deserialize)]
pub struct ClassQuery {
    pub q: Option<String>,
}

/// Request body for creating a class.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateClassRequest {
    pub name: String,
    pub subject_area: Option<String>,
    pub teacher_id: String,
    pub period: Option<String>,
    pub academic_year: Option<String>,
}

/// Generate a random 6-character class code.
fn generate_class_code() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..CODE_LENGTH)
        .map(|_| {
            let idx = rng.gen_range(0..CODE_CHARS.len());
            CODE_CHARS[idx] as char
        })
        .collect()
}

/// Summary row returned in class list responses.
#[derive(Debug, Serialize, ToSchema)]
pub struct ClassSummary {
    pub id: String,
    pub name: String,
    pub class_code: Option<String>,
    pub subject_area: Option<String>,
    pub teacher_id: String,
    pub period: Option<String>,
    pub academic_year: Option<String>,
    pub student_count: u64,
    pub created_at: i64,
}

/// Request body for enrolling students in a class.
#[derive(Debug, Deserialize, ToSchema)]
pub struct EnrollStudentsRequest {
    pub student_ids: Vec<String>,
}

/// Request body for generating sessions for a class.
#[derive(Debug, Deserialize, ToSchema)]
pub struct GenerateForClassRequest {
    pub track_profile_id: String,
    pub n_value: Option<i32>,
}

// ── Routes ──

/// Routes for class management.
pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management/classes")
        .add("/", get(list))
        .add("/", post(create))
        .add("/:id", put(update))
        .add("/:id", delete(delete_class))
        .add("/:id/students", get(list_students))
        .add("/:id/students", post(enroll_students))
        .add("/:id/students/:student_id", delete(transfer_student))
        .add("/:id/generate-sessions", post(generate_sessions))
        .add("/code/:code", get(lookup_by_code))
        .add("/code/:code/enroll", post(enroll_by_code))
}

// ── Handlers ──

/// List all classes with student counts.
#[utoipa::path(
    get,
    path = "/management/classes",
    responses(
        (status = 200, description = "List of classes", body = Vec<ClassSummary>)
    ),
    tag = TAG
)]
pub async fn list(
    State(ctx): State<AppContext>,
    Query(params): Query<ClassQuery>
) -> Result<Response> {
    let db = &ctx.db;

    let mut find = class::Entity::find();

    // Filter by search query if provided (LIKE match on name)
    if let Some(q) = &params.q {
        if !q.trim().is_empty() {
            find = find.filter(class::Column::Name.contains(q.trim()));
        }
    }

    let classes = find.all(db).await.map_err(|e| {
        tracing::error!("Failed to list classes: {:?}", e);
        loco_rs::Error::InternalServerError
    })?;

    let mut summaries = Vec::new();
    for c in classes {
        let student_count = class_enrollment::Entity::find()
            .filter(class_enrollment::Column::ClassId.eq(&c.id))
            .filter(class_enrollment::Column::Active.eq(true))
            .count(db)
            .await
            .unwrap_or(0);

        summaries.push(ClassSummary {
            id: c.id,
            name: c.name,
            class_code: c.class_code,
            subject_area: c.subject_area,
            teacher_id: c.teacher_id,
            period: c.period,
            academic_year: c.academic_year,
            student_count,
            created_at: c.created_at,
        });
    }

    format::json(summaries)
}

/// Create a new class.
#[utoipa::path(
    post,
    path = "/management/classes",
    request_body = CreateClassRequest,
    responses(
        (status = 201, description = "Class created", body = ClassSummary)
    ),
    tag = TAG
)]
pub async fn create(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Json(payload): Json<CreateClassRequest>)
     -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    let db = &ctx.db;
    let now = chrono::Utc::now().timestamp();

    let id = uuid::Uuid::new_v4().to_string();
    let class_code = generate_class_code();

    // Clone fields before moving into ActiveModel
    let name = payload.name.clone();
    let teacher_id = payload.teacher_id.clone();
    let subject_area = payload.subject_area.clone();
    let period = payload.period.clone();
    let academic_year = payload.academic_year.clone();

    let active_model = class::ActiveModel {
        id: sea_orm::ActiveValue::Set(id.clone()),
        name: sea_orm::ActiveValue::Set(payload.name),
        class_code: sea_orm::ActiveValue::Set(Some(class_code.clone())),
        subject_area: sea_orm::ActiveValue::Set(payload.subject_area),
        teacher_id: sea_orm::ActiveValue::Set(payload.teacher_id),
        period: sea_orm::ActiveValue::Set(payload.period),
        academic_year: sea_orm::ActiveValue::Set(payload.academic_year),
        created_at: sea_orm::ActiveValue::Set(now),
        updated_at: sea_orm::ActiveValue::Set(now),
    };

    class::Entity::insert(active_model)
        .exec(db)
        .await
        .map_err(|e| {
            tracing::error!("Failed to create class: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    let summary = ClassSummary {
        id,
        name,
        class_code: Some(class_code),
        subject_area,
        teacher_id,
        period,
        academic_year,
        student_count: 0,
        created_at: now,
    };

    format::json(summary)
}

/// Update class metadata.
#[utoipa::path(
    put,
    path = "/management/classes/{id}",
    request_body = CreateClassRequest,
    responses((status = 200, description = "Class updated")),
    tag = TAG
)]
pub async fn update(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(id): Path<String>,
    Json(payload): Json<CreateClassRequest>)
     -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    let db = &ctx.db;
    let now = chrono::Utc::now().timestamp();

    let mut active: class::ActiveModel = class::Entity::find_by_id(&id)
        .one(db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?
        .ok_or(loco_rs::Error::NotFound)?
        .into();

    active.name = sea_orm::ActiveValue::Set(payload.name);
    active.subject_area = sea_orm::ActiveValue::Set(payload.subject_area);
    active.teacher_id = sea_orm::ActiveValue::Set(payload.teacher_id);
    active.period = sea_orm::ActiveValue::Set(payload.period);
    active.academic_year = sea_orm::ActiveValue::Set(payload.academic_year);
    active.updated_at = sea_orm::ActiveValue::Set(now);

    class::Entity::update(active).exec(db).await.map_err(|e| {
        tracing::error!("Failed to update class: {:?}", e);
        loco_rs::Error::InternalServerError
    })?;

    format::json(serde_json::json!({"status": "ok"}))
}

/// Hard-delete a class and all its enrollments (cascading).
#[utoipa::path(
    delete,
    path = "/management/classes/{id}",
    responses((status = 200, description = "Class deleted")),
    tag = TAG
)]
pub async fn delete_class(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(id): Path<String>)
     -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    let db = &ctx.db;

    // Delete the class — enrollments cascade via FK ON DELETE CASCADE
    class::Entity::delete_by_id(&id)
        .exec(db)
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete class {}: {:?}", id, e);
            loco_rs::Error::InternalServerError
        })?;

    format::json(serde_json::json!({"status": "deleted", "id": id}))
}

/// List enrolled students for a class.
#[utoipa::path(
    get,
    path = "/management/classes/{id}/students",
    responses((status = 200, description = "Student roster")),
    tag = TAG
)]
pub async fn list_students(
    State(ctx): State<AppContext>,
    Path(id): Path<String>
) -> Result<Response> {
    let db = &ctx.db;

    let enrollments = class_enrollment::Entity::find()
        .filter(class_enrollment::Column::ClassId.eq(&id))
        .filter(class_enrollment::Column::Active.eq(true))
        .all(db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?;

    let students: Vec<serde_json::Value> = enrollments
        .into_iter()
        .map(|e| serde_json::json!({
            "student_id": e.student_id,
            "enrolled_at": e.enrolled_at,
            "active": e.active
        }))
        .collect();

    format::json(students)
}

/// Enroll students in a class.
#[utoipa::path(
    post,
    path = "/management/classes/{id}/students",
    request_body = EnrollStudentsRequest,
    responses((status = 200, description = "Students enrolled")),
    tag = TAG
)]
pub async fn enroll_students(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(id): Path<String>,
    Json(payload): Json<EnrollStudentsRequest>)
     -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    let db = &ctx.db;
    let now = chrono::Utc::now().timestamp();
    let mut count = 0;

    for student_id in &payload.student_ids {
        let active_model = class_enrollment::ActiveModel {
            id: sea_orm::ActiveValue::NotSet,
            class_id: sea_orm::ActiveValue::Set(id.clone()),
            student_id: sea_orm::ActiveValue::Set(student_id.clone()),
            enrolled_at: sea_orm::ActiveValue::Set(now),
            active: sea_orm::ActiveValue::Set(true),
            display_name: sea_orm::ActiveValue::Set(None),
            metadata_json: sea_orm::ActiveValue::Set(None),
        };

        match class_enrollment::Entity::insert(active_model).exec(db).await {
            Ok(_) => count += 1,
            Err(e) => tracing::warn!("Failed to enroll student {}: {:?}", student_id, e),
        }
    }

    format::json(serde_json::json!({"enrolled": count, "total": payload.student_ids.len()}))
}

/// Transfer a student out of a class (soft-delete).
#[utoipa::path(
    delete,
    path = "/management/classes/{id}/students/{student_id}",
    responses((status = 200, description = "Student transferred out")),
    tag = TAG
)]
pub async fn transfer_student(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path((id, student_id)): Path<(String, String)>)
     -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    let db = &ctx.db;

    let enrollment = class_enrollment::Entity::find()
        .filter(class_enrollment::Column::ClassId.eq(&id))
        .filter(class_enrollment::Column::StudentId.eq(&student_id))
        .filter(class_enrollment::Column::Active.eq(true))
        .one(db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?
        .ok_or(loco_rs::Error::NotFound)?;

    let enrollment_id = enrollment.id;
    let mut active: class_enrollment::ActiveModel = enrollment.into();
    active.active = sea_orm::ActiveValue::Set(false);
    if let Err(e) = active.update(db).await {
        if e.to_string().contains("UNIQUE constraint failed") {
            // Unique constraint failed because a matching historical inactive record already exists!
            // We can safely hard-delete this duplicate active record instead.
            class_enrollment::Entity::delete_by_id(enrollment_id).exec(db).await.map_err(|delete_err| {
                tracing::error!("Failed to fallback delete student enrollment: {:?}", delete_err);
                loco_rs::Error::InternalServerError
            })?;
        } else {
            tracing::error!("Failed to transfer student: {:?}", e);
            return Err(loco_rs::Error::InternalServerError);
        }
    }

    format::json(serde_json::json!({"status": "transferred"}))
}

/// Generate study sessions for all students in a class.
#[utoipa::path(
    post,
    path = "/management/classes/{id}/generate-sessions",
    request_body = GenerateForClassRequest,
    responses((status = 200, description = "Batch generation started")),
    tag = TAG
)]
pub async fn generate_sessions(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(id): Path<String>,
    Json(payload): Json<GenerateForClassRequest>)
     -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    let db = &ctx.db;

    // Get active student IDs for this class
    let enrollments = class_enrollment::Entity::find()
        .filter(class_enrollment::Column::ClassId.eq(&id))
        .filter(class_enrollment::Column::Active.eq(true))
        .all(db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?;

    // For now, return the count — actual generation happens via the existing
    // generate_all_sessions endpoint in profile_assignments
    let student_count = enrollments.len();

    format::json(serde_json::json!({
        "status": "queued",
        "class_id": id,
        "student_count": student_count,
        "track_profile_id": payload.track_profile_id,
        "n_value": payload.n_value.unwrap_or(3)
    }))
}

// ── Class-code based endpoints ──

/// Lookup a class by its 6-character enrollment code.
///
/// Returns class metadata (name, teacher, period) without requiring
/// authentication — students use this to verify they're joining the right class.
#[utoipa::path(
    get,
    path = "/management/classes/code/{code}",
    responses(
        (status = 200, description = "Class found"),
        (status = 404, description = "Code not found")
    ),
    tag = TAG
)]
pub async fn lookup_by_code(
    State(ctx): State<AppContext>,
    Path(code): Path<String>
) -> Result<Response> {
    let db = &ctx.db;

    let c = class::Entity::find()
        .filter(class::Column::ClassCode.eq(&code))
        .one(db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?
        .ok_or(loco_rs::Error::NotFound)?;

    format::json(serde_json::json!({
        "id": c.id,
        "name": c.name,
        "class_code": c.class_code,
        "subject_area": c.subject_area,
        "teacher_id": c.teacher_id,
        "period": c.period,
        "academic_year": c.academic_year,
    }))
}

/// Self-enroll in a class using its 6-character code.
///
/// Requires a valid JWT in the Authorization header. The authenticated student
/// is enrolled in the class matching the given code.
#[derive(Debug, Deserialize, ToSchema)]
pub struct EnrollByCodeRequest {
    /// Optional display name override.
    pub display_name: Option<String>,
}

#[utoipa::path(
    post,
    path = "/management/classes/code/{code}/enroll",
    request_body = EnrollByCodeRequest,
    responses(
        (status = 200, description = "Enrolled"),
        (status = 404, description = "Code not found"),
        (status = 409, description = "Already enrolled")
    ),
    tag = TAG
)]
pub async fn enroll_by_code(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(code): Path<String>,
    Json(payload): Json<EnrollByCodeRequest>
) -> Result<Response> {
    // Student self-enrollment: any authenticated student may join a class by code.
    // (No teacher/admin gate — this endpoint enrolls the calling user's own
    // student profile, identified by the JWT's subject.)
    let db = &ctx.db;
    let now = chrono::Utc::now().timestamp();

    // Find class by code
    let class = class::Entity::find()
        .filter(class::Column::ClassCode.eq(&code))
        .one(db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?
        .ok_or(loco_rs::Error::NotFound)?;

    let student_id = user.id.to_string();

    // Check if already enrolled
    let existing = class_enrollment::Entity::find()
        .filter(class_enrollment::Column::ClassId.eq(&class.id))
        .filter(class_enrollment::Column::StudentId.eq(&student_id))
        .filter(class_enrollment::Column::Active.eq(true))
        .one(db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?;

    if existing.is_some() {
        return Err(Error::BadRequest("already enrolled in this class".into()));
    }

    // Enroll
    let active_model = class_enrollment::ActiveModel {
        id: sea_orm::ActiveValue::NotSet,
        class_id: sea_orm::ActiveValue::Set(class.id.clone()),
        student_id: sea_orm::ActiveValue::Set(student_id),
        enrolled_at: sea_orm::ActiveValue::Set(now),
        active: sea_orm::ActiveValue::Set(true),
        display_name: sea_orm::ActiveValue::Set(payload.display_name),
        metadata_json: sea_orm::ActiveValue::Set(None),
    };

    class_enrollment::Entity::insert(active_model)
        .exec(db)
        .await
        .map_err(|e| {
            tracing::error!("Failed to enroll student: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    format::json(serde_json::json!({
        "status": "enrolled",
        "class_id": class.id,
        "class_name": class.name,
        "student_id": user.id,
    }))
}