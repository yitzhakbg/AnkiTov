//! Zone 1: Anki SQLite Forensic Reader.
//!
//! ## Purpose
//!
//! Opens `.anki2` database files directly via `rusqlite` for read-only
//! forensic analysis. Provides cold-start health checks, retention curves,
//! lapse frequency analysis, interval degradation mapping, and WAL telemetry
//! aggregation — all from raw SQLite, no Anki running required.
//!
//! ## Thread Safety
//!
//! `rusqlite::Connection` uses `RefCell` internally (not `Sync`). For use
//! in a threaded async runtime, the connection is wrapped in
//! `Arc<Mutex<Connection>>`. Methods acquire the lock for the duration of
//! each query, releasing it immediately. Only **read operations** are
//! permitted — the connection is opened read-only.
//!
//! ## Safety Rules
//!
//! | Rule | Enforcement |
//! |------|-------------|
//! | Read-only only | Connection opened with `SQLITE_OPEN_READ_ONLY` |
//! | Cold-start capable | No Anki required — works on `.anki2` files at rest |
//! | WAL safe | Multiple concurrent readers coexist with Anki's writer |
//! | No scheduling writes | All writes go through Zone 2 (AnkiConnect) |
//!
//! ## Schema Notes
//!
//! Anki's SQLite schema is **not a public API**. Known stable facts:
//!
//! | Table | Key Columns |
//! |-------|------------|
//! | `col` | `ver`, `conf` (JSON), `decks` (JSON), `dconf` (JSON) |
//! | `notes` | `id`, `mid`, `fields` (NULL-separated), `tags` |
//! | `cards` | `id`, `nid`, `did`, `mod`, `type`, `queue`, `due`, `ivl`, `factor`, `reps`, `lapses` |
//! | `revlog` | `id` (ms), `cid`, `usn`, `ease`, `ivl`, `lastIvl`, `factor`, `time`, `type` |
//!
//! Grade-to-retention mapping:
//! - `1` (Again) → 0.30
//! - `2` (Hard)   → 0.70
//! - `3` (Good)  → 0.85
//! - `4` (Easy)   → 0.95

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{OpenFlags, Connection};

/// Grade-to-retention mapping (SM-2 derived).
const GRADE_RETENTION: &[(i32, f64)] = &[(1, 0.30), (2, 0.70), (3, 0.85), (4, 0.95)];

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ForensicError {
    #[error("cannot open {0}: {1}")]
    CannotOpen(String, #[source] rusqlite::Error),

    #[error("SQL query failed: {0}")]
    QueryFailed(String, #[source] rusqlite::Error),

    #[error("deck not found: {0}")]
    DeckNotFound(String),

    #[error("no review data for deck {0} in last {1} days")]
    NoDataForDeck(String, u32),
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// A single point on a daily retention curve.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RetentionPoint {
    pub date: String,
    pub retention_rate: f64,
    pub review_count: u32,
    pub lapse_count: u32,
}

/// Lapse frequency for a deck.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LapseFrequency {
    pub total_reviews: u64,
    pub total_lapses: u64,
    pub lapse_rate: f64,
    pub avg_interval_before_lapse_secs: f64,
}

/// Interval distribution across a deck.
#[derive(Debug, Clone, serde::Serialize)]
pub struct IntervalDegradation {
    pub bucket_short_days: u64,
    pub bucket_medium_days: u64,
    pub bucket_long_days: u64,
    pub avg_interval_days: f64,
    pub avg_ease_factor: f64,
}

/// Collection-level cold-start health report.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CollectionHealth {
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

// ---------------------------------------------------------------------------
// Forensic Reader
// ---------------------------------------------------------------------------

/// Read-only SQLite forensic reader for Anki `.anki2` files.
///
/// Thread-safe (`Send + Sync`) via `Arc<Mutex<Connection>>`. All methods are
/// `&self` (no `&mut`). Queries hold the lock only during execution.
pub struct ForensicReader {
    /// Inner connection guarded by a mutex for Send + Sync.
    conn: Arc<Mutex<Connection>>,
}

impl ForensicReader {
    /// Open an `.anki2` file read-only.
    ///
    /// Anki does **not** need to be running.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ForensicError> {
        let path_str = path.as_ref().display().to_string();

        // Open in immutable mode via URI. This tells SQLite that no other process
        // will modify the database, so it skips ALL locking. This is essential for
        // forensic reads of a live Anki collection — Anki holds a write lock on the
        // .anki2 file, and without immutable mode our read-only connection would
        // hang indefinitely waiting for the lock.
        //
        // Trade-off: if Anki writes mid-read, we might get a slightly inconsistent
        // snapshot. This is acceptable for forensic analytics (retention curves,
        // lapse analysis). For live data, Zone 2 (AnkiConnect) is the source of truth.
        let uri = format!("file:{}?immutable=1", path_str);
        let conn = Connection::open_with_flags(
            &uri,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI,
        )
        .map_err(|e| ForensicError::CannotOpen(path_str, e))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Cold-start collection health check — no Anki required.
    pub fn cold_health_check(&self) -> Result<CollectionHealth, ForensicError> {
        let conn = self.conn.lock().unwrap();

        let schema_version: i32 = conn
            .query_row("SELECT ver FROM col LIMIT 1", [], |row| row.get(0))
            .unwrap_or(0);

        let note_count: u64 = conn
            .query_row("SELECT COUNT(*) FROM notes", [], |row| row.get(0))
            .unwrap_or(0);

        let card_count: u64 = conn
            .query_row("SELECT COUNT(*) FROM cards", [], |row| row.get(0))
            .unwrap_or(0);

        let review_count: u64 = conn
            .query_row("SELECT COUNT(*) FROM revlog", [], |row| row.get(0))
            .unwrap_or(0);

        let deck_count: u64 = conn
            .query_row("SELECT COUNT(*) FROM decks", [], |row| row.get(0))
            .unwrap_or(0);

        let config_size_bytes: u64 = conn
            .query_row("SELECT LENGTH(conf) FROM col LIMIT 1", [], |row| row.get(0))
            .unwrap_or(0);

        let cutoff = chrono::Utc::now().timestamp() * 1000 - 86_400_000;
        let has_recent_reviews: bool = conn
            .query_row(
                &format!("SELECT 1 FROM revlog WHERE id >= {cutoff} LIMIT 1"),
                [],
                |_| Ok(()),
            )
            .is_ok();

        let orphaned_note_count: u64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT c.nid) FROM cards c \
                 LEFT JOIN notes n ON c.nid = n.id WHERE n.id IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let orphaned_card_count: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM cards c \
                 LEFT JOIN notes n ON c.nid = n.id WHERE n.id IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        Ok(CollectionHealth {
            schema_version,
            note_count,
            card_count,
            review_count,
            deck_count,
            config_size_bytes,
            has_recent_reviews,
            orphaned_note_count,
            orphaned_card_count,
        })
    }

    /// Daily retention curve time series for a deck over the last N days.
    pub fn retention_curve(
        &self,
        deck_name: &str,
        days: u32,
    ) -> Result<Vec<RetentionPoint>, ForensicError> {
        // Use LIKE to bypass COLLATE unicase (bundled SQLite lacks it).
        // LIKE with a bound parameter avoids both collation and SQL injection.
        let deck_id: Option<i64> = {
            let conn = self.conn.lock().unwrap();
            conn.query_row(
                "SELECT id FROM decks WHERE name LIKE ?1 LIMIT 1",
                [deck_name],
                |row| row.get(0),
            )
            .ok()
        };

        let deck_id =
            deck_id.ok_or_else(|| ForensicError::DeckNotFound(deck_name.to_string()))?;

        let cutoff_ts = (chrono::Utc::now().timestamp() - (days as i64 * 86_400)) * 1000;

        // Materialize the raw (day, ease) rows while holding the lock.
        let raw_rows: Vec<(i64, i32)> = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn
                .prepare_cached(
                    "SELECT (r.id / 86400000) AS day, r.ease \
                     FROM revlog r \
                     JOIN cards c ON r.cid = c.id \
                     WHERE c.did = ?1 AND r.id >= ?2 \
                     ORDER BY day",
                )
                .map_err(|e| ForensicError::QueryFailed("retention_curve".to_string(), e))?;

            let rows = stmt
                .query_map([deck_id, cutoff_ts], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, i32>(1)?))
                })
                .map_err(|e| ForensicError::QueryFailed("retention_curve".to_string(), e))?;

            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| ForensicError::QueryFailed("retention_curve".to_string(), e))?
        };

        // Group by day in memory (lock is already released)
        let mut by_day: std::collections::HashMap<i64, (u32, f64, u32)> =
            std::collections::HashMap::new();

        for (day, ease) in raw_rows {
            let retention = GRADE_RETENTION
                .iter()
                .find(|&&(g, _)| g == ease)
                .map(|&(_, r)| r)
                .unwrap_or(0.5);

            let entry = by_day.entry(day).or_insert((0, 0.0, 0));
            entry.0 += 1;
            entry.1 += retention;
            if ease == 1 {
                entry.2 += 1;
            }
        }

        let mut points: Vec<RetentionPoint> = by_day
            .into_iter()
            .map(|(day_ts, (count, ret_sum, lapses))| {
                let day_secs = day_ts * 86400;
                let dt = chrono::DateTime::from_timestamp(day_secs, 0)
                    .unwrap_or_else(|| chrono::Utc::now());
                RetentionPoint {
                    date: dt.format("%Y-%m-%d").to_string(),
                    review_count: count,
                    retention_rate: if count > 0 {
                        (ret_sum / count as f64 * 1000.0).round() / 1000.0
                    } else {
                        0.0
                    },
                    lapse_count: lapses,
                }
            })
            .collect();

        if points.is_empty() {
            return Err(ForensicError::NoDataForDeck(deck_name.to_string(), days));
        }

        points.sort_by_key(|p| p.date.clone());
        Ok(points)
    }

    /// Name of the first deck in this collection (by id order).
    ///
    /// Used by /stats as a fallback when the live AnkiConnect deck list doesn't
    /// overlap with the forensic collection's decks (e.g. a staged demo
    /// collection whose deck names differ from the running Anki profile).
    pub fn first_deck_name(&self) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT name FROM decks ORDER BY id ASC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
    }

    /// Lapse frequency analysis for a deck over the last N days.
    pub fn lapse_analysis(
        &self,
        deck_name: &str,
        days: u32,
    ) -> Result<LapseFrequency, ForensicError> {
        // LIKE bypasses COLLATE unicase; bound parameter avoids injection.
        let deck_id: Option<i64> = {
            let conn = self.conn.lock().unwrap();
            conn.query_row(
                "SELECT id FROM decks WHERE name LIKE ?1 LIMIT 1",
                [deck_name],
                |row| row.get(0),
            )
            .ok()
        };

        let deck_id =
            deck_id.ok_or_else(|| ForensicError::DeckNotFound(deck_name.to_string()))?;
        let cutoff_ts = (chrono::Utc::now().timestamp() - (days as i64 * 86_400)) * 1000;

        let (total_reviews, total_lapses, avg_interval) = {
            let conn = self.conn.lock().unwrap();

            let total_reviews: u64 = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM revlog r \
                         JOIN cards c ON r.cid = c.id \
                         WHERE c.did = {deck_id} AND r.id >= {cutoff_ts}"
                    ),
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let total_lapses: u64 = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM revlog r \
                         JOIN cards c ON r.cid = c.id \
                         WHERE c.did = {deck_id} AND r.ease = 1 AND r.id >= {cutoff_ts}"
                    ),
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let avg_interval: f64 = conn
                .query_row(
                    &format!(
                        "SELECT AVG(r.lastIvl) FROM revlog r \
                         JOIN cards c ON r.cid = c.id \
                         WHERE c.did = {deck_id} AND r.ease = 1 AND r.id >= {cutoff_ts}"
                    ),
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0.0);

            (total_reviews, total_lapses, avg_interval)
        };

        Ok(LapseFrequency {
            total_reviews,
            total_lapses,
            lapse_rate: if total_reviews > 0 {
                total_lapses as f64 / total_reviews as f64
            } else {
                0.0
            },
            avg_interval_before_lapse_secs: avg_interval,
        })
    }

    /// Interval distribution across a deck.
    pub fn interval_degradation(
        &self,
        deck_name: &str,
    ) -> Result<IntervalDegradation, ForensicError> {
        // LIKE bypasses COLLATE unicase; bound parameter avoids injection.
        let deck_id: Option<i64> = {
            let conn = self.conn.lock().unwrap();
            conn.query_row(
                "SELECT id FROM decks WHERE name LIKE ?1 LIMIT 1",
                [deck_name],
                |row| row.get(0),
            )
            .ok()
        };

        let deck_id =
            deck_id.ok_or_else(|| ForensicError::DeckNotFound(deck_name.to_string()))?;

        let (short, medium, long, avg_ivl, avg_ease) = {
            let conn = self.conn.lock().unwrap();

            let short: u64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM cards WHERE did = {deck_id} AND ivl < 31"),
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let medium: u64 = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM cards WHERE did = {deck_id} AND ivl BETWEEN 31 AND 90"
                    ),
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let long: u64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM cards WHERE did = {deck_id} AND ivl > 90"),
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let avg_ivl: f64 = conn
                .query_row(
                    &format!("SELECT AVG(ivl) FROM cards WHERE did = {deck_id}"),
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0.0);

            let avg_ease: f64 = conn
                .query_row(
                    &format!("SELECT AVG(factor / 1000.0) FROM cards WHERE did = {deck_id}"),
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(2.5);

            (short, medium, long, avg_ivl, avg_ease)
        };

        Ok(IntervalDegradation {
            bucket_short_days: short,
            bucket_medium_days: medium,
            bucket_long_days: long,
            avg_interval_days: avg_ivl,
            avg_ease_factor: avg_ease,
        })
    }

    // -----------------------------------------------------------------------
    // School-Optimized Metrics (2026-06-29)
    // -----------------------------------------------------------------------

    /// Practice Adherence Score (PAS) — "Is this student showing up?"
    ///
    /// Returns 0–100 based on actual reviews vs due cards per day.
    /// A student who crams 200 cards after 6 missed days scores ~14, not 100.
    pub fn practice_adherence(
        &self,
        deck_name: &str,
        days: u32,
    ) -> Result<PracticeAdherence, ForensicError> {
        let deck_id = self.deck_id(deck_name)?;

        // Collect daily review counts for the analysis window.
        let cutoff_ms = (chrono::Utc::now().timestamp_millis()) - (days as i64 * 86_400_000);
        let now_day = chrono::Utc::now().timestamp_millis() / 86_400_000;

        let daily_reviews: std::collections::HashMap<i64, u64> = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn
                .prepare_cached(
                    "SELECT (r.id / 86400000) AS day, COUNT(*) \
                     FROM revlog r JOIN cards c ON r.cid = c.id \
                     WHERE c.did = ?1 AND r.id >= ?2 \
                     GROUP BY day",
                )
                .map_err(|e| ForensicError::QueryFailed("practice_adherence".into(), e))?;
            let rows = stmt
                .query_map([deck_id, cutoff_ms], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, u64>(1)?))
                })
                .map_err(|e| ForensicError::QueryFailed("practice_adherence".into(), e))?;
            rows.collect::<std::result::Result<std::collections::HashMap<_, _>, _>>()
                .map_err(|e| ForensicError::QueryFailed("practice_adherence".into(), e))?
        };

        // Count active days (days with ≥1 review).
        let active_days = daily_reviews.len() as u32;

        // Compute daily scores: 1.0 if reviewed that day, 0.0 if not.
        // (We don't have per-day due counts historically, so we use
        // "did the student review at all" as the adherence signal.)
        let total_days = days;
        let mut daily_scores: Vec<f64> = Vec::with_capacity(total_days as usize);
        for offset in 0..total_days {
            let day = now_day - offset as i64;
            let reviewed = daily_reviews.get(&day).copied().unwrap_or(0);
            if reviewed > 0 {
                daily_scores.push(1.0);
            } else {
                daily_scores.push(0.0);
            }
        }

        let pas = if total_days > 0 {
            (daily_scores.iter().sum::<f64>() / total_days as f64 * 100.0).round() as u8
        } else {
            0
        };

        Ok(PracticeAdherence {
            score: pas,
            active_days,
            total_days,
            missed_days: total_days - active_days,
            window_days: days,
        })
    }

    /// Sporadic Practice Index (SPI) — "Is this student cramming?"
    ///
    /// 0.0 = consistent daily practice, 0.8+ = erratic cramming.
    pub fn sporadic_index(
        &self,
        deck_name: &str,
        days: u32,
    ) -> Result<SporadicPractice, ForensicError> {
        let deck_id = self.deck_id(deck_name)?;
        let cutoff_ms = chrono::Utc::now().timestamp_millis() - (days as i64 * 86_400_000);

        let review_counts: Vec<u64> = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn
                .prepare_cached(
                    "SELECT (r.id / 86400000) AS day, COUNT(*) \
                     FROM revlog r JOIN cards c ON r.cid = c.id \
                     WHERE c.did = ?1 AND r.id >= ?2 \
                     GROUP BY day ORDER BY day",
                )
                .map_err(|e| ForensicError::QueryFailed("sporadic_index".into(), e))?;
            let rows = stmt
                .query_map([deck_id, cutoff_ms], |row| Ok(row.get::<_, u64>(1)?))
                .map_err(|e| ForensicError::QueryFailed("sporadic_index".into(), e))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| ForensicError::QueryFailed("sporadic_index".into(), e))?
        };

        let active_days = review_counts.len() as u32;
        let total_days = days;
        let coverage = if total_days > 0 {
            active_days as f64 / total_days as f64
        } else {
            0.0
        };

        if review_counts.is_empty() {
            return Ok(SporadicPractice {
                index: 1.0,
                active_days: 0,
                total_days,
                coverage: 0.0,
                avg_reviews_per_active_day: 0.0,
            });
        }

        let mean = review_counts.iter().map(|&c| c as f64).sum::<f64>()
            / review_counts.len() as f64;
        let variance = review_counts
            .iter()
            .map(|&c| (c as f64 - mean).powi(2))
            .sum::<f64>()
            / review_counts.len() as f64;
        let std_dev = variance.sqrt();
        let cv = if mean > 0.0 { std_dev / mean } else { 0.0 };

        let spi = (cv * (1.0 - coverage) * 1000.0).round() / 1000.0;

        Ok(SporadicPractice {
            index: spi,
            active_days,
            total_days,
            coverage: (coverage * 1000.0).round() / 1000.0,
            avg_reviews_per_active_day: (mean * 10.0).round() / 10.0,
        })
    }

    /// Gap Analysis — "How long are the gaps between practice sessions?"
    pub fn gap_analysis(
        &self,
        deck_name: &str,
        days: u32,
    ) -> Result<GapReport, ForensicError> {
        let deck_id = self.deck_id(deck_name)?;
        let cutoff_ms = chrono::Utc::now().timestamp_millis() - (days as i64 * 86_400_000);

        let active_days: Vec<i64> = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn
                .prepare_cached(
                    "SELECT DISTINCT (r.id / 86400000) AS day \
                     FROM revlog r JOIN cards c ON r.cid = c.id \
                     WHERE c.did = ?1 AND r.id >= ?2 \
                     ORDER BY day",
                )
                .map_err(|e| ForensicError::QueryFailed("gap_analysis".into(), e))?;
            let rows = stmt
                .query_map([deck_id, cutoff_ms], |row| Ok(row.get::<_, i64>(0)?))
                .map_err(|e| ForensicError::QueryFailed("gap_analysis".into(), e))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| ForensicError::QueryFailed("gap_analysis".into(), e))?
        };

        if active_days.is_empty() {
            return Ok(GapReport {
                max_gap_days: days,
                avg_gap_days: days as f64,
                gaps_over_3_days: 1,
                gaps_over_7_days: 1,
                gap_list: Vec::new(),
            });
        }

        // Compute gaps between consecutive active days.
        let mut gaps: Vec<i64> = Vec::new();
        for i in 1..active_days.len() {
            let gap = active_days[i] - active_days[i - 1] - 1; // gap in days
            if gap > 0 {
                gaps.push(gap);
            }
        }

        let max_gap = gaps.iter().copied().max().unwrap_or(0);
        let avg_gap = if gaps.is_empty() {
            0.0
        } else {
            gaps.iter().sum::<i64>() as f64 / gaps.len() as f64
        };

        let over_3 = gaps.iter().filter(|&&g| g >= 3).count() as u32;
        let over_7 = gaps.iter().filter(|&&g| g >= 7).count() as u32;

        // Build gap_list with date ranges.
        let gap_list: Vec<GapEntry> = gaps
            .iter()
            .enumerate()
            .map(|(i, &gap)| {
                let from_day = active_days[i];
                let to_day = active_days[i + 1];
                let from_dt = chrono::DateTime::from_timestamp(from_day * 86400, 0)
                    .unwrap_or_else(chrono::Utc::now);
                let to_dt = chrono::DateTime::from_timestamp(to_day * 86400, 0)
                    .unwrap_or_else(chrono::Utc::now);
                GapEntry {
                    from: from_dt.format("%Y-%m-%d").to_string(),
                    to: to_dt.format("%Y-%m-%d").to_string(),
                    gap_days: gap,
                }
            })
            .collect();

        Ok(GapReport {
            max_gap_days: max_gap as u32,
            avg_gap_days: (avg_gap * 10.0).round() / 10.0,
            gaps_over_3_days: over_3,
            gaps_over_7_days: over_7,
            gap_list,
        })
    }

    /// Deck Suitability Index (DSI) — "Is this deck too hard or too easy?"
    pub fn deck_suitability(
        &self,
        deck_name: &str,
        days: u32,
    ) -> Result<DeckSuitability, ForensicError> {
        let deck_id = self.deck_id(deck_name)?;
        let cutoff_ms = chrono::Utc::now().timestamp_millis() - (days as i64 * 86_400_000);

        let (lapse_rate, ease_factor_avg, mature_rate, time_per_card_avg) = {
            let conn = self.conn.lock().unwrap();

            // Lapse rate from revlog.
            let total_reviews: u64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM revlog r JOIN cards c ON r.cid = c.id \
                     WHERE c.did = ?1 AND r.id >= ?2",
                    [deck_id, cutoff_ms],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            let lapses: u64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM revlog r JOIN cards c ON r.cid = c.id \
                     WHERE c.did = ?1 AND r.id >= ?2 AND r.ease = 1",
                    [deck_id, cutoff_ms],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            let lapse_rate = if total_reviews > 0 {
                lapses as f64 / total_reviews as f64
            } else {
                0.0
            };

            // Ease factor from cards.
            let ease_factor: f64 = conn
                .query_row(
                    "SELECT AVG(factor / 1000.0) FROM cards WHERE did = ?1",
                    [deck_id],
                    |row| row.get(0),
                )
                .unwrap_or(2.5);

            // Mature rate (ivl >= 21 days).
            let total_cards: u64 = conn
                .query_row("SELECT COUNT(*) FROM cards WHERE did = ?1", [deck_id], |row| {
                    row.get(0)
                })
                .unwrap_or(0);
            let mature_cards: u64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM cards WHERE did = ?1 AND ivl >= 21",
                    [deck_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            let mature_rate = if total_cards > 0 {
                mature_cards as f64 / total_cards as f64
            } else {
                0.0
            };

            // Average time per card from revlog (time is in milliseconds).
            let avg_time_ms: f64 = conn
                .query_row(
                    "SELECT AVG(r.time) FROM revlog r JOIN cards c ON r.cid = c.id \
                     WHERE c.did = ?1 AND r.id >= ?2",
                    [deck_id, cutoff_ms],
                    |row| row.get(0),
                )
                .unwrap_or(0.0);
            let time_per_card = avg_time_ms / 1000.0; // convert to seconds

            (lapse_rate, ease_factor, mature_rate, time_per_card)
        };

        // Classify each indicator.
        let lapse_verdict = if lapse_rate > 0.30 {
            "Too Hard"
        } else if lapse_rate < 0.05 {
            "Too Easy"
        } else {
            "Optimal"
        };
        let ease_verdict = if ease_factor_avg < 1.8 {
            "Too Hard"
        } else if ease_factor_avg > 3.5 {
            "Too Easy"
        } else {
            "Optimal"
        };
        let mature_verdict = if mature_rate < 0.10 {
            "Too Hard"
        } else if mature_rate > 0.90 {
            "Too Easy"
        } else {
            "Optimal"
        };
        let time_verdict = if time_per_card_avg > 45.0 {
            "Too Hard"
        } else if time_per_card_avg < 3.0 {
            "Too Easy"
        } else {
            "Optimal"
        };

        // Overall verdict: majority vote.
        let verdicts = [lapse_verdict, ease_verdict, mature_verdict, time_verdict];
        let too_hard = verdicts.iter().filter(|&&v| v == "Too Hard").count();
        let too_easy = verdicts.iter().filter(|&&v| v == "Too Easy").count();
        let optimal = verdicts.iter().filter(|&&v| v == "Optimal").count();

        let overall = if too_hard >= 3 || (too_hard >= 2 && optimal <= 1) {
            "Too Hard"
        } else if too_easy >= 3 || (too_easy >= 2 && optimal <= 1) {
            "Too Easy"
        } else {
            "Optimal"
        };

        Ok(DeckSuitability {
            verdict: overall.to_string(),
            lapse_rate: (lapse_rate * 1000.0).round() / 1000.0,
            ease_factor: (ease_factor_avg * 100.0).round() / 100.0,
            mature_rate: (mature_rate * 1000.0).round() / 1000.0,
            avg_time_per_card_secs: (time_per_card_avg * 10.0).round() / 10.0,
            indicators: SuitabilityIndicators {
                lapse_rate: lapse_verdict.to_string(),
                ease_factor: ease_verdict.to_string(),
                mature_rate: mature_verdict.to_string(),
                time_per_card: time_verdict.to_string(),
            },
            struggle_points: Vec::new(), // TODO: subdeck analysis
        })
    }

    /// Helper: resolve deck ID by name (LIKE bypasses unicase).
    fn deck_id(&self, deck_name: &str) -> Result<i64, ForensicError> {
        let conn = self.conn.lock().unwrap();
        let deck_id = conn
            .query_row(
                "SELECT id FROM decks WHERE name LIKE ?1 LIMIT 1",
                [deck_name],
                |row| row.get(0),
            )
            .ok();
        drop(conn);
        deck_id.ok_or_else(|| ForensicError::DeckNotFound(deck_name.to_string()))
    }
}

// ---------------------------------------------------------------------------
// School Metric Response Types
// ---------------------------------------------------------------------------

/// Practice Adherence Score — "Is this student showing up?" (0–100)
#[derive(Debug, Clone, serde::Serialize)]
pub struct PracticeAdherence {
    pub score: u8,
    pub active_days: u32,
    pub total_days: u32,
    pub missed_days: u32,
    pub window_days: u32,
}

/// Sporadic Practice Index — "Is this student cramming?" (0.0–1.0)
#[derive(Debug, Clone, serde::Serialize)]
pub struct SporadicPractice {
    pub index: f64,
    pub active_days: u32,
    pub total_days: u32,
    pub coverage: f64,
    pub avg_reviews_per_active_day: f64,
}

/// Gap Analysis report — "How long between sessions?"
#[derive(Debug, Clone, serde::Serialize)]
pub struct GapReport {
    pub max_gap_days: u32,
    pub avg_gap_days: f64,
    pub gaps_over_3_days: u32,
    pub gaps_over_7_days: u32,
    pub gap_list: Vec<GapEntry>,
}

/// A single gap between active practice days.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GapEntry {
    pub from: String,
    pub to: String,
    pub gap_days: i64,
}

/// Deck Suitability Index — "Is this deck too hard or too easy?"
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeckSuitability {
    pub verdict: String,
    pub lapse_rate: f64,
    pub ease_factor: f64,
    pub mature_rate: f64,
    pub avg_time_per_card_secs: f64,
    pub indicators: SuitabilityIndicators,
    pub struggle_points: Vec<String>,
}

/// Per-indicator suitability classification.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SuitabilityIndicators {
    pub lapse_rate: String,
    pub ease_factor: String,
    pub mature_rate: String,
    pub time_per_card: String,
}

impl Clone for ForensicReader {
    fn clone(&self) -> Self {
        Self {
            conn: Arc::clone(&self.conn),
        }
    }
}

// SAFETY: Arc<Mutex<Connection>> is Send + Sync because:
// - Arc<T> is Send + Sync when T: Send + Sync
// - Mutex<T> is Send + Sync when T: Send
// - Connection is Send (it is Send; it uses RefCell which is !Sync, but that's fine behind Mutex)
unsafe impl Send for ForensicReader {}
unsafe impl Sync for ForensicReader {}