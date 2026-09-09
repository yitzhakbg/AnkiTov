//! Statistics Controller — Management Console Statistics tab.
//!
//! Combines Zone 1 (ForensicReader), Zone 2 (AnkiConnect), and Zone 3
//! (DeckHealthReporter) to produce the dashboard Statistics view:
//! collection overview, per-deck breakdown, and retention exceptions.

use crate::models::management::{ClassOverviewStats, DailyRetention, RetentionException};
use crate::services::anki_connect::AnkiConnectClient;
use crate::services::forensic_reader::ForensicReader;
use axum::extract::State;
use loco_rs::{app::AppContext, prelude::*};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};


const TAG: &str = "Management Console";

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management")
        .add("/stats", get(stats))
        .add("/stats/exceptions", get(exceptions))
}

// ── Config helpers (mirror anki_ops) ──────────────────────────────

fn anki_client() -> AnkiConnectClient {
    let port: u16 = std::env::var("ANKICONNECT_PORT")
        .unwrap_or_else(|_| "8765".into())
        .parse()
        .unwrap_or(8765);
    let api_key = std::env::var("ANKICONNECT_API_KEY").ok();
    AnkiConnectClient::new(port, api_key)
}

fn collection_path() -> Option<String> {
    std::env::var("ANKICOLLECTION_PATH").ok()
}

/// Full statistics overview for the dashboard Statistics tab.
///
/// Combines live AnkiConnect data with forensic read-only SQLite analysis.
#[utoipa::path(
    get,
    path = "/management/stats",
    responses(
        (status = 200, description = "Statistics overview", body = ClassOverviewStats)
    ),
    tag = TAG
)]
pub async fn stats(_ctx: State<AppContext>) -> Result<Response> {
    let client = anki_client();

    // ── Zone 2: live per-deck counts ──────────────────────────────
    let decks = client.deck_names().await.unwrap_or_default();
    let mut total_cards: u32 = 0;
    let mut total_due: u32 = 0;
    let mut deck_breakdown: Vec<serde_json::Value> = Vec::new();

    for deck_name in &decks {
        if let Ok(stats) = client.get_deck_stats(vec![deck_name.clone()]).await {
            let (total, due) = stats
                .as_object()
                .and_then(|o| o.values().find(|v| {
                    v.get("name").and_then(|n| n.as_str()) == Some(deck_name.as_str())
                }))
                .map(|ds| {
                    let t = ds.get("total_in_deck").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    // Due = new + learn + review
                    let new = ds.get("new_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let learn = ds.get("learn_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let review = ds.get("review_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    (t, new + learn + review)
                })
                .unwrap_or((0, 0));

            total_cards += total;
            total_due += due;

            let health_score = if total > 0 {
                (100.0 * (1.0 - due as f64 / total as f64 * 0.5)) as u8
            } else {
                100
            };

            deck_breakdown.push(serde_json::json!({
                "name": deck_name,
                "total": total,
                "due": due,
                "health_score": health_score,
                "status": if due > 10 { "warn" } else if due > 20 { "danger" } else { "ok" }
            }));
        }
    }

    // ── Zone 1: forensic collection metrics ───────────────────────
    let (review_count, retention_trend) = if let Some(path) = collection_path() {
        if let Ok(reader) = ForensicReader::open(&path) {
            let review_count = reader
                .cold_health_check()
                .map(|h| h.review_count)
                .unwrap_or(0);

            // Get retention curve across all decks (use first deck as proxy).
            // If the live AnkiConnect deck name isn't present in the forensic
            // collection (e.g. a staged demo), fall back to the collection's
            // own first deck so the trend still populates.
            let mut trend: Vec<DailyRetention> = Vec::new();
            if let Some(first) = decks.first() {
                match reader.retention_curve(first, 14) {
                    Ok(pts) => trend = pts
                        .into_iter()
                        .map(|p| DailyRetention {
                            date: p.date,
                            retention_rate: p.retention_rate,
                            review_count: p.review_count,
                        })
                        .collect(),
                    Err(_) => {
                        if let Some(fb) = reader.first_deck_name() {
                            if fb.as_str() != first.as_str() {
                                trend = reader
                                    .retention_curve(&fb, 14)
                                    .unwrap_or_default()
                                    .into_iter()
                                    .map(|p| DailyRetention {
                                        date: p.date,
                                        retention_rate: p.retention_rate,
                                        review_count: p.review_count,
                                    })
                                    .collect();
                            }
                        }
                    }
                }
            } else if let Some(fb) = reader.first_deck_name() {
                trend = reader
                    .retention_curve(&fb, 14)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|p| DailyRetention {
                        date: p.date,
                        retention_rate: p.retention_rate,
                        review_count: p.review_count,
                    })
                    .collect();
            }

            (review_count, trend)
        } else {
            (0, vec![])
        }
    } else {
        (0, vec![])
    };

    // ── Retention exceptions (from relational DB) ────────────────
    let exceptions_query = crate::models::entities::retention_exception::Entity::find()
        .filter(crate::models::entities::retention_exception::Column::ResolvedAt.is_null())
        .order_by_desc(crate::models::entities::retention_exception::Column::DetectedAt)
        .all(&_ctx.db)
        .await
        .unwrap_or_default();

    let exceptions: Vec<RetentionException> = exceptions_query
        .into_iter()
        .map(|e| RetentionException {
            user_id: e.user_id,
            deck_id: e.deck_id,
            exception_type: e.exception_type,
            retention_rate: e.retention_rate,
            threshold: e.threshold,
            detected_at: e.detected_at,
        })
        .collect();

    // Compute average retention from the trend
    let avg_retention = if retention_trend.is_empty() {
        0.0
    } else {
        retention_trend.iter().map(|p| p.retention_rate).sum::<f64>()
            / retention_trend.len() as f64
    };

    // Active users = student profiles with a collection.anki2
    let active_users = if let Ok(dir) = std::env::var("ANKIPLAYGROUND_PATH") {
        std::fs::read_dir(&dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| {
                        e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                            && e.path().join("collection.anki2").exists()
                    })
                    .count() as u32
            })
            .unwrap_or(0)
    } else {
        0
    };
    format::json(serde_json::json!({
        "overview": {
            "total_decks": decks.len() as u32,
            "total_cards": total_cards,
            "total_reviews": review_count,
            "average_retention": (avg_retention * 1000.0).round() / 1000.0,
            "active_users": active_users,
            "retention_trend": retention_trend,
        },
        "decks": deck_breakdown,
        "exceptions": exceptions,
    }))
}

/// Recent retention exceptions (from relational telemetry).
#[utoipa::path(
    get,
    path = "/management/stats/exceptions",
    responses(
        (status = 200, description = "List of retention exceptions")
    ),
    tag = TAG
)]
pub async fn exceptions(State(ctx): State<AppContext>) -> Result<Response> {
    // Query retention_exceptions via SeaORM
    let rows = crate::models::entities::retention_exception::Entity::find()
        .filter(
            crate::models::entities::retention_exception::Column::ResolvedAt
                .is_null(),
        )
        .order_by_desc(
            crate::models::entities::retention_exception::Column::DetectedAt,
        )
        .limit(50)
        .all(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("Failed to query retention exceptions: {e}");
            loco_rs::Error::InternalServerError
        })?;

    let exceptions: Vec<RetentionException> = rows
        .into_iter()
        .map(|e| RetentionException {
            user_id: e.user_id,
            deck_id: e.deck_id,
            exception_type: e.exception_type,
            retention_rate: e.retention_rate,
            threshold: e.threshold,
            detected_at: e.detected_at,
        })
        .collect();

    format::json(serde_json::json!({ "exceptions": exceptions }))
}

// ── Legacy stubs (keep old routes alive) ──────────────────────────

pub async fn class_overview(_ctx: State<AppContext>) -> Result<Response> {
    format::json(serde_json::json!({ "message": "Use GET /management/stats instead" }))
}

pub async fn subsection_stats(_ctx: State<AppContext>) -> Result<Response> {
    format::json(serde_json::json!({ "message": "Use GET /management/stats instead" }))
}

pub async fn list_exceptions(State(ctx): State<AppContext>) -> Result<Response> {
    exceptions(State(ctx)).await
}

pub async fn recalculate_stats(_ctx: State<AppContext>) -> Result<Response> {
    format::json(serde_json::json!({ "recalculated": true, "note": "Stats are live from AnkiConnect — no recalculation needed" }))
}
