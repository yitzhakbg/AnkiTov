//! Auth harness — Phase 0 (authentication & authorization) integration tests.
//!
//! Validates the JWT auth middleware, role gating, and the public/auth
//! endpoint surface introduced in `specs/plans/2026-08-28-nl-ops-chatbox.md`
//! Phase 0:
//!
//! 1. **`/management/*` requires auth** — a request with no bearer token is
//!    rejected with `401 Unauthorized`.
//! 2. **Teacher / admin can read** — a valid teacher (or admin) JWT is
//!    accepted by `/management/*` read endpoints.
//! 3. **Student is read-only** — a valid student JWT is *accepted* by read
//!    endpoints but rejected with `403 Forbidden` on write endpoints.
//! 4. **Public endpoints stay open** — `/health` and `/auth/*` work without a
//!    token (they are deliberately NOT behind `require_auth`).
//! 5. **Logout** — `POST /auth/logout` with a valid token returns `200`.
//!
//! These are *white-box, in-process* tests (Harness 1): they boot the Loco app
//! against an in-memory SQLite DB (`config/test.yaml` → `sqlite::memory:` with
//! `auto_migrate`) and drive the real Axum router via `loco_rs::testing`.
//!
//! Run with: `cargo nextest run harness_auth`
//!
//! ## How tokens are minted
//!
//! Rather than registering users through the DB and re-logging in, we mint
//! JWTs directly with `backend::services::auth::encode_token`. Because the
//! middleware verifies signatures with the same `ANKITOV_JWT_SECRET` the test
//! process uses, these hand-minted tokens pass `require_auth` without any DB
//! row — which keeps the tests fast, deterministic, and free of
//! bcrypt/password round-trips.

use loco_rs::testing;
use serde_json::Value;

/// Parse a response body as JSON, panicking with the raw body on failure.
fn json(text: &str) -> Value {
    match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => panic!("invalid JSON from response body: {e}\nbody: {text:?}"),
    }
}

/// Mint a valid JWT for the given subject/role/email with a 24 h expiry.
///
/// Uses the *current* process `ANKITOV_JWT_SECRET` (see `services::auth`), so
/// the token verifies against whatever secret the test server is using.
fn mint_token(user_id: i64, role: &str, email: &str) -> String {
    backend::services::auth::encode_token(user_id, role, email, 86400)
        .expect("minting a JWT for the auth harness must succeed")
}

/// A tampered token: a structurally valid JWT but signed with a *different*
/// secret, so signature verification fails → the middleware must return 401.
fn mint_garbage_token() -> String {
    use jsonwebtoken::{encode, EncodingKey, Header};
    use backend::services::auth::Claims;
    let now = chrono::Utc::now().timestamp() as usize;
    let claims = Claims {
        sub: 1,
        role: "admin".into(),
        email: "attacker@example.com".into(),
        exp: now + 86400,
        iat: now,
    };
    // A deliberately different secret so the signature won't match.
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(b"wrong-secret-just-for-the-harness"),
    )
    .expect("encoding a garbage token must succeed")
}

// ---------------------------------------------------------------------------
// 1. /management/* requires auth (no token → 401)
// ---------------------------------------------------------------------------

/// No Authorization header → 401 on a `/management/*` read endpoint.
#[tokio::test]
async fn management_read_without_token_is_401() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server.get("/api/v1/management/decks").await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::UNAUTHORIZED,
            "GET /management/decks without a token must be 401, got {}",
            resp.status_code()
        );
    })
    .await;
}

/// A different `/management/*` controller also enforces auth.
#[tokio::test]
async fn management_classes_without_token_is_401() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server.get("/api/v1/management/classes").await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::UNAUTHORIZED);
    })
    .await;
}

/// A garbage (wrong-secret) token is treated the same as no token → 401.
#[tokio::test]
async fn management_read_with_garbage_token_is_401() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .get("/api/v1/management/decks")
            .add_header("Authorization", format!("Bearer {}", mint_garbage_token()))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::UNAUTHORIZED,
            "a token signed with the wrong secret must be 401, got {}",
            resp.status_code()
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 2. Teacher / admin can read (valid token → 200)
// ---------------------------------------------------------------------------

/// A valid teacher JWT is accepted by a `/management/*` read endpoint.
#[tokio::test]
async fn management_read_as_teacher_is_200() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .get("/api/v1/management/decks")
            .add_header("Authorization", format!("Bearer {}", mint_token(101, "teacher", "t@example.edu")))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "teacher GET /management/decks must be 200, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
    })
    .await;
}

/// An admin JWT is accepted too (admin is in the teacher/admin allow-list).
#[tokio::test]
async fn management_read_as_admin_is_200() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .get("/api/v1/management/decks")
            .add_header("Authorization", format!("Bearer {}", mint_token(102, "admin", "a@example.edu")))
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    })
    .await;
}

// ---------------------------------------------------------------------------
// 3. Student is read-only
// ---------------------------------------------------------------------------

/// A valid student JWT is accepted by a *read* endpoint (200).
#[tokio::test]
async fn management_read_as_student_is_200() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .get("/api/v1/management/decks")
            .add_header("Authorization", format!("Bearer {}", mint_token(201, "student", "s@example.edu")))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "student GET /management/decks must be 200 (reads are allowed), got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
    })
    .await;
}

/// A student JWT on a *write* endpoint is rejected with 403 Forbidden.
///
/// `POST /management/classes` (create) is role-gated to teacher/admin, so a
/// student token must be stopped with 403 before any write occurs.
#[tokio::test]
async fn management_write_as_student_is_403() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .post("/api/v1/management/classes")
            .add_header("Authorization", format!("Bearer {}", mint_token(201, "student", "s@example.edu")))
            // `CreateClassRequest` requires `teacher_id`; we include it so the
            // request validates and reaches the *role* gate (which yields 403
            // for a student) rather than a 422 validation error first.
            .json(&serde_json::json!({
                "name": "Class A",
                "teacher_id": "1"
            }))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::FORBIDDEN,
            "student POST /management/classes must be 403 (read-only), got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
    })
    .await;
}

/// A second write endpoint is also 403 for a student (proves the gate is
/// per-handler, not a one-off).
#[tokio::test]
async fn management_delete_as_student_is_403() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .delete("/api/v1/management/classes/999")
            .add_header("Authorization", format!("Bearer {}", mint_token(201, "student", "s@example.edu")))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::FORBIDDEN,
            "student DELETE /management/classes/:id must be 403, got {}",
            resp.status_code()
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 4. Public endpoints stay open (no token)
// ---------------------------------------------------------------------------

/// `/api/v1/health` is public — works with no token.
#[tokio::test]
async fn health_is_public() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server.get("/api/v1/health").await;
        // Health may be 200 (db ok) or 503 (db degraded); either way it must
        // NOT be a 401 — the point is it is not behind require_auth.
        assert_ne!(
            resp.status_code(),
            axum::http::StatusCode::UNAUTHORIZED,
            "GET /health must be public, not 401"
        );
        let body = json(&resp.text());
        assert_eq!(body.get("service").and_then(|v| v.as_str()), Some("ankitov"));
    })
    .await;
}

/// The dashboard health probe (mounted outside the `/api/v1` prefix, with no
/// auth layer at all) is also public.
#[tokio::test]
async fn dashboard_health_is_public() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server.get("/health").await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
        assert_eq!(resp.text(), "OK");
    })
    .await;
}

/// `/auth/register` is public — registration must not require a prior token.
/// (The auth controller is deliberately NOT wrapped in `require_auth`.)
#[tokio::test]
async fn auth_register_is_public() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .post("/api/v1/auth/register")
            .json(&serde_json::json!({
                "username": "public_reg_test",
                "email": "public_reg_test@example.com",
                "password": "password123",
                "role": "student"
            }))
            .await;
        // 200 (created) or 409/422 (already exists on a re-run) are both
        // acceptable — the key assertion is that it is NOT a 401.
        assert_ne!(
            resp.status_code(),
            axum::http::StatusCode::UNAUTHORIZED,
            "POST /auth/register must be public, not 401 (got {})",
            resp.status_code()
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 5. Logout
// ---------------------------------------------------------------------------

/// `POST /auth/logout` with a valid token returns 200 and a logout payload.
#[tokio::test]
async fn logout_with_token_is_200() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .post("/api/v1/auth/logout")
            .add_header("Authorization", format!("Bearer {}", mint_token(301, "student", "bye@example.edu")))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "POST /auth/logout with a token must be 200, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
        let body = json(&resp.text());
        assert_eq!(body.get("logged_out").and_then(|v| v.as_bool()), Some(true));
    })
    .await;
}
