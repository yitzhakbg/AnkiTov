//! Teacher Registration (Prong 8) — management console + first-login wizard.
//!
//! Auth-gated management routes (enforce `require_auth_for_management`, role-checked):
//! - `GET  /management/teachers`                       — list active invites (admin)
//! - `POST /management/teachers`                       — enroll a teacher + mint invite (admin)
//! - `GET  /management/teachers/:user_id`             — teacher + invite detail (admin or owner)
//! - `POST /management/teachers/:user_id/resend`      — rotate invite token (admin)
//!
//! Public first-login routes (NO JWT — the user has no token yet; the invite
//! `:invite_id` + `:token` path pair is the credential, verified server-side):
//! - `GET  /management/teachers/first-login-status/:invite_id/:token`
//!        — pre-activation summary + validity for the wizard.
//! - `POST /management/teachers/first-login/:invite_id/:token`
//!        — set password + record consent + mark invite used + mint 24h JWT.
//!
//! The public HTML wizard is served by the static route `GET /d/{invite_id}/{token}`
//! registered in `app.rs::after_routes`. It reads the pair from the URL, calls
//! `first-login-status` to display the teacher's name/school/email, then POSTs
//! here to complete activation.

use axum::extract::{Path, State};
use axum::{Extension, Json};
use loco_rs::{app::AppContext, prelude::*};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::middleware::auth::{ensure_role, AuthUser};
use crate::models::entities::{teacher_invite, user};
use crate::services::{audit_logger, auth, password_policy};

const TAG: &str = "Management Console";

/// Invite lifetime in seconds (14 days).
const INVITE_TTL_SECS: i64 = 14 * 24 * 3600;
/// First-login JWT lifetime in seconds (24 hours).
const FIRST_LOGIN_TTL_SECS: usize = 24 * 3600;

// ── Request / Response types ──

/// Request body for enrolling a teacher.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RegisterTeacherRequest {
    pub full_name: String,
    pub email: String,
    pub school: String,
    pub community: Option<String>,
    pub role: Option<String>,
    pub locale: String,
}

/// The user portion of the enrollment response.
#[derive(Debug, Serialize, ToSchema)]
pub struct UserSummary {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub full_name: Option<String>,
    pub role: String,
    pub locale: String,
}

/// The invite portion of the enrollment response.
#[derive(Debug, Serialize, ToSchema)]
pub struct InviteSummary {
    pub id: String,
    pub token: String,
    pub expires_at: i64,
    pub invite_url: String,
}

/// The audit portion of the enrollment response.
#[derive(Debug, Serialize, ToSchema)]
pub struct AuditSummary {
    pub event_type: String,
}

/// Response body for successful enrollment.
#[derive(Debug, Serialize, ToSchema)]
pub struct EnrollTeacherResponse {
    pub user: UserSummary,
    pub invite: InviteSummary,
    pub audit: AuditSummary,
}

/// Request body for the first-login wizard.
#[derive(Debug, Deserialize, ToSchema)]
pub struct FirstLoginRequest {
    pub password: String,
    pub locale: Option<String>,
    pub consent: bool,
}

// ── Helpers ──

/// Mint a 256-bit random hex token.
fn new_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = rng.gen();
    hex::encode(bytes)
}

/// Derive a slug username from a full name: `first.last` (lowercased, non-alnum stripped).
fn derive_username(full_name: &str) -> String {
    let parts: Vec<&str> = full_name
        .trim()
        .split_whitespace()
        .filter(|p| !p.is_empty())
        .collect();
    let first = parts.first().unwrap_or(&"");
    let last = parts.last().unwrap_or(first);

    let mut out = String::new();
    for (i, c) in first.chars().enumerate() {
        if i == 0 {
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c.to_ascii_lowercase());
        }
    }
    if !out.is_empty() {
        out.push('.');
    }
    for c in last.chars() {
        out.push(c.to_ascii_lowercase());
    }
    out = out
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.')
        .collect();
    if out.is_empty() {
        "user".into()
    } else {
        out
    }
}

/// Check whether a username is free; return a deduped variant if needed.
async fn find_free_username(
    db: &sea_orm::DbConn,
    base: &str,
) -> Result<String, loco_rs::Error> {
    for i in 0..100 {
        let candidate = if i == 0 {
            base.to_string()
        } else {
            format!("{base}-{i}")
        };
        let exists = user::Entity::find()
            .filter(user::Column::Username.eq(&candidate))
            .one(db)
            .await
            .map_err(|_| loco_rs::Error::InternalServerError)?
            .is_some();
        if !exists {
            return Ok(candidate);
        }
    }
    Err(loco_rs::Error::CustomError(
        axum::http::StatusCode::CONFLICT,
        loco_rs::controller::ErrorDetail {
            error: Some("conflict".into()),
            description: Some("username allocation exhausted (tried 100 suffixes)".into()),
        },
    ))
}

/// Compute invite status from a model row.
fn invite_status(invite: &teacher_invite::Model) -> &'static str {
    let now = chrono::Utc::now().timestamp();
    if invite.revoked_at.is_some() {
        "revoked"
    } else if invite.used_at.is_some() {
        "active"
    } else if invite.expires_at < now {
        "expired"
    } else {
        "pending"
    }
}

// ── Routes ──

/// Routes for teacher registration and first-login.
pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management/teachers")
        .add("/", get(list))
        .add("/", post(create))
        .add("/:user_id", get(get_detail))
        // NOTE: the two first-login handlers (`first_login_status` + `first_login`)
        // are intentionally NOT registered in this `/api/v1` table. They are PUBLIC
        // (no JWT — the user has none yet) and are wired once, at the outer router,
        // via `after_routes` (see the `first_login_api` sub-router + the nested
        // `/api/v1/management/teachers/first-login{,-status}/:invite_id/:token`
        // routes). Registering them here too would create an "Overlapping method
        // route" panic in axum's path router (same path + method added twice).
        .add("/:user_id/resend", post(resend))
}

// ── Handlers ──

/// List active (pending) teacher invites, newest first, capped at 50.
#[utoipa::path(
    get,
    path = "/management/teachers",
    responses(
        (status = 200, description = "List of active teacher invites")
    ),
    tag = TAG
)]
pub async fn list(
    Extension(actor): Extension<AuthUser>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let _ = ensure_role(&actor, &["admin"])?;
    let db = &ctx.db;

    let invites = teacher_invite::Entity::find()
        .filter(teacher_invite::Column::UsedAt.is_null())
        .filter(teacher_invite::Column::RevokedAt.is_null())
        .order_by_desc(teacher_invite::Column::CreatedAt)
        .all(db)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list teacher invites: {e:?}");
            loco_rs::Error::InternalServerError
        })?
        .into_iter()
        .take(50)
        .collect::<Vec<_>>();

    let mut out = Vec::with_capacity(invites.len());
    for inv in invites {
        let uname = user::Entity::find_by_id(inv.user_id)
            .one(db)
            .await
            .ok()
            .flatten()
            .map(|u| (u.username, u.email, u.full_name))
            .unwrap_or_else(|| (String::new(), String::new(), None));

        out.push(serde_json::json!({
            "id": inv.id,
            "user_id": inv.user_id,
            "school": inv.school,
            "community": inv.community,
            "role": inv.role,
            "locale": inv.locale,
            "email": inv.email,
            "username": uname.0,
            "full_name": uname.2,
            "created_at": inv.created_at,
            "expires_at": inv.expires_at,
            "status": invite_status(&inv),
        }));
    }

    format::json(out)
}

/// Enroll a teacher and mint an invite.
#[utoipa::path(
    post,
    path = "/management/teachers",
    request_body = RegisterTeacherRequest,
    responses(
        (status = 201, description = "Teacher enrolled + invite minted", body = EnrollTeacherResponse),
        (status = 400, description = "Field validation failure"),
        (status = 409, description = "Duplicate email or active invite"),
    ),
    tag = TAG
)]
pub async fn create(
    Extension(actor): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Json(payload): Json<RegisterTeacherRequest>,
) -> Result<Response> {
    let _ = ensure_role(&actor, &["admin"])?;
    let db = &ctx.db;
    let now = chrono::Utc::now().timestamp();

    // Field validation
    let full_name = payload.full_name.trim().to_string();
    let email = payload.email.trim().to_string();
    let school = payload.school.trim().to_string();
    if full_name.is_empty() || email.is_empty() || school.is_empty() {
        return Err(loco_rs::Error::BadRequest(
            "full_name, email, and school are required".into(),
        ));
    }
    let locale_ok = crate::i18n::SUPPORTED_LOCALES
        .iter()
        .any(|(code, _, _)| *code == payload.locale);
    if !locale_ok {
        return Err(loco_rs::Error::BadRequest(format!(
            "unsupported locale: {}",
            payload.locale
        )));
    }

    // Duplicate email → 409
    let existing = user::Entity::find()
        .filter(user::Column::Email.eq(&email))
        .one(db)
        .await
        .map_err(|e| {
            tracing::error!("lookup by email failed: {e:?}");
            loco_rs::Error::InternalServerError
        })?;
    if existing.is_some() {
        return Err(loco_rs::Error::CustomError(
            axum::http::StatusCode::CONFLICT,
            loco_rs::controller::ErrorDetail {
                error: Some("conflict".into()),
                description: Some("a user with this email already exists".into()),
            },
        ));
    }

    // Check for an existing *active* invite for this email → 409
    let active_invite = teacher_invite::Entity::find()
        .filter(teacher_invite::Column::Email.eq(&email))
        .filter(teacher_invite::Column::UsedAt.is_null())
        .filter(teacher_invite::Column::RevokedAt.is_null())
        .one(db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?;
    if active_invite.is_some() {
        return Err(loco_rs::Error::CustomError(
            axum::http::StatusCode::CONFLICT,
            loco_rs::controller::ErrorDetail {
                error: Some("conflict".into()),
                description: Some("this email already has an active invite".into()),
            },
        ));
    }

    // Derive + dedupe username
    let base = derive_username(&full_name);
    let username = find_free_username(db, &base).await?;

    // Insert user (password NULL — set at first login)
    let new_user: user::ActiveModel = user::ActiveModel {
        id: sea_orm::ActiveValue::NotSet,
        username: sea_orm::ActiveValue::Set(username.clone()),
        email: sea_orm::ActiveValue::Set(email.clone()),
        full_name: sea_orm::ActiveValue::Set(Some(full_name.clone())),
        role: sea_orm::ActiveValue::Set("teacher".into()),
        subsection_id: sea_orm::ActiveValue::Set(None),
        password_hash: sea_orm::ActiveValue::Set(None),
        created_at: sea_orm::ActiveValue::Set(now),
        updated_at: sea_orm::ActiveValue::Set(now),
    };
    let inserted_user = user::Entity::insert(new_user)
        .exec(db)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("UNIQUE") || msg.contains("unique") {
                tracing::warn!("Duplicate email on insert: {msg}");
                loco_rs::Error::CustomError(axum::http::StatusCode::CONFLICT, loco_rs::controller::ErrorDetail { error: Some("conflict".into()), description: Some("a user with this email already exists".into()) })
            } else {
                tracing::error!("Failed to insert teacher user: {e:?}");
                loco_rs::Error::InternalServerError
            }
        })?;
    let user_id: i64 = inserted_user.last_insert_id;
    let user_row = user::Entity::find_by_id(user_id)
        .one(db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?
        .ok_or_else(|| loco_rs::Error::InternalServerError)?;

    // Insert invite
    let token = new_token();
    let invite_id = uuid::Uuid::new_v4().to_string();
    let invite: teacher_invite::ActiveModel = teacher_invite::ActiveModel {
        id: sea_orm::ActiveValue::Set(invite_id.clone()),
        user_id: sea_orm::ActiveValue::Set(user_row.id),
        school: sea_orm::ActiveValue::Set(school),
        community: sea_orm::ActiveValue::Set(payload.community),
        role: sea_orm::ActiveValue::Set(payload.role),
        locale: sea_orm::ActiveValue::Set(payload.locale.clone()),
        email: sea_orm::ActiveValue::Set(Some(email.clone())),
        invite_token: sea_orm::ActiveValue::Set(token.clone()),
        created_at: sea_orm::ActiveValue::Set(now),
        expires_at: sea_orm::ActiveValue::Set(now + INVITE_TTL_SECS),
        used_at: sea_orm::ActiveValue::Set(None),
        revoked_at: sea_orm::ActiveValue::Set(None),
    };
    teacher_invite::Entity::insert(invite)
        .exec(db)
        .await
        .map_err(|e| {
            tracing::error!("Failed to insert teacher_invite: {e:?}");
            loco_rs::Error::InternalServerError
        })?;

    // Audit
    let event_data = serde_json::json!({
        "user_id": user_row.id,
        "email": user_row.email,
        "full_name": user_row.full_name,
    })
    .to_string();
    audit_logger::log_event(
        db,
        "teacher_enrolled",
        &event_data,
        &actor.id.to_string(),
        Some(&user_row.id.to_string()),
    )
    .await;

    // Relative invite URL (the client resolves it against its own origin)
    let invite_url = format!("/d/{}/{}", invite_id, token);

    format::json(EnrollTeacherResponse {
        user: UserSummary {
            id: user_row.id,
            username: user_row.username,
            email: user_row.email,
            full_name: user_row.full_name,
            role: user_row.role,
            locale: payload.locale,
        },
        invite: InviteSummary {
            id: invite_id,
            token,
            expires_at: now + INVITE_TTL_SECS,
            invite_url,
        },
        audit: AuditSummary {
            event_type: "teacher_enrolled".into(),
        },
    })
}

/// Get a teacher's user + invite detail.
#[utoipa::path(
    get,
    path = "/management/teachers/{user_id}",
    params(("user_id" = i64, description = "User ID")),
    responses(
        (status = 200, description = "Teacher + invite detail"),
        (status = 403, description = "Not the owner and not admin"),
        (status = 404, description = "Teacher not found"),
    ),
    tag = TAG
)]
pub async fn get_detail(
    Extension(actor): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(user_id): Path<i64>,
) -> Result<Response> {
    // Admin can read any; otherwise self-only.
    let is_admin = actor.role.eq_ignore_ascii_case("admin");
    if !is_admin && actor.id != user_id {
        return Err(loco_rs::Error::CustomError(
            axum::http::StatusCode::FORBIDDEN,
            loco_rs::controller::ErrorDetail {
                error: Some("forbidden".into()),
                description: Some("not the owner".into()),
            },
        ));
    }
    let db = &ctx.db;

    let u = user::Entity::find_by_id(user_id)
        .one(db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?
        .ok_or_else(|| loco_rs::Error::NotFound)?;

    if !u.role.eq_ignore_ascii_case("teacher") {
        return Err(loco_rs::Error::NotFound);
    }

    let inv = teacher_invite::Entity::find()
        .filter(teacher_invite::Column::UserId.eq(user_id))
        .order_by_desc(teacher_invite::Column::CreatedAt)
        .one(db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?;

    format::json(serde_json::json!({
        "user": {
            "id": u.id,
            "username": u.username,
            "email": u.email,
            "full_name": u.full_name,
            "role": u.role,
            "password_hash_set": u.password_hash.is_some(),
        },
        "invite": inv.as_ref().map(|i| serde_json::json!({
            "id": i.id,
            "token": i.invite_token,
            "school": i.school,
            "community": i.community,
            "role": i.role,
            "locale": i.locale,
            "email": i.email,
            "created_at": i.created_at,
            "expires_at": i.expires_at,
            "used_at": i.used_at,
            "revoked_at": i.revoked_at,
            "status": invite_status(i),
        })),
    }))
}

/// Rotate the invite token (resend the invite).
#[utoipa::path(
    post,
    path = "/management/teachers/{user_id}/resend",
    params(("user_id" = i64, description = "User ID")),
    responses(
        (status = 200, description = "Token rotated", body = serde_json::Value),
        (status = 404, description = "No invite found"),
    ),
    tag = TAG
)]
pub async fn resend(
    Extension(actor): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(user_id): Path<i64>,
) -> Result<Response> {
    let _ = ensure_role(&actor, &["admin"])?;
    let db = &ctx.db;
    let now = chrono::Utc::now().timestamp();

    let invite = teacher_invite::Entity::find()
        .filter(teacher_invite::Column::UserId.eq(user_id))
        .one(db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?
        .ok_or_else(|| loco_rs::Error::NotFound)?;

    let mut am: teacher_invite::ActiveModel = invite.into();
    am.invite_token = sea_orm::ActiveValue::Set(new_token());
    am.created_at = sea_orm::ActiveValue::Set(now);
    am.expires_at = sea_orm::ActiveValue::Set(now + INVITE_TTL_SECS);
    let updated = teacher_invite::Entity::update(am)
        .exec(db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?;

    audit_logger::log_event(
        db,
        "invite_resent",
        &serde_json::json!({ "invite_id": updated.id }).to_string(),
        &actor.id.to_string(),
        Some(&user_id.to_string()),
    )
    .await;

    format::json(serde_json::json!({
        "id": updated.id,
        "token": updated.invite_token,
        "created_at": updated.created_at,
        "expires_at": updated.expires_at,
        "status": invite_status(&updated),
    }))
}

/// Public pre-activation status for the first-login wizard.
///
/// The wizard is served by the un-gated `GET /d/{invite_id}/{token}` route and has
/// no JWT (the user has no token yet). It calls this to display the teacher's
/// name/school/email and to confirm the link is still valid before the user
/// types a password. Verified by the `:invite_id` + `:token` path pair.
#[utoipa::path(
    get,
    path = "/management/teachers/first-login-status/{invite_id}/{token}",
    params(
        ("invite_id" = String, description = "Invite UUID"),
        ("token" = String, description = "256-bit invite token"),
    ),
    responses(
        (status = 200, description = "Valid invite — teacher summary + status"),
        (status = 404, description = "Invite not found"),
        (status = 410, description = "Invite expired, already used, or revoked"),
    ),
    tag = TAG
)]
pub async fn first_login_status(
    State(ctx): State<AppContext>,
    Path((invite_id, token)): Path<(String, String)>,
) -> Result<Response> {
    let db = &ctx.db;
    let invite = teacher_invite::Entity::find()
        .filter(teacher_invite::Column::Id.eq(&invite_id))
        .filter(teacher_invite::Column::InviteToken.eq(&token))
        .one(db)
        .await
        .map_err(|e| {
            tracing::error!("first-login-status lookup failed: {e:?}");
            loco_rs::Error::InternalServerError
        })?
        .ok_or_else(|| {
            loco_rs::Error::CustomError(
                axum::http::StatusCode::NOT_FOUND,
                loco_rs::controller::ErrorDetail {
                    error: Some("not_found".into()),
                    description: Some("invite not found or token mismatch".into()),
                },
            )
        })?;

    let now = chrono::Utc::now().timestamp();
    if invite.used_at.is_some()
        || invite.revoked_at.is_some()
        || invite.expires_at < now
    {
        return Err(loco_rs::Error::CustomError(
            axum::http::StatusCode::GONE,
            loco_rs::controller::ErrorDetail {
                error: Some("invite_gone".into()),
                description: Some("invite is expired, already used, or revoked".into()),
            },
        ));
    }

    let u = user::Entity::find_by_id(invite.user_id)
        .one(db)
        .await
        .map_err(|e| {
            tracing::error!("first-login-status user lookup failed: {e:?}");
            loco_rs::Error::InternalServerError
        })?
        .ok_or_else(|| loco_rs::Error::InternalServerError)?;

    format::json(serde_json::json!({
        "status": invite_status(&invite),
        "user": {
            "id": u.id,
            "username": u.username,
            "email": u.email,
            "full_name": u.full_name,
            "role": u.role,
        },
        "school": invite.school,
        "community": invite.community,
        "role": invite.role,
        "locale": invite.locale,
        "expires_at": invite.expires_at,
    }))
}

/// First-login wizard completion: set password, record consent, mark invite used,
/// mint a 24h JWT.
///
/// PUBLIC — no JWT (the user has no token yet). The `:invite_id` + `:token` path
/// pair is the credential and is verified server-side before any mutation.
#[utoipa::path(
    post,
    path = "/management/teachers/first-login/{invite_id}/{token}",
    request_body = FirstLoginRequest,
    params(
        ("invite_id" = String, description = "Invite UUID"),
        ("token" = String, description = "256-bit invite token"),
    ),
    responses(
        (status = 200, description = "First login complete", body = serde_json::Value),
        (status = 400, description = "Consent required or field error"),
        (status = 404, description = "Invite not found"),
        (status = 410, description = "Invite expired, used, or revoked"),
        (status = 422, description = "Password policy violation"),
    ),
    tag = TAG
)]
pub async fn first_login(
    State(ctx): State<AppContext>,
    Path((invite_id, token)): Path<(String, String)>,
    Json(payload): Json<FirstLoginRequest>,
) -> Result<Response> {
    let db = &ctx.db;
    let now = chrono::Utc::now().timestamp();

    if !payload.consent {
        return Err(loco_rs::Error::BadRequest(
            "privacy consent is required".into(),
        ));
    }

    // Look up + verify the invite (id + token must both match).
    let invite = teacher_invite::Entity::find()
        .filter(teacher_invite::Column::Id.eq(&invite_id))
        .filter(teacher_invite::Column::InviteToken.eq(&token))
        .one(db)
        .await
        .map_err(|e| {
            tracing::error!("first-login lookup failed: {e:?}");
            loco_rs::Error::InternalServerError
        })?
        .ok_or_else(|| {
            loco_rs::Error::CustomError(
                axum::http::StatusCode::NOT_FOUND,
                loco_rs::controller::ErrorDetail {
                    error: Some("not_found".into()),
                    description: Some("invite not found or token mismatch".into()),
                },
            )
        })?;

    let user_id = invite.user_id;

    // Rejected if already used, revoked, or expired.
    if invite.used_at.is_some()
        || invite.revoked_at.is_some()
        || invite.expires_at < now
    {
        return Err(loco_rs::Error::CustomError(
            axum::http::StatusCode::GONE,
            loco_rs::controller::ErrorDetail {
                error: Some("invite_gone".into()),
                description: Some("invite is expired, used, or revoked".into()),
            },
        ));
    }

    // Validate password against the policy (email from the invite when present).
    let email_ref: Option<&str> = invite.email.as_deref();
    if let Err(perr) = password_policy::check(&payload.password, email_ref) {
        return Err(loco_rs::Error::CustomError(
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            loco_rs::controller::ErrorDetail {
                error: Some("password_policy".into()),
                description: Some(perr.to_string()),
            },
        ));
    }

    // Hash the password.
    let hash = auth::hash_password(&payload.password).map_err(|e| {
        tracing::error!("bcrypt hash failed: {e:?}");
        loco_rs::Error::InternalServerError
    })?;

    // Set the user's password_hash.
    let u = user::Entity::find_by_id(user_id)
        .one(db)
        .await
        .map_err(|e| {
            tracing::error!("first-login user lookup failed: {e:?}");
            loco_rs::Error::InternalServerError
        })?
        .ok_or_else(|| loco_rs::Error::InternalServerError)?;
    let mut user_am: user::ActiveModel = u.into();
    user_am.password_hash = sea_orm::ActiveValue::Set(Some(hash));
    user_am.updated_at = sea_orm::ActiveValue::Set(now);
    user::Entity::update(user_am)
        .exec(db)
        .await
        .map_err(|e| {
            tracing::error!("Failed to set password_hash: {e:?}");
            loco_rs::Error::InternalServerError
        })?;

    // Mark the invite used.
    let mut inv_am: teacher_invite::ActiveModel = invite.into();
    inv_am.used_at = sea_orm::ActiveValue::Set(Some(now));
    let updated_invite = teacher_invite::Entity::update(inv_am)
        .exec(db)
        .await
        .map_err(|e| {
            tracing::error!("Failed to mark invite used: {e:?}");
            loco_rs::Error::InternalServerError
        })?;

    // Audit.
    audit_logger::log_event(
        db,
        "teacher_first_login",
        &serde_json::json!({
            "user_id": user_id,
            "invite_id": updated_invite.id,
            "locale": updated_invite.locale,
        })
        .to_string(),
        &user_id.to_string(),
        Some(&user_id.to_string()),
    )
    .await;

    // Issue the 24h JWT.
    let u2 = user::Entity::find_by_id(user_id)
        .one(db)
        .await
        .map_err(|e| {
            tracing::error!("first-login token user lookup failed: {e:?}");
            loco_rs::Error::InternalServerError
        })?
        .ok_or_else(|| loco_rs::Error::InternalServerError)?;
    let token_out = auth::encode_token(u2.id, &u2.role, &u2.email, FIRST_LOGIN_TTL_SECS)
        .map_err(|e| {
            tracing::error!("JWT encode failed: {e:?}");
            loco_rs::Error::InternalServerError
        })?;

    format::json(serde_json::json!({
        "token": token_out,
        "role": u2.role,
        "email": u2.email,
        "user_id": u2.id,
    }))
}
