//! Setup wizard — first-run admin provisioning integration tests.
//!
//! Validates the `/api/v1/setup` surface introduced by the first-run setup
//! wizard (T-123 canary friction #1 fix):
//!
//! 1. **Fresh DB is in setup mode** — `GET /setup/status` reports
//!    `setup_required: true` when zero admin accounts exist.
//! 2. **`POST /setup` creates the admin** — returns a JWT + `role: "admin"`,
//!    the credentials work on `/auth/login`, and status flips to `false`.
//! 3. **Setup is atomically disabled** — a second `POST /setup` (sequential
//!    or concurrent) is rejected with `409` and no second admin is created.
//!    The empty-admins guard runs *inside* the creation transaction.
//! 4. **Validation mirrors registration** — missing fields and short
//!    passwords are `400`; a duplicate email is `409`.
//!
//! These are *white-box, in-process* tests (Harness 1): they boot the Loco app
//! against an in-memory SQLite DB (`config/test.yaml` → `sqlite::memory:` with
//! `auto_migrate`) and drive the real Axum router via `loco_rs::testing`.
//! A fresh in-memory DB has zero users, so setup mode is the natural initial
//! state — no fixtures needed.
//!
//! Run with: `cargo nextest run setup_wizard`

use loco_rs::testing;
use serde_json::Value;

/// Parse a response body as JSON, panicking with the raw body on failure.
fn json(text: &str) -> Value {
    match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => panic!("invalid JSON from response body: {e}\nbody: {text:?}"),
    }
}

/// The canonical setup payload used across tests.
fn setup_payload(username: &str, email: &str, password: &str) -> Value {
    serde_json::json!({
        "username": username,
        "email": email,
        "password": password,
        "full_name": "Setup Admin"
    })
}

// ---------------------------------------------------------------------------
// 1. Status on a fresh DB
// ---------------------------------------------------------------------------

/// A fresh (zero-user) DB is in setup mode.
#[tokio::test]
async fn setup_status_true_when_no_admins() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server.get("/api/v1/setup/status").await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "GET /setup/status must be public and 200, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
        let body = json(&resp.text());
        assert_eq!(
            body.get("setup_required").and_then(|v| v.as_bool()),
            Some(true),
            "fresh DB with zero admins must report setup_required=true (body: {})",
            resp.text()
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 2. Create the admin
// ---------------------------------------------------------------------------

/// `POST /setup` creates an admin, returns a JWT, the credentials log in, and
/// the status flips to `setup_required: false`.
#[tokio::test]
async fn setup_creates_admin_and_flips_status() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        // Create the admin.
        let resp = server
            .post("/api/v1/setup")
            .json(&setup_payload("setupadmin", "setup-admin@example.com", "Sup3r-Secret-Passw0rd!"))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "POST /setup on a fresh DB must be 200, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
        let body = json(&resp.text());
        assert_eq!(body.get("role").and_then(|v| v.as_str()), Some("admin"));
        assert_eq!(
            body.get("email").and_then(|v| v.as_str()),
            Some("setup-admin@example.com")
        );
        assert!(
            body.get("token").and_then(|v| v.as_str()).is_some_and(|t| !t.is_empty()),
            "setup response must include a JWT (body: {})",
            resp.text()
        );

        // The created credentials work on the normal login flow.
        let resp = server
            .post("/api/v1/auth/login")
            .json(&serde_json::json!({
                "email": "setup-admin@example.com",
                "password": "Sup3r-Secret-Passw0rd!"
            }))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "login with the setup-created admin must be 200, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
        let body = json(&resp.text());
        assert_eq!(body.get("role").and_then(|v| v.as_str()), Some("admin"));

        // Status flips: setup is no longer required.
        let resp = server.get("/api/v1/setup/status").await;
        let body = json(&resp.text());
        assert_eq!(
            body.get("setup_required").and_then(|v| v.as_bool()),
            Some(false),
            "after admin creation setup_required must be false (body: {})",
            resp.text()
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 3. Atomic disable
// ---------------------------------------------------------------------------

/// A sequential second `POST /setup` is rejected with 409 and does not create
/// a second admin.
#[tokio::test]
async fn setup_second_post_is_409() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let first = server
            .post("/api/v1/setup")
            .json(&setup_payload("setupadmin", "setup-admin@example.com", "Sup3r-Secret-Passw0rd!"))
            .await;
        assert_eq!(first.status_code(), axum::http::StatusCode::OK);

        let second = server
            .post("/api/v1/setup")
            .json(&setup_payload("otheradmin", "other-admin@example.com", "Sup3r-Secret-Passw0rd!"))
            .await;
        assert_eq!(
            second.status_code(),
            axum::http::StatusCode::CONFLICT,
            "second POST /setup must be 409 once an admin exists, got {} (body: {})",
            second.status_code(),
            second.text()
        );
    })
    .await;
}

/// Two *concurrent* `POST /setup` requests race: exactly one succeeds and
/// exactly one admin exists. The empty-admins guard runs inside the creation
/// transaction, so the loser is rejected rather than double-creating.
#[tokio::test]
async fn setup_concurrent_double_post_creates_exactly_one_admin() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        // Same payload on both — the loser must fail on the guard or the
        // unique constraint, never succeed.
        let payload = setup_payload("setupadmin", "setup-admin@example.com", "Sup3r-Secret-Passw0rd!");
        let (a, b) = tokio::join!(
            server.post("/api/v1/setup").json(&payload),
            server.post("/api/v1/setup").json(&payload),
        );
        let ok_count = [a.status_code(), b.status_code()]
            .iter()
            .filter(|s| **s == axum::http::StatusCode::OK)
            .count();
        assert_eq!(
            ok_count, 1,
            "exactly one concurrent POST /setup must succeed (got {ok_count}; bodies: {} / {})",
            a.text(),
            b.text()
        );

        // Exactly one admin exists.
        let resp = server.get("/api/v1/setup/status").await;
        let body = json(&resp.text());
        assert_eq!(
            body.get("setup_required").and_then(|v| v.as_bool()),
            Some(false),
            "after the race an admin exists (body: {})",
            resp.text()
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 4. Validation
// ---------------------------------------------------------------------------

/// Missing fields are rejected: the JSON extractor answers 422 for absent
/// fields, and the handler's own validation answers 400 for present-but-empty
/// fields (same two-layer behavior as `/auth/login` — canary friction F6).
#[tokio::test]
async fn setup_rejects_missing_fields() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        // Absent fields → 422 at the extractor.
        let resp = server
            .post("/api/v1/setup")
            .json(&serde_json::json!({ "username": "onlyuser" }))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            "POST /setup without email/password must be 422, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );

        // Present-but-empty fields → 400 from handler validation.
        let resp = server
            .post("/api/v1/setup")
            .json(&setup_payload("", "setup-admin@example.com", "Sup3r-Secret-Passw0rd!"))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::BAD_REQUEST,
            "POST /setup with an empty username must be 400, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
    })
    .await;
}

/// A short password is rejected with 400 (same rule as registration).
#[tokio::test]
async fn setup_rejects_short_password() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .post("/api/v1/setup")
            .json(&setup_payload("setupadmin", "setup-admin@example.com", "abc"))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::BAD_REQUEST,
            "POST /setup with a short password must be 400, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
    })
    .await;
}

/// A duplicate email is rejected with 409 — even when no admin exists yet
/// (e.g. a teacher self-registered first, then someone tries setup with the
/// same email). No admin is created.
#[tokio::test]
async fn setup_rejects_duplicate_email() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        // Seed a teacher via public self-registration.
        let resp = server
            .post("/api/v1/auth/register")
            .json(&serde_json::json!({
                "username": "seedteacher",
                "email": "taken@example.com",
                "password": "Sup3r-Secret-Passw0rd!",
                "role": "teacher"
            }))
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::OK);

        // Setup with the same email must not succeed.
        let resp = server
            .post("/api/v1/setup")
            .json(&setup_payload("setupadmin", "taken@example.com", "Sup3r-Secret-Passw0rd!"))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::CONFLICT,
            "POST /setup with a taken email must be 409, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );

        // And setup mode is still on (no admin was created).
        let resp = server.get("/api/v1/setup/status").await;
        let body = json(&resp.text());
        assert_eq!(
            body.get("setup_required").and_then(|v| v.as_bool()),
            Some(true),
            "a failed setup attempt must not create an admin (body: {})",
            resp.text()
        );
    })
    .await;
}
