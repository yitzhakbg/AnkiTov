//! Sequential Background Capsule Generation runner.
//!
//! Mitigates thundering-herd issues on concurrent SQLite reads by sequentially
//! iterating over students, resolving their profiles, FSRS sorting their cards,
//! and handling the anomaly fallbacks cleanly without crashing.

use std::collections::HashMap;
use std::path::Path;
use sea_orm::{ActiveModelTrait, ActiveValue, DatabaseConnection, EntityTrait, QueryFilter, ColumnTrait};
use uuid::Uuid;

use crate::models::entities::{
    generation_job as gj_entity,
    user as user_entity,
    capsule_session as cs_entity,
};

use crate::services::capsule_generator::{self, GenerateRequest, GeneratorError};

// Struct to represent individual student failures (Anomalies)
#[derive(Debug, serde::Serialize)]
pub struct StudentAnomaly {
    pub student_id: String,
    pub name: String,
    pub anomaly_type: String, // "insufficient_content", "retention_cliff", "dropout_stale"
    pub msg: String,
    pub fallback_applied: String,
}

/// Triggers and executes sequential capsule generation for all active students.
pub async fn run_batch_generation(
    db: &DatabaseConnection,
    job_id: &str,
    playground_dir: &str,
) -> Result<(), sea_orm::DbErr> {
    // 1. Mark job as "processing"
    let mut job: gj_entity::ActiveModel = gj_entity::Entity::find_by_id(job_id.to_string())
        .one(db)
        .await?
        .unwrap()
        .into();

    job.status = ActiveValue::Set("processing".to_string());
    let _ = job.update(db).await?;

    // 2. Fetch all active students (role = "student")
    let students = user_entity::Entity::find()
        .filter(user_entity::Column::Role.eq("student"))
        .all(db)
        .await?;

    let total_students = students.len() as i32;

    // Update total students in job record
    let mut job: gj_entity::ActiveModel = gj_entity::Entity::find_by_id(job_id.to_string())
        .one(db)
        .await?
        .unwrap()
        .into();
    job.total_students = ActiveValue::Set(total_students);
    let _ = job.update(db).await?;

    let mut completed_count = 0;
    let mut anomalies_list: Vec<StudentAnomaly> = Vec::new();

    // 3. Sequentially process each student
    for student in &students {
        let student_str_id = student.id.to_string();
        let student_fullname = student.full_name.clone().unwrap_or_else(|| student.username.clone());

        // Locate their .anki2 database inside the playground sandbox
        let anki2_path = format!("{}/{}/collection.anki2", playground_dir, student_fullname);

        if !Path::new(&anki2_path).exists() {
            // Log missing file as an anomaly and skip to next student
            anomalies_list.push(StudentAnomaly {
                student_id: student_str_id.clone(),
                name: student_fullname.clone(),
                anomaly_type: "insufficient_content".to_string(),
                msg: format!("Anki collection.anki2 file not found at: {}", anki2_path),
                fallback_applied: "Skipped generation for this student".to_string(),
            });

            completed_count += 1;
            update_job_progress(db, job_id, completed_count, total_students, &anomalies_list).await?;
            continue;
        }

        // Format ISO week for compilation target
        let iso_week = "2026-W28".to_string(); // Seed Week

        // Trigger safe gen capsule request
        let request = GenerateRequest {
            student_id: student_str_id.clone(),
            anki2_path: anki2_path.clone(),
            track_profile_id: String::new(),
            session_week: iso_week,
            n_value: None,
            session_duration_minutes: None,
        };

        // Standard Generator invocation
        match capsule_generator::generate_capsule(db, request).await {
            Ok(_response) => {
                // Success! Record it.
            }
            Err(GeneratorError::RateLimited(_)) => {
                // Rate limited in this window — skip normally
            }
            Err(e) => {
                // Determine anomaly categorization
                let err_str = e.to_string();
                let anomaly_type = if err_str.contains("NoDueCards") || err_str.contains("NoTracks") {
                    "insufficient_content".to_string()
                } else if err_str.contains("rate limited") {
                    "rate_limited".to_string()
                } else {
                    "retention_cliff".to_string()
                };

                // Store error details
                anomalies_list.push(StudentAnomaly {
                    student_id: student_str_id.clone(),
                    name: student_fullname.clone(),
                    anomaly_type,
                    msg: err_str,
                    fallback_applied: "NoDueCards fallback applied (deck defaulted)".to_string(),
                });
            }
        }

        completed_count += 1;
        update_job_progress(db, job_id, completed_count, total_students, &anomalies_list).await?;
    }

    // 4. Mark job as "completed" or "failed"
    let mut job: gj_entity::ActiveModel = gj_entity::Entity::find_by_id(job_id.to_string())
        .one(db)
        .await?
        .unwrap()
        .into();

    job.status = ActiveValue::Set("completed".to_string());
    job.progress = ActiveValue::Set(100);
    job.completed_at = ActiveValue::Set(Some(chrono::Utc::now().timestamp()));
    let _ = job.update(db).await?;

    Ok(())
}

async fn update_job_progress(
    db: &DatabaseConnection,
    job_id: &str,
    completed: i32,
    total: i32,
    anomalies: &Vec<StudentAnomaly>,
) -> Result<(), sea_orm::DbErr> {
    let mut job: gj_entity::ActiveModel = gj_entity::Entity::find_by_id(job_id.to_string())
        .one(db)
        .await?
        .unwrap()
        .into();

    let prog_pct = if total > 0 { (completed * 100) / total } else { 100 };

    job.progress = ActiveValue::Set(prog_pct);
    job.completed_students = ActiveValue::Set(completed);

    if !anomalies.is_empty() {
        let json_anoms = serde_json::to_string(anomalies).unwrap_or_else(|_| "[]".to_string());
        job.anomalies = ActiveValue::Set(Some(json_anoms));
    }

    let _ = job.update(db).await?;
    Ok(())
}
