//! School settings — management endpoints + setup-wizard integration tests.
//!
//! Validates the `/api/v1/management/settings` surface introduced with the
//! Settings view and guided setup wizard (T-126):
//!
//! 1. **Auth gate** — `GET` without a token is `401`; a teacher may read but
//!    not write (`PUT` → `403`); only admins write.
//! 2. **Wizard roundtrip** — the JWT minted by `POST /api/v1/setup` works on
//!    `PUT /management/settings`, and values persist across `GET`s.
//! 3. **Validate-all-then-write** — an invalid locale / sync URL / oversized
//!    school name is `400` and writes *nothing*.
//! 4. **Partial updates** — a `PUT` with one field leaves the others intact.
//!
//! White-box, in-process tests (Harness 1): each test boots the Loco app
//! against a fresh in-memory SQLite DB, so settings start empty and tests
//! cannot pollute each other.
//!
//! Run with: `cargo nextest run settings_test`

use loco_rs::testing;
use serde_json::Value;

const API: &str = "/api/v1/management/settings";

/// Parse a response body as JSON, panicking with the raw body on failure.
fn json(text: &str) -> Value {
    match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => panic!("invalid JSON from response body: {e}\nbody: {text:?}"),
    }
}

/// Mint a valid JWT for the given subject/role/email (24 h). Uses the current
/// process `ANKITOV_JWT_SECRET`, so it verifies against the test server.
fn mint(user_id: i64, role: &str, email: &str) -> String {
    backend::services::auth::encode_token(user_id, role, email, 86400)
        .expect("minting JWT for the settings harness must succeed")
}

/// The canonical setup payload used to obtain a real admin token.
fn setup_payload() -> Value {
    serde_json::json!({
        "username": "settingsadmin",
        "email": "settings-admin@example.com",
        "password": "Sup3r-Secret-Passw0rd!",
        "full_name": "Settings Admin"
    })
}

// ---------------------------------------------------------------------------
// 1. Auth gate
// ---------------------------------------------------------------------------

/// `GET /management/settings` without a token is rejected by the management
/// auth middleware.
#[tokio::test]
async fn settings_get_without_token_is_401() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server.get(API).await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::UNAUTHORIZED,
            "GET settings without a token must be 401, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
    })
    .await;
}

/// Teachers may read the school settings but must not write them.
#[tokio::test]
async fn settings_teacher_can_read_but_not_write() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let read = server
            .get(API)
            .add_header("Authorization", format!("Bearer {}", mint(700, "teacher", "t@example.edu")))
            .await;
        assert_eq!(
            read.status_code(),
            axum::http::StatusCode::OK,
            "teacher GET settings must be 200, got {} (body: {})",
            read.status_code(),
            read.text()
        );
        let body = json(&read.text());
        assert_eq!(body["school_name"], Value::Null, "fresh DB must report null school_name");
        assert_eq!(body["locale"], Value::Null);
        assert_eq!(body["sync_url"], Value::Null);

        let write = server
            .put(API)
            .add_header("Authorization", format!("Bearer {}", mint(701, "teacher", "t2@example.edu")))
            .json(&serde_json::json!({ "school_name": "Nope" }))
            .await;
        assert_eq!(
            write.status_code(),
            axum::http::StatusCode::FORBIDDEN,
            "teacher PUT settings must be 403, got {} (body: {})",
            write.status_code(),
            write.text()
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 2. Wizard roundtrip (setup-minted admin token)
// ---------------------------------------------------------------------------

/// The token minted by `POST /api/v1/setup` authorizes `PUT
/// /management/settings` — the exact flow the guided wizard uses — and the
/// written values persist on `GET`.
#[tokio::test]
async fn settings_admin_roundtrip_via_setup_token() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        // Create the admin through the real setup endpoint.
        let created = server
            .post("/api/v1/setup")
            .json(&setup_payload())
            .await;
        assert_eq!(
            created.status_code(),
            axum::http::StatusCode::OK,
            "POST /setup on a fresh DB must be 200, got {} (body: {})",
            created.status_code(),
            created.text()
        );
        let token = json(&created.text())["token"]
            .as_str()
            .expect("setup response must carry a JWT")
            .to_string();

        // Wizard step 3: save the essentials with the minted token.
        let put = server
            .put(API)
            .add_header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({
                "school_name": "Maple Grove Co-op",
                "locale": "fr",
                "sync_url": "https://sync.example.edu"
            }))
            .await;
        assert_eq!(
            put.status_code(),
            axum::http::StatusCode::OK,
            "admin PUT settings must be 200, got {} (body: {})",
            put.status_code(),
            put.text()
        );
        let body = json(&put.text());
        assert_eq!(body["school_name"], "Maple Grove Co-op");
        assert_eq!(body["locale"], "fr");
        assert_eq!(body["sync_url"], "https://sync.example.edu");

        // Values persist on a fresh read.
        let get = server
            .get(API)
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        assert_eq!(get.status_code(), axum::http::StatusCode::OK);
        let body = json(&get.text());
        assert_eq!(body["school_name"], "Maple Grove Co-op");
        assert_eq!(body["locale"], "fr");
        assert_eq!(body["sync_url"], "https://sync.example.edu");
    })
    .await;
}

// ---------------------------------------------------------------------------
// 3. Validate-all-then-write
// ---------------------------------------------------------------------------

/// An unsupported locale is `400` and writes nothing.
#[tokio::test]
async fn settings_invalid_locale_rejected_writes_nothing() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let token = mint(710, "admin", "a@example.edu");

        let put = server
            .put(API)
            .add_header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({ "locale": "xx-XX" }))
            .await;
        assert_eq!(
            put.status_code(),
            axum::http::StatusCode::BAD_REQUEST,
            "invalid locale must be 400, got {} (body: {})",
            put.status_code(),
            put.text()
        );

        let get = server
            .get(API)
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        let body = json(&get.text());
        assert_eq!(body["locale"], Value::Null, "a rejected write must not persist");
    })
    .await;
}

/// A sync URL without an http(s) scheme is `400`.
#[tokio::test]
async fn settings_invalid_sync_url_rejected() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let put = server
            .put(API)
            .add_header("Authorization", format!("Bearer {}", mint(711, "admin", "a@example.edu")))
            .json(&serde_json::json!({ "sync_url": "ftp://example.edu/decks" }))
            .await;
        assert_eq!(
            put.status_code(),
            axum::http::StatusCode::BAD_REQUEST,
            "non-http sync URL must be 400, got {} (body: {})",
            put.status_code(),
            put.text()
        );
    })
    .await;
}

/// A school name over the 200-char limit is `400`.
#[tokio::test]
async fn settings_school_name_too_long_rejected() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let long_name = "a".repeat(201);
        let put = server
            .put(API)
            .add_header("Authorization", format!("Bearer {}", mint(712, "admin", "a@example.edu")))
            .json(&serde_json::json!({ "school_name": long_name }))
            .await;
        assert_eq!(
            put.status_code(),
            axum::http::StatusCode::BAD_REQUEST,
            "201-char school name must be 400, got {} (body: {})",
            put.status_code(),
            put.text()
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 4. Partial updates
// ---------------------------------------------------------------------------

/// A `PUT` with one field leaves the other keys untouched.
#[tokio::test]
async fn settings_partial_update_keeps_other_fields() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let token = mint(720, "admin", "a@example.edu");

        let first = server
            .put(API)
            .add_header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({ "school_name": "Maple Grove Co-op" }))
            .await;
        assert_eq!(first.status_code(), axum::http::StatusCode::OK);

        let second = server
            .put(API)
            .add_header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({ "locale": "de" }))
            .await;
        assert_eq!(second.status_code(), axum::http::StatusCode::OK);

        let get = server
            .get(API)
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        let body = json(&get.text());
        assert_eq!(
            body["school_name"], "Maple Grove Co-op",
            "a locale-only PUT must not clear school_name"
        );
        assert_eq!(body["locale"], "de");
        assert_eq!(body["sync_url"], Value::Null, "unset keys must stay null");
    })
    .await;
}

/// An empty `PUT` body is a no-op success (nothing validated, nothing written).
#[tokio::test]
async fn settings_empty_put_is_noop() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let token = mint(730, "admin", "a@example.edu");
        let put = server
            .put(API)
            .add_header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({}))
            .await;
        assert_eq!(
            put.status_code(),
            axum::http::StatusCode::OK,
            "empty PUT must be 200, got {} (body: {})",
            put.status_code(),
            put.text()
        );
        let get = server
            .get(API)
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        let body = json(&get.text());
        assert_eq!(body["school_name"], Value::Null);
        assert_eq!(body["locale"], Value::Null);
        assert_eq!(body["sync_url"], Value::Null);
    })
    .await;
}
