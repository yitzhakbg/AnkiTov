//! Compliance Tracking controller — sessions completed vs. N per student.
//!
//! Provides instructors and administrators with per-student, per-week
//! compliance data: how many sessions each student completed vs. their
//! assigned N value. This is the primary metric for:
//! - Identifying students falling behind
//! - Verifying institutional adoption compliance
//! - Triggering instructor interventions

use crate::models::entities::capsule_session as cs_entity;
use crate::models::management::ApiResponse;
use axum::extract::{Path, Query, State};
use loco_rs::{app::AppContext, prelude::*};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

const TAG: &str = "Management Console";

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/management/compliance")
        .add("/weekly", get(weekly_compliance))
        .add("/student/:student_id", get(student_compliance))
        .add("/students", get(list_students))
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Per-student compliance record for one ISO week.
#[derive(Debug, Serialize, ToSchema)]
pub struct StudentComplianceRecord {
    pub student_id: String,
    pub session_week: String,
    pub sessions_completed: i32,
    pub sessions_target: i32,
    pub compliance_pct: f64,
    pub total_cards_reviewed: i32,
    pub avg_cards_per_session: f64,
}

/// Query params for weekly compliance report.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ComplianceQuery {
    /// ISO week (e.g., "2026-W27"). Defaults to current week.
    pub session_week: Option<String>,
    /// Only show students below this compliance threshold (0.0—1.0).
    #[serde(default)]
    pub below_threshold: Option<f64>,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
}

fn default_page() -> u32 { 1 }
fn default_per_page() -> u32 { 50 }

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Get weekly compliance for all students (or filtered by threshold).
///
/// Returns each student's completed sessions vs. their N target for the
/// given ISO week. Students below the compliance threshold can be
/// filtered to surface only those needing intervention.
#[utoipa::path(
    get,
    path = "/management/compliance/weekly",
    params(
        ("session_week" = Option<String>, Query, description = "ISO week (default: current)"),
        ("below_threshold" = Option<f64>, Query, description = "Filter below compliance threshold"),
        ("page" = Option<u32>, Query),
        ("per_page" = Option<u32>, Query)
    ),
    responses(
        (status = 200, description = "Weekly compliance records")
    ),
    tag = TAG
)]
pub async fn weekly_compliance(
    State(ctx): State<AppContext>,
    Query(params): Query<ComplianceQuery>,
) -> Result<Response> {
    let week = params.session_week.unwrap_or_else(|| {
        use chrono::Datelike;
        let now = chrono::Utc::now();
        let iso = now.date_naive().iso_week();
        format!("{}-W{:02}", iso.year(), iso.week())
    });

    tracing::info!("Computing weekly compliance for {}", week);

    // Query all completed sessions for this week, grouped by student
    let sessions = cs_entity::Entity::find()
        .filter(cs_entity::Column::SessionWeek.eq(&week))
        .filter(
            cs_entity::Column::Status
                .eq("completed")
                .or(cs_entity::Column::Status.eq("in_progress")),
        )
        .order_by_asc(cs_entity::Column::StudentId)
        .all(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("weekly_compliance DB error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    // Aggregate by student
    let mut student_records: std::collections::HashMap<String, StudentComplianceRecord> =
        std::collections::HashMap::new();

    for s in &sessions {
        let record = student_records
            .entry(s.student_id.clone())
            .or_insert_with(|| StudentComplianceRecord {
                student_id: s.student_id.clone(),
                session_week: week.clone(),
                sessions_completed: 0,
                sessions_target: s.n_value,
                compliance_pct: 0.0,
                total_cards_reviewed: 0,
                avg_cards_per_session: 0.0,
            });

        record.sessions_completed += 1;
        record.total_cards_reviewed += s.cards_completed;
    }

    // Compute compliance percentages
    for record in student_records.values_mut() {
        if record.sessions_target > 0 {
            record.compliance_pct =
                record.sessions_completed as f64 / record.sessions_target as f64;
        }
        if record.sessions_completed > 0 {
            record.avg_cards_per_session =
                record.total_cards_reviewed as f64 / record.sessions_completed as f64;
        }
    }

    let mut results: Vec<StudentComplianceRecord> = student_records.into_values().collect();
    results.sort_by(|a, b| a.compliance_pct.partial_cmp(&b.compliance_pct).unwrap_or(std::cmp::Ordering::Equal));

    // Apply threshold filter
    if let Some(threshold) = params.below_threshold {
        results.retain(|r| r.compliance_pct < threshold);
    }

    // Paginate
    let page = params.page.max(1);
    let per_page = params.per_page.max(1);
    let start = ((page - 1) as usize * per_page as usize).min(results.len());
    let end = (start + per_page as usize).min(results.len());
    let page_results = &results[start..end];

    format::json(serde_json::json!({
        "week": week,
        "total_students": results.len(),
        "page": page,
        "per_page": per_page,
        "records": page_results,
    }))
}

/// List distinct student IDs for progressive search.
#[utoipa::path(
    get,
    path = "/management/compliance/students",
    description = "List distinct student IDs. Supports optional `q` search query.",
    responses(
        (status = 200, description = "List of student IDs", body = Vec<String>)
    ),
    tag = TAG
)]
pub async fn list_students(
    State(ctx): State<AppContext>,
    Query(params): Query<StudentSearchQuery>,
) -> Result<Response> {
    tracing::info!("Listing students (search: {:?})", params.q);

    use sea_orm::QuerySelect;

    let mut query = cs_entity::Entity::find()
        .select_only()
        .column(cs_entity::Column::StudentId)
        .distinct()
        .order_by_asc(cs_entity::Column::StudentId);

    if let Some(ref q) = params.q {
        let pattern = format!("%{}%", q);
        query = query.filter(cs_entity::Column::StudentId.like(&pattern));
    }

    let students: Vec<String> = query
        .into_tuple()
        .all(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("list_students DB error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    format::json(students)
}

/// Query params for student search.
#[derive(Debug, Deserialize, ToSchema)]
pub struct StudentSearchQuery {
    /// Optional search query to filter student IDs.
    pub q: Option<String>,
}

/// Get compliance history for a specific student across all weeks.
#[utoipa::path(
    get,
    path = "/management/compliance/student/{student_id}",
    params(("student_id" = String, Path, description = "Student ID")),
    responses(
        (status = 200, description = "Student compliance history")
    ),
    tag = TAG
)]
pub async fn student_compliance(
    State(ctx): State<AppContext>,
    Path(student_id): Path<String>,
) -> Result<Response> {
    tracing::info!("Fetching compliance history for student {}", student_id);

    let sessions = cs_entity::Entity::find()
        .filter(cs_entity::Column::StudentId.eq(&student_id))
        .order_by_desc(cs_entity::Column::SessionWeek)
        .all(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("student_compliance DB error: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;

    // Aggregate by week
    let mut week_map: std::collections::HashMap<String, StudentComplianceRecord> =
        std::collections::HashMap::new();

    for s in &sessions {
        let record = week_map
            .entry(s.session_week.clone())
            .or_insert_with(|| StudentComplianceRecord {
                student_id: student_id.clone(),
                session_week: s.session_week.clone(),
                sessions_completed: 0,
                sessions_target: s.n_value,
                compliance_pct: 0.0,
                total_cards_reviewed: 0,
                avg_cards_per_session: 0.0,
            });
        record.sessions_completed += 1;
        record.total_cards_reviewed += s.cards_completed;
        record.sessions_target = s.n_value; // most recent N wins per week
    }

    for record in week_map.values_mut() {
        if record.sessions_target > 0 {
            record.compliance_pct =
                record.sessions_completed as f64 / record.sessions_target as f64;
        }
        if record.sessions_completed > 0 {
            record.avg_cards_per_session =
                record.total_cards_reviewed as f64 / record.sessions_completed as f64;
        }
    }

    let mut history: Vec<StudentComplianceRecord> = week_map.into_values().collect();
    history.sort_by(|a, b| b.session_week.cmp(&a.session_week));

    format::json(history)
}