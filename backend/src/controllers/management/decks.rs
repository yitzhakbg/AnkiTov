//! Deck management endpoints.
//!
//! Handles upload, distribution, and listing of Anki decks across
//! user profiles, groups, and classes.

use crate::models::entities::deck as deck_entity;
use crate::models::entities::deck_distribution as dist_entity;
use crate::models::management::{ApiResponse, BindDeckTrackRequest, DeckDistributionResponse, DeckInfo, DistributeDeckRequest, ListDeckQuery};
use crate::models::entities::track as track_entity;
use axum::{body::Bytes, extract::{DefaultBodyLimit, Multipart, Path, Query, State}, Extension, Json};
use loco_rs::{app::AppContext, prelude::*};
use crate::middleware::auth::AuthUser;
use crate::middleware::auth::ensure_role;
use sea_orm::{
    entity::prelude::{EntityTrait, PaginatorTrait},
    ActiveModelTrait, ActiveValue, QueryFilter, Set,
};
use sha2::{Digest, Sha256};
use crate::services::anki_connect::AnkiConnectError;

const TAG: &str = "Management Console";

/// Routes for deck management.
pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management/decks")
        .add("/", get(list_decks))
        // Upload carries its own body limit — Loco's global default (2 MB)
        // is far too small for real media-bearing decks.
        .add("/", post(upload_deck).layer(DefaultBodyLimit::max(DEFAULT_MAX_UPLOAD_BYTES)))
        .add("/:deck_id", get(get_deck))
        .add("/:deck_id", delete(delete_deck))
        .add("/:deck_id/distribute", post(distribute_deck))
        // P2 minimal deck-track binding
        .add("/:deck_id/track", put(bind_deck_track))
        .add("/:deck_id/track", delete(unbind_deck_track))
        // Deck Library — one-click import of bundled seed decks
        .add("/available", get(super::deck_library::list_available_decks))
        .add("/available/:name/import", post(super::deck_library::import_available_deck))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// List all decks with optional filtering by distribution target.
#[utoipa::path(
    get,
    path = "/management/decks",
    params(
        ("page" = Option<u32>, Query, description = "Page number"),
        ("per_page" = Option<u32>, Query, description = "Items per page"),
        ("target_type" = Option<String>, Query, description = "Filter: user | group | class"),
        ("target_id" = Option<String>, Query, description = "Filter by target ID")
    ),
    responses(
        (status = 200, description = "List of decks", body = Vec<DeckInfo>)
    ),
    tag = TAG
)]
pub async fn list_decks(
    State(ctx): State<AppContext>,
    Query(params): Query<ListDeckQuery>
) -> Result<Response> {
    tracing::info!(
        "Listing decks — page={}, per_page={}, target={:?}",
        params.page, params.per_page, params.target_type
    );

    let page: u64 = params.page.max(1) as u64;
    let per_page: u64 = params.per_page.max(1) as u64;

    let paginator = deck_entity::Entity::find()
        .paginate(&ctx.db, per_page);

    let decks: Vec<DeckInfo> = paginator
        .fetch_page(page.saturating_sub(1))
        .await
        .map_err(|e| {
            tracing::error!("list_decks DB error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?
        .into_iter()
        .map(|d| DeckInfo {
            id: d.id.to_string(),
            name: d.name,
            card_count: d.card_count as u32,
            created_at: d.created_at,
            updated_at: d.updated_at,
            distribution_count: 0, // TODO: count from DeckDistribution
            track_id: d.track_id.clone(),
        })
        .collect();

    format::json(decks)
}

/// Maximum accepted upload size. The route also carries a matching
/// `DefaultBodyLimit` layer (Loco's global default is 2 MB, far too small
/// for real media-bearing decks). Override for tests/ops via
/// `ANKITOV_UPLOAD_MAX_BYTES`.
const DEFAULT_MAX_UPLOAD_BYTES: usize = 100 * 1024 * 1024;

fn max_upload_bytes() -> usize {
    std::env::var("ANKITOV_UPLOAD_MAX_BYTES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MAX_UPLOAD_BYTES)
}

/// Resolve the directory uploaded `.apkg` packages are persisted to.
///
/// Order: `ANKITOV_UPLOAD_DIR` env override → `uploads/decks` relative to
/// the working directory (the backend's CWD is the data dir in container
/// and release layouts, matching the seed-dir convention).
fn upload_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("ANKITOV_UPLOAD_DIR") {
        return std::path::PathBuf::from(dir);
    }
    std::path::PathBuf::from("uploads/decks")
}

/// Reduce a filename stem to a safe on-disk component
/// (`[A-Za-z0-9._-]`, everything else → `_`).
fn sanitize_stem(stem: &str) -> String {
    let cleaned: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "deck".to_string()
    } else {
        cleaned
    }
}

/// Upload a new deck package (.apkg) and import it into the shared
/// collection.
///
/// Unlike the deck library (bundled seed packages), this accepts arbitrary
/// teacher-provided packages. The upload is REAL: the package is persisted
/// under the data dir, imported through AnkiConnect `importPackage` (Zone 2
/// — the only scheduler-safe import path), and registered with the deck's
/// actual name (resolved by diffing `deckNames` across the import) and its
/// real card count (queried from the collection after import).
///
/// Validation: `.apkg` filename only, non-empty body, size cap
/// (`max_upload_bytes`). Duplicate uploads (same SHA-256) are idempotent —
/// the existing deck record is returned without re-importing.
#[utoipa::path(
    post,
    path = "/management/decks/upload",
    request_body(content = String, description = "APKG file multipart body", content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Deck uploaded and imported successfully", body = DeckInfo),
        (status = 400, description = "Invalid deck package (not .apkg, empty, oversized, or rejected by Anki)"),
        (status = 503, description = "AnkiConnect unreachable — import needs Anki running")
    ),
    tag = TAG
)]
pub async fn upload_deck(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    mut multipart: Multipart)
             -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    let mut file_data = Bytes::new();
    let mut filename = String::new();

    // Iterate over multipart fields
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            if let Some(fname) = field.file_name() {
                filename = fname.to_string();
            }
            if let Ok(bytes) = field.bytes().await {
                file_data = bytes;
            }
        }
    }

    if file_data.is_empty() {
        return format::json(ApiResponse {
            success: false,
            message: "No file uploaded or file is empty".to_string(),
        });
    }

    // .apkg-only: the Zone 2 import path (AnkiConnect importPackage) only
    // accepts Anki package archives. .zip was accepted by the old stub but
    // never actually imported — reject it up front with a clear message.
    if !filename.ends_with(".apkg") {
        return format::json(ApiResponse {
            success: false,
            message: "Only .apkg deck packages are supported".to_string(),
        });
    }

    if file_data.len() > max_upload_bytes() {
        return format::json(ApiResponse {
            success: false,
            message: format!(
                "Deck package too large ({} bytes; limit is {} bytes)",
                file_data.len(),
                max_upload_bytes()
            ),
        });
    }

    let checksum = hex::encode(Sha256::digest(&file_data));

    // Idempotency: a deck row with the same checksum means this exact
    // package was already uploaded and imported — return it unchanged
    // instead of re-importing into the collection.
    let existing = deck_entity::Entity::find()
        .filter(deck_entity::Column::ChecksumSha256.eq(checksum.clone()))
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("upload_deck dedupe lookup failed: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;
    if let Some(existing) = existing {
        return format::json(DeckInfo {
            id: existing.id.to_string(),
            name: existing.name.clone(),
            card_count: existing.card_count as u32,
            created_at: existing.created_at,
            updated_at: existing.updated_at,
            distribution_count: 0,
            track_id: existing.track_id.clone(),
        });
    }

    // Persist under the data dir so the package survives restarts and can
    // be re-imported (e.g. after the DB row is removed).
    let stem = filename.strip_suffix(".apkg").unwrap_or(&filename);
    let dir = upload_dir();
    std::fs::create_dir_all(&dir).map_err(|e| {
        tracing::error!("upload_deck create_dir_all {:?} failed: {:?}", dir, e);
        loco_rs::Error::InternalServerError
    })?;
    let path = dir.join(format!(
        "{}-{}.apkg",
        &checksum[..8],
        sanitize_stem(stem)
    ));
    std::fs::write(&path, &file_data).map_err(|e| {
        tracing::error!("upload_deck write {:?} failed: {:?}", path, e);
        loco_rs::Error::InternalServerError
    })?;
    // AnkiConnect resolves package paths against the Anki process's CWD,
    // not ours — hand it an absolute path or the import fails with
    // "No such file or directory" whenever the two CWDs differ.
    let abs_path = std::fs::canonicalize(&path).map_err(|e| {
        tracing::error!("upload_deck canonicalize {:?} failed: {:?}", path, e);
        loco_rs::Error::InternalServerError
    })?;

    // Import through Anki's own scheduler (Zone 2 — the only safe path),
    // resolving the deck's actual name by diffing deckNames across the
    // import (AnkiConnect importPackage takes only the path).
    let client = super::deck_library::anki_client();
    let expected_name = sanitize_stem(stem);
    let before: std::collections::HashSet<String> = client
        .deck_names()
        .await
        .map(|names| names.into_iter().collect())
        .unwrap_or_default();

    let imported = client
        .import_package(&abs_path.display().to_string())
        .await
        .map_err(|e| {
            // No DB row was created, so the persisted package is now an
            // orphan — best-effort cleanup.
            let _ = std::fs::remove_file(&path);
            if matches!(e, AnkiConnectError::Unreachable(_)) {
                // AnkiConnect unreachable (no Anki running — common in
                // local deploys where the backend has no Anki binary):
                // clear, actionable message instead of an opaque 500.
                return loco_rs::Error::CustomError(
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    loco_rs::controller::ErrorDetail::new(
                        "anki_unreachable",
                        "Deck upload needs Anki running with the AnkiConnect \
                         add-on, but it could not be reached. Start Anki (or \
                         click Refresh to auto-launch it) and try again.",
                    ),
                );
            }
            // Anki rejected the package (corrupt archive, unsupported
            // notetypes, …) — a client error, not a server fault.
            tracing::warn!("upload_deck import {filename} rejected: {:?}", e);
            loco_rs::Error::BadRequest(format!(
                "Anki rejected the package: {e}"
            ))
        })?;
    if !imported {
        tracing::warn!("upload_deck import {filename}: AnkiConnect returned false");
    }

    // Anki's .apkg importer materializes the deck shortly AFTER importPackage
    // responds (observed ~100-300ms lag), so poll deckNames briefly instead of
    // snapshotting once — an immediate snapshot can miss the new deck and fall
    // back to the filename stem with a 0 card count. A diff that never grows
    // is still legitimate (re-import of a package whose deck already exists
    // merges by note GUID), so stop early when the expected name was already
    // present before the import.
    let expected_in_collection = before.contains(&expected_name);
    let mut new_decks: Vec<String> = Vec::new();
    for _ in 0..20 {
        match client.deck_names().await {
            Ok(after) => {
                new_decks = after.into_iter().filter(|n| !before.contains(n)).collect();
            }
            Err(e) => {
                tracing::warn!("upload_deck post-import deckNames failed for {filename}: {:?}", e);
                break;
            }
        }
        if !new_decks.is_empty() || expected_in_collection {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let deck_name = if new_decks.len() == 1 {
        new_decks[0].clone()
    } else if new_decks.iter().any(|n| n == &expected_name) {
        expected_name.clone()
    } else if new_decks.is_empty() {
        // Deck already existed in the collection (e.g. re-import
        // after the DB row was removed) — Anki merged by note GUID.
        expected_name.clone()
    } else {
        // Multiple new decks (subdecks): deterministic pick.
        let picked = new_decks
            .into_iter()
            .min()
            .unwrap_or_else(|| expected_name.clone());
        tracing::warn!("upload_deck import {filename}: multiple new decks, picked {picked:?}");
        picked
    };

    // Real card count from the collection after import.
    let card_count: i32 = match client
        .find_cards(&format!("deck:\"{deck_name}\""))
        .await
    {
        Ok(cards) => cards.len() as i32,
        Err(e) => {
            tracing::warn!("upload_deck post-import count failed for {deck_name}: {:?}", e);
            0
        }
    };

    let now_ts = chrono::Utc::now().timestamp();
    let deck = deck_entity::ActiveModel {
        id: ActiveValue::NotSet,
        name: Set(deck_name.clone()),
        description: Set(Some(format!(
            "Uploaded via management console ({filename})"
        ))),
        card_count: Set(card_count),
        file_size_bytes: Set(file_data.len() as i64),
        checksum_sha256: Set(checksum),
        uploaded_by: Set(user.id.to_string()),
        created_at: Set(now_ts),
        updated_at: Set(now_ts),
        track_id: ActiveValue::NotSet,
    }
    .insert(&ctx.db)
    .await
    .map_err(|e| {
        tracing::error!("upload_deck insert failed: {:?}", e);
        loco_rs::Error::InternalServerError
    })?;

    tracing::info!(
        "Deck uploaded and imported: id={}, name={}, file={}, cards={}",
        deck.id,
        deck.name,
        filename,
        card_count
    );

    format::json(DeckInfo {
        id: deck.id.to_string(),
        name: deck.name,
        card_count: deck.card_count as u32,
        created_at: deck.created_at,
        updated_at: deck.updated_at,
        distribution_count: 0,
        track_id: deck.track_id.clone(),
    })
}
#[utoipa::path(
    post,
    path = "/management/decks/{deck_id}/distribute",
    params(
        ("deck_id" = String, Path, description = "Deck ID to distribute")
    ),
    request_body = DistributeDeckRequest,
    responses(
        (status = 201, description = "Deck distributed successfully", body = DeckDistributionResponse),
        (status = 404, description = "Deck not found"),
        (status = 400, description = "Invalid target")
    ),
    tag = TAG
)]
pub async fn distribute_deck(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(deck_id): Path<String>,
    Json(payload): Json<DistributeDeckRequest>)
     -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    let deck_i32: i32 = match deck_id.parse() {
        Ok(n) => n,
        Err(_) => {
            return format::json(ApiResponse {
                success: false,
                message: format!("Invalid deck ID: {}", deck_id),
            });
        }
    };

    tracing::info!(
        "Distributing deck {} ({}) to {}:{}",
        deck_id, deck_i32, payload.target_type, payload.target_id
    );

    if !["user", "group", "class"].contains(&payload.target_type.as_str()) {
        return format::json(ApiResponse {
            success: false,
            message: "target_type must be one of: user, group, class".to_string(),
        });
    }

    // Validate target_id is a well-formed UUID
    if uuid::Uuid::parse_str(&payload.target_id).is_err() {
        return format::json(ApiResponse {
            success: false,
            message: format!("Invalid target_id: '{}' is not a valid UUID", payload.target_id),
        });
    }

    let _deck = match deck_entity::Entity::find_by_id(deck_i32)
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("distribute_deck find deck DB error: {:?}", e);
            loco_rs::Error::InternalServerError
        })? {
        Some(d) => d,
        None => {
            return format::json(ApiResponse {
                success: false,
                message: format!("Deck {} not found", deck_id),
            });
        }
    };

    let dist = dist_entity::ActiveModel {
        id: ActiveValue::NotSet, // pk_auto handles this
        deck_id: Set(deck_i32),
        target_type: Set(payload.target_type.clone()),
        target_id: Set(payload.target_id.clone()),
        distributed_by: Set(uuid::Uuid::nil().to_string()),
        distributed_at: Set(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64),
    }
    .insert(&ctx.db)
    .await
    .map_err(|e| {
        tracing::error!("distribute_deck insert DB error: {:?}", e);
        loco_rs::Error::InternalServerError
    })?;

    tracing::info!(
        "Deck {} distributed to {}:{}",
        deck_id, payload.target_type, payload.target_id
    );

    format::json(DeckDistributionResponse {
        deck_id: dist.deck_id.to_string(),
        target_type: dist.target_type.clone(),
        target_id: dist.target_id.clone(),
        status: "distributed".to_string(),
        distributed_at: dist.distributed_at,
    })
}

/// Get metadata for a single deck.
#[utoipa::path(
    get,
    path = "/management/decks/{deck_id}",
    params(
        ("deck_id" = String, Path, description = "Deck ID")
    ),
    responses(
        (status = 200, description = "Deck metadata", body = DeckInfo),
        (status = 404, description = "Deck not found")
    ),
    tag = TAG
)]
pub async fn get_deck(
    State(ctx): State<AppContext>,
    Path(deck_id): Path<String>
) -> Result<Response> {
    let deck_i32: i32 = match deck_id.parse() {
        Ok(n) => n,
        Err(_) => {
            return format::json(ApiResponse {
                success: false,
                message: format!("Invalid deck ID: {}", deck_id),
            });
        }
    };

    let deck = match deck_entity::Entity::find_by_id(deck_i32)
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("get_deck DB error: {:?}", e);
            loco_rs::Error::InternalServerError
        })? {
        Some(d) => d,
        None => {
            return format::json(ApiResponse {
                success: false,
                message: format!("Deck {} not found", deck_id),
            });
        }
    };

    format::json(DeckInfo {
        id: deck.id.to_string(),
        name: deck.name,
        card_count: deck.card_count as u32,
        created_at: deck.created_at,
        updated_at: deck.updated_at,
        distribution_count: 0,
        track_id: deck.track_id.clone(),
    })
}

/// Delete a deck (only if not distributed to any targets).
#[utoipa::path(
    delete,
    path = "/management/decks/{deck_id}",
    params(
        ("deck_id" = String, Path, description = "Deck ID")
    ),
    responses(
        (status = 200, description = "Deck deleted", body = ApiResponse),
        (status = 409, description = "Deck has active distributions — cannot delete")
    ),
    tag = TAG
)]
pub async fn delete_deck(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(deck_id): Path<String>)
     -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    tracing::warn!("Delete request for deck: {}", deck_id);

    let deck_i32: i32 = match deck_id.parse() {
        Ok(n) => n,
        Err(_) => {
            return format::json(ApiResponse {
                success: false,
                message: format!("Invalid deck ID: {}", deck_id),
            });
        }
    };

    // Check for active distributions
    let dist_count: u64 = dist_entity::Entity::find()
        .filter(dist_entity::Column::DeckId.eq(deck_id.clone()))
        .count(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("delete_deck count DB error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    if dist_count > 0 {
        return format::json(ApiResponse {
            success: false,
            message: format!(
                "Cannot delete deck {} — has {} active distributions",
                deck_id, dist_count
            ),
        });
    }

    let deck = match deck_entity::Entity::find_by_id(deck_i32)
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("delete_deck find deck DB error: {:?}", e);
            loco_rs::Error::InternalServerError
        })? {
        Some(d) => d,
        None => {
            return format::json(ApiResponse {
                success: false,
                message: format!("Deck {} not found", deck_id),
            });
        }
    };

    let deck_model: deck_entity::ActiveModel = deck.into();
    deck_model.delete(&ctx.db).await.map_err(|e| {
        tracing::error!("delete_deck delete DB error: {:?}", e);
        loco_rs::Error::InternalServerError
    })?;

    format::json(ApiResponse {
        success: true,
        message: format!("Deck {} deleted", deck_id),
    })
}

/// Bind a deck to a track (P2 minimal deck-track binding).
#[utoipa::path(
    put,
    path = "/management/decks/{deck_id}/track",
    params(
        ("deck_id" = i32, Path, description = "Deck ID")
    ),
    request_body = BindDeckTrackRequest,
    responses(
        (status = 200, description = "Deck bound to track", body = ApiResponse)
    ),
    tag = TAG
)]
pub async fn bind_deck_track(
    State(ctx): State<AppContext>,
    Extension(user): Extension<AuthUser>,
    Path(deck_id): Path<String>,
    Json(payload): Json<BindDeckTrackRequest>
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;

    let deck_i32: i32 = match deck_id.parse() {
        Ok(n) => n,
        Err(_) => {
            return format::json(ApiResponse {
                success: false,
                message: format!("Invalid deck ID: {}", deck_id),
            });
        }
    };

    let deck = match deck_entity::Entity::find_by_id(deck_i32).one(&ctx.db).await.map_err(|e| {
        tracing::error!("bind_deck_track DB error (deck lookup): {:?}", e);
        loco_rs::Error::InternalServerError
    })? {
        Some(d) => d,
        None => {
            return format::json(ApiResponse {
                success: false,
                message: format!("Deck {} not found", deck_id),
            });
        }
    };

    let track = match track_entity::Entity::find_by_id(payload.track_id.clone())
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("bind_deck_track DB error (track lookup): {:?}", e);
            loco_rs::Error::InternalServerError
        })? {
        Some(t) => t,
        None => {
            return format::json(ApiResponse {
                success: false,
                message: format!("Track {} not found", payload.track_id),
            });
        }
    };

    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let mut deck: deck_entity::ActiveModel = deck.into();
    deck.track_id = Set(Some(payload.track_id.clone()));
    deck.updated_at = Set(now_ts);
    deck.update(&ctx.db).await.map_err(|e| {
        tracing::error!("bind_deck_track update failed: {:?}", e);
        loco_rs::Error::InternalServerError
    })?;

    tracing::info!(
        "Deck {} bound to track {} ({})",
        deck_i32,
        payload.track_id,
        track.name
    );
    format::json(ApiResponse {
        success: true,
        message: format!("Deck {} bound to track {}", deck_i32, payload.track_id),
    })
}

/// Unbind a deck from its track (P2 minimal deck-track binding).
#[utoipa::path(
    delete,
    path = "/management/decks/{deck_id}/track",
    params(
        ("deck_id" = i32, Path, description = "Deck ID")
    ),
    responses(
        (status = 200, description = "Deck unbound from track", body = ApiResponse)
    ),
    tag = TAG
)]
pub async fn unbind_deck_track(
    State(ctx): State<AppContext>,
    Extension(user): Extension<AuthUser>,
    Path(deck_id): Path<String>
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;

    let deck_i32: i32 = match deck_id.parse() {
        Ok(n) => n,
        Err(_) => {
            return format::json(ApiResponse {
                success: false,
                message: format!("Invalid deck ID: {}", deck_id),
            });
        }
    };

    let deck = match deck_entity::Entity::find_by_id(deck_i32).one(&ctx.db).await.map_err(|e| {
        tracing::error!("unbind_deck_track DB error: {:?}", e);
        loco_rs::Error::InternalServerError
    })? {
        Some(d) => d,
        None => {
            return format::json(ApiResponse {
                success: false,
                message: format!("Deck {} not found", deck_id),
            });
        }
    };

    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let mut deck: deck_entity::ActiveModel = deck.into();
    deck.track_id = Set(None);
    deck.updated_at = Set(now_ts);
    deck.update(&ctx.db).await.map_err(|e| {
        tracing::error!("unbind_deck_track update failed: {:?}", e);
        loco_rs::Error::InternalServerError
    })?;

    tracing::info!("Deck {} unbound from track", deck_i32);
    format::json(ApiResponse {
        success: true,
        message: format!("Deck {} unbound from track", deck_i32),
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
