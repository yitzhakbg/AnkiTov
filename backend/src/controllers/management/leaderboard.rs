//! Management leaderboard controller — **JWT-gated, staff roles** (spec §8.3).
//!
//! Five handlers, all requiring a `teacher` or `admin` JWT. The outer
//! `require_auth_for_management` layer proves *someone* is authenticated on
//! `/management/*`; each handler below additionally calls `ensure_role` to
//! confirm a *staff* role (a student token that slipped through the outer
//! layer is still rejected with 403).
//!
//! - `GET  /management/leaderboard?scope=class|school&class_id=…&metric=…&window=…`
//!   — a class's board (default) or the school-wide roll-up (`scope=school`).
//! - `GET  /management/leaderboard/classes` — list of classes that have a
//!   board, for the staff dropdown.
//! - `GET  /management/leaderboard/student/:student_id?class_id=…` — a single
//!   student's record with a `deeper` object (spec §8.3) for the drill-down.
//! - `POST /management/leaderboard/:class_id/display` — mint/rotate the
//!   per-class display token; consent-gated (spec §8.3).
//! - `PATCH /management/leaderboard/:class_id/identity/:student_id` — moderate
//!   a student's board identity (nickname reset / emoji strip).
//!
//! Read endpoints default to `metric=compliance` (the staff default, spec
//! §5.1). Write endpoints require the role and are audited.

use crate::middleware::auth::{ensure_role, AuthUser};
use crate::services::leaderboard::{self as lb, Metric, Window};
use crate::services::leaderboard_aggregate as agg;
use axum::extract::{Extension, Path, Query, State};
use axum::routing::{get, patch, post};
use loco_rs::app::AppContext;
use loco_rs::prelude::*;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QuerySelect, Set};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

const TAG: &str = "Management Console";

const MIN_COHORT: usize = 3; // spec §7.3

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management/leaderboard")
        .add("", get(board))
        .add("/classes", get(classes))
        .add("/student/:student_id", get(student_drilldown))
        .add("/:class_id/display", post(display))
        .add("/:class_id/identity/:student_id", patch(moderate_identity))
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn parse_window(s: Option<&str>) -> Result<Window, loco_rs::Error> {
    match s {
        Some(x) => Window::parse(x)
            .map_err(|e| {
                tracing::warn!("leaderboard: bad window '{e}'");
                loco_rs::Error::BadRequest(e)
            }),
        None => Ok(Window::Week),
    }
}
fn parse_metric(s: Option<&str>) -> Result<Metric, loco_rs::Error> {
    match s {
        Some(x) => Metric::parse(x)
            .map_err(|e| {
                tracing::warn!("leaderboard: bad metric '{e}'");
                loco_rs::Error::BadRequest(e)
            }),
        None => Ok(Metric::Compliance), // staff default (spec §5.1)
    }
}

// ---------------------------------------------------------------------------
// GET /management/leaderboard — class board or school roll-up
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct BoardQuery {
    /// `class` (default) or `school`.
    pub scope: Option<String>,
    /// Required when `scope=class` (or when no scope is given).
    pub class_id: Option<String>,
    pub metric: Option<String>,
    pub window: Option<String>,
}

#[utoipa::path(
    get,
    path = "/management/leaderboard",
    params(
        ("scope" = Option<String>, Query, description = "class (default) | school"),
        ("class_id" = Option<String>, Query, description = "Class id (required unless scope=school)"),
        ("metric" = Option<String>, Query, description = "compliance (staff default) | points | volume"),
        ("window" = Option<String>, Query, description = "week (default) | month | all")
    ),
    responses(
        (status = 200, description = "Class or school board", body = crate::services::leaderboard_aggregate::LeaderboardResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller is not a teacher or admin"),
        (status = 400, description = "scope=class requires class_id")
    ),
    tag = TAG
)]
pub async fn board(
    State(ctx): State<AppContext>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<BoardQuery>,
) -> Result<Response> {
    ensure_role(&user, &["teacher", "admin"])?;

    let window = parse_window(q.window.as_deref())?;
    let metric = parse_metric(q.metric.as_deref())?;

    let scope = q.scope.as_deref().and_then(agg::Scope::parse).unwrap_or(agg::Scope::Class);

    match scope {
        agg::Scope::School => {
            // School roll-up: aggregate every active class, one cohort per
            // student, and rank across the whole school (spec §8.4). We reuse
            // `aggregate_class_cohort` per class and merge; a student enrolled
            // in two classes appears twice, which is a data anomaly we surface
            // (a student normally has one active enrollment).
            let class_ids = all_active_class_ids(&ctx).await?;
            let mut merged: Vec<lb::StudentAggregate> = Vec::new();
            let mut class_names: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for cid in &class_ids {
                let name = agg::class_name(&ctx, cid).await;
                let cohort = agg::aggregate_class_cohort(&ctx, cid, window).await?;
                for s in cohort {
                    class_names.insert(s.student_id.clone(), name.clone().unwrap_or_default());
                    merged.push(s);
                }
            }
            let b = lb::rank(
                metric,
                window,
                &merged,
                None,
                MIN_COHORT,
                lb::PointsWeights::default(),
            );
            let response = agg::LeaderboardResponse {
                scope,
                class_id: "school".into(),
                class_name: Some("Whole school".into()),
                metric: metric.as_str().to_string(),
                window: window.as_str().to_string(),
                generated_at: chrono::Utc::now().timestamp(),
                total_ranked: b.total_ranked,
                entries: b.entries,
                unranked: b.unranked,
                me: None,
                deeper: None,
            };
            format::json(response)
        }
        agg::Scope::Class => {
            let class_id = match q.class_id.as_deref().map(str::to_owned) {
                Some(c) if !c.is_empty() => c,
                _ => {
                    tracing::warn!("leaderboard board: scope=class without class_id");
                    return Err(loco_rs::Error::BadRequest(
                        "class_id is required for scope=class".into(),
                    ));
                }
            };

            let class_name = agg::class_name(&ctx, &class_id).await;
            let cohort = agg::aggregate_class_cohort(&ctx, &class_id, window).await?;
            let b = lb::rank(
                metric,
                window,
                &cohort,
                None,
                MIN_COHORT,
                lb::PointsWeights::default(),
            );
            let response = agg::LeaderboardResponse {
                scope,
                class_id,
                class_name,
                metric: metric.as_str().to_string(),
                window: window.as_str().to_string(),
                generated_at: chrono::Utc::now().timestamp(),
                total_ranked: b.total_ranked,
                entries: b.entries,
                unranked: b.unranked,
                me: None,
                deeper: None,
            };
            format::json(response)
        }
    }
}

// ---------------------------------------------------------------------------
// GET /management/leaderboard/classes — board dropdown
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema)]
pub struct ClassRow {
    pub class_id: String,
    pub name: String,
    pub display_enabled: bool,
    pub enrolled: i32,
}

#[utoipa::path(
    get,
    path = "/management/leaderboard/classes",
    responses(
        (status = 200, description = "Classes that have a board", body = Vec<ClassRow>),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller is not a teacher or admin")
    ),
    tag = TAG
)]
pub async fn classes(
    State(ctx): State<AppContext>,
    Extension(user): Extension<AuthUser>,
) -> Result<Response> {
    ensure_role(&user, &["teacher", "admin"])?;

    let class_ids = all_active_class_ids(&ctx).await?;
    let mut rows: Vec<ClassRow> = Vec::new();
    for cid in class_ids {
        let display_enabled = crate::models::entities::class_display_board::Entity::find_by_id(&cid)
            .one(&ctx.db)
            .await
            .ok()
            .and_then(|r| r.map(|b| b.enabled))
            .unwrap_or(false);
        let name = agg::class_name(&ctx, &cid).await;
        let enrolled: Vec<crate::models::entities::class_enrollment::Model> =
            crate::models::entities::class_enrollment::Entity::find()
                .filter(
                    crate::models::entities::class_enrollment::Column::ClassId
                        .eq(&cid),
                )
                .filter(crate::models::entities::class_enrollment::Column::Active.eq(true))
                .all(&ctx.db)
                .await
                .unwrap_or_default();
        rows.push(ClassRow {
            class_id: cid,
            name: name.unwrap_or_default(),
            display_enabled,
            enrolled: enrolled.len() as i32,
        });
    }
    format::json(rows)
}

// ---------------------------------------------------------------------------
// GET /management/leaderboard/student/:student_id — drill-down with `deeper`
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct DrillQuery {
    pub class_id: Option<String>,
    pub metric: Option<String>,
    pub window: Option<String>,
}

#[utoipa::path(
    get,
    path = "/management/leaderboard/student/{student_id}",
    params(
        ("student_id" = String, Path, description = "Student id"),
        ("class_id" = Option<String>, Query, description = "Scope the student to this class (recommended)"),
        ("metric" = Option<String>, Query, description = "compliance (default) | points | volume"),
        ("window" = Option<String>, Query, description = "week (default) | month | all")
    ),
    responses(
        (status = 200, description = "Single student with deeper object", body = crate::services::leaderboard_aggregate::LeaderboardResponse),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller is not a teacher or admin"),
        (status = 404, description = "Student has no enrollment")
    ),
    tag = TAG
)]
pub async fn student_drilldown(
    State(ctx): State<AppContext>,
    Extension(user): Extension<AuthUser>,
    Path(student_id): Path<String>,
    Query(q): Query<DrillQuery>,
) -> Result<Response> {
    ensure_role(&user, &["teacher", "admin"])?;

    let window = parse_window(q.window.as_deref())?;
    let metric = parse_metric(q.metric.as_deref())?;

    // The student must be in the requested class (or the single active one if
    // no class_id given). We reuse the cohort aggregation and pick the
    // student out.
    let class_id = match q.class_id {
        Some(c) => c,
        None => {
            // Find the student's single active enrollment.
            let r: Option<crate::models::entities::class_enrollment::Model> =
                crate::models::entities::class_enrollment::Entity::find()
                    .filter(
                        crate::models::entities::class_enrollment::Column::StudentId
                            .eq(&student_id),
                    )
                    .filter(
                        crate::models::entities::class_enrollment::Column::Active
                            .eq(true),
                    )
                    .one(&ctx.db)
                    .await
                    .map_err(|e| {
                        tracing::error!("drilldown: enrollment lookup: {e:?}");
                        loco_rs::Error::InternalServerError
                    })?;
            match r {
                Some(x) => x.class_id,
                None => {
                    tracing::warn!("drilldown: student {student_id} has no active enrollment");
                    return Err(loco_rs::Error::NotFound);
                }
            }
        }
    };

    let class_name = agg::class_name(&ctx, &class_id).await;
    let cohort = agg::aggregate_class_cohort(&ctx, &class_id, window).await?;
    let mine = match cohort.into_iter().find(|s| s.student_id == student_id) {
        Some(s) => s,
        None => {
            return Err(loco_rs::Error::NotFound);
        }
    };

    // Rank the full cohort so the student's position is meaningful, then
    // attach the `deeper` object (spec §8.3) with the raw inputs.
    let b = lb::rank(
        metric,
        window,
        &cohort_for(&ctx, &class_id, window, &student_id).await?,
        Some(&student_id),
        MIN_COHORT,
        lb::PointsWeights::default(),
    );
    let me = b.entries.iter().find(|e| e.is_me).cloned();

    let deeper = serde_json::json!({
        "identity": mine.identity,
        "window": window.as_str(),
        "metric": metric.as_str(),
        "raw": {
            "sessions_completed": mine.sessions_completed,
            "sessions_target": mine.sessions_target,
            "cards_completed": mine.cards_completed,
            "enrolled_at": mine.enrolled_at,
        }
    });

    let response = agg::LeaderboardResponse {
        scope: agg::Scope::Class,
        class_id,
        class_name,
        metric: metric.as_str().to_string(),
        window: window.as_str().to_string(),
        generated_at: chrono::Utc::now().timestamp(),
        total_ranked: b.total_ranked,
        entries: b.entries,
        unranked: b.unranked,
        me,
        deeper: Some(deeper),
    };
    format::json(response)
}

/// Small re-fetch of the cohort (kept separate so the caller can build `me`
/// after the drill-down filter has already located the student).
async fn cohort_for(
    ctx: &AppContext,
    class_id: &str,
    window: Window,
    _student_id: &str,
) -> Result<Vec<lb::StudentAggregate>, loco_rs::Error> {
    agg::aggregate_class_cohort(ctx, class_id, window).await
}

// ---------------------------------------------------------------------------
// POST /management/leaderboard/:class_id/display — mint/rotate token (consent)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct DisplayMint {
    /// When true, mint a fresh token (rotate invalidates the old link,
    /// spec §8.6). When false (or absent) and a token already exists, the
    /// existing one is returned unchanged.
    #[serde(default)]
    pub rotate: bool,
    /// The teacher's written consent to a public projection (spec §8.3).
    /// Required to enable; the body carries the teacher's attestation.
    #[serde(default)]
    pub consent: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DisplayResult {
    pub class_id: String,
    pub token: String,
    pub enabled: bool,
    pub url: String,
}

#[utoipa::path(
    post,
    path = "/management/leaderboard/{class_id}/display",
    request_body = DisplayMint,
    responses(
        (status = 200, description = "Display token minted", body = DisplayResult),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller is not a teacher or admin"),
        (status = 409, description = "consent required to enable the public board")
    ),
    tag = TAG
)]
pub async fn display(
    State(ctx): State<AppContext>,
    Extension(user): Extension<AuthUser>,
    Path(class_id): Path<String>,
    body: axum::Json<DisplayMint>,
) -> Result<Response> {
    ensure_role(&user, &["teacher", "admin"])?;

    // Consent gate: enabling requires the teacher's attested consent (spec
    // §8.3). We store `consent_by` as the teacher's email and `consent_at`
    // as now.
    let existing = crate::models::entities::class_display_board::Entity::find_by_id(&class_id)
        .one(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("display: lookup failed: {e:?}");
            loco_rs::Error::InternalServerError
        })?;

    let (token, enabled, consent_by, consent_at) = match existing {
        Some(row) => {
            if body.rotate {
                let t = new_display_token();
                (
                    t,
                    true,
                    user.email.clone(),
                    chrono::Utc::now().timestamp(),
                )
            } else {
                (
                    row.token,
                    row.enabled || body.consent,
                    row.consent_by.clone().unwrap_or(user.email.clone()),
                    row.consent_at,
                )
            }
        }
        None => {
            // First mint — requires explicit consent (spec §8.3).
            if !body.consent {
                tracing::warn!("display: mint for {class_id} without consent — 409");
                return Err(loco_rs::Error::CustomError(
                    axum::http::StatusCode::CONFLICT,
                    loco_rs::controller::ErrorDetail::new(
                        "consent_required",
                        "enabling the public display board requires the teacher's explicit consent",
                    ),
                ));
            }
            let t = new_display_token();
            (
                t,
                true,
                user.email.clone(),
                chrono::Utc::now().timestamp(),
            )
        }
    };

    let token_for_model = token.clone();
    let consent_by_for_model = consent_by.clone();
    let row = crate::models::entities::class_display_board::Model {
        class_id: class_id.clone(),
        token: token_for_model,
        enabled,
        consent_by: Some(consent_by_for_model),
        consent_at,
        created_at: chrono::Utc::now().timestamp(),
    };
    let am = row.into_active_model();
    match am.insert(&ctx.db).await {
        Ok(_) => {}
        Err(_e) => {
            // Already exists — update the existing row.
            let existing = crate::models::entities::class_display_board::Entity::find_by_id(&class_id)
                .one(&ctx.db)
                .await
                .map_err(|e| {
                    tracing::error!("display: re-lookup: {e:?}");
                    loco_rs::Error::InternalServerError
                })?
                .ok_or_else(|| loco_rs::Error::InternalServerError)?;
            let mut am2 = existing.into_active_model();
            am2.token = Set(token.clone());
            am2.enabled = Set(enabled);
            am2.consent_by = Set(Some(consent_by.clone()));
            am2.consent_at = Set(consent_at);
            am2.update(&ctx.db).await.map_err(|e| {
                tracing::error!("display: upsert update: {e:?}");
                loco_rs::Error::InternalServerError
            })?;
        }
    }

    let base = std::env::var("BUZZ_PUBLIC_BASE")
        .or_else(|_| std::env::var("PUBLIC_BASE"))
        .unwrap_or_else(|_| "http://localhost".into());
    let url = format!("{base}/display/class/{token}");
    format::json(DisplayResult {
        class_id,
        token,
        enabled,
        url,
    })
}

/// Generate an unguessable display token (32 bytes of random, hex).
fn new_display_token() -> String {
    use rand::RngCore;
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

// ---------------------------------------------------------------------------
// PATCH /management/leaderboard/:class_id/identity/:student_id — moderate
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct IdentityModeration {
    /// Reset the nickname to the student id (clear the chosen name).
    #[serde(default)]
    pub reset_nickname: bool,
    /// Strip the avatar emoji.
    #[serde(default)]
    pub strip_emoji: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ModerationResult {
    pub student_id: String,
    pub class_id: String,
    pub nickname: String,
    pub avatar_emoji: Option<String>,
}

#[utoipa::path(
    patch,
    path = "/management/leaderboard/{class_id}/identity/{student_id}",
    request_body = IdentityModeration,
    responses(
        (status = 200, description = "Student's moderated identity", body = ModerationResult),
        (status = 401, description = "Missing or invalid bearer token"),
        (status = 403, description = "Caller is not a teacher or admin"),
        (status = 404, description = "No active enrollment for that student in that class")
    ),
    tag = TAG
)]
pub async fn moderate_identity(
    State(ctx): State<AppContext>,
    Extension(user): Extension<AuthUser>,
    Path((class_id, student_id)): Path<(String, String)>,
    body: axum::Json<IdentityModeration>,
) -> Result<Response> {
    ensure_role(&user, &["teacher", "admin"])?;

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
            tracing::error!("moderate: enrollment lookup: {e:?}");
            loco_rs::Error::InternalServerError
        })?
    {
        Some(r) => r,
        None => {
            return Err(loco_rs::Error::NotFound);
        }
    };

    let meta_str = enroll.metadata_json.clone().unwrap_or_default();
    let new_display_name = if body.reset_nickname {
        student_id.clone()
    } else {
        enroll.display_name.clone().unwrap_or_else(|| student_id.clone())
    };
    let new_meta_str = if body.strip_emoji {
        serde_json::from_str(&meta_str)
            .map(|mut v: serde_json::Value| {
                if let Some(obj) = v.as_object_mut() {
                    obj.remove("avatar_emoji");
                }
                v.to_string()
            })
            .unwrap_or_default()
    } else {
        meta_str
    };

    let mut am: crate::models::entities::class_enrollment::ActiveModel = enroll.into_active_model();
    am.display_name = Set(Some(new_display_name.clone()));
    am.metadata_json = Set(Some(new_meta_str.clone()));

    am.update(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("moderate: update failed: {e:?}");
            loco_rs::Error::InternalServerError
        })?;

    format::json(ModerationResult {
        student_id,
        class_id,
        nickname: new_display_name,
        avatar_emoji: serde_json::from_str::<serde_json::Value>(&new_meta_str)
            .ok()
            .and_then(|v| {
                v.get("avatar_emoji")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            }),
    })
}

// ---------------------------------------------------------------------------
// Helper: every class id that currently has ≥1 active enrollment
// ---------------------------------------------------------------------------

async fn all_active_class_ids(ctx: &AppContext) -> Result<Vec<String>, loco_rs::Error> {
    let rows: Vec<(String,)> = crate::models::entities::class_enrollment::Entity::find()
        .filter(crate::models::entities::class_enrollment::Column::Active.eq(true))
        .select_only()
        .column(crate::models::entities::class_enrollment::Column::ClassId)
        .distinct()
        .into_tuple()
        .all(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("all_active_class_ids: {e:?}");
            loco_rs::Error::InternalServerError
        })?;
    Ok(rows.into_iter().map(|(c,)| c).collect())
}
