//! School settings — management console + setup wizard (T-126).
//!
//! Auth-gated management routes (enforce `require_auth_for_management`):
//! - `GET /management/settings` — current school settings (teacher or admin)
//! - `PUT /management/settings` — partial update (admin only)
//!
//! The first-run setup wizard calls `PUT` right after creating the admin
//! account (step 2 hands it a fresh admin JWT), so "essential settings"
//! (school name, locale, optional sync URL) are captured during onboarding
//! and editable later from the Settings view.
//!
//! Locale writes also flip the server-side override (`i18n::set_locale`) so
//! the running process follows the new school default immediately.

use axum::extract::State;
use axum::{Extension, Json};
use loco_rs::{app::AppContext, prelude::*};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::middleware::auth::{ensure_role, AuthUser};
use crate::services::{audit_logger, settings};

const TAG: &str = "Management Console";

// ── Request / Response types ──

/// Partial update body — only provided fields are written.
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateSettingsRequest {
    pub school_name: Option<String>,
    pub locale: Option<String>,
    pub sync_url: Option<String>,
}

/// Current school settings; `null` where a key has never been set.
#[derive(Debug, Serialize, ToSchema)]
pub struct SchoolSettingsResponse {
    pub school_name: Option<String>,
    pub locale: Option<String>,
    pub sync_url: Option<String>,
}

impl From<settings::SchoolSettings> for SchoolSettingsResponse {
    fn from(s: settings::SchoolSettings) -> Self {
        Self {
            school_name: s.school_name,
            locale: s.locale,
            sync_url: s.sync_url,
        }
    }
}

// ── Handlers ──

/// GET /api/v1/management/settings — current school settings.
#[utoipa::path(get, path = "/api/management/settings", responses((status = 200, body = SchoolSettingsResponse)), tag = TAG)]
pub async fn get_settings(
    Extension(actor): Extension<AuthUser>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let _ = ensure_role(&actor, &["admin", "teacher"])?;
    let loaded = settings::load(&ctx.db).await?;
    format::json(SchoolSettingsResponse::from(loaded))
}

/// PUT /api/v1/management/settings — partial update (admin only).
///
/// Validation errors return 400 with a human-readable description; each
/// provided field is validated before any write, so a bad payload writes
/// nothing. Successful writes are audit-logged with the changed keys.
#[utoipa::path(put, path = "/api/management/settings", request_body = UpdateSettingsRequest, responses((status = 200, body = SchoolSettingsResponse)), tag = TAG)]
pub async fn update_settings(
    Extension(actor): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Json(payload): Json<UpdateSettingsRequest>,
) -> Result<Response> {
    let _ = ensure_role(&actor, &["admin"])?;
    let db = &ctx.db;

    // Validate everything first — a bad payload must write nothing.
    if let Some(name) = payload.school_name.as_deref() {
        settings::validate_school_name(name).map_err(bad_request)?;
    }
    if let Some(locale) = payload.locale.as_deref() {
        settings::validate_locale(locale).map_err(bad_request)?;
    }
    if let Some(url) = payload.sync_url.as_deref() {
        settings::validate_sync_url(url).map_err(bad_request)?;
    }

    let mut changed: Vec<&str> = Vec::new();
    if let Some(name) = payload.school_name.as_deref() {
        settings::set(db, settings::KEY_SCHOOL_NAME, name).await?;
        changed.push(settings::KEY_SCHOOL_NAME);
    }
    if let Some(locale) = payload.locale.as_deref() {
        settings::set(db, settings::KEY_LOCALE, locale).await?;
        changed.push(settings::KEY_LOCALE);
        settings::apply_locale_override(locale);
    }
    if let Some(url) = payload.sync_url.as_deref() {
        settings::set(db, settings::KEY_SYNC_URL, url).await?;
        changed.push(settings::KEY_SYNC_URL);
    }

    if !changed.is_empty() {
        audit_logger::log_event(
            db,
            "settings.update",
            &format!("keys={:?}", changed),
            &actor.id.to_string(),
            None,
        )
        .await;
    }

    let loaded = settings::load(db).await?;
    format::json(SchoolSettingsResponse::from(loaded))
}

/// Map a validation message onto a 400 with a stable `error` code.
fn bad_request(msg: String) -> loco_rs::Error {
    loco_rs::Error::CustomError(
        axum::http::StatusCode::BAD_REQUEST,
        loco_rs::controller::ErrorDetail {
            error: Some("bad_request".into()),
            description: Some(msg),
        },
    )
}

// ── Routes ──

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management/settings")
        .add("/", get(get_settings))
        .add("/", put(update_settings))
}
