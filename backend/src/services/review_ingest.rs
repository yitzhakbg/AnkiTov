//! Review Ingest — authenticated answer ingestion for the Solo Web Reviewer
//! (spec §1.2 service map). Wraps the EXISTING telemetry pipeline:
//!
//! 1. writes the backend-side review ledger row (`web_review_log`, idempotent
//!    by client UUID),
//! 2. logs the same `card_answer` audit event the Anki-client path emits
//!    (one telemetry stream — dashboards read one stream),
//! 3. updates capsule-session progress via the shared
//!    `telemetry::update_session_progress` (auto-completes the session).
//!
//! FSRS note (documented v1 gap, spec §2.2): the ledger is recorded from day
//! one, but the `fsrs_memory_states` derivation replay step over
//! `web_review_log` lands in a later slice — web answers do not yet influence
//! scheduling; capsule generation keeps using the `.anki2` revlog.

use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use serde::Serialize;

use crate::models::entities::web_review_log as wrl_entity;
use crate::services::audit_logger;

#[derive(Debug, serde::Deserialize)]
pub struct ReviewAnswer {
    /// Client-generated UUID (36 chars) — idempotency key for retries.
    pub answer_id: String,
    pub card_id: i64,
    /// FSRS rating: 1=Again, 2=Hard, 3=Good, 4=Easy.
    pub ease: i32,
    /// Milliseconds spent on the card.
    pub time_ms: i64,
    /// Capsule session this answer belongs to, when reviewing a capsule.
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct IngestResult {
    pub status: String,
    /// Capsule progress when the answer belonged to a session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_progress: Option<crate::controllers::telemetry::SessionProgress>,
}

/// Ingest one reviewer answer. `student_id` comes from the authenticated
/// JWT (never from the payload — students can only ingest for themselves).
pub async fn ingest_answer(
    db: &DatabaseConnection,
    student_id: i64,
    student_email: &str,
    answer: &ReviewAnswer,
) -> Result<IngestResult, IngestError> {
    if !(1..=4).contains(&answer.ease) {
        return Err(IngestError::BadEase(answer.ease));
    }
    if answer.answer_id.len() > 64 || answer.answer_id.is_empty() {
        return Err(IngestError::BadAnswerId);
    }

    // 1) ledger (idempotent: PK = answer_id; duplicate → Ok(duplicate))
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let row = wrl_entity::ActiveModel {
        id: Set(answer.answer_id.clone()),
        student_id: Set(student_id.to_string()),
        card_id: Set(answer.card_id),
        session_id: Set(answer.session_id.clone()),
        ease: Set(answer.ease),
        time_ms: Set(answer.time_ms),
        reviewed_at: Set(now as i64),
    };
    let duplicate = match row.insert(db).await {
        Ok(_) => false,
        Err(sea_orm::DbErr::RecordNotInserted) => false,
        Err(e) => {
            // unique violation → already ingested (retry) — treat as success
            if e.to_string().to_lowercase().contains("unique") {
                true
            } else {
                return Err(e.into());
            }
        }
    };

    // 2) audit stream (same event type as the Anki-client path)
    let event_data = serde_json::json!({
        "student_id": student_id.to_string(),
        "card_id": answer.card_id,
        "deck_name": "web-reviewer",
        "ease": answer.ease,
        "time_ms": answer.time_ms,
        "session_uuid": answer.session_id,
        "source": "web-reviewer",
        "duplicate": duplicate,
    })
    .to_string();
    audit_logger::log_event(
        db,
        "card_answer",
        &event_data,
        student_email,
        answer.session_id.as_deref(),
    )
    .await;

    // 3) capsule progress (shared with the telemetry path)
    let session_progress = if let Some(ref sid) = answer.session_id {
        match crate::controllers::telemetry::update_session_progress(db, sid).await {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::warn!("web reviewer: session progress update failed for {}: {}", sid, e);
                None
            }
        }
    } else {
        None
    };

    Ok(IngestResult {
        status: if duplicate { "duplicate" } else { "ingested" }.to_string(),
        session_progress,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("ease must be 1-4, got {0}")]
    BadEase(i32),
    #[error("answer_id must be 1-64 chars")]
    BadAnswerId,
    #[error("db error: {0}")]
    Db(#[from] sea_orm::DbErr),
}
