//! Health check controller — `/api/v1/health` endpoint.
//!
//! Provides a DB-aware health check for load balancers and monitoring.

use axum::extract::State;
use loco_rs::prelude::*;

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/health")
        .add("/", get(check))
}

/// Full health check with database connectivity.
///
/// Returns 200 `{"status":"ok",...}` if the database is reachable,
/// or 503 `{"status":"degraded",...}` if it's not.
pub async fn check(State(ctx): State<AppContext>) -> Result<Response> {
    let db_ok = ctx.db.ping().await.is_ok();
    let status = if db_ok { "ok" } else { "degraded" };
    let http_code = if db_ok { 200 } else { 503 };

    let body = serde_json::json!({
        "status": status,
        "service": "ankitov",
        "version": env!("CARGO_PKG_VERSION"),
        "db": db_ok,
    });

    Ok((
        axum::http::StatusCode::from_u16(http_code).unwrap(),
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string_pretty(&body).unwrap(),
    )
        .into_response())
}
