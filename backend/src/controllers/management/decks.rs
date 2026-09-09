//! Deck management endpoints.
//!
//! Handles upload, distribution, and listing of Anki decks across
//! user profiles, groups, and classes.

use crate::models::entities::deck as deck_entity;
use crate::models::entities::deck_distribution as dist_entity;
use crate::models::management::{ApiResponse, BindDeckTrackRequest, DeckDistributionResponse, DeckInfo, DistributeDeckRequest, ListDeckQuery};
use crate::models::entities::track as track_entity;
use axum::{body::Bytes, extract::{Multipart, Path, Query, State}, Extension, Json};
use loco_rs::{app::AppContext, prelude::*};
use crate::middleware::auth::AuthUser;
use crate::middleware::auth::ensure_role;
use sea_orm::{
    entity::prelude::{EntityTrait, PaginatorTrait},
    ActiveModelTrait, ActiveValue, Set,
};
use sha2::{Digest, Sha256};

const TAG: &str = "Management Console";

/// Routes for deck management.
pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management/decks")
        .add("/", get(list_decks))
        .add("/", post(upload_deck))
        .add("/:deck_id", get(get_deck))
        .add("/:deck_id", delete(delete_deck))
        .add("/:deck_id/distribute", post(distribute_deck))
        // P2 minimal deck-track binding
        .add("/:deck_id/track", put(bind_deck_track))
        .add("/:deck_id/track", delete(unbind_deck_track))
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

/// Upload a new deck package (.colpkg) to the system.
#[utoipa::path(
    post,
    path = "/management/decks/upload",
    request_body(content = String, description = "APKG file binary or Multipart body", content_type = "application/octet-stream"),
    responses(
        (status = 201, description = "Deck uploaded successfully", body = DeckInfo),
        (status = 400, description = "Invalid deck package"),
        (status = 500, description = "Upload failed")
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
    let mut deck_name = String::new();

    // Iterate over multipart fields
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            // Get original filename
            if let Some(filename) = field.file_name() {
                // Remove extension .apkg or .zip
                deck_name = filename
                    .strip_suffix(".apkg")
                    .or_else(|| filename.strip_suffix(".zip"))
                    .unwrap_or(filename)
                    .to_string();
            }
            if let Ok(bytes) = field.bytes().await {
                file_data = bytes;
            }
        }
    }

    // Fallback if empty
    if file_data.is_empty() {
        return format::json(ApiResponse {
            success: false,
            message: "No file uploaded or file is empty".to_string(),
        });
    }

    if deck_name.is_empty() {
        let now_ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        deck_name = format!("Deck-{}", now_ts);
    }

    let checksum = hex::encode(Sha256::digest(&file_data));
    let card_count = estimate_card_count(file_data.len());
    let now_ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;

    let deck = deck_entity::ActiveModel {
        id: ActiveValue::NotSet,
        name: Set(deck_name),
        description: Set(None),
        card_count: Set(card_count),
        file_size_bytes: Set(file_data.len() as i64),
        checksum_sha256: Set(checksum),
        uploaded_by: Set(uuid::Uuid::nil().to_string()),
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

    tracing::info!("Deck uploaded: id={}, name={}", deck.id, deck.name);

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

/// Rough card count estimate from file size.
/// TODO: replace with actual .colpkg parsing via anki library.
pub fn estimate_card_count(file_bytes: usize) -> i32 {
    const AVG_CARD_SIZE: usize = 512;
    (file_bytes / AVG_CARD_SIZE).max(1) as i32
}