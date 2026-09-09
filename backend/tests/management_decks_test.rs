//! Integration tests for deck management endpoints.
//!
//! Uses Loco's `testing::request` helper which spins up a real Axum test server
//! with an in-memory SQLite database — no HTTP server needed.
//!
//! All assertions are made via the HTTP API surface only.
//!
//! Run with: `cargo nextest run`  (or `bacon n` in Terax pane 1)

use loco_rs::testing;
use serde_json::Value;

/// Parses response text as JSON.
fn json(text: &str) -> Value {
    match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => panic!("invalid JSON from response body: {e}\nbody: {text:?}"),
    }
}

/// Build a minimal multipart/form-data body with a "file" field.
/// The deck upload controller expects `multipart/form-data` with a `file` field.
fn multipart_upload_body(filename: &str, data: &[u8]) -> (String, Vec<u8>) {
    let boundary = "ankitov-test-boundary-001";
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        ).as_bytes()
    );
    body.extend_from_slice(data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

/// Mint a valid `teacher` JWT for the management decks harness (auth-gated).
fn teacher_token() -> String {
    backend::services::auth::encode_token(999, "teacher", "teacher-morris@example.edu", 86400)
        .expect("mint teacher token")
}

// ---------------------------------------------------------------------------
// list_decks tests
// ---------------------------------------------------------------------------

/// Empty database returns HTTP 200 with empty list.
#[tokio::test]
async fn list_decks_empty_returns_200() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server.get("/api/v1/management/decks").add_header("Authorization", format!("Bearer {}", teacher_token())).await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "expected 200 OK, got {}",
            resp.status_code()
        );
        assert_eq!(resp.text(), "[]", "empty DB should return []");
    })
    .await;
}

/// Pagination params are accepted without crashing.
#[tokio::test]
async fn list_decks_accepts_pagination_params() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let r = server.get("/api/v1/management/decks?page=5&per_page=50").add_header("Authorization", format!("Bearer {}", teacher_token())).await;
        assert_eq!(r.status_code(), axum::http::StatusCode::OK);

        let r2 = server.get("/api/v1/management/decks?page=0&per_page=0").add_header("Authorization", format!("Bearer {}", teacher_token())).await;
        assert_eq!(
            r2.status_code(),
            axum::http::StatusCode::OK,
            "page=0 should coerce safely"
        );
    })
    .await;
}

/// Filter by nonexistent target returns empty list.
#[tokio::test]
async fn list_decks_filter_by_nonexistent_target() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .get("/api/v1/management/decks?target_type=class&target_id=00000000-0000-0000-0000-000000000000")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
        assert_eq!(resp.text(), "[]");
    })
    .await;
}

// ---------------------------------------------------------------------------
// upload_deck tests
// ---------------------------------------------------------------------------

/// Valid binary payload returns HTTP 200 with DeckInfo containing an id.
#[tokio::test]
async fn upload_deck_accepts_valid_payload() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let payload: Vec<u8> = (0u8..64).cycle().take(2048).collect();
        let (content_type, body_bytes) = multipart_upload_body("test-deck.apkg", &payload);
        let resp = server
            .post("/api/v1/management/decks")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .content_type(&content_type)
            .bytes(axum::body::Bytes::from(body_bytes))
            .await;

        eprintln!("[DEBUG] status={:?} body={}", resp.status_code(), resp.text());
        let status = resp.status_code();
        let body = resp.text();
        if status != axum::http::StatusCode::OK && status != axum::http::StatusCode::CREATED {
            eprintln!("[DEBUG] upload failed — got {}, body: {}", status, body);
        }
        assert!(
            status == axum::http::StatusCode::OK
                || status == axum::http::StatusCode::CREATED,
            "upload should return 200 or 201, got {:?}: {}",
            status,
            body
        );
        let j = json(&body);
        assert!(
            j.get("id").is_some() && !j["id"].as_str().unwrap().is_empty(),
            "response should contain non-empty id: {j}"
        );
        assert!(j.get("card_count").is_some(), "response should contain card_count: {j}");
    })
    .await;
}

/// Empty payload returns a failure ApiResponse.
#[tokio::test]
async fn upload_deck_rejects_empty_payload() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let (content_type, body_bytes) = multipart_upload_body("empty.apkg", &[]);
        let resp = server
            .post("/api/v1/management/decks")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .content_type(&content_type)
            .bytes(axum::body::Bytes::from(body_bytes))
            .await;

        assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
        let j = json(&resp.text());
        assert_eq!(
            j.get("success").and_then(|v| v.as_bool()),
            Some(false),
            "success should be false: {j}"
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// get_deck tests
// ---------------------------------------------------------------------------

/// Non-existent UUID returns an error response.
#[tokio::test]
async fn get_deck_returns_not_found() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .get("/api/v1/management/decks/00000000-0000-0000-0000-000000000001")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .await;
        // loco_rs Error variants are serialized as JSON with HTTP 200
        let j = json(&resp.text());
        assert_eq!(
            j.get("success").and_then(|v| v.as_bool()),
            Some(false),
            "non-existent deck should return success=false: {j}"
        );
    })
    .await;
}

/// Invalid (non-UUID) path segment is rejected with error.
#[tokio::test]
async fn get_deck_rejects_invalid_uuid() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server.get("/api/v1/management/decks/not-a-uuid").add_header("Authorization", format!("Bearer {}", teacher_token())).await;
        let j = json(&resp.text());
        assert_eq!(
            j.get("success").and_then(|v| v.as_bool()),
            Some(false),
            "invalid UUID should return success=false: {j}"
        );
    })
    .await;
}

/// After upload, get_deck returns matching metadata.
#[tokio::test]
async fn get_deck_returns_uploaded_deck() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        // Upload
        let payload: Vec<u8> = (0u8..16).cycle().take(1024).collect();
        let (content_type, body_bytes) = multipart_upload_body("test-deck.apkg", &payload);
        let upload_resp = server
            .post("/api/v1/management/decks")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .content_type(&content_type)
            .bytes(axum::body::Bytes::from(body_bytes))
            .await;
        assert_eq!(upload_resp.status_code(), axum::http::StatusCode::OK);

        let upload_text = upload_resp.text();
        let upload_j: Value = json(&upload_text);
        let deck_id = upload_j["id"].as_str().unwrap().to_string();
        let expected_name = upload_j["name"].as_str().unwrap().to_string();
        let expected_count = upload_j["card_count"].clone();

        // Fetch
        let fetch_resp = server
            .get(&format!("/api/v1/management/decks/{deck_id}"))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .await;
        assert_eq!(fetch_resp.status_code(), axum::http::StatusCode::OK);
        let fetch_j: Value = json(&fetch_resp.text());

        assert_eq!(fetch_j["id"].as_str().unwrap(), deck_id, "id mismatch");
        assert_eq!(fetch_j["name"].as_str().unwrap(), expected_name, "name mismatch");
        assert_eq!(fetch_j["card_count"], expected_count, "card_count mismatch");
    })
    .await;
}

// ---------------------------------------------------------------------------
// delete_deck tests
// ---------------------------------------------------------------------------

/// Deleting a non-existent deck returns error response.
#[tokio::test]
async fn delete_deck_returns_error_for_missing() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .delete("/api/v1/management/decks/00000000-0000-0000-0000-000000000002")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .await;
        let j = json(&resp.text());
        assert_eq!(
            j.get("success").and_then(|v| v.as_bool()),
            Some(false),
            "delete missing deck should return success=false: {j}"
        );
    })
    .await;
}

/// Deleting a deck with no distributions succeeds and the deck is gone.
#[tokio::test]
async fn delete_deck_succeeds_without_distributions() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        // Upload
        let payload: Vec<u8> = vec![42u8; 512];
        let (content_type, body_bytes) = multipart_upload_body("test-deck.apkg", &payload);
        let upload_resp = server
            .post("/api/v1/management/decks")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .content_type(&content_type)
            .bytes(axum::body::Bytes::from(body_bytes))
            .await;
        let deck_id = json(&upload_resp.text())["id"].as_str().unwrap().to_string();

        // Delete
        let delete_resp = server
            .delete(&format!("/api/v1/management/decks/{deck_id}"))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .await;
        assert_eq!(delete_resp.status_code(), axum::http::StatusCode::OK);
        let delete_j = json(&delete_resp.text());
        assert_eq!(
            delete_j.get("success").and_then(|v| v.as_bool()),
            Some(true),
            "delete should succeed: {delete_j}"
        );

        // Verify gone — get returns 200 with success=false since deck is gone
        let fetch_resp = server
            .get(&format!("/api/v1/management/decks/{deck_id}"))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .await;
        assert_eq!(fetch_resp.status_code(), axum::http::StatusCode::OK, "deck lookup should return 200");
        let fetch_j = json(&fetch_resp.text());
        assert_eq!(
            fetch_j.get("success").and_then(|v| v.as_bool()),
            Some(false),
            "deck lookup after delete should report not found: {fetch_j}"
        );
    })
    .await;
}

/// Deleting a deck with active distributions fails with failure ApiResponse.
#[tokio::test]
async fn delete_deck_fails_with_active_distributions() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        // Upload
        let payload: Vec<u8> = vec![99u8; 512];
        let (content_type, body_bytes) = multipart_upload_body("test-deck.apkg", &payload);
        let upload_resp = server
            .post("/api/v1/management/decks")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .content_type(&content_type)
            .bytes(axum::body::Bytes::from(body_bytes))
            .await;
        let deck_id = json(&upload_resp.text())["id"].as_str().unwrap().to_string();

        // Distribute
        let dist_payload = serde_json::json!({
            "target_type": "user",
            "target_id": "00000000-0000-0000-0000-000000000003"
        });
        let dist_resp = server
            .post(&format!("/api/v1/management/decks/{deck_id}/distribute"))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&dist_payload)
            .await;
        assert_eq!(dist_resp.status_code(), axum::http::StatusCode::OK);

        // Attempt delete — should fail
        let delete_resp = server
            .delete(&format!("/api/v1/management/decks/{deck_id}"))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .await;
        assert_eq!(delete_resp.status_code(), axum::http::StatusCode::OK);
        let delete_j = json(&delete_resp.text());
        assert_eq!(
            delete_j.get("success").and_then(|v| v.as_bool()),
            Some(false),
            "delete with distributions should fail: {delete_j}"
        );
        assert!(
            delete_j["message"].as_str().unwrap().contains("distribution"),
            "error should mention distributions"
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// distribute_deck tests
// ---------------------------------------------------------------------------

/// Valid distribution to a user target succeeds.
#[tokio::test]
async fn distribute_deck_to_user_succeeds() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        // Upload
        let payload: Vec<u8> = vec![1u8; 1024];
        let (content_type, body_bytes) = multipart_upload_body("test-deck.apkg", &payload);
        let upload_resp = server
            .post("/api/v1/management/decks")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .content_type(&content_type)
            .bytes(axum::body::Bytes::from(body_bytes))
            .await;
        let deck_id = json(&upload_resp.text())["id"].as_str().unwrap().to_string();

        // Distribute
        let dist_payload = serde_json::json!({
            "target_type": "user",
            "target_id": "00000000-0000-0000-0000-000000000004"
        });
        let dist_resp = server
            .post(&format!("/api/v1/management/decks/{deck_id}/distribute"))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&dist_payload)
            .await;

        assert_eq!(dist_resp.status_code(), axum::http::StatusCode::OK);
        let text = dist_resp.text();
        assert!(text.contains("\"deck_id\""), "should contain deck_id: {text}");
        assert!(
            text.contains("\"target_type\":\"user\""),
            "should contain target_type user: {text}"
        );
    })
    .await;
}

/// Invalid target_type (not user/group/class) returns failure ApiResponse.
#[tokio::test]
async fn distribute_deck_rejects_invalid_target_type() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let payload: Vec<u8> = vec![2u8; 512];
        let (content_type, body_bytes) = multipart_upload_body("test-deck.apkg", &payload);
        let upload_resp = server
            .post("/api/v1/management/decks")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .content_type(&content_type)
            .bytes(axum::body::Bytes::from(body_bytes))
            .await;
        let deck_id = json(&upload_resp.text())["id"].as_str().unwrap().to_string();

        let dist_payload = serde_json::json!({
            "target_type": "invalid",
            "target_id": "00000000-0000-0000-0000-000000000005"
        });
        let dist_resp = server
            .post(&format!("/api/v1/management/decks/{deck_id}/distribute"))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&dist_payload)
            .await;

        assert_eq!(dist_resp.status_code(), axum::http::StatusCode::OK);
        let j = json(&dist_resp.text());
        assert_eq!(
            j.get("success").and_then(|v| v.as_bool()),
            Some(false),
            "invalid target_type should fail: {j}"
        );
    })
    .await;
}

/// Distribution to a non-existent deck returns error response.
#[tokio::test]
async fn distribute_deck_returns_error_for_missing_deck() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let dist_payload = serde_json::json!({
            "target_type": "user",
            "target_id": "00000000-0000-0000-0000-000000000006"
        });
        let resp = server
            .post("/api/v1/management/decks/00000000-0000-0000-0000-000000000099/distribute")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&dist_payload)
            .await;
        let j = json(&resp.text());
        assert_eq!(
            j.get("success").and_then(|v| v.as_bool()),
            Some(false),
            "distribute to missing deck should return success=false: {j}"
        );
    })
    .await;
}

/// Distribution with invalid target_id UUID returns error response.
#[tokio::test]
async fn distribute_deck_returns_error_for_invalid_target_id() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let payload: Vec<u8> = vec![3u8; 512];
        let (content_type, body_bytes) = multipart_upload_body("test-deck.apkg", &payload);
        let upload_resp = server
            .post("/api/v1/management/decks")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .content_type(&content_type)
            .bytes(axum::body::Bytes::from(body_bytes))
            .await;
        let deck_id = json(&upload_resp.text())["id"].as_str().unwrap().to_string();

        let dist_payload = serde_json::json!({
            "target_type": "user",
            "target_id": "not-a-uuid"
        });
        let dist_resp = server
            .post(&format!("/api/v1/management/decks/{deck_id}/distribute"))
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .json(&dist_payload)
            .await;
        let j = json(&dist_resp.text());
        assert_eq!(
            j.get("success").and_then(|v| v.as_bool()),
            Some(false),
            "invalid target_id should return success=false: {j}"
        );
    })
    .await;
}