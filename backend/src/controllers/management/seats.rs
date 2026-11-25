//! Seat management endpoints — start/stop/list real-Anki Selkies seats.
//!
//! Product Law (2026-11-18): student practice = real Anki streamed via
//! Selkies. These endpoints are the teacher/admin control surface over the
//! [`crate::services::seat_manager::SeatManager`]. The student-facing flow
//! (click "Practice" → get the stream URL) rides on the same manager.

use axum::Extension;
use loco_rs::prelude::*;

use crate::middleware::auth::ensure_role;
use crate::middleware::auth::AuthUser;
use crate::services::seat_manager::{SeatLaunch, SeatManager};

fn seat_json(info: &crate::services::seat_manager::SeatInfo) -> serde_json::Value {
    serde_json::json!({
        "student_id": info.student_id,
        "port": info.port,
        "pid": info.pid,
        "gpu": info.gpu,
        "stream_url": info.stream_url(),
        "uptime_secs": info.started_at.elapsed().as_secs(),
        // Kiosk level this seat runs at (users.kiosk_level at start).
        "kiosk_level": info.kiosk_level,
        // Selkies basic auth — the browser prompts for these (user is fixed).
        "auth_user": "ubuntu",
        "auth_password": info.password,
        // Proxy-level stream auth (owner decision 2026-11-23): append
        // ?seat_token=<stream_token> to stream_url — the authz endpoint
        // exchanges it for a scoped cookie on first load.
        "stream_token": info.token,
    })
}

/// POST /management/seats/:student/start — spawn (or report) a student seat.
#[utoipa::path(
    post,
    path = "/management/seats/{student}/start",
    params(("student" = String, Path, description = "Student id")),
    responses((status = 200, description = "Seat running")),
    tag = "Management Console"
)]
pub async fn start(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    axum::extract::Path(student): axum::extract::Path<String>,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    // Per-student seat profile (owner decision 2026-11-23): kiosk level
    // lives on users.kiosk_level; the email feeds telemetry attribution.
    // SeatManager itself stays DB-free — the controller resolves and passes.
    let (kiosk_level, email) = resolve_student_seat_profile(&ctx, &student).await;
    let launch = SeatLaunch {
        kiosk_level,
        student_email: email,
        // deck/packs unset → inherit the backend env (pre-T-215 behavior).
        ..SeatLaunch::default()
    };
    match SeatManager::global().start_seat(&student, &launch).await {
        Ok(info) => format::json(serde_json::json!({ "status": "running", "seat": seat_json(&info) })),
        Err(crate::services::seat_manager::SeatError::AlreadyRunning) => {
            // Idempotent: report the existing seat.
            let existing = SeatManager::global()
                .list()
                .into_iter()
                .find(|s| s.student_id == student);
            match existing {
                Some(info) => {
                    format::json(serde_json::json!({ "status": "running", "seat": seat_json(&info) }))
                }
                None => format::json(serde_json::json!({ "status": "running" })),
            }
        }
        Err(e) => Err(loco_rs::Error::BadRequest(e.to_string())),
    }
}

/// DELETE /management/seats/:student — stop a student's seat.
#[utoipa::path(
    delete,
    path = "/management/seats/{student}",
    params(("student" = String, Path, description = "Student id")),
    responses((status = 200, description = "Seat stopped")),
    tag = "Management Console"
)]
pub async fn stop(
    Extension(user): Extension<AuthUser>,
    axum::extract::Path(student): axum::extract::Path<String>,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    match SeatManager::global().stop_seat(&student) {
        Ok(stopped) => format::json(serde_json::json!({ "status": "stopped", "stopped": stopped })),
        Err(e) => Err(loco_rs::Error::BadRequest(e.to_string())),
    }
}

/// GET /management/seats — list live seats.
#[utoipa::path(
    get,
    path = "/management/seats",
    responses((status = 200, description = "Live seats")),
    tag = "Management Console"
)]
pub async fn list(Extension(user): Extension<AuthUser>) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    let seats: Vec<serde_json::Value> = SeatManager::global()
        .list()
        .iter()
        .map(seat_json)
        .collect();
    format::json(serde_json::json!({ "seats": seats, "count": seats.len() }))
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management/seats")
        .add("/", get(list))
        .add("/:student/start", post(start))
        .add("/:student", delete(stop))
}

/// Resolve a student's seat settings from the DB: `(kiosk_level, email)`.
///
/// Lenient by design — unknown or non-numeric ids (army-style synthetic
/// students, legacy seats) fall back to the locked-down kiosk (level 0)
/// rather than failing the start. The seat is keyed by the same id either
/// way, so the proxy authz binding is unaffected.
async fn resolve_student_seat_profile(ctx: &AppContext, student: &str) -> (i32, Option<String>) {
    const DEFAULT_LEVEL: i32 = 0;
    let Ok(id) = student.parse::<i64>() else {
        return (DEFAULT_LEVEL, None);
    };
    match crate::models::entities::user::Entity::find_by_id(id)
        .one(&ctx.db)
        .await
    {
        Ok(Some(u)) => (u.kiosk_level.clamp(0, 2), Some(u.email)),
        _ => (DEFAULT_LEVEL, None),
    }
}
