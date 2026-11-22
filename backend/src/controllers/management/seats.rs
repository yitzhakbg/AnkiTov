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
use crate::services::seat_manager::SeatManager;

fn seat_json(info: &crate::services::seat_manager::SeatInfo) -> serde_json::Value {
    serde_json::json!({
        "student_id": info.student_id,
        "port": info.port,
        "pid": info.pid,
        "gpu": info.gpu,
        "stream_url": info.stream_url(),
        "uptime_secs": info.started_at.elapsed().as_secs(),
        // Selkies basic auth — the browser prompts for these (user is fixed).
        "auth_user": "ubuntu",
        "auth_password": info.password,
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
    axum::extract::Path(student): axum::extract::Path<String>,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    match SeatManager::global().start_seat(&student).await {
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
