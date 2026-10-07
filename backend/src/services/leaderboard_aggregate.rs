//! Leaderboard aggregation layer — **DB-backed** bridge between the pure
//! scoring service ([`super::leaderboard`]) and the SeaORM world.
//!
//! The pure `leaderboard` module is intentionally free of DB, HTTP, and
//! config. This module is where we actually *fetch* the data:
//!
//! - [`aggregate_class_cohort`] — for one class + one window: join
//!   `class_enrollments` (active) with `capsule_sessions`, sum
//!   `sessions_completed` / `sessions_target` / `cards_completed` per
//!   student, and produce the `Vec<StudentAggregate>` that [`super::
//!   leaderboard::rank`] ranks.
//!
//! - [`class_name`] — resolves `classes.id` → display name for the
//!   response envelope (`LeaderboardResponse.class_name`, spec §8.5).
//!
//! One grouped query over `capsule_sessions` + one join to
//! `class_enrollments` (spec §8.7). No N+1: the student set is bounded
//! by the class, so we filter on `class_id` first, then do a single
//! `IN(...)` on `capsule_sessions.student_id`.
//!
//! ## Identity fallbacks (§7.1)
//!
//! The `class_enrollments.display_name` column is the canonical board
//! identity. If it is `None` (student has never set a nickname and the
//! enrollment row was seeded with only the `student_id`), we fall back
//! to the student's id itself — the spec's "Student 1..N" style is
//! reserved for the `anonymize` escape hatch (a separate, later chunk).

use crate::models::entities::{capsule_session as cs_entity, class as cls_entity, class_enrollment as ce_entity};
use crate::services::leaderboard::{Identity, StudentAggregate, Window};
use loco_rs::app::AppContext;
use sea_orm::{
    ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Aggregation for one class + one window
// ---------------------------------------------------------------------------

/// Per-student rollup over `capsule_sessions` for a given `Window`.
///
/// - `Week`     → the current ISO week (`session_week = "<iso-week>")`).
/// - `Month`    → the last 28 days of `session_week` values.
/// - `All`      → no week filter (all rows).
///
/// Returns the cohort in arbitrary order — the scoring service
/// ([`super::leaderboard::rank`]) sorts and ranks internally.
///
/// `window` also drives the `sessions_target` sum: for `Week` and
/// `Month` it is the sum of `n_value` across the *completed* sessions
/// in that window (i.e. "how many N's were we supposed to hit"); for
/// `All` it is the sum across the student's lifetime of sessions,
/// which is the "all-time Ns" the spec's "all" window asks for.
pub async fn aggregate_class_cohort(
    ctx: &AppContext,
    class_id: &str,
    window: Window,
) -> Result<Vec<StudentAggregate>, loco_rs::Error> {
    // 1. Enrolled students in this class (active rows only).
    let enrollments: Vec<ce_entity::Model> = ce_entity::Entity::find()
        .filter(ce_entity::Column::ClassId.eq(class_id))
        .filter(ce_entity::Column::Active.eq(true))
        .all(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("aggregate_class_cohort: enrollment query failed: {e:?}");
            loco_rs::Error::InternalServerError
        })?;

    let student_ids: Vec<String> = enrollments.iter().map(|e| e.student_id.clone()).collect();
    if student_ids.is_empty() {
        return Ok(Vec::new());
    }

    // 2. Map of student_id → Identity (display_name or fallback to id).
    //    `enrolled_at` is preserved for the tie-break inside `rank()`.
    let identity_map: std::collections::HashMap<String, (Identity, i64)> = enrollments
        .iter()
        .map(|e| {
            let nickname = e
                .display_name
                .clone()
                .unwrap_or_else(|| e.student_id.clone());
            (
                e.student_id.clone(),
                (
                    Identity {
                        nickname,
                        emoji: None, // §7.1: emoji is set via PATCH /student/me/identity
                    },
                    e.enrolled_at,
                ),
            )
        })
        .collect();

    // 3. Capsule sessions for those students, in-window.
    let weeks = window_filter(&ctx, window).await?;

    let sessions: Vec<cs_entity::Model> = cs_entity::Entity::find()
        .filter(cs_entity::Column::StudentId.is_in(&student_ids))
        .filter(
            cs_entity::Column::Status
                .eq("completed")
                .or(cs_entity::Column::Status.eq("in_progress")),
        )
        .filter(cs_entity::Column::SessionWeek.is_in(weeks))
        .all(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("aggregate_class_cohort: session query failed: {e:?}");
            loco_rs::Error::InternalServerError
        })?;

    // 4. Aggregate per student.
    let mut by_student: std::collections::HashMap<
        String,
        StudentAggregate,
    > = std::collections::HashMap::new();

    for s in &sessions {
        let entry = by_student
            .entry(s.student_id.clone())
            .or_insert_with(|| {
                let (identity, enrolled_at) = identity_map
                    .get(&s.student_id)
                    .cloned()
                    .unwrap_or_else(|| {
                        // Session for a student not currently in the class —
                        // keep the row (the cohort is "active" as of the
                        // window) but fall back to the id as the identity.
                        (
                            Identity {
                                nickname: s.student_id.clone(),
                                emoji: None,
                            },
                            0,
                        )
                    });
                StudentAggregate {
                    student_id: s.student_id.clone(),
                    identity,
                    sessions_completed: 0,
                    sessions_target: 0,
                    cards_completed: 0,
                    enrolled_at,
                }
            });
        entry.sessions_completed += 1;
        entry.sessions_target += s.n_value;
        entry.cards_completed += s.cards_completed;
    }

    // 5. Students with no activity in the window must still appear in the
    //    cohort (they end up in the `unranked` partition). So seed the
    //    aggregate for every enrolled student first.
    for (sid, (identity, enrolled_at)) in &identity_map {
        by_student
            .entry(sid.clone())
            .or_insert_with(|| StudentAggregate {
                student_id: sid.clone(),
                identity: identity.clone(),
                sessions_completed: 0,
                sessions_target: 0,
                cards_completed: 0,
                enrolled_at: *enrolled_at,
            });
    }

    Ok(by_student.into_values().collect())
}

// ---------------------------------------------------------------------------
// Window → session_weeks list
// ---------------------------------------------------------------------------

/// Build the list of `session_week` values that fall inside the requested
/// [`Window`]. Returns an empty slice to signal "no filter" (we only
/// use this to constrain `is_in`; the empty-list case means "all rows"
/// and the caller treats it accordingly — see the note in
/// `aggregate_class_cohort` step 3).
///
/// For `Window::All` we return an explicit list of **all** existing
/// distinct `session_week` values, which keeps the `is_in` filter
/// equivalent to "no filter" without changing the query shape.
async fn window_filter(
    ctx: &AppContext,
    window: Window,
) -> Result<Vec<String>, loco_rs::Error> {
    use chrono::{Datelike, Utc};
    match window {
        Window::Week => {
            let now = Utc::now();
            let iso = now.date_naive().iso_week();
            Ok(vec![format!("{}-W{:02}", iso.year(), iso.week())])
        }
        Window::Month => {
            // Last 28 days of ISO weeks, inclusive of the current one.
            let mut weeks = Vec::new();
            let mut cursor = Utc::now();
            loop {
                let iso = cursor.date_naive().iso_week();
                let w = format!("{}-W{:02}", iso.year(), iso.week());
                if !weeks.contains(&w) {
                    weeks.push(w);
                }
                // Step back one week in naive date arithmetic.
                cursor = cursor - chrono::Duration::days(7);
                if cursor.timestamp() < (Utc::now() - chrono::Duration::days(28)).timestamp() {
                    break;
                }
            }
            Ok(weeks)
        }
        Window::All => {
            // Every distinct week that appears in capsule_sessions — the
            // "all-time" window. A single indexed `select_only` on a
            // string column over the whole table is fine at this scale.
            let rows: Vec<(String,)> = cs_entity::Entity::find()
                .select_only()
                .column(cs_entity::Column::SessionWeek)
                .distinct()
                .into_tuple()
                .all(&ctx.db)
                .await
                .map_err(|e| {
                    tracing::error!("window_filter(All): {e:?}");
                    loco_rs::Error::InternalServerError
                })?;
            Ok(rows.into_iter().map(|(w,)| w).collect())
        }
    }
}

// ---------------------------------------------------------------------------
// Class name
// ---------------------------------------------------------------------------

/// Resolve a student's *active* class enrollment (spec §8.2).
///
/// The student's board scope is **derived from the DB, never from a query
/// parameter** — a caller cannot ask for another class's board through this
/// path. Returns the `class_id` for the caller's single active enrollment, or
/// `None` when the student has no active enrollment (the student controller
/// maps that to a 404). If a student somehow has *two* active enrollments
/// (data anomaly), the most recently enrolled one wins.
pub async fn student_active_class(
    ctx: &AppContext,
    student_id: &str,
) -> Option<String> {
    let rows: Vec<ce_entity::Model> = ce_entity::Entity::find()
        .filter(ce_entity::Column::StudentId.eq(student_id))
        .filter(ce_entity::Column::Active.eq(true))
        .order_by_desc(ce_entity::Column::EnrolledAt)
        .all(&ctx.db)
        .await
        .ok()?
        .into_iter()
        .collect();
    rows.first().map(|r| r.class_id.clone())
}

/// Resolve `classes.id` → `name`. Used in the response envelope
/// (`LeaderboardResponse.class_name`).
pub async fn class_name(ctx: &AppContext, class_id: &str) -> Option<String> {
    let row: Option<cls_entity::Model> = cls_entity::Entity::find_by_id(class_id)
        .one(&ctx.db)
        .await
        .ok()?;
    row.map(|r| r.name)
}

// ---------------------------------------------------------------------------
// Response envelope
// ---------------------------------------------------------------------------

/// The wire shape for every leaderboard surface (spec §8.5).
///
/// `entries` / `unranked` are serialized from the pure service's
/// `Entry`/`Unranked` — we re-export them here only to attach the
/// `ToSchema` derive for utoipa; the structs themselves live in
/// `services::leaderboard` and are `Serialize`-only by design.
///
/// `LeaderboardResponse` is an **output-only** wire type — controllers build it
/// by value and serialize it; it is never deserialized from a request. So
/// `Deserialize` is deliberately dropped (it would force `Unranked` — which
/// carries a `&'static str` `reason` — to satisfy a lifetime bound serde
/// cannot).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LeaderboardResponse {
    pub scope: Scope,
    pub class_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    pub metric: String,
    pub window: String,
    pub generated_at: i64,
    pub total_ranked: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<super::leaderboard::Entry>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unranked: Vec<super::leaderboard::Unranked>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub me: Option<super::leaderboard::Entry>,
    /// Populated only on the staff drill-down.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deeper: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// One class's board.
    Class,
    /// Admin-only, all classes (spec §8.4).
    School,
}

impl Scope {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "class" => Some(Self::Class),
            "school" => Some(Self::School),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::School => "school",
        }
    }
}
