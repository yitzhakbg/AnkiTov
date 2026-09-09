//! Producer upload + verification endpoints — P1 end-to-end tests (spec:
//! retention-period-plan §Phase 1).
//!
//! Mirrors the H1 white-box patterns in `endpoint_coverage_test.rs` /
//! `management_decks_test.rs`: in-process Axum via `loco_rs::testing` against
//! an in-memory SQLite DB, JWT-minted auth, hand-built multipart bodies.
//!
//! Anki-free by construction: `ANKITOV_ANKI=test` selects the deterministic
//! [`backend::services::deck_verification::StubVerifier`] (house pattern —
//! mirrors `ANKITOV_NLU_PROVIDER=test`), so `cargo nextest` never needs
//! headless Anki. The real headless-Anki path is exercised by the black-box
//! harness (H2) against the Phase 0 seed.
//!
//! Coverage:
//! 1. `POST /management/producers` — create producer (admin) + validation + RBAC.
//! 2. `GET /management/producers` — list producers + deck catalog (admin/teacher).
//! 3. `GET /management/producers/:id/decks` — per-producer catalog + 404.
//! 4. `POST /management/producers/:id/decks` — multipart `.apkg` upload →
//!    verify → `deck` row (real card count) + `producer_deck` row
//!    (`verified` | `rejected` with reason) + RBAC + 400/404 paths.
//!
//! Run with: `cargo nextest run -p backend --test producer_decks_test`

use loco_rs::testing;
use serde_json::{json, Value};
use serial_test::serial;

/// Env var selecting the verification backend (`test` → deterministic stub).
const ENV_ANKI_PROVIDER: &str = "ANKITOV_ANKI";

/// Card count the `ANKITOV_ANKI=test` stub reports for ZIP-magic packages
/// (see `deck_verification::STUB_CARD_COUNT`).
const STUB_CARD_COUNT: i64 = 12;

/// ZIP local-file-header magic — every real `.apkg` is a ZIP archive.
const ZIP_MAGIC: &[u8] = &[b'P', b'K', 0x03, 0x04];

/// Parse a response body as JSON, panicking with the raw body on failure.
fn json_body(text: &str) -> Value {
    match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => panic!("invalid JSON: {e}\nbody: {text:?}"),
    }
}

/// Select the deterministic Anki-free stub for this test. nextest runs each
/// test in its own process; `#[serial]` guards plain `cargo test` runs.
fn use_stub_verifier() {
    std::env::set_var(ENV_ANKI_PROVIDER, "test");
}

fn admin_token() -> String {
    backend::services::auth::encode_token(901, "admin", "producer-admin@example.edu", 86400)
        .expect("minting admin token must succeed")
}

fn teacher_token() -> String {
    backend::services::auth::encode_token(902, "teacher", "producer-teacher@example.edu", 86400)
        .expect("minting teacher token must succeed")
}

fn student_token() -> String {
    backend::services::auth::encode_token(903, "student", "producer-student@example.edu", 86400)
        .expect("minting student token must succeed")
}

/// Multipart body with a `file` field — the producers upload handler reads
/// the field named `file` (same contract as `/management/decks` upload).
fn multipart_upload_body(filename: &str, data: &[u8]) -> (String, Vec<u8>) {
    let boundary = "ankitov-producer-test-bnd-77";
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

/// POST a create-producer request and return the raw JSON response body.
async fn post_create_producer(
    server: &loco_rs::TestServer,
    token: &str,
    payload: &Value,
) -> (axum::http::StatusCode, Value) {
    let resp = server
        .post("/api/v1/management/producers")
        .add_header("Authorization", format!("Bearer {token}"))
        .content_type("application/json")
        .bytes(axum::body::Bytes::from(payload.to_string()))
        .await;
    let status = resp.status_code();
    let body = json_body(&resp.text());
    (status, body)
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /management/producers — create producer (admin)
// ─────────────────────────────────────────────────────────────────────────────

/// Admin creates a producer; the row is retrievable via the list endpoint.
#[tokio::test]
#[serial]
async fn create_producer_round_trip() {
    use_stub_verifier();
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let (status, body) = post_create_producer(
            &server,
            &admin_token(),
            &json!({
                "name": "AnkiTov Pilot Seed",
                "email": "seed@ankitov.example",
                "contact_note": "one-time pilot seed (self-authored)"
            }),
        )
        .await;
        assert_eq!(
            status,
            axum::http::StatusCode::OK,
            "create producer should succeed: {body}"
        );
        assert!(body["id"].as_i64().unwrap_or(0) > 0, "id must be set: {body}");
        assert_eq!(body["name"], "AnkiTov Pilot Seed");
        assert_eq!(body["email"], "seed@ankitov.example");

        // The created producer appears in the list with an empty catalog.
        let resp = server
            .get("/api/v1/management/producers")
            .add_header("Authorization", format!("Bearer {}", admin_token()))
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
        let list = json_body(&resp.text());
        let arr = list.as_array().expect("list must be an array");
        let found = arr
            .iter()
            .find(|p| p["name"] == "AnkiTov Pilot Seed")
            .unwrap_or_else(|| panic!("created producer missing from list: {list}"));
        assert_eq!(found["decks"], json!([]), "fresh producer has empty catalog");
    })
    .await;
}

/// Teacher and student are forbidden to create producers (admin-only write).
#[tokio::test]
#[serial]
async fn create_producer_requires_admin() {
    use_stub_verifier();
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let payload = json!({"name": "Nope", "email": "nope@x.example", "contact_note": null});
        for (role, token) in [("teacher", teacher_token()), ("student", student_token())] {
            let (status, _) = post_create_producer(&server, &token, &payload).await;
            assert_eq!(
                status,
                axum::http::StatusCode::FORBIDDEN,
                "{role} must be 403 on POST /producers"
            );
        }
    })
    .await;
}

/// Blank name/email are rejected with 400 before any DB write.
#[tokio::test]
#[serial]
async fn create_producer_rejects_blank_fields() {
    use_stub_verifier();
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let (status, _) = post_create_producer(
            &server,
            &admin_token(),
            &json!({"name": "  ", "email": "x@y.example", "contact_note": null}),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);

        let (status, _) = post_create_producer(
            &server,
            &admin_token(),
            &json!({"name": "Valid Name", "email": "", "contact_note": null}),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    })
    .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /management/producers + /:producer_id/decks — read surface + RBAC
// ─────────────────────────────────────────────────────────────────────────────

/// Read surface is open to admin AND teacher, forbidden to student.
#[tokio::test]
#[serial]
async fn list_producers_rbac() {
    use_stub_verifier();
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        // Student → 403.
        let resp = server
            .get("/api/v1/management/producers")
            .add_header("Authorization", format!("Bearer {}", student_token()))
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::FORBIDDEN);

        // Teacher → 200 (empty catalog on a fresh DB).
        let resp = server
            .get("/api/v1/management/producers")
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
        assert!(json_body(&resp.text()).is_array());

        // Catalog sub-endpoint follows the same RBAC.
        let resp = server
            .get("/api/v1/management/producers/1/decks")
            .add_header("Authorization", format!("Bearer {}", student_token()))
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::FORBIDDEN);
    })
    .await;
}

/// Catalog for a nonexistent producer → 404 (not an empty 200).
#[tokio::test]
#[serial]
async fn list_producer_decks_missing_producer_404() {
    use_stub_verifier();
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .get("/api/v1/management/producers/424242/decks")
            .add_header("Authorization", format!("Bearer {}", admin_token()))
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::NOT_FOUND);
    })
    .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /management/producers/:producer_id/decks — upload → verify → catalog
// ─────────────────────────────────────────────────────────────────────────────

/// Happy path: a ZIP-magic `.apkg` verifies → `producer_deck.status ==
/// "verified"` and the `deck` row carries the REAL card count from the
/// verification report (not a byte-size estimate).
#[tokio::test]
#[serial]
async fn upload_producer_deck_verified_end_to_end() {
    use_stub_verifier();
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        // Arrange: a producer to upload against.
        let (_, producer) = post_create_producer(
            &server,
            &admin_token(),
            &json!({"name": "Seed Producer", "email": "seed-p@ankitov.example", "contact_note": null}),
        )
        .await;
        let producer_id = producer["id"].as_i64().expect("producer id");

        // Act: upload a ZIP-magic package (what the stub verifies).
        let mut pkg = ZIP_MAGIC.to_vec();
        pkg.extend_from_slice(b"stub-apkg-payload");
        let (content_type, body_bytes) = multipart_upload_body("happy-seed.apkg", &pkg);
        let resp = server
            .post(format!("/api/v1/management/producers/{producer_id}/decks").as_str())
            .add_header("Authorization", format!("Bearer {}", admin_token()))
            .content_type(&content_type)
            .bytes(axum::body::Bytes::from(body_bytes))
            .await;

        // Assert: verified with the stub's deterministic real card count.
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "upload+verify should succeed: {}",
            resp.text()
        );
        let upload = json_body(&resp.text());
        assert_eq!(upload["status"], "verified", "ZIP package must verify: {upload}");
        assert_eq!(upload["card_count"], STUB_CARD_COUNT, "card_count is the real verified count");
        assert_eq!(upload["reject_reason"], Value::Null);
        let deck_id = upload["deck_id"].as_i64().expect("deck_id");
        assert!(deck_id > 0);
        // Census: 7 basic + 3 cloze + 2 image occlusion = 12 (stub contract).
        assert_eq!(upload["card_type_census"]["basic"], 7);
        assert_eq!(upload["card_type_census"]["cloze"], 3);
        assert_eq!(upload["card_type_census"]["image_occlusion"], 2);
        assert_eq!(upload["card_type_census"]["video"], 0);

        // The catalog row reflects verified state end-to-end.
        let resp = server
            .get(format!("/api/v1/management/producers/{producer_id}/decks").as_str())
            .add_header("Authorization", format!("Bearer {}", teacher_token()))
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
        let catalog = json_body(&resp.text());
        let entries = catalog.as_array().expect("catalog must be an array");
        assert_eq!(entries.len(), 1, "exactly one catalog row: {catalog}");
        let entry = &entries[0];
        assert_eq!(entry["status"], "verified");
        assert_eq!(entry["deck_id"], deck_id);
        assert_eq!(entry["deck_name"], "happy-seed", "deck name derives from the filename");
        assert_eq!(entry["card_count"], STUB_CARD_COUNT, "catalog reports the deck's real card count");
        assert!(entry["verified_at"].is_i64(), "verified_at set on verified rows: {entry}");
        assert_eq!(entry["reject_reason"], Value::Null);

        // The producer-level list aggregates the same catalog entry.
        let resp = server
            .get("/api/v1/management/producers")
            .add_header("Authorization", format!("Bearer {}", admin_token()))
            .await;
        let list = json_body(&resp.text());
        let agg = list
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == producer_id)
            .expect("producer present in aggregate list");
        assert_eq!(agg["decks"][0]["status"], "verified", "aggregate catalog matches: {agg}");
    })
    .await;
}

/// Rejection path: corrupt/foreign bytes → status `rejected` with a
/// human-readable `reject_reason`, card count 0, no `verified_at`.
#[tokio::test]
#[serial]
async fn upload_producer_deck_rejects_corrupt_package() {
    use_stub_verifier();
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let (_, producer) = post_create_producer(
            &server,
            &admin_token(),
            &json!({"name": "Corrupt Producer", "email": "corrupt-p@ankitov.example", "contact_note": null}),
        )
        .await;
        let producer_id = producer["id"].as_i64().expect("producer id");

        let (content_type, body_bytes) =
            multipart_upload_body("corrupt.apkg", b"definitely not a zip archive");
        let resp = server
            .post(format!("/api/v1/management/producers/{producer_id}/decks").as_str())
            .add_header("Authorization", format!("Bearer {}", admin_token()))
            .content_type(&content_type)
            .bytes(axum::body::Bytes::from(body_bytes))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "rejection is a report, not an error: {}",
            resp.text()
        );
        let upload = json_body(&resp.text());
        assert_eq!(upload["status"], "rejected");
        let reason = upload["reject_reason"]
            .as_str()
            .unwrap_or_else(|| panic!("rejected upload must carry a reason: {upload}"));
        assert!(
            reason.to_ascii_lowercase().contains("zip"),
            "reason should explain the missing ZIP header: {reason}"
        );
        assert_eq!(upload["card_count"], 0);

        // Catalog shows the rejected row without a verification timestamp.
        let resp = server
            .get(format!("/api/v1/management/producers/{producer_id}/decks").as_str())
            .add_header("Authorization", format!("Bearer {}", admin_token()))
            .await;
        let catalog = json_body(&resp.text());
        let entry = &catalog.as_array().unwrap()[0];
        assert_eq!(entry["status"], "rejected");
        assert_eq!(entry["verified_at"], Value::Null, "rejected rows are never verified");
        assert!(entry["reject_reason"].is_string(), "reason persisted: {entry}");
    })
    .await;
}

/// Upload is admin-only: teacher and student → 403, no catalog row written.
#[tokio::test]
#[serial]
async fn upload_producer_deck_requires_admin() {
    use_stub_verifier();
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let (_, producer) = post_create_producer(
            &server,
            &admin_token(),
            &json!({"name": "RBAC Producer", "email": "rbac-p@ankitov.example", "contact_note": null}),
        )
        .await;
        let producer_id = producer["id"].as_i64().expect("producer id");

        let (content_type, body_bytes) =
            multipart_upload_body("forbidden.apkg", ZIP_MAGIC);
        for (role, token) in [("teacher", teacher_token()), ("student", student_token())] {
            let resp = server
                .post(format!("/api/v1/management/producers/{producer_id}/decks").as_str())
                .add_header("Authorization", format!("Bearer {token}"))
                .content_type(&content_type)
                .bytes(axum::body::Bytes::from(body_bytes.clone()))
                .await;
            assert_eq!(
                resp.status_code(),
                axum::http::StatusCode::FORBIDDEN,
                "{role} must be 403 on deck upload"
            );
        }

        // Nothing was verified or cataloged by the forbidden attempts.
        let resp = server
            .get(format!("/api/v1/management/producers/{producer_id}/decks").as_str())
            .add_header("Authorization", format!("Bearer {}", admin_token()))
            .await;
        assert_eq!(json_body(&resp.text()), json!([]), "no catalog rows from 403s");
    })
    .await;
}

/// Empty file upload → 400 (the controller refuses to verify empty payloads).
#[tokio::test]
#[serial]
async fn upload_producer_deck_empty_file_400() {
    use_stub_verifier();
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let (_, producer) = post_create_producer(
            &server,
            &admin_token(),
            &json!({"name": "Empty Producer", "email": "empty-p@ankitov.example", "contact_note": null}),
        )
        .await;
        let producer_id = producer["id"].as_i64().expect("producer id");

        let (content_type, body_bytes) = multipart_upload_body("empty.apkg", &[]);
        let resp = server
            .post(format!("/api/v1/management/producers/{producer_id}/decks").as_str())
            .add_header("Authorization", format!("Bearer {}", admin_token()))
            .content_type(&content_type)
            .bytes(axum::body::Bytes::from(body_bytes))
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::BAD_REQUEST);
    })
    .await;
}

/// Upload against a nonexistent producer → 404.
#[tokio::test]
#[serial]
async fn upload_producer_deck_missing_producer_404() {
    use_stub_verifier();
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let (content_type, body_bytes) = multipart_upload_body("ghost.apkg", ZIP_MAGIC);
        let resp = server
            .post("/api/v1/management/producers/424242/decks")
            .add_header("Authorization", format!("Bearer {}", admin_token()))
            .content_type(&content_type)
            .bytes(axum::body::Bytes::from(body_bytes))
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::NOT_FOUND);
    })
    .await;
}

/// Missing credentials → 401 on both the read and write surfaces.
#[tokio::test]
#[serial]
async fn producers_require_authentication() {
    use_stub_verifier();
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server.get("/api/v1/management/producers").await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::UNAUTHORIZED);

        let resp = server.post("/api/v1/management/producers").await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::UNAUTHORIZED);
    })
    .await;
}
