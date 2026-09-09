//! Telemetry Ingestor — consume card-answer events into the audit ledger.
//!
//! Receives telemetry pings from the DRM addon (via the backend API) and
//! writes them to the immutable append-only audit_log table. Also updates
//! per-track FSRS health metrics for the Management Console dashboard.
//!
//! The DRM addon fires on `reviewer_did_answer_card` and POSTs:
//! ```json
//! {
//!     "student_id": "...",
//!     "card_id": 123456,
//!     "deck_name": "Math Grade 7",
//!     "ease": 3,
//!     "time_ms": 4521,
//!     "session_uuid": "..."  // from cross-addon contract (v2)
//! }
//! ```
//!
//! v1: The backend resolves the card's tags via AnkiConnect to determine
//!     which track(s) the card belongs to. The session is inferred from
//!     the active capsule tag.
//!
//! v2: The session driver sets `mw.ankitov_session_uuid` for the DRM addon
//!     to include directly.

use sea_orm::DatabaseConnection;
use serde::Deserialize;

use crate::services::audit_logger;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Incoming telemetry ping from the DRM addon.
#[derive(Debug, Deserialize)]
pub struct CardAnswerEvent {
    pub student_id: String,
    pub card_id: i64,
    pub deck_name: String,
    /// Ease: 1=Again, 2=Hard, 3=Good, 4=Easy
    pub ease: i32,
    /// Time spent on the card in milliseconds.
    pub time_ms: i64,
    /// Optional session UUID (v2 cross-addon contract).
    #[serde(default)]
    pub session_uuid: Option<String>,
}

/// Track-level telemetry snapshot for the dashboard.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TrackTelemetry {
    pub track_tag: String,
    pub total_answers: u64,
    pub avg_ease: f64,
    pub avg_time_secs: f64,
    /// Approximate stability trend: avg(ivl) from the last 7 days.
    pub avg_interval_days: f64,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Ingest a card-answer telemetry event.
///
/// Records the event in the audit ledger and returns a track tag
/// lookup key for the caller to update per-track health metrics.
pub async fn ingest_card_answer(
    db: &DatabaseConnection,
    event: &CardAnswerEvent,
) -> Result<(), TelemetryError> {
    let event_data = serde_json::json!({
        "student_id": event.student_id,
        "card_id": event.card_id,
        "deck_name": event.deck_name,
        "ease": event.ease,
        "time_ms": event.time_ms,
        "session_uuid": event.session_uuid,
        "ingested_at": now_ts(),
    });

    audit_logger::log_event(
        db,
        "card_answer",
        &event_data.to_string(),
        &event.student_id,
        None,
    )
    .await;

    tracing::debug!(
        "Telemetry ingested: student={} card={} ease={} time={}ms",
        event.student_id,
        event.card_id,
        event.ease,
        event.time_ms,
    );

    Ok(())
}

/// Ingest a batch of card-answer events efficiently.
///
/// In production, the DRM addon may buffer answers and send them in
/// batches every few seconds to reduce HTTP overhead.
pub async fn ingest_batch(
    db: &DatabaseConnection,
    events: &[CardAnswerEvent],
) -> Result<usize, TelemetryError> {
    let count = events.len();
    for event in events {
        ingest_card_answer(db, event).await?;
    }
    tracing::info!("Batch ingested: {} card-answer events", count);
    Ok(count)
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("database error: {0}")]
    Database(#[from] sea_orm::DbErr),
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_card_answer_event_deserialization() {
        let json = r#"{
            "student_id": "student-1",
            "card_id": 123456,
            "deck_name": "Math Grade 7",
            "ease": 3,
            "time_ms": 4521
        }"#;
        let event: CardAnswerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.student_id, "student-1");
        assert_eq!(event.card_id, 123456);
        assert_eq!(event.ease, 3);
        assert_eq!(event.time_ms, 4521);
        assert!(event.session_uuid.is_none());
    }

    #[test]
    fn test_card_answer_event_with_session() {
        let json = r#"{
            "student_id": "student-1",
            "card_id": 789,
            "deck_name": "Vocab",
            "ease": 4,
            "time_ms": 2100,
            "session_uuid": "abc-123"
        }"#;
        let event: CardAnswerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.session_uuid.as_deref(), Some("abc-123"));
    }
}