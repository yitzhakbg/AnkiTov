//! Track Health Aggregation — per-track FSRS analytics for the dashboard.
//!
//! Groups FSRS scheduling parameters (stability, difficulty, retention rate)
//! by track tag so instructors can see which subject areas need intervention.
//!
//! Queries the Anki `.anki2` SQLite database (read-only, immutable mode)
//! to compute per-track health from the `cards`, `notes`, and `revlog` tables.
//!
//! ## Metrics Computed
//!
//! | Metric | Source | Description |
//! |--------|--------|-------------|
//! | `cards_total` | `cards` + `notes` | Cards matching the track tag |
//! | `cards_due` | `cards` + day number | Overdue cards |
//! | `avg_interval_days` | `cards.ivl` | Average scheduling interval |
//! | `avg_ease_factor` | `cards.factor / 1000` | SM-2 ease factor |
//! | `retention_7d` | `revlog` 7-day window | Recent correct-answer rate |
//! | `avg_time_secs` | `revlog.time` 30-day window | Seconds per card |

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Aggregated health metrics for a single track.
#[derive(Debug, Clone, Serialize)]
pub struct TrackHealth {
    pub track_tag: String,
    pub track_name: String,
    pub cards_total: u64,
    pub cards_due: u64,
    pub cards_mature: u64,
    pub avg_interval_days: f64,
    pub avg_ease_factor: f64,
    /// Retention rate in the last 7 days (correct answers / total answers).
    pub retention_7d: f64,
    /// Average seconds spent per card (30-day window).
    pub avg_time_secs: f64,
    /// Total reviews in the last 7 days.
    pub reviews_7d: u64,
}

/// Response for the per-track health endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct TrackHealthResponse {
    pub tracks: Vec<TrackHealth>,
    pub generated_at: String,
    pub overall_retention: f64,
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum TrackHealthError {
    #[error("cannot open {0}: {1}")]
    CannotOpen(String, #[source] rusqlite::Error),

    #[error("SQL query failed: {0}")]
    QueryFailed(String, #[source] rusqlite::Error),
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Compute per-track health metrics from the `.anki2` database.
///
/// # Arguments
/// * `anki2_path` — Path to the Anki collection file.
/// * `tracks` — List of (track_tag, track_name) pairs to compute metrics for.
pub fn compute_track_health(
    anki2_path: impl AsRef<Path>,
    tracks: &[(String, String)],  // (tag, name)
) -> Result<TrackHealthResponse, TrackHealthError> {
    let path_str = anki2_path.as_ref().display().to_string();
    let uri = format!("file:{}?immutable=1", path_str);
    let conn = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| TrackHealthError::CannotOpen(path_str, e))?;

    // Get collection creation date
    let crt: i64 = conn
        .query_row("SELECT crt FROM col LIMIT 1", [], |row| row.get(0))
        .unwrap_or(0);

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let today_day = (now_secs - crt) / 86400;

    // 7-day cutoff for retention (revlog.id is milliseconds since epoch)
    let seven_days_ago_ms = now_secs * 1000 - 7 * 86_400_000;

    // 30-day cutoff for avg time
    let thirty_days_ago_ms = now_secs * 1000 - 30 * 86_400_000;

    let mut track_healths = Vec::new();
    let mut total_correct = 0u64;
    let mut total_answers = 0u64;

    for (tag, name) in tracks {
        let health = compute_single_track_health(
            &conn,
            tag,
            name,
            today_day,
            seven_days_ago_ms,
            thirty_days_ago_ms,
        )?;

        let correct_7d = (health.retention_7d * health.reviews_7d as f64) as u64;
        total_correct += correct_7d;
        total_answers += health.reviews_7d;

        track_healths.push(health);
    }

    let overall_retention = if total_answers > 0 {
        total_correct as f64 / total_answers as f64
    } else {
        0.0
    };

    let generated_at = chrono::Utc::now().to_rfc3339();

    Ok(TrackHealthResponse {
        tracks: track_healths,
        generated_at,
        overall_retention,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn compute_single_track_health(
    conn: &Connection,
    tag: &str,
    name: &str,
    today_day: i64,
    seven_days_ago_ms: i64,
    thirty_days_ago_ms: i64,
) -> Result<TrackHealth, TrackHealthError> {
    let escaped = tag.replace('\'', "''");
    let tag_filter = format!("n.tags LIKE '%{}%'", escaped);

    // Total cards matching this track tag
    let stmt = format!(
        "SELECT COUNT(*), \
                AVG(c.ivl), \
                AVG(c.factor), \
                COUNT(CASE WHEN c.type = 2 AND c.ivl >= 21 THEN 1 END) \
         FROM cards c JOIN notes n ON c.nid = n.id \
         WHERE {tag_filter}"
    );
    let (cards_total, avg_ivl, avg_factor, cards_mature): (u64, Option<f64>, Option<f64>, u64) =
        conn.query_row(&stmt, [], |row| {
            Ok((
                row.get::<_, u64>(0)?,
                row.get::<_, Option<f64>>(1)?,
                row.get::<_, Option<f64>>(2)?,
                row.get::<_, u64>(3)?,
            ))
        })
        .map_err(|e| TrackHealthError::QueryFailed("cards stats".into(), e))?;

    // Due cards (type=2 review cards past due)
    let cards_due: u64 = {
        let stmt = format!(
            "SELECT COUNT(*) FROM cards c JOIN notes n ON c.nid = n.id \
             WHERE {tag_filter} AND c.type = 2 AND c.due <= {today_day} AND c.queue != 0"
        );
        conn.query_row(&stmt, [], |row| row.get(0))
            .unwrap_or(0)
    };

    // 7-day retention from revlog (ease 3 or 4 = correct)
    let (reviews_7d, correct_7d): (u64, u64) = {
        let stmt = format!(
            "SELECT COUNT(*), \
                    COUNT(CASE WHEN r.ease >= 3 THEN 1 END) \
             FROM revlog r JOIN cards c ON r.cid = c.id \
             JOIN notes n ON c.nid = n.id \
             WHERE {tag_filter} AND r.id >= {seven_days_ago_ms}"
        );
        conn.query_row(&stmt, [], |row| {
            Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?))
        })
        .unwrap_or((0, 0))
    };

    let retention_7d = if reviews_7d > 0 {
        correct_7d as f64 / reviews_7d as f64
    } else {
        0.0
    };

    // 30-day average time per card (revlog.time is in milliseconds)
    let avg_time_secs: f64 = {
        let stmt = format!(
            "SELECT AVG(r.time) / 1000.0 \
             FROM revlog r JOIN cards c ON r.cid = c.id \
             JOIN notes n ON c.nid = n.id \
             WHERE {tag_filter} AND r.id >= {thirty_days_ago_ms} AND r.time > 0"
        );
        conn.query_row(&stmt, [], |row| row.get::<_, f64>(0))
            .unwrap_or(0.0)
    };

    Ok(TrackHealth {
        track_tag: tag.to_string(),
        track_name: name.to_string(),
        cards_total,
        cards_due,
        cards_mature,
        avg_interval_days: avg_ivl.unwrap_or(0.0),
        avg_ease_factor: avg_factor.unwrap_or(0.0) / 1000.0, // Anki stores ×1000
        retention_7d,
        avg_time_secs,
        reviews_7d,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_health_response_structure() {
        let health = TrackHealth {
            track_tag: "math::fractions".into(),
            track_name: "Fractions".into(),
            cards_total: 50,
            cards_due: 10,
            cards_mature: 30,
            avg_interval_days: 15.5,
            avg_ease_factor: 2.5,
            retention_7d: 0.85,
            avg_time_secs: 12.3,
            reviews_7d: 40,
        };

        let response = TrackHealthResponse {
            tracks: vec![health],
            generated_at: "2026-07-07T23:00:00Z".into(),
            overall_retention: 0.85,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("math::fractions"));
        assert!(json.contains("0.85"));
    }
}