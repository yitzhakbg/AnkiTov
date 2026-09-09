//! Dashboard controller.
//!
//! Provides a standalone HTML management console UI plus static assets
//! (locale JSON, i18n module, RTL CSS).
// Forced recompile marker: 144_3

use axum::http::header;
use axum::response::IntoResponse;
use loco_rs::prelude::*;

/// Serves the dashboard HTML shell.
pub async fn index() -> Result<Response> {
    format::html(include_str!("../../resources/dashboard/index.html"))
}

/// Serves the IMP Console (Interleaved Mastery Pipeline UI).
pub async fn imp_console() -> Result<Response> {
    format::html(include_str!("../../resources/dashboard/imp-console.html"))
}

/// Health check endpoint for the dashboard.
pub async fn health() -> impl IntoResponse {
    (axum::http::StatusCode::OK, "OK")
}

// ── Static assets ────────────────────────────────────────────────

pub async fn i18n_js() -> Result<Response> {
    Ok(([(header::CONTENT_TYPE, "application/javascript; charset=utf-8")],
        include_str!("../../resources/dashboard/i18n.js"))
        .into_response())
}

pub async fn rtl_css() -> Result<Response> {
    Ok(([(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../resources/dashboard/rtl.css"))
        .into_response())
}

pub async fn locale_en_us() -> Result<Response> {
    format::json(serde_json::from_str::<serde_json::Value>(
        include_str!("../../resources/dashboard/locales/en-US.json")
    ).unwrap_or_default())
}

pub async fn locale_ar() -> Result<Response> {
    format::json(serde_json::from_str::<serde_json::Value>(
        include_str!("../../resources/dashboard/locales/ar.json")
    ).unwrap_or_default())
}

pub async fn locale_he() -> Result<Response> {
    format::json(serde_json::from_str::<serde_json::Value>(
        include_str!("../../resources/dashboard/locales/he.json")
    ).unwrap_or_default())
}

pub async fn locale_fr() -> Result<Response> {
    format::json(serde_json::from_str::<serde_json::Value>(
        include_str!("../../resources/dashboard/locales/fr.json")
    ).unwrap_or_default())
}

pub async fn locale_es() -> Result<Response> {
    format::json(serde_json::from_str::<serde_json::Value>(
        include_str!("../../resources/dashboard/locales/es.json")
    ).unwrap_or_default())
}

pub async fn locale_de() -> Result<Response> {
    format::json(serde_json::from_str::<serde_json::Value>(
        include_str!("../../resources/dashboard/locales/de.json")
    ).unwrap_or_default())
}

pub async fn locale_it() -> Result<Response> {
    format::json(serde_json::from_str::<serde_json::Value>(
        include_str!("../../resources/dashboard/locales/it.json")
    ).unwrap_or_default())
}

// zh, ko, ja
pub async fn locale_zh() -> Result<Response> {
    format::json(serde_json::from_str::<serde_json::Value>(
        include_str!("../../resources/dashboard/locales/zh.json")
    ).unwrap_or_default())
}

pub async fn locale_ko() -> Result<Response> {
    format::json(serde_json::from_str::<serde_json::Value>(
        include_str!("../../resources/dashboard/locales/ko.json")
    ).unwrap_or_default())
}

pub async fn locale_ja() -> Result<Response> {
    format::json(serde_json::from_str::<serde_json::Value>(
        include_str!("../../resources/dashboard/locales/ja.json")
    ).unwrap_or_default())
}

pub async fn locale_ru() -> Result<Response> {
    format::json(serde_json::from_str::<serde_json::Value>(
        include_str!("../../resources/dashboard/locales/ru.json")
    ).unwrap_or_default())
}

pub async fn locale_pt() -> Result<Response> {
    format::json(serde_json::from_str::<serde_json::Value>(
        include_str!("../../resources/dashboard/locales/pt.json")
    ).unwrap_or_default())
}

pub async fn locale_hi() -> Result<Response> {
    format::json(serde_json::from_str::<serde_json::Value>(
        include_str!("../../resources/dashboard/locales/hi.json")
    ).unwrap_or_default())
}

/// Serves Popper.js (tooltip positioning library, required by Tippy.js v6).
pub async fn popper_js() -> Result<Response> {
    Ok(([(header::CONTENT_TYPE, "application/javascript; charset=utf-8")],
        include_str!("../../resources/dashboard/vendor/popper.min.js"))
        .into_response())
}

/// Serves Tippy.js (tooltip library).
pub async fn tippy_js() -> Result<Response> {
    Ok(([(header::CONTENT_TYPE, "application/javascript; charset=utf-8")],
        include_str!("../../resources/dashboard/vendor/tippy.min.js"))
        .into_response())
}

/// Serves Driver.js (guided walkthrough / product tour library).
pub async fn driver_js() -> Result<Response> {
    Ok(([(header::CONTENT_TYPE, "application/javascript; charset=utf-8")],
        include_str!("../../resources/dashboard/vendor/driver.min.js"))
        .into_response())
}

/// Registers dashboard routes including static assets.
pub fn routes() -> Routes {
    Routes::new()
        .add("/", get(index))
        .add("/index.html", get(index))
        .add("/imp-console", get(imp_console))
        .add("/health", get(health))
        // Static assets for i18n
        .add("/i18n.js", get(i18n_js))
        .add("/rtl.css", get(rtl_css))
        .add("/locales/en-US.json", get(locale_en_us))
        .add("/locales/ar.json", get(locale_ar))
        .add("/locales/he.json", get(locale_he))
        .add("/locales/fr.json", get(locale_fr))
        .add("/locales/es.json", get(locale_es))
        .add("/locales/de.json", get(locale_de))
        .add("/locales/it.json", get(locale_it))
        .add("/locales/zh.json", get(locale_zh))
        .add("/locales/ko.json", get(locale_ko))
        .add("/locales/ja.json", get(locale_ja))
        .add("/locales/ru.json", get(locale_ru))
        .add("/locales/pt.json", get(locale_pt))
        .add("/locales/hi.json", get(locale_hi))
        // Vendor libraries
        .add("/vendor/popper.min.js", get(popper_js))
        .add("/vendor/tippy.min.js", get(tippy_js))
        .add("/vendor/driver.min.js", get(driver_js))
}
