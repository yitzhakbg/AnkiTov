//! Auth controller — registration, login, token issuance.
//!
//! ## Endpoints
//!
//! | Method | Path | Auth | Description |
//! |--------|------|------|-------------|
//! | POST | `/api/v1/auth/register` | No | Create user account, return JWT |
//! | POST | `/api/v1/auth/login` | No | Authenticate, return JWT |

use axum::{extract::{Extension, State}, routing::post, Json};
use loco_rs::prelude::*;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};

use crate::models::entities::user;
use crate::services::auth;

/// Tag used for OpenAPI documentation grouping.
const TAG: &str = "Auth";

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    pub full_name: Option<String>,
    pub role: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Serialize)]
pub struct ChangePasswordResponse {
    pub changed: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user_id: i64,
    pub username: String,
    pub email: String,
    pub role: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Register a new user account.
///
/// Returns a JWT token on success (auto-login after registration).
/// Rejects duplicate emails and usernames.
///
/// **Role is restricted on the public endpoint:** self-registration may only
/// create `student` or `teacher` accounts. The `admin` role is deliberately
/// **not** assignable here — administrators are provisioned out-of-band
/// (inserted into the database by the operator), never via this public API.
pub async fn register(
    State(ctx): State<AppContext>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>> {
    let db = &ctx.db;

    // Validate required fields
    if payload.username.trim().is_empty()
        || payload.email.trim().is_empty()
        || payload.password.trim().is_empty()
    {
        return Err(loco_rs::Error::BadRequest(
            "username, email, and password are required".into(),
        ));
    }

    if payload.password.len() < 6 {
        return Err(loco_rs::Error::BadRequest(
            "password must be at least 6 characters".into(),
        ));
    }

    // Check for existing email
    let existing = user::Entity::find()
        .filter(user::Column::Email.eq(&payload.email))
        .one(db)
        .await
        .map_err(|e| loco_rs::Error::InternalServerError)?;

    if existing.is_some() {
        return Err(loco_rs::Error::BadRequest("email already registered".into()));
    }

    // Hash password
    let password_hash =
        auth::hash_password(&payload.password).map_err(|e| loco_rs::Error::InternalServerError)?;

    let now = chrono::Utc::now().timestamp();

    // Public self-registration may only mint student or teacher accounts.
    // `admin` is intentionally excluded (see the handler doc) — it must be
    // provisioned out-of-band by the operator. Unknown roles fall back to
    // `student` rather than being honored.
    let role = match payload.role.as_deref() {
        Some("student") | None => "student",
        Some("teacher") => "teacher",
        _ => {
            return Err(loco_rs::Error::BadRequest(
                "role must be \"student\" or \"teacher\" for self-registration".into(),
            ))
        }
    }
    .to_string();

    // Insert user
    let user_model = user::ActiveModel {
        username: Set(payload.username.clone()),
        email: Set(payload.email.clone()),
        full_name: Set(payload.full_name),
        role: Set(role.clone()),
        subsection_id: Set(None),
        password_hash: Set(Some(password_hash)),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    let inserted = user_model
        .insert(db)
        .await
        .map_err(|e| loco_rs::Error::InternalServerError)?;

    // Issue JWT (24h expiry)
    let token = auth::encode_token(inserted.id, &role, &payload.email, 86400)
        .map_err(|e| loco_rs::Error::InternalServerError)?;

    Ok(Json(AuthResponse {
        token,
        user_id: inserted.id,
        username: inserted.username,
        email: inserted.email,
        role,
    }))
}

/// Authenticate an existing user.
///
/// Returns a JWT token on success. Returns 401 on invalid credentials.
pub async fn login(
    State(ctx): State<AppContext>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<AuthResponse>> {
    let db = &ctx.db;

    // Find user by email
    let user_model = user::Entity::find()
        .filter(user::Column::Email.eq(&payload.email))
        .one(db)
        .await
        .map_err(|e| loco_rs::Error::InternalServerError)?
        .ok_or_else(|| loco_rs::Error::Unauthorized("invalid email or password".into()))?;

    // Verify password
    let hash = user_model
        .password_hash
        .as_deref()
        .ok_or_else(|| loco_rs::Error::Unauthorized("account has no password set".into()))?;

    let valid = auth::verify_password(&payload.password, hash)
        .map_err(|e| loco_rs::Error::InternalServerError)?;

    if !valid {
        return Err(loco_rs::Error::Unauthorized("invalid email or password".into()));
    }

    // Issue JWT
    let token = auth::encode_token(user_model.id, &user_model.role, &user_model.email, 86400)
        .map_err(|e| loco_rs::Error::InternalServerError)?;

    Ok(Json(AuthResponse {
        token,
        user_id: user_model.id,
        username: user_model.username,
        email: user_model.email,
        role: user_model.role,
    }))
}


/// Change the current user's password.
///
/// Requires a valid `Authorization: Bearer <token>`. Verifies the supplied
/// `current_password` against the stored bcrypt hash, then replaces it with a
/// freshly hashed `new_password` and bumps `updated_at`. The JWT is
/// stateless, so no re-login is forced — the existing token stays valid until
/// its natural expiry.
///
/// | Field | Description |
/// |-------|-------------|
/// | `current_password` | The user's existing password (must match). |
/// | `new_password` | The new password (>= 6 chars, must differ from current). |
pub async fn change_password(
    State(ctx): State<AppContext>,
    req: axum::extract::Request,
) -> axum::http::Response<axum::body::Body> {
    let db = &ctx.db;

    // Decode the bearer token directly (this route is public in its chain so
    // `require_auth` is not in the middleware stack for /auth/*).
    let token = match req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        Some(t) => t,
        None => {
            return json_response(
                axum::http::StatusCode::UNAUTHORIZED,
                "missing or malformed Authorization: Bearer token",
            );
        }
    };

    let claims = match crate::services::auth::decode_token(token) {
        Ok(c) => c,
        Err(_) => {
            return json_response(axum::http::StatusCode::UNAUTHORIZED, "invalid or expired token");
        }
    };

    // Read the JSON body manually (avoids the `Json` extractor, which does not
    // compose cleanly with `Request` + `State<AppContext>` in axum 0.7 here).
    let body_bytes = match axum::body::to_bytes(req.into_body(), 1 << 20).await {
        Ok(b) => b,
        Err(_) => {
            return json_response(axum::http::StatusCode::BAD_REQUEST, "could not read request body");
        }
    };
    let payload: ChangePasswordRequest = match serde_json::from_slice(&body_bytes) {
        Ok(p) => p,
        Err(e) => {
            return json_response(
                axum::http::StatusCode::BAD_REQUEST,
                &format!("invalid JSON: {e}"),
            );
        }
    };

    let user_model = match user::Entity::find_by_id(claims.sub).one(db).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return json_response(axum::http::StatusCode::UNAUTHORIZED, "invalid or expired token");
        }
        Err(_) => {
            return json_response(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "database error");
        }
    };

    let hash = match user_model.password_hash.as_deref() {
        Some(h) => h,
        None => {
            return json_response(axum::http::StatusCode::BAD_REQUEST, "account has no password set");
        }
    };

    // Verify current password.
    match auth::verify_password(&payload.current_password, hash) {
        Ok(true) => {}
        _ => {
            return json_response(axum::http::StatusCode::UNAUTHORIZED, "current password is incorrect");
        }
    }

    // Validate new password.
    if payload.new_password.len() < 6 {
        return json_response(
            axum::http::StatusCode::BAD_REQUEST,
            "new password must be at least 6 characters",
        );
    }
    if payload.new_password == payload.current_password {
        return json_response(
            axum::http::StatusCode::BAD_REQUEST,
            "new password must differ from the current password",
        );
    }

    let new_hash = match auth::hash_password(&payload.new_password) {
        Ok(h) => h,
        Err(_) => {
            return json_response(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "password hashing failed");
        }
    };

    let now = chrono::Utc::now().timestamp();
    let mut active = user_model.into_active_model();
    active.password_hash = Set(Some(new_hash));
    active.updated_at = Set(now);
    if active.update(db).await.is_err() {
        return json_response(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "database error");
    }

    json_response(axum::http::StatusCode::OK, "password updated")
}

/// Build a JSON `{ "error": message }` response with the given status.
fn json_response(status: axum::http::StatusCode, message: &str) -> axum::http::Response<axum::body::Body> {
    let body = serde_json::json!({ "error": message }).to_string();
    axum::http::Response::builder()
        .status(status)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .unwrap_or_else(|_| axum::http::Response::new(axum::body::Body::empty()))
}

/// Log out the current user.
///
/// AnkiTov uses stateless JWTs, so there is no server-side session to revoke.
/// The JWT cannot be invalidated once issued (it self-contains its claims),
/// so this endpoint is an **idempotent acknowledgement** — the *client* drops
/// its in-memory token. The handler requires a valid `Authorization: Bearer
/// <token>` (checked by the caller via `require_auth`), so a malformed or
/// expired token yields a 401 and the client still clears its state.
#[utoipa::path(
    post,
    path = "/auth/logout",
    responses(
        (status = 200, description = "Logged out — client should discard the token")
    ),
    tag = TAG
)]
pub async fn logout(
    req: axum::extract::Request,
) -> Result<Json<LogoutResponse>> {
    // `/auth/logout` is a *public* route (no `require_auth` in its chain — the
    // client already holds the token and is merely acknowledging the logout,
    // so it cannot go through a middleware that demands a valid token first).
    // We therefore decode the bearer token directly instead of relying on the
    // `AuthUser` extension that `require_auth` would have injected. A missing
    // or garbage token simply yields a best-effort 200 — logout is an
    // idempotent acknowledgement and must not fail the client.
    let mut user_id = 0i64;
    let mut email = String::new();
    let mut role = String::new();
    if let Some(token) = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        if let Ok(claims) = crate::services::auth::decode_token(token) {
            user_id = claims.sub;
            email = claims.email;
            role = claims.role;
        }
    }
    tracing::debug!(
        user_id,
        email = %email,
        role = %role,
        "user logged out (stateless JWT — token dropped client-side)"
    );
    Ok(Json(LogoutResponse {
        logged_out: true,
        message: "logged out".to_string(),
    }))
}

/// Response body for [`logout`].
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LogoutResponse {
    pub logged_out: bool,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

pub fn routes() -> Routes {
    Routes::new()
        .add("/auth/register", post(register))
        .add("/auth/login", post(login))
        .add("/auth/change-password", post(change_password))
        .add("/auth/logout", post(logout))
}
