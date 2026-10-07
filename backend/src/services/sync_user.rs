//! Per-student Anki sync-server user provisioning.
//!
//! # Why this exists
//!
//! The self-hosted Anki sync server (`ankitov-sync-server`, Anki `--syncserver`)
//! is a *multi-user* sync hub: each student connects with their own
//! `username:password` and the server keeps their collection isolated. The
//! server discovers its users from `SYNC_USER1..N` env vars, which the
//! container entrypoint populates from a shared file
//! (`/etc/ankitov/sync-users.env`, one `username:password` per line) that both
//! the **backend** and **sync-server** containers mount.
//!
//! This module is the *writer* of that file. When a student is imported/enrolled
//! it:
//!
//! 1. Derives a **unique Anki profile name from the student's name** —
//!    `first.last`, lowercased, non-alphanumerics stripped, with a `-<id>`
//!    suffix if another student already resolved to the same base name.
//! 2. Sets the sync **password = the student's numeric id** (the id from the
//!    imported student list, i.e. the `users.id` primary key).
//! 3. Writes `profile_name:<id>` into the shared `sync-users.env`.
//!
//! The same `profile_name` / `id` pair is the credential the Selkies stream
//! container uses to log in as that student (stream `PASSWD` = the student id),
//! so a single identity map drives both "sync your deck on your device" and
//! "pick up where you left off on the streamed device."
//!
//! # Safety rules
//!
//! | Rule | Enforcement |
//! |------|-------------|
//! | Profile names are unique in the sync file | base name + `-<id>` suffix on collision; the numeric `id` (the password) is always unique regardless |
//! | Sync password is never the real login password | password = numeric id only; the *sync* credential is separate from the AnkiTov login password |
//! | File is a 600-mode file, written atomically | temp file + rename; `set_permissions(0o600)` |
//! | Never a write to the Anki DB | this only touches the credential file, never the `.anki2` collection |

// `ColumnTrait` supplies `user::Column::Username.eq(..)`. Without it the
// compiler resolves `.eq` against the enclosing `Iterator` impl and reports
// the misleading "`user::Column` is not an iterator" (E0599).
use sea_orm::{ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use thiserror::Error;

use crate::models::entities::user;

#[derive(Error, Debug)]
pub enum SyncUserError {
    #[error("no full_name and no username for user {0}; cannot derive a profile name")]
    NoName(i64),
    #[error("could not write sync user file: {0}")]
    Io(std::io::Error),
    #[error("{0}")]
    Other(String),
}

/// The shared file path. The backend container mounts the sync-server's
/// `/etc/ankitov` volume at the same path; overridable via `ANKITOV_SYNC_USER_FILE`.
pub const DEFAULT_SYNC_USER_FILE: &str = "/etc/ankitov/sync-users.env";

/// Resolve the file path from env (or default).
pub fn sync_user_file() -> std::path::PathBuf {
    std::env::var("ANKITOV_SYNC_USER_FILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(DEFAULT_SYNC_USER_FILE))
}

/// Derive a base Anki profile name from a full name: `first.last`,
/// lowercased, non-alphanumeric characters (except `.`) removed.
///
/// Mirrors `derive_username` in `management/teachers.rs` so a student's
/// AnkiTov username and Anki profile stay in the same family.
///
/// # Examples
/// ```
/// assert_eq!(super::base_profile_name("Alice Smith"), "alice.smith");
/// assert_eq!(super::base_profile_name("  Marcus  Nguyen  "), "marcus.nguyen");
/// assert_eq!(super::base_profile_name("O'Neil"), "oneil");
/// assert_eq!(super::base_profile_name("Solo"), "solo");
/// ```
pub fn base_profile_name(full_name: &str) -> String {
    let parts: Vec<&str> = full_name
        .trim()
        .split_whitespace()
        .filter(|p| !p.is_empty())
        .collect();
    let first = parts.first().copied().unwrap_or("");
    let last = if parts.len() >= 2 {
        parts.last().copied().unwrap_or(first)
    } else {
        ""
    };

    let mut out = String::new();
    for c in first.chars() {
        out.push(c.to_ascii_lowercase());
    }
    if !last.is_empty() {
        if !out.is_empty() {
            out.push('.');
        }
        for c in last.chars() {
            out.push(c.to_ascii_lowercase());
        }
    }
    let cleaned: String = out
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.')
        .collect();
    let trimmed = cleaned.trim_matches('.').to_string();
    if trimmed.is_empty() {
        "user".into()
    } else {
        trimmed
    }
}

/// Produce a *unique* profile name for the given user, deduping against
/// existing students.
///
/// Starts from the name-derived base. If another student already has a row
/// whose username equals the base (i.e. the bare name is taken), append the
/// student's numeric `id` as a `-<id>` suffix so the two never collide. The
/// password (also `id`) is the true uniqueness key, so even a base-name
/// collision still yields a distinct sync identity; the suffix keeps the
/// *name* readable and unambiguous in the UI.
pub async fn unique_profile_name(
    db: &DatabaseConnection,
    user_id: i64,
    full_name: &str,
) -> Result<String, SyncUserError> {
    let base = base_profile_name(full_name);

    // One filter on the base username, then exclude ourselves in Rust (an id
    // exclusion would need a second predicate, and `Select::filter` takes a
    // condition directly — chain a second `.filter(..)` if that ever changes).
    let candidates = user::Entity::find()
        .filter(user::Column::Username.eq(base.clone()))
        .all(db)
        .await
        .map_err(|e| SyncUserError::Other(e.to_string()))?;

    let others = candidates.iter().filter(|u| u.id != user_id).count();

    if others > 0 {
        Ok(format!("{base}-{user_id}"))
    } else {
        Ok(base)
    }
}

/// Write a single student's sync credential into the shared file.
///
/// `user_id` (the student's numeric id) becomes the *password*. Adds-or-replaces
/// the line for this student's profile name, preserving every other student's
/// line. Returns the profile name that was written.
pub async fn upsert_sync_user(
    db: &DatabaseConnection,
    user_id: i64,
    full_name: &str,
) -> Result<String, SyncUserError> {
    let profile = unique_profile_name(db, user_id, full_name).await?;
    let line = format!("{}:{}", profile, user_id);
    let path = sync_user_file();
    upsert_line(&path, &line)?;
    Ok(profile)
}

/// Add-or-replace a `username:password` line, preserving all other lines.
fn upsert_line(path: &std::path::Path, line: &str) -> Result<(), SyncUserError> {
    let target_user: &str = line.split(':').next().unwrap_or("");
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut out = String::new();
    let mut replaced = false;
    for l in existing.lines() {
        let l = l.trim();
        if l.is_empty() || l.starts_with('#') {
            out.push_str(l);
            out.push('\n');
            continue;
        }
        let existing_user = l.split(':').next().unwrap_or("");
        if existing_user == target_user {
            out.push_str(line);
            out.push('\n');
            replaced = true;
        } else {
            out.push_str(l);
            out.push('\n');
        }
    }
    if !replaced {
        out.push_str(line);
        out.push('\n');
    }
    write_atomic(path, &out)
}

/// Rewrite the whole file from a slice of `(profile_name, user_id)` pairs,
/// de-duplicated by profile name and sorted for deterministic output. Used by
/// the batch re-provisioning endpoint so a full import can be (re)written in
/// one shot.
pub async fn rewrite_sync_user_file(
    path: &std::path::Path,
    pairs: &[(String, i64)],
) -> Result<(), SyncUserError> {
    let mut seen: std::collections::HashSet<String> = Default::default();
    let mut out = String::new();
    for (u, id) in pairs {
        if !seen.insert(u.clone()) {
            continue;
        }
        out.push_str(&format!("{}:{}", u, id));
        out.push('\n');
    }
    write_atomic(path, &out)
}

/// Write `content` to `path` atomically (temp file + rename) with 0600 mode.
fn write_atomic(path: &std::path::Path, content: &str) -> Result<(), SyncUserError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(SyncUserError::Io)?;
    }
    let mut tmp = path.to_path_buf();
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("sync-users.env");
    let suffix = std::process::id();
    tmp.push(format!(".{}.{}.tmp", name, suffix));

    std::fs::write(&tmp, content).map_err(SyncUserError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, path).map_err(SyncUserError::Io)?;
    Ok(())
}

// -- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_name_two_words() {
        assert_eq!(base_profile_name("Alice Smith"), "alice.smith");
    }
    #[test]
    fn base_name_extra_whitespace() {
        assert_eq!(base_profile_name("  Marcus   Nguyen  "), "marcus.nguyen");
    }
    #[test]
    fn base_name_single_word() {
        assert_eq!(base_profile_name("Solo"), "solo");
    }
    #[test]
    fn base_name_strips_punctuation() {
        assert_eq!(base_profile_name("O'Neil"), "oneil");
    }
    #[test]
    fn base_name_empty_falls_back() {
        assert_eq!(base_profile_name("   "), "user");
    }
    #[test]
    fn base_name_multisyllabic_last() {
        assert_eq!(base_profile_name("Mary Jane Watson"), "mary.watson");
    }
}
