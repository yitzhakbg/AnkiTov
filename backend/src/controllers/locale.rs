//! Locale controller — serves i18n data to the frontend.
//!
//! Endpoints:
//! - `GET /api/v1/locale` → full translation map for current locale
//! - `GET /api/v1/locale/info` → locale metadata (code, direction, name)
//! - `GET /api/v1/locale/list` → all supported locales
//! - `POST /api/v1/locale/set` → set locale override (from language switcher)

use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::i18n;

// ── Response types ──

/// Key-value map of all translated strings for the current locale.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LocaleMap {
    pub locale: String,
    pub direction: String,
    pub messages: HashMap<String, String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LocaleInfo {
    pub locale: String,
    pub direction: String,
    pub native_name: String,
    pub english_name: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SetLocaleRequest {
    pub locale: String,
}

// ── Handlers ──

/// GET /api/v1/locale — full translation map for the current locale.
#[utoipa::path(
    get,
    path = "/locale",
    tag = "Locale",
    responses((status = 200, body = LocaleMap))
)]
pub async fn get_locale() -> Json<LocaleMap> {
    let locale = i18n::current_locale();
    let direction = if i18n::is_rtl() { "rtl" } else { "ltr" }.to_string();

    let message_ids: &[&str] = &[
        "app-name", "app-tagline",
        "error-not-found", "error-unauthorized", "error-internal",
        "error-validation", "error-rate-limit", "error-class-not-found",
        "error-student-not-found", "error-track-not-found",
        "error-profile-not-found", "error-deck-not-found",
        "error-upload-failed", "error-sync-failed", "error-enroll-failed",
        "error-transfer-failed", "error-capsule-generation-failed",
        "success-created", "success-updated", "success-deleted",
        "success-enrolled", "success-transferred",
        "success-capsule-generated", "success-deck-uploaded",
        "success-deck-distributed", "success-sync-started",
        "notify-health-ok", "notify-health-unreachable",
        "notify-syncing", "notify-loading",
        "audit-capsule-generated", "audit-n-value-changed",
        "audit-profile-assigned", "audit-profile-revoked",
        "audit-class-created", "audit-class-deleted",
        "audit-deck-uploaded", "audit-deck-distributed",
    ];

    let mut messages = HashMap::with_capacity(message_ids.len());
    for id in message_ids {
        // Use t_optional to skip messages that require variables we can't provide
        if let Some(msg) = i18n::t_optional(id) {
            messages.insert(id.to_string(), msg);
        }
    }

    Json(LocaleMap {
        locale: locale.clone(),
        direction,
        messages,
    })
}

/// GET /api/v1/locale/info — metadata for the current locale.
#[utoipa::path(
    get,
    path = "/locale/info",
    tag = "Locale",
    responses((status = 200, body = LocaleInfo))
)]
pub async fn get_locale_info() -> Json<LocaleInfo> {
    let locale_code = i18n::current_locale();
    let direction = if i18n::is_rtl() { "rtl" } else { "ltr" };

    let (native_name, english_name) = i18n::SUPPORTED_LOCALES
        .iter()
        .find(|(code, _, _)| *code == locale_code)
        .map(|(_, native, english)| (native.to_string(), english.to_string()))
        .unwrap_or_else(|| (locale_code.clone(), locale_code.clone()));

    Json(LocaleInfo {
        locale: locale_code,
        direction: direction.to_string(),
        native_name,
        english_name,
    })
}

/// GET /api/v1/locale/list — all supported locales.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LocaleEntry {
    pub code: String,
    pub native_name: String,
    pub english_name: String,
    pub direction: String,
}

#[utoipa::path(
    get,
    path = "/locale/list",
    tag = "Locale",
    responses((status = 200, body = Vec<LocaleEntry>))
)]
pub async fn list_locales() -> Json<Vec<LocaleEntry>> {
    let entries: Vec<LocaleEntry> = i18n::SUPPORTED_LOCALES
        .iter()
        .map(|(code, native, english)| {
            let dir = match *code {
                "ar" | "he" | "fa" | "ur" | "yi" | "ku" | "sd" => "rtl",
                _ => "ltr",
            };
            LocaleEntry {
                code: code.to_string(),
                native_name: native.to_string(),
                english_name: english.to_string(),
                direction: dir.to_string(),
            }
        })
        .collect();
    Json(entries)
}

/// POST /api/v1/locale/set — override locale for current session.
#[utoipa::path(
    post,
    path = "/locale/set",
    tag = "Locale",
    responses((status = 200, body = LocaleInfo))
)]
pub async fn set_locale(Json(body): Json<SetLocaleRequest>) -> Json<LocaleInfo> {
    i18n::set_locale(&body.locale);

    let locale_code = i18n::current_locale();
    let direction = if i18n::is_rtl() { "rtl" } else { "ltr" };

    let (native_name, english_name) = i18n::SUPPORTED_LOCALES
        .iter()
        .find(|(code, _, _)| *code == locale_code)
        .map(|(_, native, english)| (native.to_string(), english.to_string()))
        .unwrap_or_else(|| (locale_code.clone(), locale_code.clone()));

    Json(LocaleInfo {
        locale: locale_code,
        direction: direction.to_string(),
        native_name,
        english_name,
    })
}

// ── Loco.rs Routes ──

use axum::routing::{get, post};
use loco_rs::prelude::Routes;

/// Return Loco.rs-compatible routes for the locale endpoints.
/// Mounted at `/api/v1/locale/*`.
pub fn routes() -> Routes {
    Routes::new()
        .prefix("/locale")
        .add("/", get(get_locale))
        .add("/info", get(get_locale_info))
        .add("/list", get(list_locales))
        .add("/set", post(set_locale))
}
