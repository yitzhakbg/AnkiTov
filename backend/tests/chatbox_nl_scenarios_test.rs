//! Plain language teacher scenario tests for the AnkiTov Chatbox.
//!
//! Tests real-world conversational phrasing, questions about student
//! performance, classroom decks, practice gaps, and unhandled requests.
//!
//! Run with: `cargo nextest run -p backend --test chatbox_nl_scenarios_test`

use backend::models::entities as entities;
use loco_rs::testing;
use sea_orm::{entity::prelude::*, ActiveValue};
use serde_json::{json, Value};

fn json_body(text: &str) -> Value {
    match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => panic!("invalid JSON: {e}\nbody: {text:?}"),
    }
}


/// Mint a valid `teacher` JWT for the NL scenarios harness (management auth-gated).
fn teacher_token() -> String {
    backend::services::auth::encode_token(999, "teacher", "teacher-morris@example.edu", 86400)
        .expect("mint teacher token")
}
#[tokio::test]
async fn test_teacher_plain_language_scenarios() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Populate realistic test data
        // 1. Retention exceptions
        let excs = [
            ("Maya Chen", "7th Grade Science", 0.32, 0.60, now - 86400),
            ("Ben Carter", "Geometry Essentials", 0.52, 0.75, now - 86400 * 3),
            ("Zoe Okafor", "Spanish Vocabulary", 0.38, 0.60, now - 86400 * 5),
            ("Alon Harish", "7th Grade Science", 0.45, 0.60, now - 86400 * 2),
        ];

        for (i, (user, deck, rate, thresh, ts)) in excs.iter().enumerate() {
            let exc = entities::retention_exception::ActiveModel {
                id: ActiveValue::Set(format!("nl-exc-{i}")),
                user_id: ActiveValue::Set((*user).to_string()),
                deck_id: ActiveValue::Set((*deck).to_string()),
                exception_type: ActiveValue::Set("retention_drop".to_string()),
                retention_rate: ActiveValue::Set(*rate),
                threshold: ActiveValue::Set(*thresh),
                detected_at: ActiveValue::Set(*ts),
                resolved_at: ActiveValue::Set(None),
            };
            exc.insert(&ctx.db).await.unwrap();
        }

        // 2. Sync statuses
        let syncs = [
            ("Amir Hassan", Some(now - 86400 * 4)), // missed 4 days
            ("Diego Ramirez", None),                // never synced
            ("Ella Johansson", Some(now - 3600)),   // synced 1 hour ago
        ];

        for (i, (user, last_sync)) in syncs.iter().enumerate() {
            let sync = entities::sync_status::ActiveModel {
                id: ActiveValue::Set(format!("nl-sync-{i}")),
                user_id: ActiveValue::Set((*user).to_string()),
                status: ActiveValue::Set(if last_sync.is_some() { "active".into() } else { "idle".into() }),
                last_sync: ActiveValue::Set(*last_sync),
                pending_changes: ActiveValue::Set(0),
                error_message: ActiveValue::Set(None),
            };
            sync.insert(&ctx.db).await.unwrap();
        }

        // Scenario 1: Teacher asks who is failing science
        let resp1 = server
            .post("/api/v1/management/ask")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({"query": "which students are failing science?"}))
            .await;
        assert_eq!(resp1.status_code(), 200);
        let j1 = json_body(&resp1.text());
        assert_eq!(j1["intent"], "struggling_students");
        assert_eq!(j1["rows"].as_array().unwrap().len(), 2); // Maya & Alon

        // Scenario 2: Teacher asks about practice decline over past two weeks
        let resp2 = server
            .post("/api/v1/management/ask")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({"query": "did anyone's retention decline over the past two weeks?"}))
            .await;
        assert_eq!(resp2.status_code(), 200);
        let j2 = json_body(&resp2.text());
        assert_eq!(j2["intent"], "retention_drop");
        assert_eq!(j2["rows"].as_array().unwrap().len(), 4);

        // Scenario 3: Teacher asks who hasn't practiced
        let resp3 = server
            .post("/api/v1/management/ask")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({"query": "who hasn't practiced for two consecutive days?"}))
            .await;
        assert_eq!(resp3.status_code(), 200);
        let j3 = json_body(&resp3.text());
        assert_eq!(j3["intent"], "missed_practice");
        let rows3 = j3["rows"].as_array().unwrap();
        assert_eq!(rows3.len(), 2); // Amir (4 days) and Diego (never synced)

        // Scenario 4: Teacher asks how decks are holding up
        let resp4 = server
            .post("/api/v1/management/ask")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({"query": "what is the overall health status of our decks?"}))
            .await;
        assert_eq!(resp4.status_code(), 200);
        let j4 = json_body(&resp4.text());
        assert_eq!(j4["intent"], "deck_health");
        let rows4 = j4["rows"].as_array().unwrap();
        assert_eq!(rows4.len(), 3); // 7th Grade Science, Geometry Essentials, Spanish Vocabulary

        // Scenario 5: Off-topic / unsupported teacher request
        let resp5 = server
            .post("/api/v1/management/ask")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({"query": "can you order pizza for the class party?"}))
            .await;
        assert_eq!(resp5.status_code(), 200);
        let j5 = json_body(&resp5.text());
        assert_eq!(j5["intent"], "unknown");
        assert!(j5["summary"].as_str().unwrap().contains("I'm not sure"));
    })
    .await;
}
