//! Request and response types for the Management Console API.
//!
//! All structs are annotated with `#[derive(Serialize, Deserialize)]` for JSON
//! conversion and `#[derive(utoipa::ToSchema)]` for OpenAPI documentation.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// =============================================================================
// DECK MANAGEMENT
// =============================================================================

/// Request to distribute a deck to a target (user, group, or class).
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct DistributeDeckRequest {
    /// Target type: "user", "group", or "class"
    pub target_type: String,
    /// Target identifier
    pub target_id: String,
}

/// Metadata about an uploaded deck.
#[derive(Debug, Serialize, ToSchema)]
pub struct DeckInfo {
    pub id: String,
    pub name: String,
    pub card_count: u32,
    pub created_at: i64,
    pub updated_at: i64,
    pub distribution_count: u32,
    /// Track Library track this deck is bound to (P2 minimal); None = unassigned.
    pub track_id: Option<String>,
}

/// Request body for binding a deck to a Track Library track (P2 minimal).
#[derive(Debug, Deserialize, ToSchema)]
pub struct BindDeckTrackRequest {
    pub track_id: String,
}

/// Response for deck distribution operation.
#[derive(Debug, Serialize, ToSchema)]
pub struct DeckDistributionResponse {
    pub deck_id: String,
    pub target_type: String,
    pub target_id: String,
    pub status: String,
    pub distributed_at: i64,
}

// =============================================================================
// STATISTICS
// =============================================================================

/// Whole-class overview statistics.
#[derive(Debug, Serialize, ToSchema)]
pub struct ClassOverviewStats {
    pub total_decks: u32,
    pub total_cards: u32,
    pub total_reviews: u64,
    pub average_retention: f64,
    pub active_users: u32,
    pub retention_trend: Vec<DailyRetention>,
}

/// Daily retention data point.
#[derive(Debug, Serialize, ToSchema)]
pub struct DailyRetention {
    pub date: String,
    pub retention_rate: f64,
    pub review_count: u32,
}

/// Retention anomaly / exception.
#[derive(Debug, Serialize, ToSchema)]
pub struct RetentionException {
    pub user_id: String,
    pub deck_id: String,
    pub exception_type: String,
    pub retention_rate: f64,
    pub threshold: f64,
    pub detected_at: i64,
}

/// Subsection statistics (group or class subsection).
#[derive(Debug, Serialize, ToSchema)]
pub struct SubsectionStats {
    pub subsection_id: String,
    pub subsection_name: String,
    pub total_decks: u32,
    pub total_cards: u32,
    pub average_retention: f64,
    pub exceptions: Vec<RetentionException>,
}

// =============================================================================
// ADDON MANAGEMENT
// =============================================================================

/// Metadata about an installed Anki addon.
#[derive(Debug, Serialize, ToSchema)]
pub struct AddonInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub installed_at: i64,
    pub distribution_count: u32,
}

/// Request to distribute an addon.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct DistributeAddonRequest {
    pub target_type: String,
    pub target_id: String,
}

// =============================================================================
// SYNC CONTROL
// =============================================================================

/// Overall sync server status.
#[derive(Debug, Serialize, ToSchema)]
pub struct SyncStatusResponse {
    pub server_version: String,
    pub connected_users: u32,
    pub pending_syncs: u32,
    pub last_full_sync: i64,
    pub status: String,
}

/// Individual user sync status.
#[derive(Debug, Serialize, ToSchema)]
pub struct UserSyncStatus {
    pub user_id: String,
    pub full_name: Option<String>,
    pub cohort_id: Option<String>,
    pub last_sync: i64,
    pub pending_changes: u32,
    pub status: String,
}

// =============================================================================
// ANKI OPERATIONS (Zone 1 + Zone 2 + Zone 3)
// =============================================================================
// See `crate::services` for Zone documentation.

/// AnkiConnect reachable status.
#[derive(Debug, Serialize, ToSchema)]
pub struct AnkiConnectHealth {
    pub ok: bool,
    pub message: String,
}

/// List of Anki deck names from AnkiConnect.
#[derive(Debug, Serialize, ToSchema)]
pub struct DeckListResponse {
    pub decks: Vec<String>,
}

/// Live due card IDs for a deck.
#[derive(Debug, Serialize, ToSchema)]
pub struct DueCardsResponse {
    pub due: Vec<i64>,
}

/// Live deck health report from AnkiConnect (Zone 2).
#[derive(Debug, Serialize, ToSchema)]
pub struct DeckHealthReport {
    pub deck_id: String,
    pub health_score: u8,
    pub new_count: u32,
    pub learn_count: u32,
    pub review_count: u32,
    pub total_count: u32,
    pub due_count: u32,
    pub issues: Vec<String>,
    pub suggestions: Vec<String>,
}

/// Collection-level cold-start health check (Zone 1).
#[derive(Debug, Serialize, ToSchema)]
pub struct CollectionHealthReport {
    pub schema_version: i32,
    pub note_count: u64,
    pub card_count: u64,
    pub review_count: u64,
    pub deck_count: u64,
    pub config_size_bytes: u64,
    pub has_recent_reviews: bool,
    pub orphaned_note_count: u64,
    pub orphaned_card_count: u64,
}

/// Daily retention curve time series (Zone 1).
#[derive(Debug, Serialize, ToSchema)]
pub struct RetentionCurveResponse {
    pub deck: String,
    pub window_days: u32,
    pub curve: Vec<RetentionPoint>,
}

/// A single point on a daily retention curve.
#[derive(Debug, Serialize, ToSchema)]
pub struct RetentionPoint {
    pub date: String,
    pub retention_rate: f64,
    pub review_count: u32,
    pub lapse_count: u32,
}

/// Lapse frequency analysis response (Zone 1).
#[derive(Debug, Serialize, ToSchema)]
pub struct LapseAnalysisResponse {
    pub deck: String,
    pub window_days: u32,
    pub lapse: LapseStats,
}

/// Lapse frequency statistics.
#[derive(Debug, Serialize, ToSchema)]
pub struct LapseStats {
    pub total_reviews: u64,
    pub total_lapses: u64,
    pub lapse_rate: f64,
    pub avg_interval_before_lapse_secs: f64,
}

/// Interval distribution map (Zone 1).
#[derive(Debug, Serialize, ToSchema)]
pub struct IntervalDegradationResponse {
    pub deck: String,
    pub interval: IntervalDist,
}

/// Interval distribution across a deck.
#[derive(Debug, Serialize, ToSchema)]
pub struct IntervalDist {
    pub bucket_short_days: u64,
    pub bucket_medium_days: u64,
    pub bucket_long_days: u64,
    pub avg_interval_days: f64,
    pub avg_ease_factor: f64,
}

/// Full composed health report (Zone 3) — fuses Zone 1 + Zone 2.
#[derive(Debug, Serialize, ToSchema)]
pub struct FullHealthReport {
    pub deck_name: String,
    pub generated_at: String,
    pub live: LiveStats,
    pub due_now: u64,
    pub retention_trend: Vec<RetentionPoint>,
    pub lapse_stats: LapseStats,
    pub interval_distribution: IntervalDist,
    pub collection_health: CollectionHealthReport,
    pub warnings: Vec<HealthWarning>,
    pub health_score: u8,
}

/// Live scheduler statistics from AnkiConnect.
#[derive(Debug, Serialize, ToSchema)]
pub struct LiveStats {
    pub total_cards: u64,
    pub new_cards: u64,
    pub learn_cards: u64,
    pub review_cards: u64,
    pub mature_cards: u64,
    pub avg_ease: f64,
}

/// Warning embedded in a partial composed report.
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthWarning {
    pub zone: String,
    pub metric: String,
    pub reason: String,
}

/// Timestamp: backup file path from AnkiConnect.
#[derive(Debug, Serialize, ToSchema)]
pub struct BackupResponse {
    pub backup_id: String,
    pub file_path: String,
    pub created_at: i64,
    pub size_bytes: u64,
}

// =============================================================================
// QUERY PARAMETERS (used in #[utoipa::path] annotations)
// (uses serde::Deserialize and utoipa::ToSchema from top-level imports)

/// Query params for paginated deck listing.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ListDeckQuery {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
}

fn default_page() -> u32 { 1 }
fn default_per_page() -> u32 { 20 }
fn default_session_duration() -> i32 { 10 }

/// Query params for statistics filtering.
#[derive(Debug, Deserialize, ToSchema)]
pub struct StatsQuery {
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
    pub deck_id: Option<String>,
    pub user_id: Option<String>,
}

/// Query params for user sync status listing.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SyncStatusQuery {
    pub status: Option<String>,
}

// =============================================================================
// GENERIC API RESPONSES
// =============================================================================

/// Generic success response with a message.
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiResponse {
    pub success: bool,
    pub message: String,
}

/// Paginated list response wrapper.
#[derive(Debug, Serialize, ToSchema)]
pub struct PaginatedResponse<T: Serialize> {
    pub data: Vec<T>,
    pub total: u32,
    pub page: u32,
    pub per_page: u32,
}

// =============================================================================
// SCHOOL-OPTIMIZED METRICS (2026-06-29)
// =============================================================================

/// Practice Adherence Score — "Is this student showing up?" (0–100)
#[derive(Debug, Serialize, ToSchema)]
pub struct PracticeAdherenceResponse {
    pub score: u8,
    pub active_days: u32,
    pub total_days: u32,
    pub missed_days: u32,
    pub window_days: u32,
}

/// Sporadic Practice Index — "Is this student cramming?" (0.0–1.0)
#[derive(Debug, Serialize, ToSchema)]
pub struct SporadicPracticeResponse {
    pub index: f64,
    pub active_days: u32,
    pub total_days: u32,
    pub coverage: f64,
    pub avg_reviews_per_active_day: f64,
}

/// Gap Analysis report — "How long between sessions?"
#[derive(Debug, Serialize, ToSchema)]
pub struct GapAnalysisResponse {
    pub max_gap_days: u32,
    pub avg_gap_days: f64,
    pub gaps_over_3_days: u32,
    pub gaps_over_7_days: u32,
    pub gap_list: Vec<GapEntryResponse>,
}

/// A single gap between active practice days.
#[derive(Debug, Serialize, ToSchema)]
pub struct GapEntryResponse {
    pub from: String,
    pub to: String,
    pub gap_days: i64,
}

/// Deck Suitability Index — "Is this deck too hard or too easy?"
#[derive(Debug, Serialize, ToSchema)]
pub struct DeckSuitabilityResponse {
    pub verdict: String,
    pub lapse_rate: f64,
    pub ease_factor: f64,
    pub mature_rate: f64,
    pub avg_time_per_card_secs: f64,
    pub indicators: SuitabilityIndicatorsResponse,
    pub struggle_points: Vec<String>,
}

/// Per-indicator suitability classification.
#[derive(Debug, Serialize, ToSchema)]
pub struct SuitabilityIndicatorsResponse {
    pub lapse_rate: String,
    pub ease_factor: String,
    pub mature_rate: String,
    pub time_per_card: String,
}

// =============================================================================
// INTERLEAVED MASTERY PIPELINE — Phase 2 request/response types (IMP-4 through IMP-19)
// =============================================================================

// -- Track Library -------------------------------------------------------------

/// Response for a single track in the Track Library.
#[derive(Debug, Serialize, ToSchema)]
pub struct TrackInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub subject_area: Option<String>,
    pub tag: String,
    pub target_retention: f64,
    pub n_value: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Request to create a new track.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateTrackRequest {
    pub name: String,
    pub description: Option<String>,
    pub subject_area: Option<String>,
    pub tag: String,
    #[serde(default = "default_target_retention")]
    pub target_retention: f64,
    #[serde(default = "default_n_value")]
    pub n_value: i32,
}

/// Request to update an existing track.
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateTrackRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub subject_area: Option<String>,
    pub tag: Option<String>,
    pub target_retention: Option<f64>,
    pub n_value: Option<i32>,
}

// -- Track Profiles ------------------------------------------------------------

/// Response for a track profile with its assigned tracks.
#[derive(Debug, Serialize, ToSchema)]
pub struct TrackProfileInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub target_retention: Option<f64>,
    pub n_value: Option<i32>,
    pub session_duration_minutes: i32,
    pub track_count: u32,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Detailed profile response including the list of assigned track IDs.
#[derive(Debug, Serialize, ToSchema)]
pub struct TrackProfileDetail {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub target_retention: Option<f64>,
    pub n_value: Option<i32>,
    pub session_duration_minutes: i32,
    pub tracks: Vec<AssignedTrackEntry>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A single track assignment within a profile.
#[derive(Debug, Serialize, ToSchema)]
pub struct AssignedTrackEntry {
    pub track_id: String,
    pub track_name: String,
    pub subject_area: Option<String>,
    pub sort_order: i32,
}

/// Request to create a new track profile.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateTrackProfileRequest {
    pub name: String,
    pub description: Option<String>,
    pub target_retention: Option<f64>,
    pub n_value: Option<i32>,
    #[serde(default = "default_session_duration")]
    pub session_duration_minutes: i32,
}

/// Request to update a track profile.
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateTrackProfileRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub target_retention: Option<f64>,
    pub n_value: Option<i32>,
    pub session_duration_minutes: Option<i32>,
}

/// Request to assign/reorder a track within a profile.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AssignTrackRequest {
    pub track_id: String,
    #[serde(default)]
    pub sort_order: i32,
}

// -- Capsule Sessions ----------------------------------------------------------

/// Response for a single capsule session.
#[derive(Debug, Serialize, ToSchema)]
pub struct CapsuleSessionInfo {
    pub id: String,
    pub student_id: String,
    pub track_profile_id: String,
    pub n_value: i32,
    pub session_duration_minutes: i32,
    pub card_ids: String,
    pub status: String,
    pub session_week: String,
    pub cards_completed: i32,
    pub cards_total: i32,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

/// Query params for capsule session listing.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CapsuleSessionQuery {
    pub student_id: Option<String>,
    pub status: Option<String>,
    pub session_week: Option<String>,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
}

/// Request to generate a new remediation capsule for a student.
#[derive(Debug, Deserialize, ToSchema)]
pub struct GenerateCapsuleRequest {
    /// Student identifier (maps to user ID in the system).
    pub student_id: String,
    /// Path to the student's `.anki2` collection file.
    #[serde(default)]
    pub anki2_path: String,
    /// Track profile ID to use for capsule generation.
    #[serde(default)]
    pub track_profile_id: String,
    /// ISO week identifier (e.g., "2026-W27") for compliance tracking.
    #[serde(default)]
    pub session_week: String,
    /// N-value override (sessions per week).
    pub n_value: Option<i32>,
    /// Override session duration in minutes (falls back to profile default).
    pub session_duration_minutes: Option<i32>,
}

fn default_target_retention() -> f64 { 0.80 }
fn default_n_value() -> i32 { 3 }

// =============================================================================
// RIG AI CHATBOX — Natural Language Queries
// =============================================================================

/// Natural language query from the dashboard AI chatbox.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AskQuery {
    /// Free-form natural language question about student performance, decks, etc.
    pub query: String,
}

/// Structured result from a natural language query.
#[derive(Debug, Serialize, ToSchema)]
pub struct AskResponse {
    /// Human-readable summary of what was found.
    pub summary: String,
    /// Intent that was detected (e.g., "struggling_students", "retention_drop", "deck_health").
    pub intent: String,
    /// Recommended dashboard section to navigate to.
    pub navigate_to: Option<String>,
    /// Structured data rows for rendering in the chatbox.
    pub rows: Vec<AskRow>,
}

/// A single data row in the chatbox response.
#[derive(Debug, Serialize, ToSchema)]
pub struct AskRow {
    pub label: String,
    pub value: String,
    pub status: Option<String>, // "danger", "warn", "ok"
    pub detail: Option<String>,
}
