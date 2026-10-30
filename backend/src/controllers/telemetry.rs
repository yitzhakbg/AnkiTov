//! Telemetry controller — card-answer event ingestion.
//!
//! Receives telemetry pings from the AnkiTov DRM addon (or session driver)
//! when a student answers a card.  Events are written to the immutable audit
//! log and capsule session progress is updated.
//!
//! ## Endpoints
//!
//! | Method | Path | Auth | Description |
//! |--------|------|------|-------------|
//! | POST | `/api/v1/telemetry/card-answer` | Yes | Record a card answer |
//! | POST | `/api/v1/telemetry/session-complete` | Yes | Session-level completion signal from the session driver addon (T-180): `completed` marks the capsule session complete (idempotent-ish: takes max of reviewed counts); `aborted`/`replaced`/`cancelled` are audit-logged without mutating the session lifecycle |

use axum::extract::State;
use axum::Json;
use loco_rs::prelude::*;
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use serde::Deserialize;

use crate::models::entities::capsule_session as cs_entity;
use crate::services::audit_logger;

const TAG: &str = "Telemetry";

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/telemetry")
        .add("/card-answer", post(ingest_card_answer))
        .add("/session-complete", post(session_complete))
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Incoming telemetry ping from the DRM addon / session driver.
///
/// Fired on `reviewer_did_answer_card` Anki hook.
#[derive(Debug, Deserialize)]
pub struct CardAnswerRequest {
    pub student_id: String,
    pub card_id: i64,
    pub deck_name: String,
    /// Ease: 1=Again, 2=Hard, 3=Good, 4=Easy
    pub ease: i32,
    /// Time spent on the card in milliseconds.
    pub time_ms: i64,
    /// Session UUID from the capsule delivery.
    #[serde(default)]
    pub session_uuid: Option<String>,
}

/// Response returned after successful ingestion.
#[derive(Debug, serde::Serialize)]
pub struct IngestResponse {
    pub status: String,
    pub session_progress: Option<SessionProgress>,
}

#[derive(Debug, serde::Serialize)]
pub struct SessionProgress {
    pub session_id: String,
    pub cards_completed: i32,
    pub cards_total: i32,
    pub is_complete: bool,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Ingest a card-answer telemetry event.
///
/// Writes the event to the audit log and updates the capsule session's
/// `cards_completed` counter.  If this was the last card in the session,
/// marks the session as `completed` and fires a `capsule_complete` audit
/// event.
pub async fn ingest_card_answer(
    State(ctx): State<AppContext>,
    Json(payload): Json<CardAnswerRequest>,
) -> Result<Response> {
    let db = &ctx.db;

    // ── Log to audit trail ──
    let event_data = serde_json::json!({
        "student_id": payload.student_id,
        "card_id": payload.card_id,
        "deck_name": payload.deck_name,
        "ease": payload.ease,
        "time_ms": payload.time_ms,
        "session_uuid": payload.session_uuid,
    })
    .to_string();

    audit_logger::log_event(
        db,
        "card_answer",
        &event_data,
        &payload.student_id,
        payload.session_uuid.as_deref(),
    )
    .await;

    // ── Update capsule session progress ──
    let progress = if let Some(ref session_uuid) = payload.session_uuid {
        match update_session_progress(db, session_uuid).await {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::warn!("Failed to update session progress for {}: {}", session_uuid, e);
                None
            }
        }
    } else {
        None
    };

    tracing::info!(
        "Telemetry ingested: student={}, card={}, ease={}, session={:?}",
        payload.student_id,
        payload.card_id,
        payload.ease,
        payload.session_uuid,
    );

    format::json(IngestResponse {
        status: "ingested".to_string(),
        session_progress: progress,
    })
}

// ---------------------------------------------------------------------------
// Session-level completion (T-180)
// ---------------------------------------------------------------------------

/// Session-level completion signal from the session driver addon.
///
/// The addon posts `{session_uuid, cards_reviewed, status}` when the reviewer
/// closes: `completed` (all capsule cards reviewed), `aborted` (closed early),
/// `replaced` (a new session took over), or `cancelled`.
///
/// `completed` marks the capsule session complete when the reviewed count
/// reaches `cards_total` (and raises `cards_completed` to at least the
/// reported count). Non-completed statuses are audit-logged only — the
/// session lifecycle stays driven by card-answer progress.
#[derive(Debug, Deserialize)]
pub struct SessionCompleteRequest {
    pub session_uuid: String,
    #[serde(default)]
    pub cards_reviewed: i32,
    pub status: String,
}

#[derive(Debug, serde::Serialize)]
pub struct SessionCompleteResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cards_completed: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cards_total: Option<i32>,
}

/// Handle a session-level completion signal.
///
/// T-180: this is the route the session driver addon's `POST
/// /session/complete` was always meant to hit (it previously 404'd — the
/// addon posted to a path that never existed server-side).
pub async fn session_complete(
    State(ctx): State<AppContext>,
    Json(payload): Json<SessionCompleteRequest>,
) -> Result<Response> {
    let db = &ctx.db;

    let event_data = serde_json::json!({
        "session_id": payload.session_uuid,
        "cards_reviewed": payload.cards_reviewed,
        "status": payload.status,
    })
    .to_string();
    audit_logger::log_event(
        db,
        "session_complete",
        &event_data,
        "system",
        Some(&payload.session_uuid),
    )
    .await;

    if payload.status != "completed" {
        tracing::info!(
            "Session signal (no mutation): session={}, status={}",
            payload.session_uuid,
            payload.status
        );
        return format::json(SessionCompleteResponse {
            status: "logged".to_string(),
            session_id: None,
            cards_completed: None,
            cards_total: None,
        });
    }

    let session = match cs_entity::Entity::find_by_id(&payload.session_uuid).one(db).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            tracing::warn!("session-complete for unknown session {}", payload.session_uuid);
            return format::json(SessionCompleteResponse {
                status: "unknown".to_string(),
                session_id: None,
                cards_completed: None,
                cards_total: None,
            });
        }
        Err(e) => {
            tracing::error!("session-complete db error for {}: {}", payload.session_uuid, e);
            return format::json(SessionCompleteResponse {
                status: "error".to_string(),
                session_id: None,
                cards_completed: None,
                cards_total: None,
            });
        }
    };

    let cards_total = session.cards_total;
    let was_completed = session.status == "completed";
    let new_completed = session.cards_completed.max(payload.cards_reviewed);
    let is_complete = new_completed >= cards_total;

    let mut active: cs_entity::ActiveModel = session.into();
    active.cards_completed = ActiveValue::Set(new_completed);
    if is_complete && !was_completed {
        active.status = ActiveValue::Set("completed".to_string());
        active.completed_at = ActiveValue::Set(Some(now_ts()));
        let done = serde_json::json!({
            "session_id": payload.session_uuid,
            "cards_total": cards_total,
            "via": "session-complete",
        })
        .to_string();
        audit_logger::log_event(db, "capsule_complete", &done, "system", Some(&payload.session_uuid)).await;
    }
    active.update(db).await?;

    tracing::info!(
        "Session completed via addon signal: session={}, reviewed={}",
        payload.session_uuid,
        payload.cards_reviewed
    );
    format::json(SessionCompleteResponse {
        status: "ok".to_string(),
        session_id: Some(payload.session_uuid),
        cards_completed: Some(new_completed),
        cards_total: Some(cards_total),
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Increment the capsule session's `cards_completed` counter.
///
/// If all cards have been reviewed, marks the session as `completed`,
/// records a `capsule_complete` audit event, and sets `completed_at`.
pub(crate) async fn update_session_progress(
    db: &sea_orm::DatabaseConnection,
    session_uuid: &str,
) -> Result<SessionProgress, sea_orm::DbErr> {
    let session = cs_entity::Entity::find_by_id(session_uuid)
        .one(db)
        .await?
        .ok_or(sea_orm::DbErr::RecordNotFound(session_uuid.to_string()))?;

    let cards_total = session.cards_total;
    let new_completed = session.cards_completed + 1;
    let is_complete = new_completed >= cards_total;

    let mut active: cs_entity::ActiveModel = session.into();
    active.cards_completed = ActiveValue::Set(new_completed);

    if is_complete {
        active.status = ActiveValue::Set("completed".to_string());
        active.completed_at = ActiveValue::Set(Some(now_ts()));

        // Audit: capsule complete
        let event_data = serde_json::json!({
            "session_id": session_uuid,
            "cards_total": active.cards_total.as_ref(),
            "cards_completed": new_completed,
        })
        .to_string();
        audit_logger::log_event(db, "capsule_complete", &event_data, "system", Some(session_uuid)).await;
    }

    active.update(db).await?;

    Ok(SessionProgress {
        session_id: session_uuid.to_string(),
        cards_completed: new_completed,
        cards_total,
        is_complete,
    })
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
