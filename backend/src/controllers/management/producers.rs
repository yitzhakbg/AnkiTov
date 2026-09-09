//! Producer management endpoints (P1: Producer upload + verification, spec:
//! retention-period-plan §Phase 1).
//!
//! Routes (prefix `/management/producers`):
//! - `POST /`                — create producer (admin)
//! - `GET /`                 — list producers + deck catalog (admin/teacher)
//! - `GET /:producer_id/decks` — catalog status per producer (admin/teacher)
//! - `POST /:producer_id/decks` — multipart `.apkg` upload → verify → write
//!     `deck` + `producer_deck` (admin; producer uploads proxied by admin in
//!     v1 — self-service producer login is post-v1)
//!
//! Verification is consumed via the injectable
//! [`crate::services::deck_verification::Verifier`] with env-driven
//! selection: `ANKITOV_ANKI=test` selects the deterministic Anki-free stub
//! (unit/integration tests), the default selects the real headless-Anki path.

use crate::middleware::auth::{ensure_role, AuthUser};
use crate::models::entities::deck as deck_entity;
use crate::models::entities::producer as producer_entity;
use crate::models::entities::producer_deck as producer_deck_entity;
use crate::services::deck_verification::{verifier, CardTypeCensus, MediaIntegrity};
use axum::{extract::{Multipart, Path, State}, Extension, Json};
use loco_rs::{app::AppContext, prelude::*};
use sea_orm::{
    entity::prelude::{EntityTrait, ColumnTrait, QueryFilter},
    ActiveModelTrait, ActiveValue, Set,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

const TAG: &str = "Management Console";

/// Routes for producer management.
pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management/producers")
        .add("/", post(create_producer))
        .add("/", get(list_producers))
        .add("/:producer_id/decks", get(list_producer_decks))
        .add("/:producer_id/decks", post(upload_producer_deck))
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// Producer identity response.
#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct ProducerResponse {
    pub id: i32,
    pub name: String,
    pub email: String,
    pub contact_note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Create-producer request body.
#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct CreateProducerRequest {
    pub name: String,
    pub email: String,
    pub contact_note: Option<String>,
}

/// One catalog entry: a producer's deck and its verification status.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ProducerCatalogEntry {
    pub producer_deck_id: i32,
    pub deck_id: i32,
    pub deck_name: String,
    pub status: String,
    pub reject_reason: Option<String>,
    pub verified_at: Option<i64>,
    pub card_count: i32,
}

/// Producer + its deck catalog.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ProducerCatalogResponse {
    #[serde(flatten)]
    pub producer: ProducerResponse,
    pub decks: Vec<ProducerCatalogEntry>,
}

/// Upload → verify → write `deck` + `producer_deck` response.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct UploadProducerDeckResponse {
    pub producer_id: i32,
    pub deck_id: i32,
    pub producer_deck_id: i32,
    /// `verified` | `rejected`
    pub status: String,
    /// Real card count from verification (not a byte-size estimate).
    pub card_count: i32,
    pub card_type_census: CardTypeCensus,
    pub media_integrity: MediaIntegrity,
    pub reject_reason: Option<String>,
    pub warnings: Vec<String>,
}

impl ProducerResponse {
    fn from_model(m: producer_entity::Model) -> Self {
        Self {
            id: m.id,
            name: m.name,
            email: m.email,
            contact_note: m.contact_note,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Create a producer identity (admin).
#[utoipa::path(
    post,
    path = "/management/producers",
    request_body = CreateProducerRequest,
    responses(
        (status = 201, description = "Producer created", body = ProducerResponse),
        (status = 400, description = "Invalid request body"),
        (status = 403, description = "Forbidden — admin role required"),
    ),
    tag = TAG
)]
pub async fn create_producer(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Json(payload): Json<CreateProducerRequest>,
) -> Result<Response> {
    ensure_role(&user, &["admin"])?;

    if payload.name.trim().is_empty() || payload.email.trim().is_empty() {
        return Err(loco_rs::Error::BadRequest(
            "producer name and email are required".into(),
        ));
    }

    let ts = now_ts();
    let producer = producer_entity::ActiveModel {
        id: ActiveValue::NotSet,
        name: Set(payload.name.trim().to_string()),
        email: Set(payload.email.trim().to_string()),
        contact_note: Set(payload.contact_note),
        created_at: Set(ts),
        updated_at: Set(ts),
    }
    .insert(&ctx.db)
    .await
    .map_err(|e| {
        tracing::error!("create_producer insert failed: {e:?}");
        loco_rs::Error::InternalServerError
    })?;

    tracing::info!("Producer created: id={} name={}", producer.id, producer.name);
    format::json(ProducerResponse::from_model(producer))
}

/// List producers with their deck catalogs (admin/teacher).
#[utoipa::path(
    get,
    path = "/management/producers",
    responses(
        (status = 200, description = "Producers with deck catalogs", body = Vec<ProducerCatalogResponse>),
        (status = 403, description = "Forbidden — admin or teacher role required"),
    ),
    tag = TAG
)]
pub async fn list_producers(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    ensure_role(&user, &["admin", "teacher"])?;

    let producers = producer_entity::Entity::find()
        .all(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("list_producers query failed: {e:?}");
            loco_rs::Error::InternalServerError
        })?;

    // Fetch all catalog rows + deck names, group by producer in memory.
    let catalog = producer_deck_entity::Entity::find()
        .all(&ctx.db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?;
    let decks = deck_entity::Entity::find()
        .all(&ctx.db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?;
    let deck_names: std::collections::HashMap<i32, (String, i32)> = decks
        .into_iter()
        .map(|d| (d.id, (d.name, d.card_count)))
        .collect();

    let mut by_producer: std::collections::HashMap<i32, Vec<ProducerCatalogEntry>> =
        std::collections::HashMap::new();
    for row in catalog {
        let (deck_name, card_count) = deck_names
            .get(&row.deck_id)
            .cloned()
            .unwrap_or(("<deleted>".to_string(), 0));
        by_producer.entry(row.producer_id).or_default().push(ProducerCatalogEntry {
            producer_deck_id: row.id,
            deck_id: row.deck_id,
            deck_name,
            status: row.status,
            reject_reason: row.reject_reason,
            verified_at: row.verified_at,
            card_count,
        });
    }

    let out: Vec<ProducerCatalogResponse> = producers
        .into_iter()
        .map(|p| ProducerCatalogResponse {
            producer: ProducerResponse::from_model(p.clone()),
            decks: by_producer.remove(&p.id).unwrap_or_default(),
        })
        .collect();

    format::json(out)
}

/// Catalog status for one producer (admin/teacher).
#[utoipa::path(
    get,
    path = "/management/producers/{producer_id}/decks",
    params(
        ("producer_id" = i32, Path, description = "Producer ID")
    ),
    responses(
        (status = 200, description = "Producer deck catalog", body = Vec<ProducerCatalogEntry>),
        (status = 403, description = "Forbidden — admin or teacher role required"),
        (status = 404, description = "Producer not found"),
    ),
    tag = TAG
)]
pub async fn list_producer_decks(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(producer_id): Path<i32>,
) -> Result<Response> {
    ensure_role(&user, &["admin", "teacher"])?;

    producer_entity::Entity::find_by_id(producer_id)
        .one(&ctx.db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?
        .ok_or(loco_rs::Error::NotFound)?;

    let rows = producer_deck_entity::Entity::find()
        .filter(producer_deck_entity::Column::ProducerId.eq(producer_id))
        .all(&ctx.db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let deck = deck_entity::Entity::find_by_id(row.deck_id)
            .one(&ctx.db)
            .await
            .map_err(|_| loco_rs::Error::InternalServerError)?;
        let (deck_name, card_count) = match deck {
            Some(d) => (d.name, d.card_count),
            None => ("<deleted>".to_string(), 0),
        };
        out.push(ProducerCatalogEntry {
            producer_deck_id: row.id,
            deck_id: row.deck_id,
            deck_name,
            status: row.status,
            reject_reason: row.reject_reason,
            verified_at: row.verified_at,
            card_count,
        });
    }

    format::json(out)
}

/// Upload an `.apkg` for a producer: verify → write `deck` + `producer_deck`
/// (admin; producer uploads proxied by admin in v1).
#[utoipa::path(
    post,
    path = "/management/producers/{producer_id}/decks",
    params(
        ("producer_id" = i32, Path, description = "Producer ID")
    ),
    request_body(content = String, description = "APKG multipart upload", content_type = "multipart/form-data"),
    responses(
        (status = 201, description = "Package processed — producer_deck row written (verified or rejected)", body = UploadProducerDeckResponse),
        (status = 400, description = "No file uploaded or file is empty"),
        (status = 403, description = "Forbidden — admin role required"),
        (status = 404, description = "Producer not found"),
        (status = 504, description = "Verification backend (headless Anki) unavailable"),
    ),
    tag = TAG
)]
pub async fn upload_producer_deck(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(producer_id): Path<i32>,
    mut multipart: Multipart,
) -> Result<Response> {
    ensure_role(&user, &["admin"])?;

    // Producer must exist.
    producer_entity::Entity::find_by_id(producer_id)
        .one(&ctx.db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?
        .ok_or(loco_rs::Error::NotFound)?;

    // Read the multipart `file` field.
    let mut file_data = bytes::Bytes::new();
    let mut file_name = String::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name().unwrap_or("") == "file" {
            if let Some(filename) = field.file_name() {
                file_name = filename.to_string();
            }
            if let Ok(bytes) = field.bytes().await {
                file_data = bytes;
            }
        }
    }

    if file_data.is_empty() {
        return Err(loco_rs::Error::BadRequest(
            "No file uploaded or file is empty".into(),
        ));
    }

    // Persist to a temp file so the verifier (real or stub) can read it.
    let temp_path = std::env::temp_dir().join(format!(
        "ankitov-verify-{}-{}-{}",
        producer_id,
        uuid::Uuid::new_v4(),
        now_ts()
    ));
    std::fs::write(&temp_path, &file_data)
        .map_err(|e| loco_rs::Error::Message(format!("failed to stage upload: {e}")))?;

    // Consume verification via the env-selected Verifier.
    let report = verifier().verify(&temp_path).await;
    let _ = std::fs::remove_file(&temp_path);
    let report = report.map_err(|e| {
        tracing::error!("deck verification backend failed: {e}");
        loco_rs::Error::CustomError(
            axum::http::StatusCode::GATEWAY_TIMEOUT,
            loco_rs::controller::ErrorDetail {
                error: Some("verification_unavailable".into()),
                description: Some(e.to_string()),
            },
        )
    })?;

    // Write the deck row with the REAL card count from verification.
    let deck_name = if file_name.is_empty() {
        report.deck_name.clone()
    } else {
        file_name
            .strip_suffix(".apkg")
            .or_else(|| file_name.strip_suffix(".zip"))
            .unwrap_or(&file_name)
            .to_string()
    };
    let checksum = hex::encode(Sha256::digest(&file_data));
    let ts = now_ts();

    let deck = deck_entity::ActiveModel {
        id: ActiveValue::NotSet,
        track_id: ActiveValue::NotSet,
        name: Set(deck_name),
        description: Set(None),
        card_count: Set(report.real_card_count),
        file_size_bytes: Set(file_data.len() as i64),
        checksum_sha256: Set(checksum),
        uploaded_by: Set(uuid::Uuid::nil().to_string()),
        created_at: Set(ts),
        updated_at: Set(ts),
    }
    .insert(&ctx.db)
    .await
    .map_err(|e| {
        tracing::error!("upload_producer_deck deck insert failed: {e:?}");
        loco_rs::Error::InternalServerError
    })?;

    // Write the producer_deck catalog row: verified | rejected.
    let status = if report.ok {
        producer_deck_entity::STATUS_VERIFIED.to_string()
    } else {
        producer_deck_entity::STATUS_REJECTED.to_string()
    };
    let verified_at = if report.ok { Some(ts) } else { None };

    let producer_deck = producer_deck_entity::ActiveModel {
        id: ActiveValue::NotSet,
        producer_id: Set(producer_id),
        deck_id: Set(deck.id),
        status: Set(status.clone()),
        reject_reason: Set(report.reject_reason.clone()),
        verified_at: Set(verified_at),
        created_at: Set(ts),
        updated_at: Set(ts),
    }
    .insert(&ctx.db)
    .await
    .map_err(|e| {
        tracing::error!("upload_producer_deck producer_deck insert failed: {e:?}");
        loco_rs::Error::InternalServerError
    })?;

    tracing::info!(
        "Producer deck processed: producer_id={} deck_id={} status={}",
        producer_id,
        deck.id,
        status
    );

    format::json(UploadProducerDeckResponse {
        producer_id,
        deck_id: deck.id,
        producer_deck_id: producer_deck.id,
        status,
        card_count: report.real_card_count,
        card_type_census: report.card_type_census,
        media_integrity: report.media_integrity,
        reject_reason: report.reject_reason,
        warnings: report.warnings,
    })
}
