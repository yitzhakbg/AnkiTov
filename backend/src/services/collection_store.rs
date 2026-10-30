//! Collection Store — server-side per-student Anki collections (spec §2.3).
//!
//! Cloud-first storage: the web reviewer reads cards from a server-side
//! `collection.anki2` + `collection.media/` under a data root, NOT from
//! desktop-Anki profile folders. Layout:
//!
//! ```text
//! $ANKITOV_COLLECTION_ROOT (default: ./data/collections)
//!   {student_id}/
//!     collection.anki2      — the student's collection (read-only for review)
//!     collection.media/     — media files referenced by cards
//!     media.db2             — Anki media DB (optional; presence tolerated)
//! ```
//!
//! Seed packs (`content/seed/packages/*.apkg`) can be imported into a
//! student's collection: an `.apkg` is a zip with `collection.anki2` inside
//! plus a `media` JSON manifest mapping zip entries to real file names.
//! Import REPLACES the student's collection (v1 semantics — documented).
//!
//! The store is read-only at review time; scheduling reads happen through
//! `fsrs_sort` (same brain as the Anki-client path).

use std::path::{Path, PathBuf};

/// Resolve the collection root from env (default `./data/collections`).
pub fn collection_root() -> PathBuf {
    std::env::var("ANKITOV_COLLECTION_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./data/collections"))
}

/// Per-student collection paths.
#[derive(Debug, Clone)]
pub struct StudentCollection {
    pub anki2: PathBuf,
    pub media_dir: PathBuf,
}

impl StudentCollection {
    /// Resolve paths for a student (no existence check — see `exists`).
    pub fn for_student(student_id: &str) -> Self {
        let safe = sanitize_student_id(student_id);
        let base = collection_root().join(safe);
        Self {
            anki2: base.join("collection.anki2"),
            media_dir: base.join("collection.media"),
        }
    }

    pub fn exists(&self) -> bool {
        self.anki2.is_file()
    }
}

/// Student ids become directory names — allowlist hard (no traversal, no dots).
fn sanitize_student_id(student_id: &str) -> String {
    let s: String = student_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if s.is_empty() {
        "_invalid".into()
    } else {
        s
    }
}

/// Import a seed `.apkg` package into a student's server-side collection.
///
/// Replaces any existing collection for that student (v1 semantics). Returns
/// the number of media files extracted. The `.apkg` layout: zip containing
/// `collection.anki2` and a `media` JSON manifest `{"0": "file.jpg", ...}`.
pub fn import_apkg(student_id: &str, apkg_path: &Path) -> Result<usize, CollectionStoreError> {
    let safe = sanitize_student_id(student_id);
    if !apkg_path.is_file() {
        return Err(CollectionStoreError::PackageNotFound(
            apkg_path.display().to_string(),
        ));
    }
    let base = collection_root().join(&safe);
    let media_dir = base.join("collection.media");
    std::fs::create_dir_all(&media_dir)?;

    let file = std::fs::File::open(apkg_path)?;
    let mut zip = zip::ZipArchive::new(file)?;

    // 1) collection.anki2
    let mut anki2 = zip
        .by_name("collection.anki2")
        .map_err(|_| CollectionStoreError::MissingCollectionAnki2)?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut anki2, &mut bytes)?;
    drop(anki2);
    std::fs::write(base.join("collection.anki2"), &bytes)?;

    // 2) media manifest + files
    let mut media_count = 0usize;
    let manifest_bytes = match zip.by_name("media") {
        Ok(mut manifest) => {
            let mut buf = Vec::new();
            match std::io::Read::read_to_end(&mut manifest, &mut buf) {
                Ok(_) => buf,
                Err(e) => return Err(e.into()),
            }
        }
        Err(_) => Vec::new(), // no media in this package
    };
    if !manifest_bytes.is_empty() {
        let map: std::collections::HashMap<String, String> =
            serde_json::from_slice(&manifest_bytes)?;
        for (entry, real_name) in map {
            // real_name must be a bare filename (no traversal)
            let clean = real_name.replace('\\', "/");
            if clean.contains("..") || clean.contains('/') {
                continue; // skip suspicious entries
            }
            if let Ok(mut f) = zip.by_name(&entry) {
                let mut buf = Vec::new();
                if std::io::Read::read_to_end(&mut f, &mut buf).is_ok() {
                    let _ = std::fs::write(media_dir.join(&clean), &buf);
                    media_count += 1;
                }
            }
        }
    }
    Ok(media_count)
}

/// Serve a media file for a student (path-validated).
pub fn media_path(student_id: &str, file_name: &str) -> Option<PathBuf> {
    let clean = file_name.replace('\\', "/");
    if clean.is_empty() || clean.contains("..") || clean.contains('/') {
        return None;
    }
    let col = StudentCollection::for_student(student_id);
    let p = col.media_dir.join(&clean);
    if p.is_file() { Some(p) } else { None }
}

#[derive(Debug, thiserror::Error)]
pub enum ReadCardsError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum CollectionStoreError {
    #[error("package not found: {0}")]
    PackageNotFound(String),
    #[error("package has no collection.anki2")]
    MissingCollectionAnki2,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

// ---------------------------------------------------------------------------
// Card reading (rusqlite, read-only)
// ---------------------------------------------------------------------------

/// A raw card + its note/template data, ready for `card_renderer::render_card`.
pub struct CollectionCard {
    pub card_id: i64,
    pub note_id: i64,
    pub card_ord: i32,
    pub deck_name: String,
    pub fields: Vec<String>,
    /// Note-type field names (template references resolve against these).
    pub field_names: Vec<String>,
    pub qfmt: String,
    pub afmt: String,
    pub is_cloze: bool,
}

/// Read the given card ids from a student's collection.
///
/// Anki 2 schema (schema ver 11/18): `cards(id, nid, ord, did)`,
/// `notes(id, mid, flds)`, `models/col.models` JSON holds field names +
/// templates. Deck names come from `col.decks` JSON.
pub fn read_cards(col: &StudentCollection, card_ids: &[i64]) -> Result<Vec<CollectionCard>, ReadCardsError> {
    let conn = rusqlite::Connection::open_with_flags(
        &col.anki2,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;

    // models + decks JSON from col
    let col_row: (String, String) = conn.query_row(
        "SELECT models, decks FROM col WHERE id = 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let models: serde_json::Value = serde_json::from_str(&col_row.0)?;
    let decks: serde_json::Value = serde_json::from_str(&col_row.1)?;

    let mut out = Vec::with_capacity(card_ids.len());
    for cid in card_ids {
        let row = conn.query_row(
            "SELECT c.nid, c.ord, c.did, n.mid, n.flds FROM cards c JOIN notes n ON n.id = c.nid WHERE c.id = ?1",
            [cid],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i32>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, String>(4)?)),
        );
        let Ok((nid, ord, did, mid, flds)) = row else { continue };
        let Some(model) = models.get(mid.to_string()) else { continue };
        let field_names: Vec<String> = model["flds"]
            .as_array()
            .map(|a| a.iter().map(|f| f["name"].as_str().unwrap_or("").to_string()).collect())
            .unwrap_or_default();
        let is_cloze = model["type"].as_i64() == Some(1);
        let tmpls = model["tmpls"].as_array().cloned().unwrap_or_default();
        // cloze models have ONE template; standard models index by ord
        let tmpl = if is_cloze { tmpls.first() } else { tmpls.get(ord as usize) };
        let (qfmt, afmt) = match tmpl {
            Some(t) => (
                t["qfmt"].as_str().unwrap_or("").to_string(),
                t["afmt"].as_str().unwrap_or("").to_string(),
            ),
            None => (String::new(), String::new()),
        };
        let deck_name = decks[did.to_string()]["name"]
            .as_str()
            .unwrap_or("Unknown")
            .to_string();
        out.push(CollectionCard {
            card_id: *cid,
            note_id: nid,
            card_ord: ord,
            deck_name,
            fields: flds.split('\u{1f}').map(String::from).collect(),
            field_names,
            qfmt,
            afmt,
            is_cloze,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_blocks_traversal() {
        assert_eq!(sanitize_student_id("../../etc"), "etc");
        assert_eq!(sanitize_student_id("a/b/c"), "abc");
        assert_eq!(sanitize_student_id("army-student-1"), "army-student-1");
        assert_eq!(sanitize_student_id(""), "_invalid");
    }

    #[test]
    fn media_path_rejects_traversal() {
        assert!(media_path("s1", "../secrets.txt").is_none());
        assert!(media_path("s1", "sub/dir.png").is_none());
        assert!(media_path("s1", "ok.png").is_none()); // file absent → None
    }
}
