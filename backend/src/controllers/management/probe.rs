//! Debug probe endpoints — isolated from other routes to avoid path conflicts.
//!
//! These are internal tooling endpoints used during development to diagnose
//! database and routing issues. They are NOT part of the public API.

use loco_rs::{app::AppContext, prelude::*};
use sea_orm::entity::ActiveModelTrait;
use sea_orm::ActiveValue;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::models::entities::deck as deck_entity;

/// Routes for probe endpoints.
pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management/probe")
        .add("/deck-insert", post(deck_insert))
        .add("/deck-insert-minimal", post(deck_insert_minimal))
        .add("/health", get(db_health))
        .add("/schema", get(schema_dump))
        .add("/indexes", get(index_dump))
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
#[schema(example = json!({
    "name": "Test Deck",
    "card_count": 42,
    "file_size_bytes": 1024,
    "checksum_sha256": "a".repeat(64),
    "uploaded_by": "00000000-0000-0000-0000-000000000000",
    "created_at": 1751078400,
    "updated_at": 1751078400
}))]
pub(crate) struct DeckProbeRequest {
    name: Option<String>,
    card_count: Option<i32>,
    file_size_bytes: Option<i64>,
    checksum_sha256: Option<String>,
    uploaded_by: Option<String>,
    created_at: Option<i64>,
    updated_at: Option<i64>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Insert a deck row with explicit per-column values to isolate
/// which field causes "datatype mismatch".
#[utoipa::path(
    post,
    path = "/management/probe/deck-insert",
    request_body = DeckProbeRequest,
    responses(
        (status = 201, description = "Insert succeeded"),
        (status = 500, description = "Insert failed: {detail}")
    ),
    tag = "PROBE"
)]
pub async fn deck_insert(
    State(ctx): State<AppContext>,
    Json(payload): Json<DeckProbeRequest>,
) -> Result<Response> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let model = deck_entity::ActiveModel {
        id: ActiveValue::NotSet,
        track_id: ActiveValue::NotSet,
        name: Set(payload.name.unwrap_or_else(|| "probe".to_string())),
        description: Set(None),
        card_count: Set(payload.card_count.unwrap_or(0)),
        file_size_bytes: Set(payload.file_size_bytes.unwrap_or(0)),
        checksum_sha256: Set(payload.checksum_sha256.unwrap_or_else(|| "x".repeat(64))),
        uploaded_by: Set(payload.uploaded_by.unwrap_or_else(|| uuid::Uuid::nil().to_string())),
        created_at: Set(payload.created_at.unwrap_or(now)),
        updated_at: Set(payload.updated_at.unwrap_or(now)),
    };

    model.insert(&ctx.db).await.map_err(|e| {
        tracing::error!("deck_insert failed: {:?}", e);
        loco_rs::Error::InternalServerError
    })?;

    format::json(crate::models::management::ApiResponse {
        success: true,
        message: "inserted".to_string(),
    })
}

/// Insert a deck using ONLY the id and name columns (everything else is DEFAULT).
#[utoipa::path(
    post,
    path = "/management/probe/deck-insert-minimal",
    request_body = DeckProbeRequest,
    responses(
        (status = 201, description = "Minimal insert succeeded"),
        (status = 500, description = "Insert failed")
    ),
    tag = "PROBE"
)]
pub async fn deck_insert_minimal(
    State(ctx): State<AppContext>,
    Json(payload): Json<DeckProbeRequest>,
) -> Result<Response> {
    let name = payload.name.unwrap_or_else(|| "probe".to_string());

    let model = deck_entity::ActiveModel {
        id: ActiveValue::NotSet,
        track_id: ActiveValue::NotSet,
        name: Set(name),
        description: Set(None),
        card_count: Set(0),
        file_size_bytes: Set(0),
        checksum_sha256: Set("x".repeat(64)),
        uploaded_by: Set(uuid::Uuid::nil().to_string()),
        created_at: Set(0),
        updated_at: Set(0),
    };

    model.insert(&ctx.db).await.map_err(|e| {
        tracing::error!("deck_insert_minimal failed: {:?}", e);
        loco_rs::Error::InternalServerError
    })?;

    format::json(crate::models::management::ApiResponse {
        success: true,
        message: "inserted".to_string(),
    })
}

/// Return basic DB connectivity and schema info.
#[utoipa::path(
    get,
    path = "/management/probe/health",
    responses(
        (status = 200, description = "DB is reachable")
    ),
    tag = "PROBE"
)]
pub async fn db_health(State(ctx): State<AppContext>) -> Result<Response> {
    let result: Result<i64, _> = ctx
        .db
        .query_one(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT 1 + 1 AS result",
            [],
        ))
        .await
        .map_err(|e| {
            tracing::error!("db_health query failed: {:?}", e);
            loco_rs::Error::InternalServerError
        })?
        .ok_or(loco_rs::Error::InternalServerError)
        .and_then(|row| {
            row.try_get::<i64>("", "result")
                .map_err(|_| loco_rs::Error::InternalServerError)
        });

    match result {
        Ok(val) => format::json(serde_json::json!({
            "db": "reachable",
            "1+1": val,
        })),
        Err(e) => Err(e),
    }
}

/// Dump the raw CREATE TABLE statements for all known tables.
#[utoipa::path(
    get,
    path = "/management/probe/schema",
    responses(
        (status = 200, description = "Schema dump")
    ),
    tag = "PROBE"
)]
pub async fn schema_dump(State(ctx): State<AppContext>) -> Result<Response> {
    let rows = ctx
        .db
        .query_all(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT name, sql FROM sqlite_master WHERE type='table' ORDER BY name",
            [],
        ))
        .await
        .map_err(|e| {
            tracing::error!("schema_dump query failed: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    let tables: Vec<serde_json::Value> = rows
        .into_iter()
        .filter_map(|row| {
            let name: String = row.try_get("name", "name").ok()?;
            let sql: Option<String> = row.try_get("sql", "sql").ok();
            Some(serde_json::json!({ "name": name, "sql": sql }))
        })
        .collect();

    format::json(serde_json::json!({ "tables": tables }))
}

/// List all indexes on the deck table.
#[utoipa::path(
    get,
    path = "/management/probe/indexes",
    responses(
        (status = 200, description = "Index list")
    ),
    tag = "PROBE"
)]
pub async fn index_dump(State(ctx): State<AppContext>) -> Result<Response> {
    let rows = ctx
        .db
        .query_all(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT name, sql FROM sqlite_master WHERE type='index' AND tbl_name='decks' ORDER BY name",
            [],
        ))
        .await
        .map_err(|e| {
            tracing::error!("index_dump query failed: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    let indexes: Vec<serde_json::Value> = rows
        .into_iter()
        .filter_map(|row| {
            let name: String = row.try_get("name", "name").ok()?;
            let sql: Option<String> = row.try_get("sql", "sql").ok();
            Some(serde_json::json!({ "name": name, "sql": sql }))
        })
        .collect();

    format::json(serde_json::json!({ "indexes": indexes }))
}