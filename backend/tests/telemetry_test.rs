//! Telemetry ingestion integration tests.
//!
//! Tests the card-answer → audit log pipeline using Loco's test helper
//! (in-memory SQLite, no HTTP server needed).

use loco_rs::testing;
use serde_json::{json, Value};

fn json_body(text: &str) -> Value {
    match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => panic!("invalid JSON: {e}\nbody: {text:?}"),
    }
}

/// Ingest a card-answer event without a session context.

/// Mint a valid `teacher` JWT for the telemetry compliance harness (auth-gated).
fn teacher_token() -> String {
    backend::services::auth::encode_token(999, "teacher", "teacher-morris@example.edu", 86400)
        .expect("mint teacher token")
}
#[tokio::test]
async fn ingest_card_answer_writes_audit_log() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .post("/api/v1/telemetry/card-answer")
            .json(&json!({
                "student_id": "1",
                "card_id": 1001,
                "deck_name": "Test Deck",
                "ease": 3,
                "time_ms": 4521,
                "session_uuid": null
            }))
            .await;

        let body = json_body(&resp.text());
        assert_eq!(resp.status_code(), 200);
        assert_eq!(body["status"], "ingested");

        // Verify audit log recorded the event
        use backend::models::entities::audit_log;
        use sea_orm::EntityTrait;

        let logs = audit_log::Entity::find()
            .all(&_ctx.db)
            .await
            .expect("find audit logs");

        let card_answers = logs.iter().filter(|l| l.event_type == "card_answer").count();
        assert!(
            card_answers >= 1,
            "expected at least 1 card_answer event, got {card_answers} (total logs: {})",
            logs.len()
        );
    })
    .await;
}

/// Ingest multiple card-answer events and verify they're all recorded.
#[tokio::test]
async fn ingest_multiple_events_all_logged() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        for i in 0..5 {
            let resp = server
                .post("/api/v1/telemetry/card-answer")
                .json(&json!({
                    "student_id": "1",
                    "card_id": 1000 + i,
                    "deck_name": "Test Deck",
                    "ease": 3,
                    "time_ms": 3000,
                    "session_uuid": null
                }))
                .await;
            assert_eq!(resp.status_code(), 200, "event {} failed", i);
        }

        use backend::models::entities::audit_log;
        use sea_orm::EntityTrait;

        let logs = audit_log::Entity::find()
            .all(&_ctx.db)
            .await
            .expect("find audit logs");

        let card_answers = logs.iter().filter(|l| l.event_type == "card_answer").count();
        assert_eq!(card_answers, 5, "expected 5 card_answer events, got {card_answers}");
    })
    .await;
}

/// Verify the compliance endpoint returns valid structure with seed data.
#[tokio::test]
async fn compliance_weekly_returns_structure() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .get("/api/v1/management/compliance/weekly?session_week=2026-W32")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .await;

        assert_eq!(resp.status_code(), 200);
        let body = json_body(&resp.text());
        assert_eq!(body["week"], "2026-W32");
        assert!(body["records"].as_array().is_some());
    })
    .await;
}

/// Verify student compliance endpoint works.
#[tokio::test]
async fn student_compliance_works() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .get("/api/v1/management/compliance/student/test-student-1")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .await;

        assert_eq!(resp.status_code(), 200);
        let body = json_body(&resp.text());
        assert!(body.is_array());
    })
    .await;
}