//! Leaderboard scoring service — **one rule, every surface**.
//!
//! Implements `specs/2026-10-06-leaderboard.md` §5–§6. The class display, the
//! student view, and the staff console all call [`rank`], so a given student
//! shows the *same* number everywhere and a rank is always explainable from the
//! returned [`Inputs`].
//!
//! Pure and dependency-light: no DB, no HTTP. Callers aggregate
//! `capsule_sessions` per student into [`StudentAggregate`] and hand the cohort
//! in. This keeps the ranking rules unit-testable in isolation.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Metric / window enums (§5, §6.1)
// ---------------------------------------------------------------------------

/// Ranking metric. `Retention`/`Adherence`/`Composite` are **deferred** (Q3):
/// they need per-review telemetry that `retention_logs` does not yet receive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// Gamified derived score — default for display + student surfaces (§5.4).
    Points,
    /// `Σ sessions_completed / Σ n_value` — default for staff (§5.1).
    Compliance,
    /// `Σ cards_completed` (§5.2).
    Volume,
}

impl Metric {
    /// Parse a query-string metric. Deferred metrics return a clear message so
    /// the controller can map it to HTTP 400 (§12 Q3).
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "points" => Ok(Metric::Points),
            "compliance" => Ok(Metric::Compliance),
            "volume" => Ok(Metric::Volume),
            "retention" | "adherence" | "composite" => {
                Err(format!("metric '{s}' is not available yet (needs review telemetry)"))
            }
            other => Err(format!("unknown metric '{other}'")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Metric::Points => "points",
            Metric::Compliance => "compliance",
            Metric::Volume => "volume",
        }
    }
}

/// Time window (§6.1). Weeks are the native grain (`capsule_sessions.session_week`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Window {
    /// Current ISO week (default).
    Week,
    /// Rolling 28 days.
    Month,
    /// All-time.
    All,
}

impl Window {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "week" => Ok(Window::Week),
            "month" => Ok(Window::Month),
            "all" => Ok(Window::All),
            other => Err(format!("unknown window '{other}'")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Window::Week => "week",
            Window::Month => "month",
            Window::All => "all",
        }
    }
}

// ---------------------------------------------------------------------------
// Points weights (§5.4)
// ---------------------------------------------------------------------------

/// Weights for [`Metric::Points`]. Conservative default: showing up is worth
/// more than volume, and the score is strictly monotonic (Q2b — no decay).
#[derive(Debug, Clone, Copy)]
pub struct PointsWeights {
    pub per_session: f64,
    pub per_card: f64,
}

impl Default for PointsWeights {
    fn default() -> Self {
        Self { per_session: 10.0, per_card: 1.0 }
    }
}

// ---------------------------------------------------------------------------
// Inputs / outputs
// ---------------------------------------------------------------------------

/// Board identity (§7.1) — student-chosen nickname + optional emoji.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Identity {
    pub nickname: String,
    pub emoji: Option<String>,
}

/// Per-student rollup of `capsule_sessions` over the requested window.
#[derive(Debug, Clone)]
pub struct StudentAggregate {
    pub student_id: String,
    pub identity: Identity,
    /// Sessions completed in the window.
    pub sessions_completed: i32,
    /// Sum of `n_value` targets in the window.
    pub sessions_target: i32,
    /// Cards reviewed in the window.
    pub cards_completed: i32,
    /// Enrollment timestamp — tie-break #2 (§6.2).
    pub enrolled_at: i64,
}

impl StudentAggregate {
    fn has_activity(&self) -> bool {
        self.sessions_completed > 0 || self.cards_completed > 0
    }
}

/// The inputs behind a score — returned so any surface can explain a rank (§5.0).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Inputs {
    pub sessions_completed: i32,
    pub sessions_target: i32,
    pub cards_reviewed: i32,
}

/// One ranked row.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Entry {
    pub rank: usize,
    pub student_id: String,
    pub identity: Identity,
    pub score: f64,
    pub score_display: String,
    pub inputs: Inputs,
    pub is_me: bool,
}

/// A student with no activity in the window — listed separately, never ranked (§6.4).
/// Serialize-only: `reason` is a constructed `&'static` display label, never parsed
/// from the wire, so `Deserialize` is dropped (it would force a lifetime bound
/// serde cannot satisfy).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Unranked {
    pub student_id: String,
    pub identity: Identity,
    pub reason: &'static str,
}

/// A fully ranked board. Serialize-only — the board is *produced* by `rank()`
/// and serialized to a JSON response; it is never deserialized from input.
#[derive(Debug, Clone, Serialize)]
pub struct Board {
    pub metric: Metric,
    pub window: Window,
    pub total_ranked: usize,
    pub entries: Vec<Entry>,
    pub unranked: Vec<Unranked>,
    /// True when the cohort is too small to display a board (§7.3 / `min_cohort`).
    pub below_min_cohort: bool,
}

// ---------------------------------------------------------------------------
// Scoring (§5)
// ---------------------------------------------------------------------------

/// Score one student under one metric. Pure.
pub fn score(metric: Metric, s: &StudentAggregate, w: PointsWeights) -> f64 {
    match metric {
        Metric::Points => w.per_session * f64::from(s.sessions_completed)
            + w.per_card * f64::from(s.cards_completed),
        Metric::Compliance => {
            if s.sessions_target > 0 {
                f64::from(s.sessions_completed) / f64::from(s.sessions_target)
            } else {
                0.0
            }
        }
        Metric::Volume => f64::from(s.cards_completed),
    }
}

/// Human-facing rendering of a score (§6.3).
pub fn score_display(metric: Metric, value: f64) -> String {
    match metric {
        Metric::Points => format!("{} pts", value.round() as i64),
        Metric::Compliance => format!("{}%", (value * 100.0).round() as i64),
        Metric::Volume => format!("{} cards", value.round() as i64),
    }
}

// ---------------------------------------------------------------------------
// Ranking (§6.2, §6.4)
// ---------------------------------------------------------------------------

/// Rank a cohort. `viewer` (a student id) marks the caller's row.
///
/// - Inactive students go to `unranked` (§6.4) — never rank 0.
/// - Ties use **dense** ranking `1,2,2,3` (Q4), tie-broken by higher volume,
///   then earlier enrollment, then `student_id` (§6.2).
/// - Cohorts below `min_cohort` yield no ranked entries (§7.3).
pub fn rank(
    metric: Metric,
    window: Window,
    cohort: &[StudentAggregate],
    viewer: Option<&str>,
    min_cohort: usize,
    w: PointsWeights,
) -> Board {
    let (active, unranked): (Vec<_>, Vec<_>) =
        cohort.iter().partition(|s: &&StudentAggregate| s.has_activity());

    let below_min_cohort = active.len() < min_cohort;
    if below_min_cohort {
        return Board {
            metric,
            window,
            total_ranked: 0,
            entries: Vec::new(),
            unranked: unranked.into_iter().map(to_unranked).collect(),
            below_min_cohort: true,
        };
    }

    let mut scored: Vec<(f64, &StudentAggregate)> = active
        .into_iter()
        .map(|s| (score(metric, s, w), s))
        .collect();

    // Sort: score desc, then volume desc, then enrolled_at asc, then id asc.
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.1.cards_completed.cmp(&a.1.cards_completed))
            .then_with(|| a.1.enrolled_at.cmp(&b.1.enrolled_at))
            .then_with(|| a.1.student_id.cmp(&b.1.student_id))
    });

    // Dense rank: equal scores share a rank; the next distinct score steps by 1.
    let mut entries = Vec::with_capacity(scored.len());
    let mut rank = 0usize;
    let mut prev: Option<f64> = None;
    for (value, s) in scored {
        let new_rank = match prev {
            Some(p) if (p - value).abs() < f64::EPSILON => rank,
            _ => {
                rank += 1;
                rank
            }
        };
        prev = Some(value);
        entries.push(Entry {
            rank: new_rank,
            student_id: s.student_id.clone(),
            identity: s.identity.clone(),
            score: value,
            score_display: score_display(metric, value),
            inputs: Inputs {
                sessions_completed: s.sessions_completed,
                sessions_target: s.sessions_target,
                cards_reviewed: s.cards_completed,
            },
            is_me: viewer.map(|v| v == s.student_id).unwrap_or(false),
        });
    }

    Board {
        metric,
        window,
        total_ranked: entries.len(),
        entries,
        unranked: unranked.into_iter().map(to_unranked).collect(),
        below_min_cohort: false,
    }
}

fn to_unranked(s: &StudentAggregate) -> Unranked {
    Unranked {
        student_id: s.student_id.clone(),
        identity: s.identity.clone(),
        reason: "no_activity",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn stu(id: &str, sessions: i32, target: i32, cards: i32, enrolled: i64) -> StudentAggregate {
        StudentAggregate {
            student_id: id.into(),
            identity: Identity { nickname: id.into(), emoji: None },
            sessions_completed: sessions,
            sessions_target: target,
            cards_completed: cards,
            enrolled_at: enrolled,
        }
    }

    #[test]
    fn metric_parse_defers_telemetry_metrics() {
        assert_eq!(Metric::parse("points").unwrap(), Metric::Points);
        assert!(Metric::parse("retention").is_err());
        assert!(Metric::parse("bogus").is_err());
    }

    #[test]
    fn compliance_is_ratio_and_volume_is_cards() {
        let s = stu("a", 3, 5, 100, 0);
        let w = PointsWeights::default();
        assert!((score(Metric::Compliance, &s, w) - 0.6).abs() < 1e-9);
        assert_eq!(score(Metric::Volume, &s, w), 100.0);
        assert_eq!(score(Metric::Points, &s, w), 130.0);
        assert_eq!(score_display(Metric::Compliance, 0.6), "60%");
    }

    #[test]
    fn compliance_and_volume_disagree() {
        // Bea: 5/5 sessions, 20 cards each. Ali: 3/5 sessions, 100 cards each.
        let bea = stu("bea", 5, 5, 100, 0);
        let ali = stu("ali", 3, 5, 300, 0);
        let w = PointsWeights::default();
        // Compliance: Bea wins. Volume: Ali wins.
        assert!(score(Metric::Compliance, &bea, w) > score(Metric::Compliance, &ali, w));
        assert!(score(Metric::Volume, &ali, w) > score(Metric::Volume, &bea, w));
    }

    #[test]
    fn dense_ranking_and_unranked_separation() {
        let cohort = vec![
            stu("a", 5, 5, 100, 0), // 150 pts
            stu("b", 5, 5, 100, 1), // 150 pts (tie with a)
            stu("c", 1, 5, 10, 2),  // 20 pts
            stu("z", 0, 0, 0, 3),   // no activity
        ];
        let board = rank(Metric::Points, Window::Week, &cohort, Some("c"), 3, PointsWeights::default());
        assert_eq!(board.entries.len(), 3);
        assert_eq!(board.entries[0].rank, 1);
        assert_eq!(board.entries[1].rank, 1, "tie → dense rank 1");
        assert_eq!(board.entries[2].rank, 2, "next distinct → 2 (dense)");
        // Tie broken by earlier enrollment: "a" (0) before "b" (1).
        assert_eq!(board.entries[0].student_id, "a");
        assert!(board.entries[2].is_me, "viewer 'c' flagged");
        assert_eq!(board.unranked.len(), 1);
        assert_eq!(board.unranked[0].student_id, "z");
        assert!(!board.below_min_cohort);
    }

    #[test]
    fn small_cohort_is_not_a_board() {
        let cohort = vec![stu("a", 5, 5, 100, 0), stu("b", 1, 5, 10, 1)];
        let board = rank(Metric::Points, Window::Week, &cohort, None, 3, PointsWeights::default());
        assert!(board.below_min_cohort);
        assert!(board.entries.is_empty());
        assert_eq!(board.total_ranked, 0);
    }
}
