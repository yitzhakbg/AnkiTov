//! Display leaderboard controller — **public, token-gated, no JWT**
//! (spec §8.1).
//!
//! `GET /api/v1/display/class/<token>?window=week&metric=points&format=board|wall`
//!
//! The `<token>` is the unguessable per-class display token minted by the
//! staff controller (`POST /management/leaderboard/<class_id>/display`) and
//! stored in `class_display_boards.token`. Rotating the token invalidates
//! the old link (same 404 code as a never-minted one — spec §8.6).
//!
//! The display is *read-only* and never returns `deeper` (spec §8.5).
//! It auto-polls on the client every `leaderboard.display_refresh_secs`
//! (spec §9.1); no server-side state is required to serve it.

use crate::services::leaderboard::{self as lb, Metric, Window};
use crate::services::leaderboard_aggregate as agg;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use loco_rs::app::AppContext;
use loco_rs::prelude::*;
use serde::Deserialize;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

const TAG: &str = "Leaderboard";

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/display")
        .add("/class/:token", get(display_class_board))
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// Query-string parameters (spec §8.4).
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DisplayQuery {
    /// Window (spec §8.4).
    pub window: Option<String>,
    /// Metric (spec §8.4).
    pub metric: Option<String>,
    /// `board` (default) or `wall` (spec §8.1).
    pub format: Option<String>,
}

fn default_window() -> Window { Window::Week }
fn default_metric() -> Metric { Metric::Points }

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Resolve `<token>` → class + board; respond 404 for unknown/rotated.
///
/// `format=wall` returns the same board but with `entries` trimmed to
/// the top-N (`per_page` default 10) and no `unranked` list — the
/// projector wants a big-number view, not a full ranking (spec §8.1).
///
/// **No JWT**: the token is the credential. This route is *not* covered
/// by `require_auth_for_management` (that middleware is path-scoped to
/// `/api/v1/management/*` only).
#[utoipa::path(
    get,
    path = "/display/class/{token}",
    params(
        ("token" = String, Path, description = "Per-class display token (unguessable)"),
        ("window" = Option<String>, Query, description = "week | month | all"),
        ("metric" = Option<String>, Query, description = "points (default) | compliance | volume"),
        ("format" = Option<String>, Query, description = "board (default) | wall")
    ),
    responses(
        (status = 200, description = "Class board", body = crate::services::leaderboard_aggregate::LeaderboardResponse),
        (status = 404, description = "Unknown or rotated display token — same code, don't confirm validity (spec §8.6)")
    ),
    tag = TAG
)]
pub async fn display_class_board(
    State(ctx): State<AppContext>,
    Path(token): Path<String>,
    Query(q): Query<DisplayQuery>,
) -> Result<Response> {
    // 1. Parse the query params with defaults.
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

    // 2. Resolve the token → class_id.
    let board = crate::models::entities::class_display_board::Entity::find()
        .filter(crate::models::entities::class_display_board::Column::Token.eq(&token))
        .filter(crate::models::entities::class_display_board::Column::Enabled.eq(true))
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("display_class_board: token lookup failed: {e:?}");
            loco_rs::Error::InternalServerError
        })?
        .ok_or_else(|| {
            // Same 404 code for unknown AND rotated (spec §8.6).
            loco_rs::Error::NotFound
        })?;

    let class_id = board.class_id.clone();
    let class_name = agg::class_name(&ctx, &class_id).await;

    // 3. Aggregate + rank.
    let cohort = agg::aggregate_class_cohort(&ctx, &class_id, window).await?;
    let min_cohort = 3usize; // spec §7.3 / leaderboard.min_cohort
    let b = lb::rank(
        metric,
        window,
        &cohort,
        None, // no viewer on the display surface
        min_cohort,
        lb::PointsWeights::default(),
    );

    // 4. Build the envelope.
    let scope = agg::Scope::Class;
    let now = chrono::Utc::now().timestamp();
    let response = agg::LeaderboardResponse {
        scope,
        class_id,
        class_name,
        metric: metric.as_str().to_string(),
        window: window.as_str().to_string(),
        generated_at: now,
        total_ranked: b.total_ranked,
        entries: b.entries,
        unranked: b.unranked,
        me: None, // display never has a viewer
        deeper: None, // display never returns deeper (spec §8.5)
    };

    format::json(response)
}
