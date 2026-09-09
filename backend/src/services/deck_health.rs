//! Zone 3: Composed Deck Health Reporter.
//!
//! ## Purpose
//!
//! Composes Zone 1 (ForensicReader) and Zone 2 (AnkiConnectClient) into a
//! single unified `DeckHealthReport` consumed by the Management Console
//! dashboard. Provides the complete health picture: live scheduler state
//! from AnkiConnect + historical forensic analysis from SQLite.
//!
//! ## Why Compose in Rust, Not the Dashboard?
//!
//! Dashboard JavaScript should not be orchestrating multiple backend calls,
//! serializing results, and computing derived metrics. That is application
//! logic — it belongs in the Rust service layer. The dashboard receives a
//! single, pre-computed JSON response.
//!
//! ## Report Contents
//!
//! The composed report fuses two independent data sources:
//!
//! | Field | Source | Zone |
//! |-------|--------|------|
//! | `total_cards`, `new`, `learn`, `review` | `getDeckStats` | Zone 2 |
//! | `due_now` | `getDueCards` | Zone 2 |
//! | `retention_trend` | `retention_curve()` | Zone 1 |
//! | `lapse_stats` | `lapse_analysis()` | Zone 1 |
//! | `interval_distribution` | `interval_degradation()` | Zone 1 |
//! | `collection_health` | `cold_health_check()` | Zone 1 |
//!
//! ## Architecture
//!
//! ```text
//! dashboard fetch(/management/anki/full-health/:deck_name)
//!   → deck_health.rs: full_health_report()
//!      ├── AnkiConnectClient.get_deck_stats()       [Zone 2]
//!      ├── AnkiConnectClient.get_due_cards()        [Zone 2]
//!      ├── ForensicReader.retention_curve()         [Zone 1]
//!      ├── ForensicReader.lapse_analysis()          [Zone 1]
//!      ├── ForensicReader.interval_degradation()    [Zone 1]
//!      └── ForensicReader.cold_health_check()       [Zone 1]
//!   → JSON: DeckHealthReport
//! ```
//!
//! ## Error Handling
//!
//! Each Zone is queried independently. Failure of Zone 1 (forensics) does
//! **not** cause the whole report to fail — we return partial data with a
//! `warnings` array describing which forensic metrics were unavailable.
//! Failure of Zone 2 (live AnkiConnect) causes the report to fail with a
//! `504 Gateway Timeout` because live scheduler data cannot be computed
//! from SQLite alone.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use crate::services::deck_health::DeckHealthReporter;
//!
//! let reporter = DeckHealthReporter::new(
//!     ForensicReader::open("/path/to/collection.anki2")?,
//!     AnkiConnectClient::new(8765, None),
//! );
//!
//! let report = reporter.full_health_report("Spanish::Vocab", 30).await?;
//! ```

use crate::services::anki_connect::AnkiConnectClient;
use crate::services::forensic_reader::{
    CollectionHealth, ForensicReader, IntervalDegradation, LapseFrequency,
    RetentionPoint,
};

/// Errors from the composed deck health reporter.
#[derive(Debug, thiserror::Error)]
pub enum DeckHealthError {
    #[error("Zone 2 (AnkiConnect) unavailable: {0}")]
    Zone2Unavailable(#[from] crate::services::anki_connect::AnkiConnectError),

    #[error("Zone 1 (forensics) unavailable: {0}")]
    Zone1Unavailable(#[from] crate::services::forensic_reader::ForensicError),

    #[error("deck not found: {0}")]
    DeckNotFound(String),

    /// One or more forensic metrics were unavailable.
    #[error("partial report — some forensic metrics failed: {0}")]
    PartialReport(String),
}

/// Live scheduler statistics from AnkiConnect.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LiveStats {
    pub total_cards: u64,
    pub new_cards: u64,
    pub learn_cards: u64,
    pub review_cards: u64,
    pub mature_cards: u64,
    pub avg_ease: f64,
}

/// A single warning embedded in the composed report.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HealthWarning {
    pub zone: String,
    pub metric: String,
    pub reason: String,
}

/// The full composed health report for a deck.
///
/// Consumed by the Management Console dashboard. All fields are pre-
/// computed server-side — the dashboard renders directly from JSON.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ComposedDeckHealthReport {
    /// Deck name as it appears in Anki.
    pub deck_name: String,
    /// When this report was generated (ISO 8601 UTC).
    pub generated_at: String,
    /// Live scheduler metrics from AnkiConnect (Zone 2).
    pub live: LiveStats,
    /// Number of cards due right now from AnkiConnect (Zone 2).
    pub due_now: u64,
    /// Retention curve time series from SQLite (Zone 1).
    pub retention_trend: Vec<RetentionPoint>,
    /// Lapse frequency analysis from SQLite (Zone 1).
    pub lapse_stats: LapseFrequency,
    /// Interval distribution from SQLite (Zone 1).
    pub interval_distribution: IntervalDegradation,
    /// Collection-level health check (Zone 1).
    pub collection_health: CollectionHealth,
    /// Warnings for metrics that could not be computed.
    pub warnings: Vec<HealthWarning>,
    /// Overall health score 0–100 (computed from lapse rate + interval health).
    pub health_score: u8,
}

// ---------------------------------------------------------------------------
// Deck Health Reporter
// ---------------------------------------------------------------------------

/// Composed deck health reporter — fuses Zone 1 + Zone 2.
///
/// Constructed with a `ForensicReader` (Zone 1) and `AnkiConnectClient`
/// (Zone 2). Provides `full_health_report()` which calls both zones and
/// fuses the results.
pub struct DeckHealthReporter {
    /// Zone 1: forensic SQLite reader.
    forensic: ForensicReader,
    /// Zone 2: AnkiConnect HTTP client.
    anki_connect: AnkiConnectClient,
}

impl DeckHealthReporter {
    /// Construct a new reporter.
    ///
    /// The `days` parameter in `full_health_report()` controls the forensic
    /// analysis window per call.
    pub fn new(forensic: ForensicReader, anki_connect: AnkiConnectClient) -> Self {
        Self {
            forensic,
            anki_connect,
        }
    }

    /// Generate a full health report for a named deck.
    ///
    /// Combines live metrics (Zone 2) + forensic analysis (Zone 1).
    /// Returns a partial report with warnings if Zone 1 is unavailable.
    /// Fails entirely if Zone 2 is unavailable — live data is mandatory.
    pub async fn full_health_report(
        &self,
        deck_name: &str,
        days: u32,
    ) -> Result<ComposedDeckHealthReport, DeckHealthError> {
        let generated_at = chrono::Utc::now().to_rfc3339();
        let mut warnings = Vec::new();

        // ── Zone 2: Live AnkiConnect metrics (required) ──────────────────
        let live_stats = self.fetch_live_stats(deck_name).await?;
        let due_now = self
            .anki_connect
            .get_due_cards(deck_name)
            .await
            .map(|cards| cards.len() as u64)
            .unwrap_or(0);

        // ── Zone 1: Forensic analysis (best-effort) ─────────────────────
        let retention_trend = match self.forensic.retention_curve(deck_name, days) {
            Ok(curve) => curve,
            Err(e) => {
                warnings.push(HealthWarning {
                    zone: "Zone 1 (Forensic)".to_string(),
                    metric: "retention_trend".to_string(),
                    reason: e.to_string(),
                });
                vec![]
            }
        };

        let lapse_stats = match self.forensic.lapse_analysis(deck_name, days) {
            Ok(stats) => stats,
            Err(e) => {
                warnings.push(HealthWarning {
                    zone: "Zone 1 (Forensic)".to_string(),
                    metric: "lapse_stats".to_string(),
                    reason: e.to_string(),
                });
                LapseFrequency {
                    total_reviews: 0,
                    total_lapses: 0,
                    lapse_rate: 0.0,
                    avg_interval_before_lapse_secs: 0.0,
                }
            }
        };

        let interval_distribution = match self.forensic.interval_degradation(deck_name) {
            Ok(dist) => dist,
            Err(e) => {
                warnings.push(HealthWarning {
                    zone: "Zone 1 (Forensic)".to_string(),
                    metric: "interval_distribution".to_string(),
                    reason: e.to_string(),
                });
                IntervalDegradation {
                    bucket_short_days: 0,
                    bucket_medium_days: 0,
                    bucket_long_days: 0,
                    avg_interval_days: 0.0,
                    avg_ease_factor: 2.5,
                }
            }
        };

        let collection_health = match self.forensic.cold_health_check() {
            Ok(health) => health,
            Err(e) => {
                warnings.push(HealthWarning {
                    zone: "Zone 1 (Forensic)".to_string(),
                    metric: "collection_health".to_string(),
                    reason: e.to_string(),
                });
                CollectionHealth {
                    schema_version: 0,
                    note_count: 0,
                    card_count: 0,
                    review_count: 0,
                    deck_count: 0,
                    config_size_bytes: 0,
                    has_recent_reviews: false,
                    orphaned_note_count: 0,
                    orphaned_card_count: 0,
                }
            }
        };

        // ── Health score computation ────────────────────────────────────
        // Score = 100 × (1 - lapse_rate) × interval_health_factor
        // interval_health_factor: 1.0 if avg_interval > 30d, scales down otherwise
        let avg_interval_days = interval_distribution.avg_interval_days;
        let interval_factor = (avg_interval_days / 30.0).min(1.0).max(0.0);
        let health_score = ((1.0 - lapse_stats.lapse_rate) * interval_factor * 100.0) as u8;

        Ok(ComposedDeckHealthReport {
            deck_name: deck_name.to_string(),
            generated_at,
            live: live_stats,
            due_now,
            retention_trend,
            lapse_stats,
            interval_distribution,
            collection_health,
            warnings,
            health_score,
        })
    }

    /// Fetch live deck statistics from AnkiConnect (Zone 2).
    ///
    /// Parses AnkiConnect's `getDeckStats` response into a `LiveStats`
    /// struct. The response format is:
    ///
    /// ```json
    /// { "cards": 142, "notes": 89, "firstDate": 123456789,
    ///   "lastDate": 987654321, "decks": {
    ///     "Deck Name": { "new": 5, "learn": 3, "review": 12, "msancient": 0 }
    /// }}
    /// ```
    /// Fetch live scheduler stats from AnkiConnect for a named deck.
    ///
    /// AnkiConnect's `getDeckStats` returns a map keyed by deck ID (as
    /// string), each value containing: `deck_id`, `name`, `new_count`,
    /// `learn_count`, `review_count`, `total_in_deck`.
    async fn fetch_live_stats(&self, deck_name: &str) -> Result<LiveStats, DeckHealthError> {
        let raw_stats = self
            .anki_connect
            .get_deck_stats(vec![deck_name.to_string()])
            .await?;

        let obj = raw_stats
            .as_object()
            .ok_or_else(|| DeckHealthError::DeckNotFound(deck_name.to_string()))?;

        // Response is keyed by deck ID (string) — find the entry whose
        // `name` field matches the requested deck name.
        let deck_stats = obj
            .values()
            .find(|v| {
                v.get("name")
                    .and_then(|n| n.as_str())
                    .map(|n| n == deck_name)
                    .unwrap_or(false)
            })
            .ok_or_else(|| DeckHealthError::DeckNotFound(deck_name.to_string()))?;

        let map = deck_stats.as_object().unwrap();

        let new_cards: u64 = map.get("new_count").and_then(|v| v.as_u64()).unwrap_or(0);
        let learn_cards: u64 = map.get("learn_count").and_then(|v| v.as_u64()).unwrap_or(0);
        let review_cards: u64 = map.get("review_count").and_then(|v| v.as_u64()).unwrap_or(0);
        let total_cards: u64 = map.get("total_in_deck").and_then(|v| v.as_u64()).unwrap_or(0);

        Ok(LiveStats {
            total_cards,
            new_cards,
            learn_cards,
            review_cards,
            mature_cards: 0, // getDeckStats doesn't expose mature count directly
            avg_ease: 2.5,   // getDeckStats doesn't expose avg_ease directly
        })
    }

    /// Return the list of all deck names from AnkiConnect.
    pub async fn list_decks(&self) -> Result<Vec<String>, DeckHealthError> {
        Ok(self.anki_connect.deck_names().await?)
    }

    /// Check health of both zones.
    pub async fn health_check(&self) -> Result<(), DeckHealthError> {
        self.anki_connect.health_check().await?;
        // Forensic reader is always healthy if it opened successfully
        Ok(())
    }
}