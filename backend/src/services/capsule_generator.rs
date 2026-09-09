//! Capsule Generator — server-side pipeline that produces a single interleaved
//! "Remediation Capsule" per student from a customized cocktail of prerequisite
//! tracks.
//!
//! ## Pipeline Flow
//!
//! ```text
//! resolve profile ──→ query due cards ──→ FSRS sort ──→ slice ──→ persist
//! ```
//!
//! 1. Resolve the student's TrackProfile (N, session_duration, track tags) via SeaORM.
//! 2. Query the `.anki2` SQLite database for all overdue cards matching assigned
//!    track tags (`fsrs_sort::fetch_and_sort`).
//! 3. Slice the ranked list to the computed capsule size (`capsule_slicer`).
//! 4. Persist a `CapsuleSession` entity with status `pending`.
//! 5. Return the capsule card IDs so the PWA can deliver them to Anki.
//!
//! ## Configuration
//!
//! The generator reads from:
//! - SeaORM (libSQL): track profiles, tracks, capsule sessions
//! - SQLite (.anki2): card data, revlog timing history (via `fsrs_sort`)
//!
//! All writes go through SeaORM. The `.anki2` file is only read, never written.

use std::path::Path;

use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter};

use crate::models::entities::{
    capsule_session as cs_entity,
    profile_assignment as pa_entity,
    track as track_entity,
    track_profile as tp_entity,
    track_profile_track as tpt_entity,
    user as user_entity,
};

use crate::services::audit_logger;
use crate::services::capsule_slicer;
use crate::services::fsrs_sort;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum GeneratorError {
    #[error("database error: {0}")]
    Database(#[from] sea_orm::DbErr),

    #[error("fsrs sort error: {0}")]
    FsrsSort(#[from] fsrs_sort::FsrsSortError),

    #[error("no track profile assigned for student: {0}")]
    NoProfileForStudent(String),

    #[error("no tracks assigned to profile: {0}")]
    NoTracksInProfile(String),

    #[error("profile not found: {0}")]
    ProfileNotFound(String),

    #[error("no due cards found for the assigned tracks")]
    NoDueCards,

    #[error("rate limited: student {0} already had a session in the last 30 minutes")]
    RateLimited(String),

    #[error("serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// Input to the capsule generator.
#[derive(Debug, Clone)]
pub struct GenerateRequest {
    /// Student identifier (maps to user ID in the system).
    pub student_id: String,
    /// Path to the student's `.anki2` collection file.
    pub anki2_path: String,
    /// Track profile ID to use for capsule generation.
    pub track_profile_id: String,
    /// ISO week identifier (e.g., "2026-W27") for compliance tracking.
    pub session_week: String,
    /// N-value override (sessions per week).
    pub n_value: Option<i32>,
    /// Override session duration in minutes (falls back to profile default).
    pub session_duration_minutes: Option<i32>,
}

/// Output from the capsule generator — what the PWA needs to display.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GenerateResponse {
    /// Capsule session ID (UUID).
    pub session_id: String,
    /// Card IDs in review order (most urgent first).
    pub card_ids: Vec<i64>,
    /// Number of cards in this capsule.
    pub capsule_size: usize,
    /// Current N value (sessions per week).
    pub n_value: i32,
    /// Session duration in minutes.
    pub session_duration_minutes: i32,
    /// Status of the capsule.
    pub status: String,
}

// ---------------------------------------------------------------------------
// Profile Resolution Helper — Hybrid Assignment Resolver
// ---------------------------------------------------------------------------

/// Resolves the correct TrackProfile ID for a student using the Hybrid lookup:
/// 1. Student Override (target_type = "student", target_id = student_id)
/// 2. Cohort Default (target_type = "cohort", target_id = user.subsection_id)
/// 3. Fallback (First available TrackProfile)
pub async fn resolve_profile_for_student(
    db: &DatabaseConnection,
    student_id: &str,
) -> Result<String, GeneratorError> {
    // 1. Check Student Override
    let student_override = pa_entity::Entity::find()
        .filter(pa_entity::Column::TargetType.eq("student"))
        .filter(pa_entity::Column::TargetId.eq(student_id))
        .one(db)
        .await
        .map_err(GeneratorError::Database)?;

    if let Some(assignment) = student_override {
        return Ok(assignment.track_profile_id);
    }

    // 2. Check Cohort Default (requires fetching student's user record)
    let id_val = student_id.parse::<i64>().unwrap_or(0);
    let student_user = user_entity::Entity::find_by_id(id_val)
        .one(db)
        .await
        .map_err(GeneratorError::Database)?;

    if let Some(user) = student_user {
        if let Some(cohort_id) = user.subsection_id {
            let cohort_default = pa_entity::Entity::find()
                .filter(pa_entity::Column::TargetType.eq("cohort"))
                .filter(pa_entity::Column::TargetId.eq(&cohort_id))
                .one(db)
                .await
                .map_err(GeneratorError::Database)?;

            if let Some(assignment) = cohort_default {
                return Ok(assignment.track_profile_id);
            }
        }
    }

    // 3. Fallback: Legacy "First Profile Wins"
    let junctions = tpt_entity::Entity::find()
        .all(db)
        .await
        .map_err(GeneratorError::Database)?;

    if let Some(j) = junctions.first() {
        return Ok(j.track_profile_id.clone());
    }

    Err(GeneratorError::NoProfileForStudent(student_id.to_string()))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Generate a remediation capsule for a student.
///
/// This is the main entry point for the Interleaved Mastery Pipeline (Phase 3).
/// It resolves the student's assigned track profile, queries overdue cards from
/// their `.anki2` database, ranks them by FSRS urgency, slices to the optimal
/// capsule size, and persists a `CapsuleSession` record.
///
/// # Rate Limiting
///
/// Enforces a 30-minute cooldown: at most 1 capsule per student per
/// 30-minute window. Returns [`GeneratorError::RateLimited`] if a
/// `pending` or `in_progress` session exists within that window.
pub async fn generate_capsule(
    db: &DatabaseConnection,
    request: GenerateRequest,
) -> Result<GenerateResponse, GeneratorError> {
    let student_id = &request.student_id;
    let session_week = if request.session_week.is_empty() {
        &current_iso_week()
    } else {
        &request.session_week
    };

    // ── Step 0: Rate-limit guard (30-minute window) ──────────────────
    // Max 1 capsule per student per 30-minute window per the plan spec.
    let thirty_min_ago = now_ts() - 1800;
    let existing = cs_entity::Entity::find()
        .filter(cs_entity::Column::StudentId.eq(student_id))
        .filter(cs_entity::Column::CreatedAt.gte(thirty_min_ago))
        .filter(
            cs_entity::Column::Status
                .eq("pending")
                .or(cs_entity::Column::Status.eq("in_progress")),
        )
        .count(db)
        .await
        .map_err(GeneratorError::Database)?;

    if existing > 0 {
        return Err(GeneratorError::RateLimited(student_id.clone()));
    }

    // ── Step 1: Resolve TrackProfile ──────────────────────────────────
    // Direct profile ID takes precedence over student assignment lookup
    let profile_id = if !request.track_profile_id.is_empty() {
        request.track_profile_id.clone()
    } else {
        resolve_profile_for_student(db, student_id).await?
    };

    let profile = tp_entity::Entity::find_by_id(&profile_id)
        .one(db)
        .await
        .map_err(GeneratorError::Database)?
        .ok_or_else(|| GeneratorError::ProfileNotFound(profile_id.clone()))?;

    // ── Step 2: Get assigned track tags ──────────────────────────────
    let track_junctions = tpt_entity::Entity::find()
        .filter(tpt_entity::Column::TrackProfileId.eq(&profile_id))
        .all(db)
        .await
        .map_err(GeneratorError::Database)?;

    if track_junctions.is_empty() {
        return Err(GeneratorError::NoTracksInProfile(profile_id));
    }

    let track_ids: Vec<String> = track_junctions.iter().map(|j| j.track_id.clone()).collect();
    let tracks = track_entity::Entity::find()
        .filter(track_entity::Column::Id.is_in(track_ids))
        .all(db)
        .await
        .map_err(GeneratorError::Database)?;

    let track_tags: Vec<String> = tracks.iter().map(|t| t.tag.clone()).collect();

    // ── Step 3: Query & sort due cards via FSRS ──────────────────────
    let anki2_path = Path::new(&request.anki2_path);
    let sort_result = fsrs_sort::fetch_and_sort(anki2_path, &track_tags, 30)?;

    if sort_result.ranked.is_empty() {
        return Err(GeneratorError::NoDueCards);
    }

    // ── Step 4: Compute capsule size ─────────────────────────────────
    let n_value = request.n_value
        .or(profile.n_value)
        .unwrap_or(capsule_slicer::DEFAULT_N_VALUE);
    let session_duration = match request.session_duration_minutes {
        Some(dur) if dur > 0 => dur,
        _ => {
            if profile.session_duration_minutes > 0 {
                profile.session_duration_minutes
            } else {
                capsule_slicer::DEFAULT_SESSION_DURATION_MINUTES
            }
        }
    };

    let pool_size = sort_result.ranked.len();
    let num_tracks = sort_result.track_counts.len();
    let avg_seconds = sort_result.avg_seconds_per_card;

    let target_size = capsule_slicer::compute_capsule_size(
        pool_size,
        n_value,
        capsule_slicer::DEFAULT_HARD_CAP,
        session_duration,
        avg_seconds,
        num_tracks,
    );

    // ── Step 5: Slice ────────────────────────────────────────────────
    let ranked_pairs: Vec<(i64, String)> = sort_result
        .ranked
        .iter()
        .map(|c| (c.card_id, c.track_tag.clone()))
        .collect();

    let slice = capsule_slicer::slice_capsule(
        &ranked_pairs,
        &sort_result.track_counts,
        target_size,
    );

    // ── Step 6: Persist CapsuleSession ───────────────────────────────
    let session_id = uuid::Uuid::new_v4().to_string();
    let card_ids_json = serde_json::to_string(&slice.selected)?;
    let ts = now_ts();

    let session = cs_entity::ActiveModel {
        id: ActiveValue::Set(session_id.clone()),
        student_id: ActiveValue::Set(student_id.clone()),
        track_profile_id: ActiveValue::Set(profile_id.clone()),
        n_value: ActiveValue::Set(n_value),
        session_duration_minutes: ActiveValue::Set(session_duration),
        card_ids: ActiveValue::Set(card_ids_json),
        status: ActiveValue::Set("pending".to_string()),
        session_week: ActiveValue::Set(session_week.clone()),
        cards_completed: ActiveValue::Set(0),
        cards_total: ActiveValue::Set(slice.selected.len() as i32),
        created_at: ActiveValue::Set(ts),
        completed_at: ActiveValue::Set(None),
    }
    .insert(db)
    .await
    .map_err(GeneratorError::Database)?;

    // Audit: log capsule generation
    audit_logger::log_capsule_generate(
        db,
        student_id,
        &profile_id,
        session.cards_total,
        n_value,
        &session.id,
    )
    .await;

    tracing::info!(
        "Capsule generated: session={}, student={}, cards={}, N={}, duration={}min",
        session.id,
        student_id,
        session.cards_total,
        n_value,
        session_duration,
    );

    Ok(GenerateResponse {
        session_id: session.id.clone(),
        card_ids: slice.selected,
        capsule_size: slice.actual_size,
        n_value,
        session_duration_minutes: session_duration,
        status: session.status.clone(),
    })
}

/// Compute the current ISO week string (e.g., "2026-W27").
pub fn current_iso_week() -> String {
    use chrono::Datelike;
    let now = chrono::Utc::now();
    let iso = now.date_naive().iso_week();
    format!("{}-W{:02}", iso.year(), iso.week())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_iso_week_format() {
        let week = current_iso_week();
        // Should match pattern YYYY-WNN
        assert!(week.len() >= 7);
        assert!(week.contains("-W"));
    }
}