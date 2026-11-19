//! Classroom mode toggle — the teacher clicks a button in the dashboard and
//! the server opens itself to the network. No CLI, no restart.
//!
//! The primary listener keeps its private bind (127.0.0.1 by default, set by
//! the launcher). Enabling classroom mode binds an ADDITIONAL listener on the
//! LAN IP serving the exact same router, so students on the same network can
//! reach `/reviewer`. Disabling drops that listener. The localhost listener
//! is never touched — the teacher's own access is unaffected either way.
//!
//! The router is stashed by `App::after_routes` (see `set_router`); a handler
//! cannot reach the live router through `AppContext`, so the stash is the
//! bridge between boot and the toggle.

use axum::{Extension, Router};
use loco_rs::prelude::*;
use std::sync::{Mutex, OnceLock};

use crate::middleware::auth::ensure_role;
use crate::middleware::auth::AuthUser;
static ROUTER: OnceLock<Router> = OnceLock::new();
static CLASSROOM: Mutex<Option<ClassroomListener>> = Mutex::new(None);

struct ClassroomListener {
    lan_ip: String,
    port: u16,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::task::JoinHandle<()>,
}

/// Called once from `App::after_routes` with the finished router.
pub fn set_router(router: Router) {
    let _ = ROUTER.set(router);
}

fn port() -> u16 {
    std::env::var("ANKITOV_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(5150)
}

fn status_json(enabled: bool, lan_ip: Option<String>) -> serde_json::Value {
    let port = port();
    match (enabled, lan_ip) {
        (true, Some(ip)) => serde_json::json!({
            "enabled": true, "lan_ip": ip, "port": port,
            "join_url": format!("http://{ip}:{port}/reviewer"),
        }),
        (true, None) => serde_json::json!({"enabled": true, "port": port}),
        (false, _) => serde_json::json!({"enabled": false, "port": port}),
    }
}

/// POST /management/classroom/enable — bind a LAN listener serving the app.
#[utoipa::path(
    post,
    path = "/management/classroom/enable",
    responses((status = 200, description = "Classroom listener active")),
    tag = "Management Console"
)]
pub async fn enable(Extension(user): Extension<AuthUser>) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;

    {
        let guard = CLASSROOM.lock().unwrap();
        if let Some(l) = guard.as_ref() {
            // Already on — idempotent.
            return format::json(status_json(true, Some(l.lan_ip.clone())));
        }
    }

    let lan_ip = super::classes::lan_ip().ok_or_else(|| {
        loco_rs::Error::BadRequest("could not determine this machine's LAN IP".into())
    })?;
    let router = ROUTER.get().ok_or_else(|| {
        tracing::error!("classroom enable: router not stashed at boot");
        loco_rs::Error::InternalServerError
    })?;
    let port = port();

    let listener = tokio::net::TcpListener::bind((lan_ip.as_str(), port))
        .await
        .map_err(|e| {
            tracing::error!("classroom bind {lan_ip}:{port} failed: {e:?}");
            loco_rs::Error::BadRequest(format!(
                "could not bind {lan_ip}:{port} — is another instance already listening? ({e})"
            ))
        })?;

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let router = router.clone();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.changed().await;
            })
            .await;
        tracing::info!("classroom listener stopped");
    });

    *CLASSROOM.lock().unwrap() = Some(ClassroomListener {
        lan_ip: lan_ip.clone(),
        port,
        shutdown: shutdown_tx,
        handle,
    });
    tracing::info!("classroom mode ON — students join at http://{lan_ip}:{port}/reviewer");
    format::json(status_json(true, Some(lan_ip)))
}

/// POST /management/classroom/disable — drop the LAN listener.
#[utoipa::path(
    post,
    path = "/management/classroom/disable",
    responses((status = 200, description = "Classroom listener stopped")),
    tag = "Management Console"
)]
pub async fn disable(Extension(user): Extension<AuthUser>) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    let listener = CLASSROOM.lock().unwrap().take();
    if let Some(l) = listener {
        let _ = l.shutdown.send(true);
        l.handle.abort();
        tracing::info!("classroom mode OFF");
    }
    format::json(status_json(false, None))
}

/// GET /management/classroom/status
#[utoipa::path(
    get,
    path = "/management/classroom/status",
    responses((status = 200, description = "Classroom mode status")),
    tag = "Management Console"
)]
pub async fn status(Extension(user): Extension<AuthUser>) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    let guard = CLASSROOM.lock().unwrap();
    match guard.as_ref() {
        Some(l) => format::json(status_json(true, Some(l.lan_ip.clone()))),
        None => format::json(status_json(false, None)),
    }
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management/classroom")
        .add("/enable", post(enable))
        .add("/disable", post(disable))
        .add("/status", get(status))
}
