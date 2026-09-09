//! Chatbox End-to-End Actions Test Harness
//!
//! Tests real lifecycle workflows:
//! 1. Creating classes & enrolling students
//! 2. Uploading decks & creating distributions
//! 3. Creating tracks & track profiles (custom sets for students)
//! 4. Entering plain-language queries into the /management/ask chatbox
//! 5. Verifying structured responses reflect database state dynamically
//!
//! Run with: `cargo nextest run -p backend --test chatbox_actions_test`

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


/// Mint a valid `teacher` JWT for the chatbox action tests. The management
/// endpoints are auth-gated (Phase 0), so every management request in this
/// harness must carry a valid teacher/admin bearer token.
fn teacher_token() -> String {
    backend::services::auth::encode_token(999, "teacher", "teacher-morris@example.edu", 86400)
        .expect("minting teacher token for chatbox actions test must succeed")
}


fn multipart_upload_body(filename: &str, data: &[u8]) -> (String, Vec<u8>) {
    let boundary = "ankitov-test-bnd-999";
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. CLASS CREATION & ENROLLMENT HARNESS
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_create_class_and_enroll_students() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        // Step 1: Create a Class via POST /api/v1/management/classes
        let create_resp = server
            .post("/api/v1/management/classes")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({
                "name": "Period 1 — 7th Grade Algebra",
                "subject_area": "math",
                "teacher_id": "teacher-morris",
                "period": "1",
                "academic_year": "2026-2027"
            }))
            .await;
        assert_eq!(create_resp.status_code(), 200);
        let class_data = json_body(&create_resp.text());
        let class_id = class_data["id"].as_str().unwrap();
        let class_code = class_data["class_code"].as_str().unwrap();
        assert_eq!(class_code.len(), 6);

        // Step 2: Lookup class by public code
        let lookup_resp = server
            .get(&format!("/api/v1/management/classes/code/{class_code}"))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .await;
        assert_eq!(lookup_resp.status_code(), 200);
        let looked_up = json_body(&lookup_resp.text());
        assert_eq!(looked_up["name"], "Period 1 — 7th Grade Algebra");

        // Step 3: Enroll students into this class
        let enroll_resp = server
            .post(&format!("/api/v1/management/classes/{class_id}/students"))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({
                "student_ids": ["student-alex", "student-maya", "student-sam"]
            }))
            .await;
        assert_eq!(enroll_resp.status_code(), 200);
        let enroll_res = json_body(&enroll_resp.text());
        assert_eq!(enroll_res["enrolled"], 3);

        // Step 4: List students in the class
        let list_resp = server
            .get(&format!("/api/v1/management/classes/{class_id}/students"))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .await;
        assert_eq!(list_resp.status_code(), 200);
        let students_list = json_body(&list_resp.text());
        let students_arr = students_list.as_array().unwrap();
        assert_eq!(students_arr.len(), 3);

        // Step 5: Transfer out one student
        let transfer_resp = server
            .delete(&format!(
                "/api/v1/management/classes/{class_id}/students/student-sam"
            ))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .await;
        assert_eq!(transfer_resp.status_code(), 200);

        // Step 6: Verify roster has 2 active students left
        let list_resp2 = server
            .get(&format!("/api/v1/management/classes/{class_id}/students"))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .await;
        let students_arr2 = json_body(&list_resp2.text());
        assert_eq!(students_arr2.as_array().unwrap().len(), 2);
    })
    .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. DECK UPLOAD & DISTRIBUTION HARNESS
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_upload_deck_and_distribute() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        // Step 1: Upload a Deck package (.apkg)
        let dummy_apkg_bytes: Vec<u8> = (0u8..32).cycle().take(1024).collect();
        let (content_type, body_bytes) =
            multipart_upload_body("Biology-Grade7-Photosynthesis.apkg", &dummy_apkg_bytes);

        let upload_resp = server
            .post("/api/v1/management/decks")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .content_type(&content_type)
            .bytes(axum::body::Bytes::from(body_bytes))
            .await;
        assert_eq!(upload_resp.status_code(), 200);
        let deck_info = json_body(&upload_resp.text());
        let deck_id = deck_info["id"].as_str().unwrap();
        assert_eq!(deck_info["name"], "Biology-Grade7-Photosynthesis");

        // Step 2: Distribute to a Class target
        let target_uuid = uuid::Uuid::new_v4().to_string();
        let dist_resp = server
            .post(&format!("/api/v1/management/decks/{deck_id}/distribute"))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({
                "target_type": "class",
                "target_id": target_uuid
            }))
            .await;
        assert_eq!(dist_resp.status_code(), 200);
        let dist_info = json_body(&dist_resp.text());
        assert_eq!(dist_info["status"], "distributed");
        assert_eq!(dist_info["target_type"], "class");

        // Step 3: Deletion should be blocked while active distributions exist
        let del_resp = server
            .delete(&format!("/api/v1/management/decks/{deck_id}"))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .await;
        let del_info = json_body(&del_resp.text());
        assert_eq!(del_info["success"], false);
        assert!(del_info["message"]
            .as_str()
            .unwrap()
            .contains("active distributions"));
    })
    .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. CUSTOM TRACK SETS & REMEDIATION CAPSULE HARNESS
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_create_custom_sets_and_profiles() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        // Step 1: Create 2 individual curriculum tracks
        let track1_resp = server
            .post("/api/v1/management/tracks")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({
                "name": "Fractions & Decimals",
                "description": "7th Grade arithmetic foundation",
                "subject_area": "math",
                "tag": "math::fractions",
                "target_retention": 0.80,
                "n_value": 3
            }))
            .await;
        assert_eq!(track1_resp.status_code(), 200);
        let t1_id = json_body(&track1_resp.text())["id"].as_str().unwrap().to_string();

        let track2_resp = server
            .post("/api/v1/management/tracks")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({
                "name": "Linear Equations",
                "description": "Pre-Algebra equations",
                "subject_area": "math",
                "tag": "math::equations",
                "target_retention": 0.85,
                "n_value": 3
            }))
            .await;
        assert_eq!(track2_resp.status_code(), 200);
        let t2_id = json_body(&track2_resp.text())["id"].as_str().unwrap().to_string();

        // Step 2: Create a composite Track Profile (Remediation Capsule Set)
        let profile_resp = server
            .post("/api/v1/management/track-profiles")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({
                "name": "Middle School Math Recovery Set",
                "description": "Blended remediation profile",
                "target_retention": 0.82,
                "n_value": 4,
                "session_duration_minutes": 15
            }))
            .await;
        assert_eq!(profile_resp.status_code(), 200);
        let profile_id = json_body(&profile_resp.text())["id"].as_str().unwrap().to_string();

        // Step 3: Assign both tracks to the profile
        let a1 = server
            .post(&format!("/api/v1/management/track-profiles/{profile_id}/tracks"))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({"track_id": t1_id, "sort_order": 1}))
            .await;
        assert_eq!(a1.status_code(), 200);

        let a2 = server
            .post(&format!("/api/v1/management/track-profiles/{profile_id}/tracks"))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({"track_id": t2_id, "sort_order": 2}))
            .await;
        assert_eq!(a2.status_code(), 200);

        // Step 4: Verify profile detail contains 2 tracks
        let prof_detail = server
            .get(&format!("/api/v1/management/track-profiles/{profile_id}"))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .await;
        let pjson = json_body(&prof_detail.text());
        assert_eq!(pjson["tracks"].as_array().unwrap().len(), 2);
        assert_eq!(pjson["n_value"], 4);

        // Step 5: Assign profile directly to a student
        let pa_resp = server
            .post("/api/v1/management/profile-assignments")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({
                "target_type": "student",
                "target_id": "student-maya-101",
                "track_profile_id": profile_id,
                "assigned_by": "instructor-jones"
            }))
            .await;
        assert_eq!(pa_resp.status_code(), 200);
        let pa_data = json_body(&pa_resp.text());
        assert_eq!(pa_data["target_id"], "student-maya-101");
        assert_eq!(pa_data["track_profile_id"], profile_id);

        let _ = ctx;
    })
    .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. CHAT BOX QUERIES & ACTION-DRIVEN RESPONSES
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_chatbox_queries_with_actions() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Seed students with varying retention exceptions
        let exc1 = entities::retention_exception::ActiveModel {
            id: ActiveValue::Set("exc-alex-math".to_string()),
            user_id: ActiveValue::Set("student-alex".to_string()),
            deck_id: ActiveValue::Set("7th Grade Math".to_string()),
            exception_type: ActiveValue::Set("retention_drop".to_string()),
            retention_rate: ActiveValue::Set(0.35), // danger
            threshold: ActiveValue::Set(0.60),
            detected_at: ActiveValue::Set(now - 86400 * 2),
            resolved_at: ActiveValue::Set(None),
        };
        exc1.insert(&ctx.db).await.unwrap();

        let exc2 = entities::retention_exception::ActiveModel {
            id: ActiveValue::Set("exc-dave-bio".to_string()),
            user_id: ActiveValue::Set("student-dave".to_string()),
            deck_id: ActiveValue::Set("Biology 101".to_string()),
            exception_type: ActiveValue::Set("retention_drop".to_string()),
            retention_rate: ActiveValue::Set(0.55), // warn
            threshold: ActiveValue::Set(0.70),
            detected_at: ActiveValue::Set(now - 86400 * 4),
            resolved_at: ActiveValue::Set(None),
        };
        exc2.insert(&ctx.db).await.unwrap();

        // Seed sync status for missed practice queries
        let sync1 = entities::sync_status::ActiveModel {
            id: ActiveValue::Set("sync-charlie".to_string()),
            user_id: ActiveValue::Set("student-charlie".to_string()),
            status: ActiveValue::Set("idle".to_string()),
            last_sync: ActiveValue::Set(Some(now - 86400 * 5)), // 5 days ago
            pending_changes: ActiveValue::Set(0),
            error_message: ActiveValue::Set(None),
        };
        sync1.insert(&ctx.db).await.unwrap();

        // ── Query 1: "Which students are struggling?" ──
        let ask1 = server
            .post("/api/v1/management/ask")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({"query": "which students are having trouble?"}))
            .await;
        assert_eq!(ask1.status_code(), 200);
        let ans1 = json_body(&ask1.text());
        assert_eq!(ans1["intent"], "struggling_students");
        let rows1 = ans1["rows"].as_array().unwrap();
        assert_eq!(rows1.len(), 2);
        assert_eq!(rows1[0]["label"], "Student student-alex");
        assert_eq!(rows1[0]["status"], "danger"); // 35% < 40%

        // ── Query 2: "Who is struggling with math?" (subject filtered) ──
        let ask2 = server
            .post("/api/v1/management/ask")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({"query": "who is struggling with math?"}))
            .await;
        assert_eq!(ask2.status_code(), 200);
        let ans2 = json_body(&ask2.text());
        assert_eq!(ans2["intent"], "struggling_students");
        let rows2 = ans2["rows"].as_array().unwrap();
        assert_eq!(rows2.len(), 1);
        assert_eq!(rows2[0]["label"], "Student student-alex");

        // ── Query 3: "Who missed practice in the past 3 days?" ──
        let ask3 = server
            .post("/api/v1/management/ask")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({"query": "who missed practice in the past 3 days?"}))
            .await;
        assert_eq!(ask3.status_code(), 200);
        let ans3 = json_body(&ask3.text());
        assert_eq!(ans3["intent"], "missed_practice");
        let rows3 = ans3["rows"].as_array().unwrap();
        assert!(!rows3.is_empty());
        assert_eq!(rows3[0]["label"], "Student student-charlie");

        // ── Query 4: "How are my decks doing?" (Deck health summary) ──
        let ask4 = server
            .post("/api/v1/management/ask")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({"query": "how are my decks doing?"}))
            .await;
        assert_eq!(ask4.status_code(), 200);
        let ans4 = json_body(&ask4.text());
        assert_eq!(ans4["intent"], "deck_health");
        let rows4 = ans4["rows"].as_array().unwrap();
        assert_eq!(rows4.len(), 2); // 7th Grade Math + Biology 101

        // ── Query 5: "Show me recent exceptions" ──
        let ask5 = server
            .post("/api/v1/management/ask")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&json!({"query": "show me recent alerts and exceptions"}))
            .await;
        assert_eq!(ask5.status_code(), 200);
        let ans5 = json_body(&ask5.text());
        assert_eq!(ans5["intent"], "recent_exceptions");
        let rows5 = ans5["rows"].as_array().unwrap();
        assert_eq!(rows5.len(), 2);
    })
    .await;
}
