//! Setup wizard controller — first-run admin provisioning.
//!
//! When the `users` table has **zero** admin accounts, the app is in *setup
//! mode*: the dashboard redirects to the `/setup` screen and `POST /api/v1/setup`
//! creates the school admin account. The endpoint is **atomically disabled**
//! once any admin exists — the empty-admins check runs *inside* the creation
//! transaction, so a concurrent double-submit can mint at most one admin.
//! Subsequent entries use the normal login flow.
//!
//! Security posture: the packaged launcher binds the server to loopback by
//! default (`ANKITOV_BIND=127.0.0.1`), so the setup window is local-only.
//! Operators exposing the app to a network (host-a-school) should complete
//! setup first — see `docs-repository/user-docs/download.mdx`.
//!
//! ## Endpoints
//!
//! | Method | Path | Auth | Description |
//! |--------|------|------|-------------|
//! | GET  | `/api/v1/setup/status` | No | `{setup_required: bool}` |
//! | POST | `/api/v1/setup` | No | Create the first admin, return JWT |
//!
//! Both are deliberately public (no JWT): by definition no admin exists yet,
//! so there is no token to present. The transaction guard is the gate.

use axum::{
    extract::State,
    routing::{get, post},
    Json,
};
use loco_rs::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, Set,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};

use crate::models::entities::user;
use crate::services::auth;

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SetupRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    pub full_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SetupStatusResponse {
    pub setup_required: bool,
}

/// `409 Conflict` — setup is finished (an admin already exists).
fn setup_completed_conflict() -> loco_rs::Error {
    loco_rs::Error::CustomError(
        axum::http::StatusCode::CONFLICT,
        loco_rs::controller::ErrorDetail {
            error: Some("conflict".into()),
            description: Some(
                "setup is already complete: an admin account exists; log in instead".into(),
            ),
        },
    )
}

/// `409 Conflict` — the email/username is taken (unique constraint).
fn duplicate_conflict() -> loco_rs::Error {
    loco_rs::Error::CustomError(
        axum::http::StatusCode::CONFLICT,
        loco_rs::controller::ErrorDetail {
            error: Some("conflict".into()),
            description: Some("a user with this email or username already exists".into()),
        },
    )
}

/// Map a failed setup transaction to the right HTTP error.
fn map_txn_error(e: sea_orm::TransactionError<sea_orm::DbErr>) -> loco_rs::Error {
    let db_err = match e {
        sea_orm::TransactionError::Connection(db_err) => db_err,
        sea_orm::TransactionError::Transaction(db_err) => db_err,
    };
    let msg = db_err.to_string();
    if msg.contains("setup already completed") {
        setup_completed_conflict()
    } else if msg.contains("UNIQUE") || msg.contains("unique") {
        duplicate_conflict()
    } else {
        tracing::error!("Setup transaction failed: {db_err:?}");
        loco_rs::Error::InternalServerError
    }
}

/// Count admin accounts on the given connection (used inside and outside the
/// creation transaction — both `DatabaseConnection` and `DatabaseTransaction`
/// implement `ConnectionTrait`).
async fn count_admins<C>(db: &C) -> Result<u64, sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    user::Entity::find()
        .filter(user::Column::Role.eq("admin"))
        .count(db)
        .await
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Report whether the app is in first-run setup mode.
///
/// `setup_required` is `true` exactly when no admin account exists.
pub async fn status(State(ctx): State<AppContext>) -> Result<Json<SetupStatusResponse>> {
    let admins = count_admins(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("Setup status query failed: {e:?}");
            loco_rs::Error::InternalServerError
        })?;
    Ok(Json(SetupStatusResponse {
        setup_required: admins == 0,
    }))
}

/// Create the first (and only) admin account.
///
/// Validation mirrors `/api/v1/auth/register`, but the role is forced to
/// `admin` — this is the one public endpoint allowed to mint an admin, and
/// only while zero admins exist. Returns a JWT (auto-login after setup).
pub async fn create_admin(
    State(ctx): State<AppContext>,
    Json(payload): Json<SetupRequest>,
) -> Result<Json<crate::controllers::auth::AuthResponse>> {
    let db = &ctx.db;

    // Validate required fields (same rules as public registration).
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

    let password_hash = auth::hash_password(&payload.password)
        .map_err(|e| {
            tracing::error!("Password hashing failed during setup: {e:?}");
            loco_rs::Error::InternalServerError
        })?;
    let now = chrono::Utc::now().timestamp();

    // Race-safe guard: the empty-admins check and the insert run inside ONE
    // transaction. A concurrent double-submit either sees the first admin
    // (→ 409 "setup already completed") or loses the unique-constraint race
    // (→ 409 duplicate) — at most one admin can ever be created here.
    let inserted: user::Model = db
        .transaction(|txn| {
            Box::pin(async move {
                let admins = count_admins(txn).await?;
                if admins > 0 {
                    return Err(sea_orm::DbErr::Custom("setup already completed".into()));
                }
                user::ActiveModel {
                    username: Set(payload.username.clone()),
                    email: Set(payload.email.clone()),
                    full_name: Set(payload.full_name.clone()),
                    role: Set("admin".to_string()),
                    subsection_id: Set(None),
                    password_hash: Set(Some(password_hash.clone())),
                    created_at: Set(now),
                    updated_at: Set(now),
                    ..Default::default()
                }
                .insert(txn)
                .await
            })
        })
        .await
        .map_err(map_txn_error)?;

    // Issue JWT (24h expiry), same contract as register/login.
    let token = auth::encode_token(inserted.id, "admin", &inserted.email, 86400)
        .map_err(|e| {
            tracing::error!("JWT issuance failed during setup: {e:?}");
            loco_rs::Error::InternalServerError
        })?;

    Ok(Json(crate::controllers::auth::AuthResponse {
        token,
        user_id: inserted.id,
        username: inserted.username,
        email: inserted.email,
        role: "admin".to_string(),
    }))
}

/// Route table for the setup API (registered in `app.rs` beside `auth`).
///
/// Public by design: the `require_auth_for_management` layer is path-scoped
/// to `/api/v1/management/*`, so these routes stay open — the transaction
/// guard in `create_admin` is the gate.
pub fn routes() -> Routes {
    Routes::new()
        .prefix("/setup")
        .add("/", post(create_admin))
        .add("/status", get(status))
}
