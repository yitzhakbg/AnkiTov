//! Reviewer controller — the Solo Web Reviewer API (spec §3.2).
//!
//! | Method | Path | Auth | Description |
//! |--------|------|------|-------------|
//! | GET  | `/api/v1/reviewer/session/current` | JWT (student) | Active capsule session + progress |
//! | GET  | `/api/v1/reviewer/session/cards`   | JWT (student) | Rendered cards for the active session |
//! | POST | `/api/v1/reviewer/answer`          | JWT (student) | Ingest one answer (ledger + audit + progress) |
//! | POST | `/api/v1/reviewer/import-seed`     | JWT (student) | Import a seed `.apkg` into the student's server-side collection |
//! | GET  | `/api/v1/reviewer/media/{file}`    | JWT (student) | Serve a media file from the student's collection |
//!
//! Static SPA: `/reviewer` (+ assets) served from `resources/reviewer/` via
//! `include_str!`/`include_bytes!` on the PUBLIC branch (zero-CDN claim).
//!
//! v1 slice scope (documented): the reviewer serves the student's ACTIVE
//! capsule session — the capsule generator already did the FSRS sorting, so
//! there is exactly one scheduling brain. Free-practice due queues and the
//! `web_review_log` FSRS replay step land in later slices.

use axum::extract::{Path as AxumPath, State};
use axum::response::IntoResponse;
use axum::{Extension, Json};
use loco_rs::prelude::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::middleware::auth::AuthUser;
use crate::models::entities::capsule_session as cs_entity;
use crate::services::card_renderer::{self, RawCard};
use crate::services::collection_store::{self, StudentCollection};
use crate::services::review_ingest::{self, ReviewAnswer};

const TAG: &str = "Reviewer";

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/reviewer")
        .add("/session/current", get(current_session))
        .add("/session/cards", get(session_cards))
        .add("/answer", post(answer))
        .add("/import-seed", post(import_seed))
        .add("/media/{file}", get(media))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /session/current — the student's active capsule session.
async fn current_session(
    State(ctx): State<AppContext>,
    Extension(user): Extension<AuthUser>,
) -> Result<Response> {
    let db = &ctx.db;
    let sessions = cs_entity::Entity::find()
        .filter(cs_entity::Column::StudentId.eq(user.id.to_string()))
        .filter(cs_entity::Column::Status.is_in(["pending", "in_progress"]))
        .order_by_desc(cs_entity::Column::CreatedAt)
        .all(db)
        .await?;

    match sessions.first() {
        Some(s) => {
            let card_ids: Vec<i64> =
                serde_json::from_str(&s.card_ids).unwrap_or_default();
            format::json(serde_json::json!({
                "status": "ok",
                "session": {
                    "id": s.id,
                    "status": s.status,
                    "cards_completed": s.cards_completed,
                    "cards_total": s.cards_total,
                    "card_ids": card_ids,
                    "session_week": s.session_week,
                },
            }))
        }
        None => format::json(serde_json::json!({
            "status": "no_session",
            "message": "No active practice session. Ask your teacher to assign one, or import a seed pack below.",
        })),
    }
}

/// GET /session/cards — rendered cards for the active session.
async fn session_cards(
    State(ctx): State<AppContext>,
    Extension(user): Extension<AuthUser>,
) -> Result<Response> {
    let db = &ctx.db;
    let session = cs_entity::Entity::find()
        .filter(cs_entity::Column::StudentId.eq(user.id.to_string()))
        .filter(cs_entity::Column::Status.is_in(["pending", "in_progress"]))
        .order_by_desc(cs_entity::Column::CreatedAt)
        .one(db)
        .await?;
    let Some(session) = session else {
        return format::json(serde_json::json!({"status": "no_session"}));
    };
    let card_ids: Vec<i64> = serde_json::from_str(&session.card_ids).unwrap_or_default();

    let col = StudentCollection::for_student(&user.id.to_string());
    if !col.exists() {
        return format::json(serde_json::json!({
            "status": "no_collection",
            "message": "No server-side collection for this student yet. Import a seed pack first.",
        }));
    }

    let raw_cards = match collection_store::read_cards(&col, &card_ids) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("[{}] failed to read cards for {}: {}", TAG, user.id, e);
            return format::json(serde_json::json!({"status": "error", "message": e.to_string()}));
        }
    };

    let rendered: Vec<serde_json::Value> = raw_cards
        .iter()
        .map(|rc| {
            let raw = RawCard {
                card_id: rc.card_id,
                note_id: rc.note_id,
                card_ord: rc.card_ord,
                deck_name: rc.deck_name.clone(),
                fields: rc.fields.clone(),
                qfmt: rc.qfmt.clone(),
                afmt: rc.afmt.clone(),
                is_cloze: rc.is_cloze,
            };
            let c = card_renderer::render_card(&raw, &rc.field_names);
            serde_json::json!({
                "card_id": c.card_id,
                "deck": c.deck_name,
                "question": c.question.html,
                "answer": c.answer.html,
                "flags": c.flags,
            })
        })
        .collect();

    format::json(serde_json::json!({
        "status": "ok",
        "session_id": session.id,
        "cards_completed": session.cards_completed,
        "cards_total": session.cards_total,
        "cards": rendered,
    }))
}

/// POST /answer — ingest one answer.
async fn answer(
    State(ctx): State<AppContext>,
    Extension(user): Extension<AuthUser>,
    Json(payload): Json<ReviewAnswer>,
) -> Result<Response> {
    match review_ingest::ingest_answer(&ctx.db, user.id, &user.email, &payload).await {
        Ok(result) => format::json(result),
        Err(review_ingest::IngestError::BadEase(e)) => {
            format::json(serde_json::json!({"status": "error", "message": format!("ease must be 1-4, got {}", e)}))
        }
        Err(review_ingest::IngestError::BadAnswerId) => {
            format::json(serde_json::json!({"status": "error", "message": "answer_id must be 1-64 chars"}))
        }
        Err(e) => {
            tracing::error!("[{}] ingest failed for {}: {}", TAG, user.id, e);
            format::json(serde_json::json!({"status": "error", "message": "ingest failed"}))
        }
    }
}

/// POST /import-seed — import a seed `.apkg` into the student's collection.
async fn import_seed(
    State(ctx): State<AppContext>,
    Extension(user): Extension<AuthUser>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Response> {
    let Some(package) = payload.get("package").and_then(|v| v.as_str()) else {
        return format::json(serde_json::json!({"status": "error", "message": "missing package"}));
    };
    // strict allowlist, same as the deck library
    let valid = !package.is_empty()
        && package.len() <= 128
        && package.ends_with(".apkg")
        && package
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !package.contains("..");
    if !valid {
        return format::json(serde_json::json!({"status": "error", "message": "invalid package name"}));
    }
    // resolve the seed dir (same order as deck_library)
    let seed_dir = std::env::var("ANKITOV_SEED_DIR")
        .map(|d| std::path::PathBuf::from(d).join("packages"))
        .unwrap_or_else(|_| std::path::PathBuf::from("content/seed/packages"));
    let path = seed_dir.join(package);
    match collection_store::import_apkg(&user.id.to_string(), &path) {
        Ok(media) => {
            tracing::info!("[{}] imported {} for {} ({} media)", TAG, package, user.id, media);
            format::json(serde_json::json!({"status": "ok", "package": package, "media_files": media}))
        }
        Err(e) => {
            tracing::warn!("[{}] import failed for {}: {}", TAG, user.id, e);
            format::json(serde_json::json!({"status": "error", "message": e.to_string()}))
        }
    }
}

// ── Static SPA (public, zero-CDN — include_str! into the binary) ──

pub async fn spa_index() -> Result<Response> {
    let mut resp: Response = format::html(include_str!("../../resources/reviewer/index.html"))?;
    resp.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::header::HeaderValue::from_static("no-store, no-cache, must-revalidate, max-age=0"),
    );
    Ok(resp)
}

pub async fn spa_css() -> impl IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8")],
     include_str!("../../resources/reviewer/style.css"))
}

pub async fn spa_js() -> impl IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "application/javascript; charset=utf-8")],
     include_str!("../../resources/reviewer/app.js"))
}

pub async fn spa_manifest() -> impl IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "application/manifest+json")],
     include_str!("../../resources/reviewer/manifest.webmanifest"))
}

pub async fn spa_sw() -> impl IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "application/javascript; charset=utf-8")],
     include_str!("../../resources/reviewer/sw.js"))
}

pub async fn spa_icon() -> impl IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "image/svg+xml")],
     include_str!("../../resources/reviewer/icon.svg"))
}

/// GET /media/{file} — serve a media file from the student's collection.
async fn media(
    AxumPath(file): AxumPath<String>,
    Extension(user): Extension<AuthUser>,
) -> Result<Response> {
    match collection_store::media_path(&user.id.to_string(), &file) {
        Some(path) => {
            let bytes = tokio::fs::read(&path).await.unwrap_or_default();
            let mime = match path.extension().and_then(|e| e.to_str()) {
                Some("mp3") => "audio/mpeg",
                Some("ogg") => "audio/ogg",
                Some("wav") => "audio/wav",
                Some("png") => "image/png",
                Some("jpg") | Some("jpeg") => "image/jpeg",
                Some("webp") => "image/webp",
                Some("svg") => "image/svg+xml",
                Some("gif") => "image/gif",
                _ => "application/octet-stream",
            };
            Ok((
                [(axum::http::header::CONTENT_TYPE, mime)],
                bytes,
            )
                .into_response())
        }
        None => Ok(([(axum::http::header::CONTENT_TYPE, "text/plain")], "not found").into_response()),
    }
}
