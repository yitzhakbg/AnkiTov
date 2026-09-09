//! Student import endpoint — scans AnkiPlayGround for student profiles.
//!
//! Each subdirectory in AnkiPlayGround (containing collection.anki2) is treated
//! as a student profile. The directory name becomes the student's display name
//! and a slug is generated as the student ID.

use axum::extract::{Query, State};
use axum::Json;
use axum::extract::Multipart;
use loco_rs::{app::AppContext, prelude::*};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use std::path::Path;
use utoipa::ToSchema;

use crate::models::entities::{class_enrollment, deck};
use crate::services;


const TAG: &str = "Management Console";

/// Query params for scan-playground endpoint.
#[derive(Debug, Deserialize)]
pub struct ScanQuery {
    pub path: Option<String>,
}

/// Response entry for a found student profile.
#[derive(Debug, Serialize, ToSchema)]
pub struct StudentProfileEntry {
    pub name: String,
    pub student_id: String,
    pub display_name: String,
    pub anki2_path: String,
    pub has_collection: bool,
}

/// Request to enroll imported students into a class.
#[derive(Debug, Deserialize, ToSchema)]
pub struct BulkEnrollRequest {
    pub class_id: String,
    pub student_ids: Vec<String>,
    pub display_names: Option<Vec<String>>,
}

/// Routes for student import.
pub fn routes() -> Routes {
    Routes::new()
        .add("/management/students/scan-playground", get(scan_playground))
        .add("/management/students/bulk-enroll", post(bulk_enroll))
        .add("/management/students/create-profiles", post(create_profiles))
        .add("/management/students/populate-decks", post(populate_decks))
        .add("/management/students/import-file", post(import_file))
}

/// Scan AnkiPlayGround directory and return all student profiles found.
#[utoipa::path(
    get,
    path = "/management/students/scan-playground",
    responses(
        (status = 200, description = "List of student profiles found", body = Vec<StudentProfileEntry>)
    ),
    tag = TAG
)]
pub async fn scan_playground(
    State(_ctx): State<AppContext>,
    Query(params): Query<ScanQuery>,
) -> Result<Response> {
    let playground_path = params.path.unwrap_or_else(|| {
        // Prefer ANKITOV_PROFILES_ROOT env var, fall back to hardcoded default
        std::env::var("ANKITOV_PROFILES_ROOT")
            .unwrap_or_else(|_| "/Volumes/YBG1TB4Mac/AnkiTov/AnkiPlayGround".to_string())
    });
    let dir = Path::new(&playground_path);

    if !dir.exists() || !dir.is_dir() {
        return format::json(Vec::<StudentProfileEntry>::new());
    }

    let mut profiles = Vec::new();
    let skip = ["addons21", "logs", "User 1"];

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if skip.contains(&name.as_str()) || name.starts_with('.') || name == "README.txt" {
                continue;
            }
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }

            let student_id = name.to_lowercase().replace(' ', "-");
            let display_name = name.split('-').map(|w| {
                let mut c = w.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().to_string() + c.as_str(),
                    None => String::new()
                }
            }).collect::<Vec<_>>().join(" ");
            let collection_path = entry.path().join("collection.anki2");
            let has_collection = collection_path.exists();

            profiles.push(StudentProfileEntry {
                name: name.clone(),
                student_id,
                display_name: display_name.clone(),
                anki2_path: collection_path.to_string_lossy().to_string(),
                has_collection,
            });
        }
    }

    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    format::json(profiles)
}

/// Bulk-enroll students into a class.
#[utoipa::path(
    post,
    path = "/management/students/bulk-enroll",
    request_body = BulkEnrollRequest,
    responses(
        (status = 200, description = "Students enrolled")
    ),
    tag = TAG
)]
pub async fn bulk_enroll(
    State(ctx): State<AppContext>,
    Json(payload): Json<BulkEnrollRequest>,
) -> Result<Response> {
    let db = &ctx.db;
    let now = chrono::Utc::now().timestamp();
    let mut enrolled = 0;
    let mut skipped = 0;

    for (i, student_id) in payload.student_ids.iter().enumerate() {
        let display_name = payload.display_names.as_ref().and_then(|names| names.get(i).cloned());
        // Try direct insert — if it fails due to unique constraint, skip
        let active_model = crate::models::entities::class_enrollment::ActiveModel {
            id: sea_orm::ActiveValue::NotSet,
            class_id: sea_orm::ActiveValue::Set(payload.class_id.clone()),
            student_id: sea_orm::ActiveValue::Set(student_id.clone()),
            enrolled_at: sea_orm::ActiveValue::Set(now),
            active: sea_orm::ActiveValue::Set(true),
            display_name: sea_orm::ActiveValue::Set(display_name),
            metadata_json: sea_orm::ActiveValue::Set(None),
        };

        match crate::models::entities::class_enrollment::Entity::insert(active_model)
            .exec(db)
            .await
        {
            Ok(_) => enrolled += 1,
            Err(e) => {
                tracing::warn!("Enroll skip for {}: {:?}", student_id, e);
                skipped += 1;
            }
        }
    }

    format::json(serde_json::json!({
        "enrolled": enrolled,
        "skipped": skipped,
        "total": payload.student_ids.len()
    }))
}

/// Request to create Anki profile directories.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateProfilesRequest {
    pub names: Vec<String>,
    pub base_path: Option<String>,
    pub copy_demo_collection: Option<bool>,
}

/// Request to bulk-populate decks into student profiles.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PopulateDecksRequest {
    pub class_id: String,
    pub deck_ids: Option<Vec<String>>,
}

/// Request to import students from file.
#[derive(Debug, Deserialize)]
pub struct ImportFileQuery {
    pub class_id: String,
    pub delimiter: Option<char>,
}

/// Response for file import endpoint.
#[derive(Debug, Serialize, ToSchema)]
pub struct ImportFileResponse {
    pub enrolled: usize,
    pub skipped: usize,
    pub errors: Vec<ImportError>,
    #[schema(value_type = Vec<Object>)]
    pub students: Vec<serde_json::Value>,
}

/// Error during file import.
#[derive(Debug, Serialize, ToSchema)]
pub struct ImportError {
    pub row: usize,
    pub reason: String,
}

/// Successfully imported student.
#[derive(Debug, Serialize, ToSchema)]
pub struct ImportedStudent {
    pub student_id: String,
    pub display_name: String,
}

/// Create Anki profile directories for a list of student names.
#[utoipa::path(
    post,
    path = "/management/students/create-profiles",
    request_body = CreateProfilesRequest,
    responses((status = 200, description = "Profiles created")),
    tag = TAG
)]
pub async fn create_profiles(
    State(_ctx): State<AppContext>,
    Json(payload): Json<CreateProfilesRequest>,
) -> Result<Response> {
    let base_path = payload.base_path.unwrap_or_else(|| {
        std::env::var("ANKITOV_PROFILES_ROOT")
            .unwrap_or_else(|_| "/Volumes/YBG1TB4Mac/AnkiTov/AnkiPlayGround".to_string())
    });
    let base = Path::new(&base_path);
    let demo = Path::new("/Users/ybg/Library/Application Support/Anki2/demo/collection.anki2");
    let copy_demo = payload.copy_demo_collection.unwrap_or(true);

    let mut created = 0;
    let mut skipped = 0;
    let skip = ["addons21", "logs", "User 1"];
    let mut profiles = Vec::new();

    for name in &payload.names {
        let trimmed = name.trim();
        if trimmed.is_empty() || skip.contains(&trimmed) { skipped += 1; continue; }

        let dir = base.join(trimmed);
        let student_id = trimmed.to_lowercase().replace(' ', "-");
        let display_name = trimmed.to_string();

        if dir.exists() {
            skipped += 1;
            profiles.push(serde_json::json!({
                "name": trimmed, "student_id": student_id,
                "display_name": display_name, "status": "exists"
            }));
            continue;
        }

        match std::fs::create_dir_all(&dir) {
            Ok(_) => {
                // Optionally copy demo collection
                let dest = dir.join("collection.anki2");
                if copy_demo && demo.exists() {
                    let _ = std::fs::copy(demo, &dest);
                }
                created += 1;
                profiles.push(serde_json::json!({
                    "name": trimmed, "student_id": student_id,
                    "display_name": display_name, "status": "created",
                    "anki2_path": dest.to_string_lossy()
                }));
            }
            Err(e) => {
                skipped += 1;
                tracing::warn!("Failed to create dir for {}: {:?}", trimmed, e);
                profiles.push(serde_json::json!({
                    "name": trimmed, "student_id": student_id,
                    "status": "error", "error": e.to_string()
                }));
            }
        }
    }

    format::json(serde_json::json!({
        "created": created, "skipped": skipped,
        "total": payload.names.len(), "profiles": profiles
    }))
}

/// Bulk-populate decks into all student profiles in a class.
#[utoipa::path(
    post,
    path = "/management/students/populate-decks",
    request_body = PopulateDecksRequest,
    responses((status = 200, description = "Decks populated")),
    tag = TAG
)]
pub async fn populate_decks(
    State(ctx): State<AppContext>,
    Json(payload): Json<PopulateDecksRequest>,
) -> Result<Response> {
    let db = &ctx.db;

    // Get all enrolled students for this class
    let enrollments = class_enrollment::Entity::find()
        .filter(class_enrollment::Column::ClassId.eq(&payload.class_id))
        .filter(class_enrollment::Column::Active.eq(true))
        .all(db)
        .await
        .map_err(|_| loco_rs::Error::InternalServerError)?;

    // Get deck info if specific decks requested
    let mut deck_paths = Vec::new();
    if let Some(ref deck_ids) = payload.deck_ids {
        for deck_id_str in deck_ids {
            if let Ok(deck_id) = deck_id_str.parse::<i32>() {
                let deck = deck::Entity::find_by_id(deck_id)
                    .one(db).await.map_err(|_| loco_rs::Error::InternalServerError)?;
                if let Some(d) = deck { deck_paths.push(d.name); }
            }
        }
    }

    let base = Path::new("/Volumes/YBG1TB4Mac/AnkiTov/AnkiPlayGround");
    let mut populated = 0;

    for enrollment in &enrollments {
        let dir = base.join(&enrollment.student_id);
        if !dir.exists() { continue; }

        // For now, ensure collection.anki2 exists
        let coll = dir.join("collection.anki2");
        if !coll.exists() {
            // Copy demo collection as base
            let demo = Path::new("/Users/ybg/Library/Application Support/Anki2/demo/collection.anki2");
            if demo.exists() {
                let _ = std::fs::copy(demo, &coll);
                populated += 1;
            }
        } else {
            populated += 1; // Already has collection
        }
    }

    format::json(serde_json::json!({
        "populated": populated,
        "total_students": enrollments.len(),
        "decks": deck_paths
    }))
}

/// Import students from uploaded CSV/TSV/XLSX file.
#[utoipa::path(
    post,
    path = "/management/students/import-file",
    responses(
        (status = 200, description = "Import results", body = ImportFileResponse),
        (status = 422, description = "Validation error"),
        (status = 404, description = "Class not found")
    ),
    tag = TAG
)]
pub async fn import_file(
    State(ctx): State<AppContext>,
    mut multipart: Multipart,
) -> Result<Response> {
    let db = &ctx.db;
    
    // Extract class_id from form field
    let mut class_id = None;
    let mut file_data = None;
    let mut file_name = String::from("import.csv");
    
    while let Some(field) = multipart.next_field().await.map_err(|e| {
        tracing::error!("Multipart error: {:?}", e);
        Error::InternalServerError
    })? {
        let name = field.name().unwrap_or("").to_string();
        if name == "class_id" {
            match field.text().await {
                Ok(text) => class_id = Some(text),
                Err(e) => {
                    tracing::error!("Failed to read class_id field: {:?}", e);
                    return Err(Error::BadRequest("Invalid class_id field".to_string()));
                }
            }
        } else if name == "file" {
            if let Some(fname) = field.file_name() {
                file_name = fname.to_string();
            }
            match field.bytes().await {
                Ok(bytes) => {
                    file_data = Some(bytes);
                }
                Err(e) => {
                    tracing::error!("Failed to read file field: {:?}", e);
                    return Err(Error::BadRequest("Invalid file field".to_string()));
                }
            }
        }
    }
    
    let class_id = class_id.ok_or_else(|| {
        Error::BadRequest("Missing class_id parameter".to_string())
    })?;
    
    let file_data = file_data.ok_or_else(|| {
        Error::BadRequest("Missing file upload".to_string())
    })?;
    
    // Check class exists
    let class = crate::models::entities::class::Entity::find_by_id(class_id.clone())
        .one(db)
        .await
        .map_err(|_| Error::InternalServerError)?
        .ok_or(Error::NotFound)?;
    
    // Determine delimiter
    let delimiter = if file_name.ends_with(".tsv") {
        '\t'
    } else {
        ','
    };
    
    // Parse file
    let rows = services::student_import::parse_text_file(&file_data, delimiter)
        .map_err(|e| Error::BadRequest(e))?;
    
    // Validate rows
    let (validated, validation_errors) = services::student_import::validate_rows(rows);
    
    // Import using existing bulk-enroll logic
    let mut enrolled = 0;
    let mut skipped = 0;
    let mut imported_students = Vec::new();
    
    for row in &validated {
        let active_model = crate::models::entities::class_enrollment::ActiveModel {
            id: sea_orm::ActiveValue::NotSet,
            class_id: sea_orm::ActiveValue::Set(class_id.clone()),
            student_id: sea_orm::ActiveValue::Set(row.student_id.clone()),
            enrolled_at: sea_orm::ActiveValue::Set(chrono::Utc::now().timestamp()),
            active: sea_orm::ActiveValue::Set(true),
            display_name: sea_orm::ActiveValue::Set(Some(row.display_name.clone())),
            metadata_json: sea_orm::ActiveValue::Set(None),
        };
        
        match crate::models::entities::class_enrollment::Entity::insert(active_model)
            .exec(db)
            .await
        {
            Ok(_) => {
                enrolled += 1;
                imported_students.push(serde_json::json!({
                    "student_id": row.student_id,
                    "display_name": row.display_name
                }));
            }
            Err(_) => {
                skipped += 1;
            }
        }
    }
    
    // Build error list
    let errors: Vec<ImportError> = validation_errors.iter().map(|e| ImportError {
        row: e.row,
        reason: e.reason.clone(),
    }).collect();
    
    format::json(ImportFileResponse {
        enrolled,
        skipped,
        errors,
        students: imported_students,
    })
}
