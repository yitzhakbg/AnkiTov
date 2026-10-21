//! Deck Library — one-click import of bundled seed decks.
//!
//! ## Purpose
//!
//! Lets a teacher/admin add a deck from the built-in deck library with a
//! single click from the management console, instead of the old
//! download-then-upload ceremony. The library is the set of `.apkg` seed
//! packages shipped with the install (`content/seed/packages/`).
//!
//! ## Flow
//!
//! 1. `GET /management/decks/available` — lists every bundled package with
//!    its title, language, expected card count, size, and whether it is
//!    already imported. Metadata comes from the seed manifest
//!    (`content/seed/manifest.json`) when available, with filename-derived
//!    fallbacks.
//! 2. `POST /management/decks/available/:name/import` — imports the package
//!    into the shared collection via AnkiConnect `importPackage` (the only
//!    scheduler-safe import path, per Zone 2 rules), then registers it in
//!    the `decks` table under the deck's actual name (AnkiConnect takes
//!    only the package path; the deck name comes from inside the package,
//!    so it is resolved by diffing `deckNames` across the import).
//!    Idempotent: re-importing an already-imported package is a no-op that
//!    returns the existing record.
//!
//! ## Safety
//!
//! - Package names are validated against a strict allowlist pattern
//!   (`[A-Za-z0-9._-]+`, must end in `.apkg`) — no path traversal.
//! - Imports go through Anki's own scheduler; no direct SQLite writes.
//! - Requires `teacher` or `admin` role, like the rest of deck management.

use crate::middleware::auth::{ensure_role, AuthUser};
use crate::models::entities::deck as deck_entity;
use crate::models::management::{ApiResponse, DeckInfo};
use crate::services::anki_connect::AnkiConnectClient;
use axum::extract::{Path, State};
use axum::Extension;
use loco_rs::prelude::*;
use sea_orm::{entity::prelude::EntityTrait, ActiveModelTrait, ActiveValue, Set};
use serde::Serialize;
use utoipa::ToSchema;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const TAG: &str = "Management Console";

/// Resolve the seed-packages directory.
///
/// Order: `ANKITOV_SEED_DIR` env override → `/app/ankitov/content/seed`
/// (container layout) → `content/seed` relative to the working directory
/// (local `cargo run` development).
fn seed_packages_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ANKITOV_SEED_DIR") {
        let p = PathBuf::from(&dir).join("packages");
        if p.is_dir() {
            return p;
        }
        return PathBuf::from(&dir);
    }
    let container = PathBuf::from("/app/ankitov/content/seed/packages");
    if container.is_dir() {
        return container;
    }
    PathBuf::from("content/seed/packages")
}

/// Validate a requested package filename: strict allowlist, no traversal.
fn valid_package_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.ends_with(".apkg")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !name.contains("..")
}

/// Human-friendly deck name from a package filename stem:
/// `english-vocabulary-en.apkg` → `English Vocabulary (en)`.
fn friendly_deck_name(stem: &str) -> String {
    let (base, lang) = match stem.rsplit_once('-') {
        Some((b, l)) if l.len() == 2 && l.chars().all(|c| c.is_ascii_alphabetic()) => {
            (b, Some(l))
        }
        _ => (stem, None),
    };
    let title = base
        .split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    match lang {
        Some(l) => format!("{title} ({l})"),
        None => title,
    }
}

/// One deck entry from the seed manifest (`content/seed/manifest.json`).
#[derive(Debug, Default, Clone)]
struct ManifestEntry {
    title: Option<String>,
    locale: Option<String>,
    card_count: Option<i64>,
}

/// Load the seed manifest keyed by package filename (e.g.
/// `english-vocabulary-en.apkg`).
///
/// The manifest lives next to the packages directory (`content/seed/
/// manifest.json`). Missing or malformed manifests degrade gracefully —
/// callers fall back to filename-derived metadata.
fn seed_manifest() -> std::collections::BTreeMap<String, ManifestEntry> {
    let mut map = std::collections::BTreeMap::new();
    let manifest_path = seed_packages_dir()
        .parent()
        .map(|p| p.join("manifest.json"));
    let Some(manifest_path) = manifest_path else {
        return map;
    };
    let Ok(raw) = std::fs::read_to_string(&manifest_path) else {
        tracing::debug!("deck library: no seed manifest at {:?}", manifest_path);
        return map;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        tracing::warn!("deck library: seed manifest at {manifest_path:?} is not valid JSON");
        return map;
    };
    let Some(decks) = value.get("decks").and_then(|d| d.as_array()) else {
        return map;
    };
    for deck in decks {
        let file = deck.get("file").and_then(|f| f.as_str()).unwrap_or("");
        let package = file.rsplit('/').next().unwrap_or(file).to_string();
        if package.is_empty() {
            continue;
        }
        map.insert(
            package,
            ManifestEntry {
                title: deck.get("title").and_then(|t| t.as_str()).map(String::from),
                locale: deck.get("locale").and_then(|l| l.as_str()).map(String::from),
                card_count: deck.get("card_count").and_then(|c| c.as_i64()),
            },
        );
    }
    map
}

/// One entry in the deck library listing.
#[derive(Debug, Serialize, ToSchema)]
pub struct AvailableDeck {
    /// Package filename (the `:name` path parameter for import).
    pub package: String,
    /// Deck title (from the seed manifest when available).
    pub name: String,
    /// Language tag (manifest locale, or parsed from the filename).
    pub language: Option<String>,
    /// Expected card count from the seed manifest, if known.
    pub card_count: Option<i64>,
    /// File size in bytes.
    pub file_size_bytes: i64,
    /// True if this package is already registered/imported.
    pub imported: bool,
}

/// List the bundled deck library.
///
/// Returns every `.apkg` in the seed-packages directory with import status.
#[utoipa::path(
    get,
    path = "/management/decks/available",
    responses(
        (status = 200, description = "Deck library listing", body = Vec<AvailableDeck>),
        (status = 500, description = "Library directory unavailable")
    ),
    tag = TAG
)]
pub async fn list_available_decks(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;

    let dir = seed_packages_dir();
    let mut entries: Vec<AvailableDeck> = Vec::new();
    let read_dir = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) => {
            tracing::error!("deck library dir {:?} unreadable: {:?}", dir, e);
            return Err(loco_rs::Error::InternalServerError);
        }
    };

    // Decks already registered in the decks table (imported at least once).
    let existing: Vec<deck_entity::Model> = deck_entity::Entity::find()
        .all(&ctx.db)
        .await
        .unwrap_or_default();
    let manifest = seed_manifest();

    let mut packages: Vec<(String, i64)> = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("apkg") {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let size = entry
            .metadata()
            .map(|m| m.len() as i64)
            .unwrap_or(0);
        packages.push((file_name.to_string(), size));
    }
    packages.sort();

    for (package, size) in packages {
        let stem = package.trim_end_matches(".apkg").to_string();
        let meta = manifest.get(&package);
        let language = meta
            .and_then(|m| m.locale.clone())
            .or_else(|| {
                stem.rsplit_once('-').and_then(|(_, l)| {
                    if l.len() == 2 && l.chars().all(|c| c.is_ascii_alphabetic()) {
                        Some(l.to_string())
                    } else {
                        None
                    }
                })
            });
        let name = meta
            .and_then(|m| m.title.clone())
            .unwrap_or_else(|| friendly_deck_name(&stem));
        let card_count = meta.and_then(|m| m.card_count);
        // Imported = a registered deck carries this package's marker in its
        // description, or its name matches the expected deck title/stem.
        let imported = existing.iter().any(|d| {
            d.description
                .as_deref()
                .map(|desc| desc.contains(&package))
                .unwrap_or(false)
                || d.name == name
                || d.name == stem
        });
        entries.push(AvailableDeck {
            package,
            name,
            language,
            card_count,
            file_size_bytes: size,
            imported,
        });
    }

    format::json(entries)
}

/// Import a deck from the library with one click.
///
/// Imports the named bundled package into the shared collection via
/// AnkiConnect and registers it in the `decks` table. Idempotent.
#[utoipa::path(
    post,
    path = "/management/decks/available/{name}/import",
    params(
        ("name" = String, Path, description = "Package filename from the deck library (e.g. english-vocabulary-en.apkg)")
    ),
    responses(
        // House style: client errors return 200 with `success: false`
        // (same as the rest of the management surface).
        (status = 200, description = "Deck imported (idempotent re-import returns the existing record); invalid name or unknown package returns success = false", body = DeckInfo),
        (status = 500, description = "Import failed")
    ),
    tag = TAG
)]
pub async fn import_available_deck(
    Extension(user): Extension<AuthUser>,
    State(ctx): State<AppContext>,
    Path(name): Path<String>,
) -> Result<Response> {
    let _ = ensure_role(&user, &["teacher", "admin"])?;

    if !valid_package_name(&name) {
        return format::json(ApiResponse {
            success: false,
            message: "Invalid package name".to_string(),
        });
    }

    let dir = seed_packages_dir();
    let path = dir.join(&name);
    if !path.is_file() {
        return format::json(ApiResponse {
            success: false,
            message: format!("Package not found in library: {name}"),
        });
    }

    let stem = name.trim_end_matches(".apkg").to_string();
    let manifest = seed_manifest();
    let expected_name = manifest
        .get(&name)
        .and_then(|m| m.title.clone())
        .unwrap_or_else(|| friendly_deck_name(&stem));

    // Idempotency: a registered deck carries this package's marker in its
    // description, or its name matches the expected deck title.
    let all_decks = deck_entity::Entity::find()
        .all(&ctx.db)
        .await
        .map_err(|e| {
            tracing::error!("deck library idempotency lookup failed: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;
    if let Some(existing) = all_decks.iter().find(|d| {
        d.description
            .as_deref()
            .map(|desc| desc.contains(&name))
            .unwrap_or(false)
            || d.name == expected_name
    }) {
        return format::json(DeckInfo {
            id: existing.id.to_string(),
            name: existing.name.clone(),
            card_count: existing.card_count as u32,
            created_at: existing.created_at,
            updated_at: existing.updated_at,
            distribution_count: 0,
            track_id: existing.track_id.clone(),
        });
    }

    // Import through Anki's own scheduler (Zone 2 — the only safe path).
    let client = anki_client();
    let file_data = std::fs::read(&path).map_err(|e| {
        tracing::error!("deck library read {:?} failed: {:?}", path, e);
        loco_rs::Error::InternalServerError
    })?;
    let checksum = hex::encode(Sha256::digest(&file_data));
    let file_size = file_data.len() as i64;

    // AnkiConnect importPackage takes only the path — the deck name comes
    // from inside the package. Resolve the actual imported deck name by
    // diffing deckNames across the import.
    let before: std::collections::HashSet<String> = client
        .deck_names()
        .await
        .map(|names| names.into_iter().collect())
        .unwrap_or_default();

    let imported = client
        .import_package(&path.display().to_string())
        .await
        .map_err(|e| {
            tracing::error!("deck library import {name} failed: {:?}", e);
            loco_rs::Error::InternalServerError
        })?;
    if !imported {
        tracing::warn!("deck library import {name}: AnkiConnect returned false");
    }

    let deck_name = match client.deck_names().await {
        Ok(after) => {
            let new_decks: Vec<String> =
                after.into_iter().filter(|n| !before.contains(n)).collect();
            if new_decks.len() == 1 {
                new_decks[0].clone()
            } else if new_decks.iter().any(|n| n == &expected_name) {
                expected_name.clone()
            } else if new_decks.is_empty() {
                // Deck already existed in the collection (e.g. re-import
                // after the DB row was removed) — Anki merged by note GUID.
                expected_name.clone()
            } else {
                // Multiple new decks (subdecks): deterministic pick.
                let picked = new_decks
                    .into_iter()
                    .min()
                    .unwrap_or_else(|| expected_name.clone());
                tracing::warn!("deck library import {name}: multiple new decks, picked {picked:?}");
                picked
            }
        }
        Err(e) => {
            tracing::warn!("deck library post-import deckNames failed for {name}: {:?}", e);
            expected_name.clone()
        }
    };

    // Real card count from the collection after import.
    let card_count: i32 = match client
        .find_cards(&format!("deck:\"{deck_name}\""))
        .await
    {
        Ok(cards) => cards.len() as i32,
        Err(e) => {
            tracing::warn!("deck library post-import count failed for {deck_name}: {:?}", e);
            0
        }
    };

    let now_ts = chrono::Utc::now().timestamp();
    let deck = deck_entity::ActiveModel {
        id: ActiveValue::NotSet,
        name: Set(deck_name.clone()),
        description: Set(Some(format!("Imported from deck library ({name})"))),
        card_count: Set(card_count),
        file_size_bytes: Set(file_size),
        checksum_sha256: Set(checksum),
        uploaded_by: Set(user.id.to_string()),
        created_at: Set(now_ts),
        updated_at: Set(now_ts),
        track_id: ActiveValue::NotSet,
    }
    .insert(&ctx.db)
    .await
    .map_err(|e| {
        tracing::error!("deck library register {deck_name} failed: {:?}", e);
        loco_rs::Error::InternalServerError
    })?;

    tracing::info!(
        "Deck imported from library: id={}, name={}, package={}, cards={}",
        deck.id,
        deck.name,
        name,
        card_count
    );

    format::json(DeckInfo {
        id: deck.id.to_string(),
        name: deck.name,
        card_count: deck.card_count as u32,
        created_at: deck.created_at,
        updated_at: deck.updated_at,
        distribution_count: 0,
        track_id: deck.track_id.clone(),
    })
}

/// Construct a Zone 2 AnkiConnect client from environment config
/// (same resolution as `management::anki_ops`).
fn anki_client() -> AnkiConnectClient {
    let port: u16 = std::env::var("ANKICONNECT_PORT")
        .unwrap_or_else(|_| "8765".to_string())
        .parse()
        .unwrap_or(8765);
    let api_key = std::env::var("ANKICONNECT_API_KEY").ok();
    AnkiConnectClient::new(port, api_key)
}
