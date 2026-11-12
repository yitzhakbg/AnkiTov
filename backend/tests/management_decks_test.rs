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
// AnkiConnect mock (Zone 2 import path)
// ---------------------------------------------------------------------------

/// Mock AnkiConnect JSON-RPC server for upload tests.
///
/// Simulates the sequence `upload_deck` drives against a real Anki:
/// 1. `deckNames` → `["Default"]` (before-import snapshot)
/// 2. `importPackage` → `true`
/// 3. `deckNames` → `["Default", "Test Deck"]` (diff resolves the name)
/// 4. `findCards` → 3 card ids (real card count)
///
/// wiremock matches the FIRST-mounted matching mock, so the before-snapshot
/// is mounted first with `up_to_n_times(1)` — the first `deckNames` call
/// consumes it and every later call falls through to the after-snapshot.
///
/// NOTE: `ANKICONNECT_PORT` is process-global. These tests rely on
/// cargo-nextest's one-process-per-test isolation; do not run them under
/// plain `cargo test` with default threading.
async fn start_anki_mock() -> wiremock::MockServer {
    let server = wiremock::MockServer::start().await;

    // before-import deckNames (single shot — mounted first, matched first)
    wiremock::Mock::given(wiremock::matchers::body_partial_json(
        serde_json::json!({"action": "deckNames"}),
    ))
    .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
        serde_json::json!({"result": ["Default"], "error": null}),
    ))
    .up_to_n_times(1)
    .mount(&server)
    .await;

    // importPackage
    wiremock::Mock::given(wiremock::matchers::body_partial_json(
        serde_json::json!({"action": "importPackage"}),
    ))
    .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
        serde_json::json!({"result": true, "error": null}),
    ))
    .mount(&server)
    .await;

    // findCards → 3 cards
    wiremock::Mock::given(wiremock::matchers::body_partial_json(
        serde_json::json!({"action": "findCards"}),
    ))
    .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
        serde_json::json!({"result": [101, 102, 103], "error": null}),
    ))
    .mount(&server)
    .await;

    // after-import deckNames (unlimited — matched after the single-shot
    // before-snapshot is exhausted)
    wiremock::Mock::given(wiremock::matchers::body_partial_json(
        serde_json::json!({"action": "deckNames"}),
    ))
    .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
        serde_json::json!({"result": ["Default", "Test Deck"], "error": null}),
    ))
    .mount(&server)
    .await;

    server
}

/// Point the backend's Zone 2 AnkiConnect client at the mock server.
fn set_anki_port(server: &wiremock::MockServer) {
    let uri = server.uri(); // http://127.0.0.1:<port>
    let port: u16 = uri.rsplit(':').next().unwrap().parse().expect("mock port");
    std::env::set_var("ANKICONNECT_PORT", port.to_string());
}

/// Redirect persisted uploads to a unique temp dir so tests don't litter
/// `backend/uploads/decks/`. Safe under nextest (one process per test);
/// do NOT run this suite under plain `cargo test` with default threading.
fn set_unique_upload_dir() {
    let dir = std::env::temp_dir().join(format!(
        "ankitov-upload-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("ANKITOV_UPLOAD_DIR", dir);
}

/// POST a multipart .apkg upload to the decks endpoint.
async fn post_upload(
    server: &loco_rs::TestServer,
    filename: &str,
    data: &[u8],
) -> axum_test::TestResponse {
    let (content_type, body_bytes) = multipart_upload_body(filename, data);
    server
        .post("/api/v1/management/decks")
        .add_header("Authorization", format!("Bearer {}", teacher_token()))
        .content_type(&content_type)
        .bytes(axum::body::Bytes::from(body_bytes))
        .await
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

/// Valid .apkg upload routes through the Zone 2 import path: the deck name
/// comes from the deckNames diff and the card count is the real collection
/// count returned by the (mocked) AnkiConnect.
#[tokio::test]
async fn upload_deck_accepts_valid_payload() {
    let mock = start_anki_mock().await;
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        set_anki_port(&mock);
        set_unique_upload_dir();
        let payload: Vec<u8> = (0u8..64).cycle().take(2048).collect();
        let resp = post_upload(&server, "test-deck.apkg", &payload).await;

        let status = resp.status_code();
        let body = resp.text();
        assert_eq!(
            status,
            axum::http::StatusCode::OK,
            "upload should return 200, got {status}: {body}"
        );
        let j = json(&body);
        assert!(
            j.get("id").is_some() && !j["id"].as_str().unwrap().is_empty(),
            "response should contain non-empty id: {j}"
        );
        // Real deck name from the deckNames diff (not the filename stem).
        assert_eq!(
            j["name"].as_str(),
            Some("Test Deck"),
            "deck name should come from the import diff: {j}"
        );
        // Real card count from findCards after import (not a size estimate).
        assert_eq!(
            j["card_count"].as_u64(),
            Some(3),
            "card_count should be the real collection count: {j}"
        );
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

/// Non-.apkg filenames are rejected up front — the Zone 2 import path only
/// accepts Anki package archives (.zip was accepted by the old stub but
/// never actually imported).
#[tokio::test]
async fn upload_deck_rejects_non_apkg() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let payload: Vec<u8> = vec![7u8; 256];
        let resp = post_upload(&server, "test-deck.zip", &payload).await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
        let j = json(&resp.text());
        assert_eq!(
            j.get("success").and_then(|v| v.as_bool()),
            Some(false),
            ".zip upload should be rejected: {j}"
        );
        assert!(
            j["message"].as_str().unwrap().contains(".apkg"),
            "rejection should mention .apkg: {j}"
        );
    })
    .await;
}

/// Oversized uploads are rejected by the handler cap
/// (`ANKITOV_UPLOAD_MAX_BYTES` override keeps the test payload small).
#[tokio::test]
async fn upload_deck_rejects_oversized() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        std::env::set_var("ANKITOV_UPLOAD_MAX_BYTES", "1024");
        let payload: Vec<u8> = vec![9u8; 2048];
        let resp = post_upload(&server, "big-deck.apkg", &payload).await;
        std::env::remove_var("ANKITOV_UPLOAD_MAX_BYTES");
        assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
        let j = json(&resp.text());
        assert_eq!(
            j.get("success").and_then(|v| v.as_bool()),
            Some(false),
            "oversized upload should be rejected: {j}"
        );
        assert!(
            j["message"].as_str().unwrap().contains("too large"),
            "rejection should mention the size cap: {j}"
        );
    })
    .await;
}

/// Duplicate upload (same bytes → same SHA-256) is idempotent: the second
/// upload returns the existing deck record and never re-imports.
#[tokio::test]
async fn upload_deck_duplicate_is_idempotent() {
    // Arc so the server survives the `async move` closure and can be
    // verified afterwards.
    let mock = std::sync::Arc::new(start_anki_mock().await);
    let mock_for_verify = mock.clone();
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        set_anki_port(&mock);
        set_unique_upload_dir();
        let payload: Vec<u8> = vec![5u8; 512];

        let first = post_upload(&server, "dup-deck.apkg", &payload).await;
        assert_eq!(first.status_code(), axum::http::StatusCode::OK);
        let first_j = json(&first.text());
        let deck_id = first_j["id"].as_str().unwrap().to_string();

        let second = post_upload(&server, "dup-deck.apkg", &payload).await;
        assert_eq!(second.status_code(), axum::http::StatusCode::OK);
        let second_j = json(&second.text());
        assert_eq!(
            second_j["id"].as_str(),
            Some(deck_id.as_str()),
            "duplicate upload should return the existing deck id: {second_j}"
        );
    })
    .await;
    // Exactly one importPackage call reached AnkiConnect across both uploads.
    mock_for_verify.verify().await;
}

/// When Anki is unreachable the upload fails with a 503 carrying the
/// actionable `anki_unreachable` error — not an opaque 500.
#[tokio::test]
async fn upload_deck_anki_unreachable_returns_503() {
    // Bind-and-drop a listener to claim a port with nothing behind it.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let dead_port = listener.local_addr().unwrap().port();
    drop(listener);
    std::env::set_var("ANKICONNECT_PORT", dead_port.to_string());
    set_unique_upload_dir();

    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let payload: Vec<u8> = vec![1u8; 256];
        let resp = post_upload(&server, "no-anki-deck.apkg", &payload).await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "unreachable AnkiConnect should yield 503, got {}: {}",
            resp.status_code(),
            resp.text()
        );
        let j = json(&resp.text());
        assert!(
            j.to_string().contains("anki_unreachable"),
            "503 body should carry the anki_unreachable error code: {j}"
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
    let mock = start_anki_mock().await;
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        set_anki_port(&mock);
        set_unique_upload_dir();
        // Upload
        let payload: Vec<u8> = (0u8..16).cycle().take(1024).collect();
        let upload_resp = post_upload(&server, "test-deck.apkg", &payload).await;
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
    let mock = start_anki_mock().await;
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        set_anki_port(&mock);
        set_unique_upload_dir();
        // Upload
        let payload: Vec<u8> = vec![42u8; 512];
        let upload_resp = post_upload(&server, "test-deck.apkg", &payload).await;
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
    let mock = start_anki_mock().await;
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        set_anki_port(&mock);
        set_unique_upload_dir();
        // Upload
        let payload: Vec<u8> = vec![99u8; 512];
        let upload_resp = post_upload(&server, "test-deck.apkg", &payload).await;
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
    let mock = start_anki_mock().await;
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        set_anki_port(&mock);
        set_unique_upload_dir();
        // Upload
        let payload: Vec<u8> = vec![1u8; 1024];
        let upload_resp = post_upload(&server, "test-deck.apkg", &payload).await;
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
    let mock = start_anki_mock().await;
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        set_anki_port(&mock);
        set_unique_upload_dir();
        let payload: Vec<u8> = vec![2u8; 512];
        let upload_resp = post_upload(&server, "test-deck.apkg", &payload).await;
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
    let mock = start_anki_mock().await;
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        set_anki_port(&mock);
        set_unique_upload_dir();
        let payload: Vec<u8> = vec![3u8; 512];
        let upload_resp = post_upload(&server, "test-deck.apkg", &payload).await;
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