//! Anki operations integration — Zone 1 + Zone 2 + Zone 3.
//!
//! ## Zones
//!
//! | Zone | Mechanism | Purpose |
//! |------|-----------|---------|
//! | Zone 1 | `ForensicReader` (rusqlite) | Cold-start health, retention curve, lapse analysis, interval degradation. Read-only. |
//! | Zone 2 | `AnkiConnectClient` (reqwest → :8765) | Live scheduler queries, sync, .apkg import/export, suspend. Scheduler-safe. |
//! | Zone 3 | `DeckHealthReporter` | Composes Zone 1 + Zone 2 into a single dashboard endpoint. |
//!
//! ## Configuration (environment variables)
//!
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `ANKICONNECT_PORT` | `8765` | AnkiConnect HTTP port |
//! | `ANKICONNECT_API_KEY` | — | Optional API key for AnkiConnect auth |
//! | `ANKICOLLECTION_PATH` | — | Path to `.anki2` file for forensic reads |
//!
//! ## Safety
//!
//! - Zone 1 is **read-only only.** Connection opened with `SQLITE_OPEN_READ_ONLY`.
//! - Zone 2 serializes all calls via `tokio::sync::Mutex` (concurrency = 1).
//! - Zone 2 whitelist: no `create_flashcard` or note-editing actions.
//! - No Goose, no MCP server in production.
//!
//! See: [services module docs](crate::services)

use crate::models::management::{
    ApiResponse, BackupResponse, DeckHealthReport,
    PracticeAdherenceResponse, SporadicPracticeResponse,
    GapAnalysisResponse, DeckSuitabilityResponse,
};
use crate::services::anki_connect::AnkiConnectClient;
use crate::services::deck_health::DeckHealthReporter;
use crate::services::forensic_reader::ForensicReader;
use axum::extract::{Path, Query, State, Extension};
use loco_rs::{app::AppContext, prelude::*};
use serde::Deserialize;
use crate::middleware::auth::AuthUser;
use crate::middleware::auth::ensure_role;
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Runtime profile state (supports dashboard profile switcher)
// ---------------------------------------------------------------------------

/// Global runtime override for the active Anki collection path.
///
/// When set via `POST /management/anki/active-profile`, all Zone 1 forensic
/// reads use this path instead of the `ANKICOLLECTION_PATH` environment variable.
/// This enables the dashboard profile switcher to change which user's collection
/// is being analyzed without restarting the backend.
static ACTIVE_COLLECTION: OnceLock<std::sync::Mutex<Option<String>>> = OnceLock::new();

fn active_collection() -> &'static std::sync::Mutex<Option<String>> {
    ACTIVE_COLLECTION.get_or_init(|| std::sync::Mutex::new(None))
}

/// Profile list response for the dashboard switcher.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ProfileListResponse {
    pub profiles: Vec<ProfileEntry>,
    pub active_profile: Option<String>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ProfileEntry {
    pub name: String,
    pub has_collection: bool,
    pub collection_size_kb: u64,
}

/// Request body for switching the active profile.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SwitchProfileRequest {
    pub profile: String,
}

/// Query parameters for forensic deck endpoints.
#[derive(Debug, Deserialize)]
pub struct ForensicDeckQuery {
    #[serde(default = "default_days")]
    pub days: u32,
}

fn default_days() -> u32 { 30 }

/// Query parameters for the full health report endpoint.
#[derive(Debug, Deserialize)]
pub struct FullHealthQuery {
    #[serde(default = "default_days")]
    pub days: u32,
}

/// Query parameters for the backup endpoint.
#[derive(Debug, Deserialize)]
pub struct BackupQuery {
    pub deck: Option<String>,
    pub dir: Option<String>,
}

/// Query params for school metric endpoints (days window).
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DaysQuery {
    pub days: Option<u32>,
}

const TAG: &str = "Management Console";

// ---------------------------------------------------------------------------
// Configuration (environment-driven)
// ---------------------------------------------------------------------------

/// Read AnkiConnect configuration from environment variables.
///
/// These values are injected at container startup via the AnkiTov orchestrator
/// (factory harness). Defaults are sensible for a local dev environment.
fn anki_config() -> (u16, Option<String>) {
    let port: u16 = std::env::var("ANKICONNECT_PORT")
        .unwrap_or_else(|_| "8765".to_string())
        .parse()
        .unwrap_or(8765);
    let api_key = std::env::var("ANKICONNECT_API_KEY").ok();
    (port, api_key)
}

/// Read the Anki collection path — checks runtime override first, then env var.
///
/// The runtime override is set by `POST /management/anki/active-profile`.
/// Falls back to `ANKICOLLECTION_PATH` environment variable if no override.
fn collection_path() -> Option<String> {
    if let Ok(guard) = active_collection().lock() {
        if let Some(path) = guard.as_ref() {
            return Some(path.clone());
        }
    }
    std::env::var("ANKICOLLECTION_PATH").ok()
}

/// Construct a Zone 2 client from environment config.
fn anki_client() -> AnkiConnectClient {
    let (port, api_key) = anki_config();
    AnkiConnectClient::new(port, api_key)
}

/// Construct a Zone 1 reader from environment config.
///
/// Returns `None` if `ANKICOLLECTION_PATH` is not set. Handlers that need
/// forensic data will return partial reports with warnings.
fn forensic_reader() -> Option<ForensicReader> {
    collection_path().and_then(|p| ForensicReader::open(&p).ok())
}

// ---------------------------------------------------------------------------
// Deck Health Reporter (Zone 3)
// ---------------------------------------------------------------------------

/// Build a composed deck health reporter (Zone 3).
///
/// Returns `None` if the collection path is not configured. The endpoint
/// returns a 503 in that case.
fn health_reporter() -> Option<DeckHealthReporter> {
    let forensic = forensic_reader()?;
    let anki = anki_client();
    Some(DeckHealthReporter::new(forensic, anki))
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Routes for Anki operations.
///
/// All routes are prefixed `/management/anki` by `routes()`.
pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management/anki")
        .add("/health", get(health_check))
        .add("/sync", post(sync_collection))
        .add("/decks", get(list_decks))
        .add("/deck-health/:deck_name", get(deck_health))
        .add("/full-health/:deck_name", get(full_health_report))
        .add("/due/:deck_name", get(due_cards))
        .add("/retention-curve/:deck_name", get(retention_curve_forensic))
        .add("/lapse/:deck_name", get(lapse_analysis))
        .add("/interval/:deck_name", get(interval_degradation))
        .add("/collection-health", get(collection_health))
        .add("/backup", post(create_backup))
        .add("/profiles", get(list_profiles))
        .add("/active-profile", get(get_active_profile))
        .add("/active-profile", post(switch_profile))
        // School-optimized metrics
        .add("/adherence/:deck", get(practice_adherence))
        .add("/sporadic/:deck", get(sporadic_index))
        .add("/gaps/:deck", get(gap_analysis))
        .add("/suitability/:deck", get(deck_suitability))
}

// ---------------------------------------------------------------------------
// Zone 2: AnkiConnect Direct (Live Operations)
// ---------------------------------------------------------------------------

/// Check whether AnkiConnect is reachable and Anki is running.
///
/// Ping via `version` action. Returns 200 if version 6 is confirmed, 503 if
/// AnkiConnect is unreachable.
#[utoipa::path(
    get,
    path = "/management/anki/health",
    responses(
        (status = 200, description = "AnkiConnect reachable, version 6 confirmed"),
        (status = 503, description = "AnkiConnect unreachable or Anki not running")
    ),
    tag = TAG
)]
pub async fn health_check(_ctx: State<AppContext>) -> Result<Response> {
    let client = anki_client();

    // Fast path: AnkiConnect is already responding.
    if let Ok(()) = client.health_check().await {
        return format::json(ApiResponse {
            success: true,
            message: "AnkiConnect v6 reachable — Anki is running".to_string(),
        });
    }

    // Auto-launch: try to start Anki headless if not running.
    tracing::info!("AnkiConnect down — attempting auto-launch");
    let (_port, _) = anki_config();
    let port = _port;
    let launch = crate::services::anki_launcher::ensure_anki_running(port).await;

    match launch {
        crate::services::anki_launcher::LaunchResult::AlreadyRunning => {
            // Race condition: Anki came up between our check and launch.
            format::json(ApiResponse {
                success: true,
                message: "AnkiConnect v6 reachable — Anki is running".to_string(),
            })
        }
        crate::services::anki_launcher::LaunchResult::Launched { elapsed } => {
            format::json(ApiResponse {
                success: true,
                message: format!(
                    "Anki launched headless — AnkiConnect v6 ready after {elapsed:?}"
                ),
            })
        }
        crate::services::anki_launcher::LaunchResult::Timeout { elapsed } => {
            tracing::warn!("AnkiConnect did not respond within {elapsed:?}");
            format::json(ApiResponse {
                success: false,
                message: format!(
                    "Anki launched but AnkiConnect did not respond within {elapsed:?}"
                ),
            })
        }
        crate::services::anki_launcher::LaunchResult::NotConfigured { reason } => {
            tracing::warn!("Cannot auto-launch Anki: {reason}");
            format::json(ApiResponse {
                success: false,
                message: format!("AnkiConnect unavailable — auto-launch failed: {reason}"),
            })
        }
    }
}

/// Trigger a full Anki Web sync.
#[utoipa::path(
    post,
    path = "/management/anki/sync",
    responses(
        (status = 200, description = "Sync triggered", body = ApiResponse),
        (status = 502, description = "AnkiConnect unavailable")
    ),
    tag = TAG
)]
pub async fn sync_collection(
    Extension(user): Extension<AuthUser>,
    _ctx: State<AppContext>,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    let client = anki_client();
    match client.sync().await {
        Ok(_) => format::json(ApiResponse {
            success: true,
            message: "Sync completed".to_string(),
        }),
        Err(e) => {
            tracing::error!("Sync failed: {e}");
            // Detect the common "auth not configured" error and provide a
            // user-friendly message instead of the raw RPC error string.
            let msg = e.to_string();
            let friendly = if msg.contains("auth not configured") {
                "Sync not configured — Anki needs an AnkiWeb account or self-hosted sync server credentials. Set SYNC_USER1 env var before launching Anki.".to_string()
            } else if msg.contains("cannot connect") || msg.contains("Unreachable") {
                "Anki is not running. Click Refresh to auto-launch, then try sync again.".to_string()
            } else {
                format!("Sync failed: {e}")
            };
            format::json(ApiResponse {
                success: false,
                message: friendly,
            })
        }
    }
}

/// List all deck names in the Anki collection.
#[utoipa::path(
    get,
    path = "/management/anki/decks",
    responses(
        (status = 200, description = "List of deck names"),
        (status = 502, description = "AnkiConnect unavailable")
    ),
    tag = TAG
)]
pub async fn list_decks(_ctx: State<AppContext>) -> Result<Response> {
    let client = anki_client();
    match client.deck_names().await {
        Ok(decks) => format::json(serde_json::json!({ "decks": decks })),
        Err(e) => {
            tracing::error!("Failed to list decks: {e}");
            format::json(ApiResponse {
                success: false,
                message: format!("Failed to list decks: {e}"),
            })
        }
    }
}

/// Return live due card count and deck statistics for a named deck.
#[utoipa::path(
    get,
    path = "/management/anki/deck-health/{deck_name}",
    params(
        ("deck_name" = String, Path, description = "Name of the Anki deck")
    ),
    responses(
        (status = 200, description = "Live deck health", body = DeckHealthReport),
        (status = 502, description = "AnkiConnect unavailable")
    ),
    tag = TAG
)]
pub async fn deck_health(
    _ctx: State<AppContext>,
    Path(deck_name): Path<String>
) -> Result<Response> {
    let client = anki_client();

    let (stats, due) = match (
        client.get_deck_stats(vec![deck_name.clone()]).await,
        client.get_due_cards(&deck_name).await
    ) {
        (Ok(s), Ok(d)) => (s, d),
        (Err(e), _) | (_, Err(e)) => {
            tracing::error!("Deck health failed for '{deck_name}': {e}");
            return format::json(ApiResponse {
                success: false,
                message: format!("AnkiConnect unavailable: {e}"),
            });
        }
    };

    let due_count = due.len() as u32;

    // Parse AnkiConnect getDeckStats response: keyed by deck ID, each value has
    // new_count, learn_count, review_count, total_in_deck, name, deck_id.
    let (new_count, learn_count, review_count, total_count) = stats
        .as_object()
        .and_then(|o| {
            // Iterate values to find the one matching our deck name
            o.values().find(|v| {
                v.get("name").and_then(|n| n.as_str()) == Some(&deck_name)
            })
        })
        .map(|ds| {
            (
                ds.get("new_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                ds.get("learn_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                ds.get("review_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                ds.get("total_in_deck").and_then(|v| v.as_u64()).unwrap_or(0) as u32
            )
        })
        .unwrap_or((0, 0, 0, 0));

    // Compute a simple health score: 100 - (due_ratio × 100), capped
    let health_score = if total_count > 0 {
        let ratio = due_count as f64 / total_count as f64;
        (100.0 * (1.0 - ratio * 0.5)) as u8
    } else {
        100
    };

    let report = DeckHealthReport {
        deck_id: deck_name,
        health_score,
        new_count,
        learn_count,
        review_count,
        total_count,
        due_count,
        issues: vec![],
        suggestions: vec![],
    };
    format::json(report)
}

/// Return the list of card IDs due in a named deck.
#[utoipa::path(
    get,
    path = "/management/anki/due/{deck_name}",
    params(
        ("deck_name" = String, Path, description = "Name of the Anki deck")
    ),
    responses(
        (status = 200, description = "List of due card IDs"),
        (status = 502, description = "AnkiConnect unavailable")
    ),
    tag = TAG
)]
pub async fn due_cards(
    _ctx: State<AppContext>,
    Path(deck_name): Path<String>
) -> Result<Response> {
    let client = anki_client();
    match client.get_due_cards(&deck_name).await {
        Ok(cards) => format::json(serde_json::json!({ "due": cards })),
        Err(e) => {
            tracing::error!("Due cards failed for '{deck_name}': {e}");
            format::json(ApiResponse {
                success: false,
                message: format!("AnkiConnect unavailable: {e}"),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Zone 1: Forensic Reader (Read-Only SQLite)
// ---------------------------------------------------------------------------

/// Run a cold-start health check on the `.anki2` file.
///
/// Does not require Anki to be running. Inspects schema version, table
/// counts, orphaned records, and recent review activity.
#[utoipa::path(
    get,
    path = "/management/anki/collection-health",
    responses(
        (status = 200, description = "Collection health report"),
        (status = 503, description = "Collection path not configured or file unreadable")
    ),
    tag = TAG
)]
pub async fn collection_health(_ctx: State<AppContext>) -> Result<Response> {
    let Some(reader) = forensic_reader() else {
        return format::json(ApiResponse {
            success: false,
            message: "ANKICOLLECTION_PATH not configured — forensic reads unavailable".to_string(),
        });
    };

    match reader.cold_health_check() {
        Ok(health) => format::json(health),
        Err(e) => {
            tracing::error!("Collection health check failed: {e}");
            format::json(ApiResponse {
                success: false,
                message: format!("Collection health check failed: {e}"),
            })
        }
    }
}

/// Compute a daily retention curve for a deck (last N days) via SQLite.
///
/// AnkiConnect's `reviewStats` returns a single aggregated value.
/// This returns the **full time series** needed for dashboards and anomaly detection.
#[utoipa::path(
    get,
    path = "/management/anki/retention-curve/{deck_name}",
    params(
        ("deck_name" = String, Path, description = "Name of the Anki deck"),
        ("days" = Option<u32>, Query, description = "Analysis window in days (default: 30)")
    ),
    responses(
        (status = 200, description = "Daily retention time series"),
        (status = 404, description = "Deck not found"),
        (status = 503, description = "Collection path not configured")
    ),
    tag = TAG
)]
pub async fn retention_curve_forensic(
    _ctx: State<AppContext>,
    Path(deck_name): Path<String>,
    Query(params): Query<ForensicDeckQuery>
) -> Result<Response> {
    let Some(reader) = forensic_reader() else {
        return format::json(ApiResponse {
            success: false,
            message: "ANKICOLLECTION_PATH not configured".to_string(),
        });
    };

    let days = params.days;
    match reader.retention_curve(&deck_name, days) {
        Ok(curve) => format::json(serde_json::json!({
            "deck": deck_name,
            "window_days": days,
            "curve": curve
        })),
        Err(e) => {
            tracing::warn!("Retention curve failed for '{deck_name}': {e}");
            format::json(ApiResponse {
                success: false,
                message: e.to_string(),
            })
        }
    }
}

/// Analyze lapse frequency for a deck via SQLite.
#[utoipa::path(
    get,
    path = "/management/anki/lapse/{deck_name}",
    params(
        ("deck_name" = String, Path, description = "Name of the Anki deck"),
        ("days" = Option<u32>, Query, description = "Analysis window in days (default: 30)")
    ),
    responses(
        (status = 200, description = "Lapse frequency analysis"),
        (status = 503, description = "Collection path not configured")
    ),
    tag = TAG
)]
pub async fn lapse_analysis(
    _ctx: State<AppContext>,
    Path(deck_name): Path<String>,
    Query(params): Query<ForensicDeckQuery>
) -> Result<Response> {
    let Some(reader) = forensic_reader() else {
        return format::json(ApiResponse {
            success: false,
            message: "ANKICOLLECTION_PATH not configured".to_string(),
        });
    };

    let days = params.days;
    match reader.lapse_analysis(&deck_name, days) {
        Ok(stats) => format::json(serde_json::json!({
            "deck": deck_name,
            "window_days": days,
            "lapse": stats
        })),
        Err(e) => {
            tracing::warn!("Lapse analysis failed for '{deck_name}': {e}");
            format::json(ApiResponse {
                success: false,
                message: e.to_string(),
            })
        }
    }
}

/// Map interval distribution across a deck via SQLite.
#[utoipa::path(
    get,
    path = "/management/anki/interval/{deck_name}",
    params(
        ("deck_name" = String, Path, description = "Name of the Anki deck")
    ),
    responses(
        (status = 200, description = "Interval degradation map"),
        (status = 503, description = "Collection path not configured")
    ),
    tag = TAG
)]
pub async fn interval_degradation(
    _ctx: State<AppContext>,
    Path(deck_name): Path<String>
) -> Result<Response> {
    let Some(reader) = forensic_reader() else {
        return format::json(ApiResponse {
            success: false,
            message: "ANKICOLLECTION_PATH not configured".to_string(),
        });
    };

    match reader.interval_degradation(&deck_name) {
        Ok(dist) => format::json(serde_json::json!({
            "deck": deck_name,
            "interval": dist
        })),
        Err(e) => {
            tracing::warn!("Interval degradation failed for '{deck_name}': {e}");
            format::json(ApiResponse {
                success: false,
                message: e.to_string(),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Zone 3: Composed Analytics (Dashboard Endpoint)
// ---------------------------------------------------------------------------

/// Full composed health report — fuses Zone 1 + Zone 2.
///
/// This is the primary endpoint for the Management Console dashboard.
/// Returns a single pre-computed JSON response combining live scheduler
/// metrics (Zone 2) with historical forensic analysis (Zone 1).
#[utoipa::path(
    get,
    path = "/management/anki/full-health/{deck_name}",
    params(
        ("deck_name" = String, Path, description = "Name of the Anki deck"),
        ("days" = Option<u32>, Query, description = "Forensic window in days (default: 30)")
    ),
    responses(
        (status = 200, description = "Full composed health report"),
        (status = 504, description = "Zone 2 (AnkiConnect) unavailable — live data required")
    ),
    tag = TAG
)]
#[axum::debug_handler]
pub async fn full_health_report(
    _ctx: State<AppContext>,
    Path(deck_name): Path<String>,
    Query(params): Query<FullHealthQuery>
) -> Result<Response> {
    let Some(reporter) = health_reporter() else {
        return format::json(ApiResponse {
            success: false,
            message: "ANKICOLLECTION_PATH not configured — forensic reads unavailable".to_string(),
        });
    };

    match reporter.full_health_report(&deck_name, params.days).await {
        Ok(report) => format::json(report),
        Err(e) => {
            tracing::error!("Full health report failed for '{deck_name}': {e}");
            format::json(ApiResponse {
                success: false,
                message: format!("AnkiConnect unavailable (live data required): {e}"),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Write-safe Operations
// ---------------------------------------------------------------------------

/// Create a timestamped `.apkg` backup of a deck via AnkiConnect `exportPackage`.
///
/// Query parameter `deck` (default: "Default") specifies which deck to export.
/// Query parameter `dir` (default: "/tmp") specifies the output directory.
#[utoipa::path(
    post,
    path = "/management/anki/backup",
    params(
        ("deck" = Option<String>, Query, description = "Deck name to export (default: Default)"),
        ("dir" = Option<String>, Query, description = "Output directory (default: /tmp)")
    ),
    responses(
        (status = 201, description = "Backup created", body = BackupResponse),
        (status = 502, description = "AnkiConnect unavailable")
    ),
    tag = TAG
)]
pub async fn create_backup(
    Extension(user): Extension<AuthUser>,
    _ctx: State<AppContext>,
    Query(params): Query<BackupQuery>,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    let client = anki_client();
    let deck = params.deck.unwrap_or_else(|| "Default".to_string());
    let dir = params.dir.unwrap_or_else(|| "/tmp".to_string());
    match client.create_backup(&deck, &dir).await {
        Ok(file_path) => {
            let now = chrono::Utc::now().timestamp();
            let resp = BackupResponse {
                backup_id: file_path.clone(),
                file_path,
                created_at: now,
                size_bytes: 0,
            };
            format::json(resp)
        }
        Err(e) => {
            tracing::error!("Backup failed: {e}");
            format::json(ApiResponse {
                success: false,
                message: format!("Backup failed: {e}"),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Profile Management (Dashboard Switcher)
// ---------------------------------------------------------------------------

/// List all Anki profiles found in the AnkiPlayground base directory.
///
/// Scans the directory specified by `ANKIPLAYGROUND_PATH` (or falls back to
/// the parent of `ANKICOLLECTION_PATH`) for subdirectories containing a
/// `collection.anki2` file. Returns profile names, collection presence, and
/// file sizes for the dashboard profile switcher.
#[utoipa::path(
    get,
    path = "/management/anki/profiles",
    responses(
        (status = 200, description = "List of profiles", body = ProfileListResponse)
    ),
    tag = TAG
)]
pub async fn list_profiles(_ctx: State<AppContext>) -> Result<Response> {
    let playground_dir = resolve_playground_dir();
    let mut profiles: Vec<ProfileEntry> = Vec::new();

    if let Some(dir) = &playground_dir {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    // Skip non-profile directories
                    if name.starts_with('.') || name == "addons21" || name == "logs" {
                        continue;
                    }
                    let col_path = entry.path().join("collection.anki2");
                    let has_collection = col_path.exists();
                    let collection_size_kb = if has_collection {
                        std::fs::metadata(&col_path)
                            .map(|m| m.len() / 1024)
                            .unwrap_or(0)
                    } else {
                        0
                    };
                    profiles.push(ProfileEntry {
                        name,
                        has_collection,
                        collection_size_kb,
                    });
                }
            }
        }
    }

    profiles.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let active = collection_path().and_then(|p| {
        std::path::Path::new(&p)
            .parent()
            .and_then(|parent| parent.file_name())
            .map(|f| f.to_string_lossy().to_string())
    });

    format::json(ProfileListResponse {
        profiles,
        active_profile: active,
    })
}

/// Get the currently active profile name (for dashboard display).
#[utoipa::path(
    get,
    path = "/management/anki/active-profile",
    responses(
        (status = 200, description = "Active profile info")
    ),
    tag = TAG
)]
pub async fn get_active_profile(_ctx: State<AppContext>) -> Result<Response> {
    let active = collection_path().and_then(|p| {
        std::path::Path::new(&p)
            .parent()
            .and_then(|parent| parent.file_name())
            .map(|f| f.to_string_lossy().to_string())
    });
    let full_path = collection_path();

    format::json(serde_json::json!({
        "active_profile": active,
        "collection_path": full_path,
    }))
}

/// Switch the active Anki collection profile at runtime.
///
/// Sets the Zone 1 forensic reader to use a different user's `collection.anki2`
/// file. Does NOT affect Zone 2 (AnkiConnect) — that always reflects whatever
/// profile Anki currently has open.
#[utoipa::path(
    post,
    path = "/management/anki/active-profile",
    request_body = SwitchProfileRequest,
    responses(
        (status = 200, description = "Profile switched successfully"),
        (status = 404, description = "Profile or collection not found")
    ),
    tag = TAG
)]
pub async fn switch_profile(
    Extension(user): Extension<AuthUser>,
    _ctx: State<AppContext>,
    body: axum::Json<SwitchProfileRequest>,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;
    let playground_dir = resolve_playground_dir();

    let Some(dir) = &playground_dir else {
        return format::json(ApiResponse {
            success: false,
            message: "ANKIPLAYGROUND_PATH not configured — cannot resolve profiles".to_string(),
        });
    };

    let col_path = std::path::Path::new(dir)
        .join(&body.profile)
        .join("collection.anki2");

    if !col_path.exists() {
        return format::json(ApiResponse {
            success: false,
            message: format!("Profile '{}' not found or has no collection.anki2", body.profile),
        });
    }

    let path_str = col_path.display().to_string();
    if let Ok(mut guard) = active_collection().lock() {
        *guard = Some(path_str.clone());
    }

    tracing::info!("Switched active forensic profile to '{}' ({})", body.profile, path_str);

    format::json(serde_json::json!({
        "success": true,
        "active_profile": body.profile,
        "collection_path": path_str,
        "note": "Zone 2 (AnkiConnect) still reflects the profile Anki has open. Only Zone 1 forensic reads are affected."
    }))
}

// ---------------------------------------------------------------------------
// School-Optimized Metrics (2026-06-29)
// ---------------------------------------------------------------------------

/// Practice Adherence Score — "Is this student showing up?" (0–100)
#[utoipa::path(
    get,
    path = "/management/anki/adherence/{deck}",
    params(
        ("deck" = String, Path, description = "Deck name"),
        ("days" = Option<u32>, Query, description = "Analysis window in days (default: 30)")
    ),
    responses(
        (status = 200, description = "Practice Adherence Score", body = PracticeAdherenceResponse),
        (status = 404, description = "Deck not found"),
        (status = 503, description = "Collection path not configured")
    ),
    tag = TAG
)]
pub async fn practice_adherence(
    _ctx: State<AppContext>,
    Path(deck): Path<String>,
    Query(params): Query<DaysQuery>
) -> Result<Response> {
    let reader = match forensic_reader() {
        Some(r) => r,
        None => return format::json(ApiResponse {
            success: false,
            message: "ANKICOLLECTION_PATH not set — cannot perform forensic analysis".to_string(),
        }),
    };
    let days = params.days.unwrap_or(30);
    match reader.practice_adherence(&deck, days) {
        Ok(data) => format::json(serde_json::json!({
            "score": data.score,
            "active_days": data.active_days,
            "total_days": data.total_days,
            "missed_days": data.missed_days,
            "window_days": data.window_days,
        })),
        Err(e) => format::json(ApiResponse { success: false, message: e.to_string() }),
    }
}

/// Sporadic Practice Index — "Is this student cramming?" (0.0–1.0)
#[utoipa::path(
    get,
    path = "/management/anki/sporadic/{deck}",
    params(
        ("deck" = String, Path, description = "Deck name"),
        ("days" = Option<u32>, Query, description = "Analysis window in days (default: 30)")
    ),
    responses(
        (status = 200, description = "Sporadic Practice Index", body = SporadicPracticeResponse),
        (status = 404, description = "Deck not found"),
        (status = 503, description = "Collection path not configured")
    ),
    tag = TAG
)]
pub async fn sporadic_index(
    _ctx: State<AppContext>,
    Path(deck): Path<String>,
    Query(params): Query<DaysQuery>
) -> Result<Response> {
    let reader = match forensic_reader() {
        Some(r) => r,
        None => return format::json(ApiResponse {
            success: false,
            message: "ANKICOLLECTION_PATH not set".to_string(),
        }),
    };
    let days = params.days.unwrap_or(30);
    match reader.sporadic_index(&deck, days) {
        Ok(data) => format::json(serde_json::json!({
            "index": data.index,
            "active_days": data.active_days,
            "total_days": data.total_days,
            "coverage": data.coverage,
            "avg_reviews_per_active_day": data.avg_reviews_per_active_day,
        })),
        Err(e) => format::json(ApiResponse { success: false, message: e.to_string() }),
    }
}

/// Gap Analysis — "How long between practice sessions?"
#[utoipa::path(
    get,
    path = "/management/anki/gaps/{deck}",
    params(
        ("deck" = String, Path, description = "Deck name"),
        ("days" = Option<u32>, Query, description = "Analysis window in days (default: 30)")
    ),
    responses(
        (status = 200, description = "Gap Analysis report", body = GapAnalysisResponse),
        (status = 404, description = "Deck not found"),
        (status = 503, description = "Collection path not configured")
    ),
    tag = TAG
)]
pub async fn gap_analysis(
    _ctx: State<AppContext>,
    Path(deck): Path<String>,
    Query(params): Query<DaysQuery>
) -> Result<Response> {
    let reader = match forensic_reader() {
        Some(r) => r,
        None => return format::json(ApiResponse {
            success: false,
            message: "ANKICOLLECTION_PATH not set".to_string(),
        }),
    };
    let days = params.days.unwrap_or(30);
    match reader.gap_analysis(&deck, days) {
        Ok(data) => format::json(serde_json::json!({
            "max_gap_days": data.max_gap_days,
            "avg_gap_days": data.avg_gap_days,
            "gaps_over_3_days": data.gaps_over_3_days,
            "gaps_over_7_days": data.gaps_over_7_days,
            "gap_list": data.gap_list,
        })),
        Err(e) => format::json(ApiResponse { success: false, message: e.to_string() }),
    }
}

/// Deck Suitability Index — "Is this deck too hard or too easy?"
#[utoipa::path(
    get,
    path = "/management/anki/suitability/{deck}",
    params(
        ("deck" = String, Path, description = "Deck name"),
        ("days" = Option<u32>, Query, description = "Analysis window in days (default: 30)")
    ),
    responses(
        (status = 200, description = "Deck Suitability Index", body = DeckSuitabilityResponse),
        (status = 404, description = "Deck not found"),
        (status = 503, description = "Collection path not configured")
    ),
    tag = TAG
)]
pub async fn deck_suitability(
    _ctx: State<AppContext>,
    Path(deck): Path<String>,
    Query(params): Query<DaysQuery>
) -> Result<Response> {
    let reader = match forensic_reader() {
        Some(r) => r,
        None => return format::json(ApiResponse {
            success: false,
            message: "ANKICOLLECTION_PATH not set".to_string(),
        }),
    };
    let days = params.days.unwrap_or(30);
    match reader.deck_suitability(&deck, days) {
        Ok(data) => format::json(serde_json::json!({
            "verdict": data.verdict,
            "lapse_rate": data.lapse_rate,
            "ease_factor": data.ease_factor,
            "mature_rate": data.mature_rate,
            "avg_time_per_card_secs": data.avg_time_per_card_secs,
            "indicators": data.indicators,
            "struggle_points": data.struggle_points,
        })),
        Err(e) => format::json(ApiResponse { success: false, message: e.to_string() }),
    }
}

/// Resolve the AnkiPlayground base directory from env vars or infer from collection path.
fn resolve_playground_dir() -> Option<String> {
    // Explicit env var
    if let Ok(dir) = std::env::var("ANKIPLAYGROUND_PATH") {
        return Some(dir);
    }
    // Infer from ANKICOLLECTION_PATH (parent directory of the .anki2 file)
    if let Ok(col_path) = std::env::var("ANKICOLLECTION_PATH") {
        let path = std::path::Path::new(&col_path);
        if let Some(parent) = path.parent() {
            return Some(parent.display().to_string());
        }
    }
    // Check runtime override
    if let Ok(guard) = active_collection().lock() {
        if let Some(ref col_path) = *guard {
            let path = std::path::Path::new(col_path);
            if let Some(parent) = path.parent() {
                return Some(parent.display().to_string());
            }
        }
    }
    None
}