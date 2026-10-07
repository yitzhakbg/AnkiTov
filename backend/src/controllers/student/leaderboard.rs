//! Student leaderboard controller — **JWT-gated, self-scoped** (spec §8.2).
//!
//! Three routes, all bound to the caller's *own* identity derived from
//! `class_enrollments` (never from a query parameter):
//!
//! - `GET /student/leaderboard?window=week&metric=points` — the caller's
//!   class board with the caller's own row flagged (`me`).
//! - `GET /student/me/stats?window=all` — the caller's personal tally
//!   (sessions completed, N's, cards) across a window — the "you" view.
//! - `PATCH /student/me/identity { nickname, avatar_emoji }` — set the
//!   caller's board identity (nickname + emoji), stored on the enrollment.
//!
//! All three require a valid bearer token (enforced by `require_auth` in
//! `app.rs`, scoped to the `/student` prefix). A student with **no active
//! enrollment** gets a 404 on the board/stats routes (there is no class to
//! scope to) and an identity write fails the same way.

use crate::middleware::auth::AuthUser;
use crate::services::leaderboard::{self as lb, Metric, Window};
use crate::services::leaderboard_aggregate as agg;
use axum::extract::{Extension, Query, State};
use axum::routing::{get, patch};
use loco_rs::app::AppContext;
use loco_rs::prelude::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};

const TAG: &str = "Leaderboard";

const MIN_COHORT: usize = 3; // spec §7.3 / leaderboard.min_cohort

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/student")
        .add("/leaderboard", get(my_board))
        .add("/me/stats", get(my_stats))
        .add("/me/identity", patch(patch_identity))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn default_window() -> Window {
    Window::Week
}
fn default_metric() -> Metric {
    Metric::Points
}

/// Resolve the caller's active class. `user.id` (i64) is the users PK;
/// the enrollment `student_id` is that value as a *string* (see
/// `management::classes` which stores `user.id.to_string()`).
///
/// Returns `Ok(None)` when the student has no active enrollment → the
/// caller maps that to a 404.
async fn caller_class(
    ctx: &AppContext,
    user: &AuthUser,
) -> Result<Option<String>, loco_rs::Error> {
    let student_id = user.id.to_string();
    let class = agg::student_active_class(ctx, &student_id).await;
    Ok(class)
}

/// 404 when the student has no class to scope to.
async fn require_class(ctx: &AppContext, user: &AuthUser) -> Result<String, loco_rs::Error> {
    match caller_class(ctx, user).await? {
        Some(c) => Ok(c),
        None => Err(loco_rs::Error::NotFound),
    }
}

// ---------------------------------------------------------------------------
// GET /student/leaderboard — the caller's class board, `me` flagged
// ---------------------------------------------------------------------------

/// Query params (spec §8.2).
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct StudentBoardQuery {
    pub window: Option<String>,
    pub metric: Option<String>,
}

#[utoipa::path(
    get,
    path = "/student/leaderboard",
    params(
        ("window" = Option<String>, Query, description = "week (default) | month | all"),
        ("metric" = Option<String>, Query, description = "points (default) | compliance | volume")
    ),
    responses(
        (status = 200, description = "Caller's class board with own row flagged", body = crate::services::leaderboard_aggregate::LeaderboardResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 404, description = "Caller has no active class enrollment")
    ),
    tag = TAG
)]
pub async fn my_board(
    State(ctx): State<AppContext>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<StudentBoardQuery>,
) -> Result<Response> {
    let window = q
        .window
        .as_deref()
        .and_then(|w| Window::parse(w).ok())
        .unwrap_or(default_window());
    let metric = q
        .metric
        .as_deref()
        .and_then(|m| Metric::parse(m).ok())
        .unwrap_or(default_metric());

    // Self-scope: caller's own active class only (spec §8.2).
    let class_id = require_class(&ctx, &user).await?;
    let class_name = agg::class_name(&ctx, &class_id).await;

    let cohort = agg::aggregate_class_cohort(&ctx, &class_id, window).await?;
    let my_id = user.id.to_string();
    let board = lb::rank(
        metric,
        window,
        &cohort,
        Some(&my_id),
        MIN_COHORT,
        lb::PointsWeights::default(),
    );

    // The caller's own row (ranked or unranked). `rank()` sets `is_me` on the
    // ranked entry; if the student is in the unranked partition we surface that
    // instead (spec §8.2 — the student always sees *themselves*).
    let me = match board.entries.iter().find(|e| e.is_me) {
        Some(e) => Some(e.clone()),
        None => None,
    };

    let response = agg::LeaderboardResponse {
        scope: agg::Scope::Class,
        class_id,
        class_name,
        metric: metric.as_str().to_string(),
        window: window.as_str().to_string(),
        generated_at: chrono::Utc::now().timestamp(),
        total_ranked: board.total_ranked,
        entries: board.entries,
        unranked: board.unranked,
        me,
        deeper: None,
    };

    format::json(response)
}

// ---------------------------------------------------------------------------
// GET /student/me/stats — the caller's personal tally
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct StudentStatsQuery {
    /// `all` is the default for the personal "you" view (spec §8.2).
    pub window: Option<String>,
}

/// The caller's own tallies across a window — the "you" panel. Reuses the
/// class-cohort aggregation (it is a per-student rollup) and projects just
/// the caller's row.
#[utoipa::path(
    get,
    path = "/student/me/stats",
    params(
        ("window" = Option<String>, Query, description = "all (default) | week | month")
    ),
    responses(
        (status = 200, description = "Caller's personal tally"),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 404, description = "Caller has no active class enrollment")
    ),
    tag = TAG
)]
pub async fn my_stats(
    State(ctx): State<AppContext>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<StudentStatsQuery>,
) -> Result<Response> {
    let window = q
        .window
        .as_deref()
        .and_then(|w| Window::parse(w).ok())
        .unwrap_or(Window::All);

    let class_id = require_class(&ctx, &user).await?;
    let cohort = agg::aggregate_class_cohort(&ctx, &class_id, window).await?;
    let my_id = user.id.to_string();
    let mine = cohort.into_iter().find(|s| s.student_id == my_id);

    let stats = match mine {
        Some(s) => {
            // `me.stats` — personal numbers the student owns (spec §8.2).
            let compliance = if s.sessions_target > 0 {
                f64::from(s.sessions_completed) / f64::from(s.sessions_target)
            } else {
                0.0
            };
            let points =
                lb::PointsWeights::default().per_session * f64::from(s.sessions_completed)
                    + lb::PointsWeights::default().per_card * f64::from(s.cards_completed);
            serde_json::json!({
                "window": window.as_str(),
                "sessions_completed": s.sessions_completed,
                "sessions_target": s.sessions_target,
                "cards_completed": s.cards_completed,
                "compliance": (compliance * 1000.0).round() / 1000.0, // 3-decimal percent
                "points": (points * 10.0).round() / 10.0,
            })
        }
        None => {
            // Enrolled but no session rows in the window → zeroed stats.
            serde_json::json!({
                "window": window.as_str(),
                "sessions_completed": 0,
                "sessions_target": 0,
                "cards_completed": 0,
                "compliance": 0.0,
                "points": 0.0,
            })
        }
    };

    format::json(stats)
}

// ---------------------------------------------------------------------------
// PATCH /student/me/identity — set the caller's nickname + emoji
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct IdentityPatch {
    /// New nickname (≤ 40 chars, spec §7.1). Empty string clears it.
    pub nickname: Option<String>,
    /// Optional avatar emoji (1–2 codepoints, spec §7.1). `null` clears.
    pub avatar_emoji: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct IdentityResult {
    pub nickname: String,
    pub avatar_emoji: Option<String>,
}

#[utoipa::path(
    patch,
    path = "/student/me/identity",
    request_body = IdentityPatch,
    responses(
        (status = 200, description = "Caller's updated board identity", body = IdentityResult),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 404, description = "Caller has no active class enrollment"),
        (status = 400, description = "nickname or emoji exceeds length limit")
    ),
    tag = TAG
)]
pub async fn patch_identity(
    State(ctx): State<AppContext>,
    Extension(user): Extension<AuthUser>,
    body: axum::Json<IdentityPatch>,
) -> Result<Response> {
    let class_id = require_class(&ctx, &user).await?;

    // Validate lengths (spec §7.1: nickname ≤ 40 codepoints, emoji ≤ 2).
    let nickname = body
        .nickname
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    if nickname.chars().count() > 40 {
        tracing::warn!("patch_identity: nickname exceeds 40 codepoints");
        return Err(loco_rs::Error::BadRequest("nickname exceeds 40 codepoints".into()));
    }

    // `avatar_emoji: Some("")` or a >2-codepoint string clears/ignores it.
    let emoji: Option<String> = body
        .avatar_emoji
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .filter(|t| t.chars().count() <= 2)
        .map(str::to_string);

    // Persist onto the caller's active enrollment for this class.
    let student_id = user.id.to_string();
    let enroll = match crate::models::entities::class_enrollment::Entity::find()
        .filter(
            crate::models::entities::class_enrollment::Column::ClassId
                .eq(&class_id),
        )
        .filter(
            crate::models::entities::class_enrollment::Column::StudentId
                .eq(&student_id),
        )
        .filter(crate::models::entities::class_enrollment::Column::Active.eq(true))
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("patch_identity: enrollment lookup failed: {e:?}");
            loco_rs::Error::InternalServerError
        })?
    {
        Some(r) => r,
        None => return Err(loco_rs::Error::NotFound),
    };

    let old_meta = enroll.metadata_json.clone();
    let mut am: crate::models::entities::class_enrollment::ActiveModel =
        enroll.into_active_model();
    am.display_name = Set(Some(nickname.clone()));
    // Store the emoji inside metadata_json (spec §7.1 — the board identity is
    // `{ nickname, avatar_emoji }`). Read-modify-write so other metadata
    // fields survive.
    let mut meta: serde_json::Value =
        serde_json::from_str(old_meta.as_deref().unwrap_or("{}")).unwrap_or_default();
    if meta.is_object() {
        let obj = meta.as_object_mut().unwrap();
        match &emoji {
            Some(e) => {
                obj.insert("avatar_emoji".into(), serde_json::Value::String(e.clone()));
            }
            None => {
                obj.remove("avatar_emoji");
            }
        }
    }
    am.metadata_json = Set(Some(meta.to_string()));
    am.update(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("patch_identity: update failed: {e:?}");
            loco_rs::Error::InternalServerError
        })?;

    format::json(IdentityResult {
        nickname,
        avatar_emoji: emoji,
    })
}
