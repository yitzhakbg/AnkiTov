//! Audit Logger — immutable append-only event ledger.
//!
//! Every N-change, profile assignment/revocation, capsule generation, and
//! capsule completion is recorded. This forms the audit trail for:
//! - Payment/billing verification
//! - Instructor activity review
//! - Compliance reporting
//!
//! Writes are fire-and-forget from callers — errors are logged but do not
//! block the primary operation.

use sea_orm::{ActiveModelTrait, ActiveValue, DatabaseConnection};

use crate::models::entities::audit_log;

/// Record an event in the audit ledger.
///
/// # Arguments
/// * `db` — SeaORM database connection.
/// * `event_type` — Category: `"n_change"`, `"profile_assign"`, `"profile_revoke"`,
///   `"capsule_generate"`, `"capsule_complete"`.
/// * `event_data` — JSON blob with event-specific details.
/// * `actor_id` — Who triggered it (instructor UUID, `"system"`, or student UUID).
/// * `target_id` — Who/what was affected (student UUID, cohort ID, or None).
pub async fn log_event(
    db: &DatabaseConnection,
    event_type: &str,
    event_data: &str,
    actor_id: &str,
    target_id: Option<&str>,
) {
    let id = uuid::Uuid::new_v4().to_string();
    let ts = now_ts();

    let entry = audit_log::ActiveModel {
        id: ActiveValue::Set(id.clone()),
        event_type: ActiveValue::Set(event_type.to_string()),
        event_data: ActiveValue::Set(event_data.to_string()),
        actor_id: ActiveValue::Set(actor_id.to_string()),
        target_id: ActiveValue::Set(target_id.map(|s| s.to_string())),
        created_at: ActiveValue::Set(ts),
    };

    match entry.insert(db).await {
        Ok(_) => {
            tracing::debug!(
                "Audit log: id={} type={} actor={} target={:?}",
                id,
                event_type,
                actor_id,
                target_id,
            );
        }
        Err(e) => {
            tracing::error!(
                "Audit log write failed (non-blocking): id={} error={:?}",
                id,
                e,
            );
        }
    }
}

/// Convenience: log an N-value change.
pub async fn log_n_change(
    db: &DatabaseConnection,
    actor_id: &str,
    cohort_id: Option<&str>,
    old_n: i32,
    new_n: i32,
) {
    let data = serde_json::json!({
        "old_n": old_n,
        "new_n": new_n,
        "cohort_id": cohort_id,
    });
    log_event(
        db,
        "n_change",
        &data.to_string(),
        actor_id,
        cohort_id,
    )
    .await;
}

/// Convenience: log a profile assignment event.
pub async fn log_profile_assign(
    db: &DatabaseConnection,
    actor_id: &str,
    student_id: &str,
    profile_id: &str,
    profile_name: &str,
) {
    let data = serde_json::json!({
        "profile_id": profile_id,
        "profile_name": profile_name,
        "student_id": student_id,
    });
    log_event(
        db,
        "profile_assign",
        &data.to_string(),
        actor_id,
        Some(student_id),
    )
    .await;
}

/// Convenience: log a profile revocation event.
pub async fn log_profile_revoke(
    db: &DatabaseConnection,
    actor_id: &str,
    student_id: &str,
    profile_id: &str,
    profile_name: &str,
) {
    let data = serde_json::json!({
        "profile_id": profile_id,
        "profile_name": profile_name,
        "student_id": student_id,
    });
    log_event(
        db,
        "profile_revoke",
        &data.to_string(),
        actor_id,
        Some(student_id),
    )
    .await;
}

/// Convenience: log a capsule generation event.
pub async fn log_capsule_generate(
    db: &DatabaseConnection,
    student_id: &str,
    profile_id: &str,
    capsule_size: i32,
    n_value: i32,
    session_id: &str,
) {
    let data = serde_json::json!({
        "session_id": session_id,
        "profile_id": profile_id,
        "student_id": student_id,
        "capsule_size": capsule_size,
        "n_value": n_value,
    });
    log_event(
        db,
        "capsule_generate",
        &data.to_string(),
        "system",
        Some(student_id),
    )
    .await;
}

/// Convenience: log a capsule completion event.
pub async fn log_capsule_complete(
    db: &DatabaseConnection,
    student_id: &str,
    session_id: &str,
    cards_reviewed: i32,
    status: &str,
) {
    let data = serde_json::json!({
        "session_id": session_id,
        "cards_reviewed": cards_reviewed,
        "completion_status": status,
    });
    log_event(
        db,
        "capsule_complete",
        &data.to_string(),
        student_id,
        Some(student_id),
    )
    .await;
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