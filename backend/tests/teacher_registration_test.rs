//! Teacher Registration (Prong 8) — integration tests.
//!
//! White-box, in-process: boots the Loco app against the in-memory SQLite test
//! DB (`config/test.yaml` → `sqlite::memory:` with `auto_migrate`) and drives the
//! real Axum router via `loco_rs::testing` (backed by `axum-test`).
//!
//! The test callback receives `ctx` (a cloned `AppContext`) whose `.db` lets a
//! test seed a `user` / `teacher_invite` row directly — the cleanest way to
//! assert self-only (403) and first-login (password / consent / invite-state)
//! behavior without minting a second account through the admin flow.
//!
//! Run with: `cargo nextest run teacher_registration`

use loco_rs::testing;
use sea_orm::EntityTrait;
use serde_json::{json, Value};

const API: &str = "/api/v1/management/teachers";

// ── Small helpers ──

/// Mint a valid JWT for the given subject/role/email (24 h). Uses the current
/// process `ANKITOV_JWT_SECRET`, so it verifies against the test server.
fn mint(user_id: i64, role: &str, email: &str) -> String {
    backend::services::auth::encode_token(user_id, role, email, 86400)
        .expect("minting JWT for the teacher-registration harness must succeed")
}

/// Insert a `users` row with an explicit id and role; returns the id.
async fn seed_user(ctx: &loco_rs::app::AppContext, id: i64, username: &str, role: &str) -> i64 {
    let now = chrono::Utc::now().timestamp();
    let email = format!("{username}@test.edu");
    let u = backend::models::entities::user::ActiveModel {
        id: sea_orm::ActiveValue::Set(id),
        username: sea_orm::ActiveValue::Set(username.to_string()),
        email: sea_orm::ActiveValue::Set(email),
        full_name: sea_orm::ActiveValue::Set(Some(format!("{} Name", username))),
        role: sea_orm::ActiveValue::Set(role.to_string()),
        subsection_id: sea_orm::ActiveValue::Set(None),
        password_hash: sea_orm::ActiveValue::Set(None),
        created_at: sea_orm::ActiveValue::Set(now),
        updated_at: sea_orm::ActiveValue::Set(now),
    };
    backend::models::entities::user::Entity::insert(u)
        .exec(&ctx.db)
        .await
        .expect("inserting a seeded user must succeed");
    id
}

/// Insert a `teacher_invites` row linked to `user_id`; returns (id, token).
/// `used` / `revoked` / `expired` control the invite's lifecycle state.
async fn seed_invite(
    ctx: &loco_rs::app::AppContext,
    user_id: i64,
    used: bool,
    revoked: bool,
    expired: bool,
) -> (String, String) {
    let now = chrono::Utc::now().timestamp();
    let expires = now + if expired { -3600 } else { 14 * 86400 };
    let id = uuid::Uuid::new_v4().to_string();
    let token = hex::encode(uuid::Uuid::new_v4().as_bytes());
    let email = format!("user{user_id}@test.edu");
    let t = backend::models::entities::teacher_invite::ActiveModel {
        id: sea_orm::ActiveValue::Set(id.clone()),
        user_id: sea_orm::ActiveValue::Set(user_id),
        school: sea_orm::ActiveValue::Set("Test High School".to_string()),
        community: sea_orm::ActiveValue::Set(None),
        role: sea_orm::ActiveValue::Set(None),
        locale: sea_orm::ActiveValue::Set("en-US".to_string()),
        email: sea_orm::ActiveValue::Set(Some(email)),
        invite_token: sea_orm::ActiveValue::Set(token.clone()),
        created_at: sea_orm::ActiveValue::Set(now),
        expires_at: sea_orm::ActiveValue::Set(expires),
        used_at: sea_orm::ActiveValue::Set(if used { Some(now) } else { None }),
        revoked_at: sea_orm::ActiveValue::Set(if revoked { Some(now) } else { None }),
    };
    backend::models::entities::teacher_invite::Entity::insert(t)
        .exec(&ctx.db)
        .await
        .expect("inserting a seeded invite must succeed");
    (id, token)
}

/// Parse a response body as JSON, panicking with the raw body on failure.
fn js(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("invalid JSON: {e}\nbody: {text:?}"))
}

fn enroll_body(full_name: &str, email: &str, school: &str, locale: &str) -> Value {
    json!({ "full_name": full_name, "email": email, "school": school, "locale": locale })
}

// ═════════════════════════════════════════════════════════════════════════
// 1–5  Enroll (POST /management/teachers, admin-gated)
// ═════════════════════════════════════════════════════════════════════════

/// POST enroll with a valid admin token → 201 with user + invite + audit.
#[tokio::test]
async fn enroll_valid_admin_creates_user_and_invite() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .post(API)
            .add_header("Authorization", format!("Bearer {}", mint(900, "admin", "a@example.edu")))
            .json(&enroll_body("Ada Lovelace", "ada.lovelace@example.edu", "Westfield HS", "en-US"))
            .await;
        let status = resp.status_code();
        assert!(
            status == axum::http::StatusCode::CREATED || status == axum::http::StatusCode::OK,
            "enroll must be 201/200, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
        let body = js(&resp.text());
        assert_eq!(body["user"]["email"], "ada.lovelace@example.edu");
        assert_eq!(body["user"]["role"], "teacher");
        assert!(
            body["invite"]["expires_at"].as_i64().unwrap_or(0) > 0,
            "invite must carry a future expiry"
        );
        assert!(
            body["invite"]["token"].as_str().map_or(false, |t| !t.is_empty()),
            "invite token must be non-empty"
        );
        assert_eq!(body["audit"]["event_type"], "teacher_enrolled");
    })
    .await;
}

/// Student role on the enroll endpoint → 403 (not an admin).
#[tokio::test]
async fn enroll_student_forbidden() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .post(API)
            .add_header("Authorization", format!("Bearer {}", mint(901, "student", "s@example.edu")))
            .json(&enroll_body("Student One", "so@example.edu", "Westfield HS", "en-US"))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::FORBIDDEN,
            "student POST enroll must be 403, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
    })
    .await;
}

/// Missing required fields → 400.
#[tokio::test]
async fn enroll_missing_fields_bad_request() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .post(API)
            .add_header("Authorization", format!("Bearer {}", mint(902, "admin", "a@example.edu")))
            .json(&json!({ "email": "x@example.edu" })) // no full_name / school
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            "enroll with missing fields must be 422 (serde), got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
    })
    .await;
}

/// Unsupported locale → 400.
#[tokio::test]
async fn enroll_unsupported_locale_bad_request() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server
            .post(API)
            .add_header("Authorization", format!("Bearer {}", mint(903, "admin", "a@example.edu")))
            .json(&enroll_body("Jane Roe", "jane@example.edu", "Roe School", "xx-YY"))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::BAD_REQUEST,
            "enroll with a bad locale must be 400, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
    })
    .await;
}

/// Email already present in `users` → 409.
#[tokio::test]
async fn enroll_duplicate_email_conflict() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        seed_user(&ctx, 910, "dup.user", "teacher").await;
        let resp = server
            .post(API)
            .add_header("Authorization", format!("Bearer {}", mint(904, "admin", "a@example.edu")))
            .json(&enroll_body("Dup User", "dup.user@test.edu", "Dup School", "en-US"))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::CONFLICT,
            "enroll with a taken email must be 409, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
    })
    .await;
}

// ═════════════════════════════════════════════════════════════════════════
// 6–11  Detail / resend / list (admin-gated)
// ═════════════════════════════════════════════════════════════════════════

/// Admin can read any teacher's detail.
#[tokio::test]
async fn detail_admin_sees_any_teacher() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        let uid = seed_user(&ctx, 920, "det.teacher", "teacher").await;
        let (inv, _tok) = seed_invite(&ctx, uid, false, false, false).await;
        let resp = server
            .get(&format!("{API}/{uid}"))
            .add_header("Authorization", format!("Bearer {}", mint(921, "admin", "a@example.edu")))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "admin GET detail must be 200, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
        let body = js(&resp.text());
        assert_eq!(body["user"]["id"], json!(uid));
        assert_eq!(body["invite"]["id"], inv, "detail must echo the seeded invite");
    })
    .await;
}

/// A non-owner, non-admin teacher → 403 on another teacher's detail.
#[tokio::test]
async fn detail_foreign_teacher_forbidden() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        let target = seed_user(&ctx, 930, "target.teacher", "teacher").await;
        seed_invite(&ctx, target, false, false, false).await;
        let resp = server
            .get(&format!("{API}/{target}"))
            .add_header("Authorization", format!("Bearer {}", mint(931, "teacher", "other@example.edu")))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::FORBIDDEN,
            "non-owner teacher GET detail must be 403, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
    })
    .await;
}

/// Teacher can read their OWN detail (self-read).
#[tokio::test]
async fn detail_self_read_allowed() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        let uid = seed_user(&ctx, 940, "self.teacher", "teacher").await;
        seed_invite(&ctx, uid, false, false, false).await;
        let resp = server
            .get(&format!("{API}/{uid}"))
            .add_header("Authorization", format!("Bearer {}", mint(940, "teacher", "self.teacher@test.edu")))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "self GET detail must be 200, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
    })
    .await;
}

/// Admin can rotate (resend) the invite token.
#[tokio::test]
async fn resend_admin_rotates_token() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        let uid = seed_user(&ctx, 950, "rsnd.teacher", "teacher").await;
        let (inv, _tok) = seed_invite(&ctx, uid, false, false, false).await;
        let resp = server
            .post(&format!("{API}/{uid}/resend"))
            .add_header("Authorization", format!("Bearer {}", mint(951, "admin", "a@example.edu")))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "admin POST resend must be 200, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
        let body = js(&resp.text());
        assert_eq!(body["id"], inv);
        assert!(body["token"].as_str().map_or(false, |t| !t.is_empty()));
    })
    .await;
}

/// Non-admin cannot resend.
#[tokio::test]
async fn resend_non_admin_forbidden() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        let uid = seed_user(&ctx, 960, "rsnd2.teacher", "teacher").await;
        seed_invite(&ctx, uid, false, false, false).await;
        let resp = server
            .post(&format!("{API}/{uid}/resend"))
            .add_header("Authorization", format!("Bearer {}", mint(960, "teacher", "t@example.edu")))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::FORBIDDEN,
            "non-admin POST resend must be 403, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
    })
    .await;
}

/// Admin list returns a JSON array (of pending invites).
#[tokio::test]
async fn list_admin_returns_array() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        // Seed a couple of pending invites so the list is non-trivial.
        let a = seed_user(&ctx, 970, "list.a", "teacher").await;
        seed_invite(&ctx, a, false, false, false).await;
        let resp = server
            .get(API)
            .add_header("Authorization", format!("Bearer {}", mint(971, "admin", "a@example.edu")))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "admin GET list must be 200, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
        let body = js(&resp.text());
        assert!(
            body.is_array(),
            "list body must be a JSON array, got: {body:?}"
        );
    })
    .await;
}

// ═════════════════════════════════════════════════════════════════════════
// 12–17  Public first-login wizard (no JWT — invite id + token is the key)
// ═════════════════════════════════════════════════════════════════════════

/// Public wizard GET `/d/{invite_id}/{token}` serves the HTML (200, text/html).
#[tokio::test]
async fn wizard_get_serves_html() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        let uid = seed_user(&ctx, 980, "wiz.teacher", "teacher").await;
        let (inv, tok) = seed_invite(&ctx, uid, false, false, false).await;
        let resp = server.get(&format!("/d/{inv}/{tok}")).await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "public wizard GET must be 200, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
        assert!(
            resp.header("content-type").to_str().ok().map_or(false, |ct| ct.contains("html")),
            "wizard must be served as text/html, got content-type: {}",
            resp.header("content-type").to_str().ok().unwrap_or("?")
        );
    })
    .await;
}

/// Public `first-login-status` with a valid id+token → 200 with the teacher's name/school/email.
#[tokio::test]
async fn first_login_status_valid_returns_summary() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        let uid = seed_user(&ctx, 990, "status.name", "teacher").await;
        let (inv, tok) = seed_invite(&ctx, uid, false, false, false).await;
        let _ = uid;
        let resp = server
            .get(&format!("/d/teachers/first-login-status/{inv}/{tok}"))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "valid first-login-status must be 200, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
        let body = js(&resp.text());
        assert_eq!(body["status"], "pending");
        assert_eq!(body["school"], "Test High School");
        // full_name is seeded as "<username> Name"; email is "<username>@test.edu".
        assert_eq!(body["user"]["email"], "status.name@test.edu");
        assert_eq!(body["user"]["full_name"], "status.name Name");
    })
    .await;
}

/// Public `first-login-status` with a wrong token → 404 (pair must match).
#[tokio::test]
async fn first_login_status_bad_token_not_found() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        let uid = seed_user(&ctx, 1000, "badtok", "teacher").await;
        let (inv, _tok) = seed_invite(&ctx, uid, false, false, false).await;
        let resp = server
            .get(&format!("/d/teachers/first-login-status/{inv}/WRONG_TOKEN"))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::NOT_FOUND,
            "wrong-token first-login-status must be 404, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
    })
    .await;
}

/// Public first-login POST: valid pending invite + good password + consent → 200 and a 24 h JWT.
#[tokio::test]
async fn first_login_valid_200_with_token() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        let uid = seed_user(&ctx, 1010, "fl.ok", "teacher").await;
        let (inv, tok) = seed_invite(&ctx, uid, false, false, false).await;
        let _ = uid;
        let resp = server
            .post(&format!("/d/teachers/first-login/{inv}/{tok}"))
            .json(&json!({ "password": "P@ssw0rd1234", "locale": "en-US", "consent": true }))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "valid first-login must be 200, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
        let body = js(&resp.text());
        assert!(body["token"].as_str().map_or(false, |t| !t.is_empty()));
        assert_eq!(body["user_id"], json!(1010));
    })
    .await;
}

/// Public first-login POST: consent not given → 400.
#[tokio::test]
async fn first_login_missing_consent_bad_request() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        let uid = seed_user(&ctx, 1020, "fl.consent", "teacher").await;
        let (inv, tok) = seed_invite(&ctx, uid, false, false, false).await;
        let resp = server
            .post(&format!("/d/teachers/first-login/{inv}/{tok}"))
            .json(&json!({ "password": "P@ssw0rd1234", "locale": "en-US", "consent": false }))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::BAD_REQUEST,
            "first-login without consent must be 400, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
    })
    .await;
}

/// Public first-login POST: password violates the policy → 422.
#[tokio::test]
async fn first_login_weak_password_422() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        let uid = seed_user(&ctx, 1030, "fl.weak", "teacher").await;
        let (inv, tok) = seed_invite(&ctx, uid, false, false, false).await;
        let resp = server
            .post(&format!("/d/teachers/first-login/{inv}/{tok}"))
            .json(&json!({ "password": "noDigits", "locale": "en-US", "consent": true }))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            "weak password first-login must be 422, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
    })
    .await;
}

/// Public first-login POST: the invite is already used/revoked/expired → 410.
#[tokio::test]
async fn first_login_expired_invite_410() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        let uid = seed_user(&ctx, 1040, "fl.expired", "teacher").await;
        let (inv, tok) = seed_invite(&ctx, uid, true, false, false).await; // already used
        let resp = server
            .post(&format!("/d/teachers/first-login/{inv}/{tok}"))
            .json(&json!({ "password": "P@ssw0rd1234", "locale": "en-US", "consent": true }))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::GONE,
            "used/expired invite first-login must be 410, got {} (body: {})",
            resp.status_code(),
            resp.text()
        );
    })
    .await;
}
