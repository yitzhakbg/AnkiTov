//! School settings service (T-126) — typed access over the `app_settings`
//! key/value table.
//!
//! The first-run setup wizard and the management Settings view both read and
//! write through here, so validation lives in exactly one place. Keys are
//! plain string constants; adding a setting is a constant + a field on
//! [`SchoolSettings`] — no migration, no schema change.
//!
//! Locale changes additionally flip the server-side override
//! (`i18n::set_locale`) so any server-rendered or API-side locale-sensitive
//! output follows the school default immediately.

use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use std::collections::HashMap;

use crate::models::entities::app_setting;

/// `app_settings` key holding the school display name.
pub const KEY_SCHOOL_NAME: &str = "school_name";
/// `app_settings` key holding the school default locale code (e.g. `en-US`).
pub const KEY_LOCALE: &str = "locale";
/// `app_settings` key holding the optional Anki sync-server base URL.
pub const KEY_SYNC_URL: &str = "sync_url";

/// All setting keys this service manages, for iteration and audit summaries.
pub const ALL_KEYS: &[&str] = &[KEY_SCHOOL_NAME, KEY_LOCALE, KEY_SYNC_URL];

/// Maximum accepted length for the school name.
pub const SCHOOL_NAME_MAX: usize = 200;
/// Maximum accepted length for the sync URL.
pub const SYNC_URL_MAX: usize = 500;

/// The school-level settings, `None` where a key has never been set.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SchoolSettings {
    pub school_name: Option<String>,
    pub locale: Option<String>,
    pub sync_url: Option<String>,
}

/// Read one raw setting value.
pub async fn get(db: &DatabaseConnection, key: &str) -> Result<Option<String>, sea_orm::DbErr> {
    let row = app_setting::Entity::find_by_id(key.to_string()).one(db).await?;
    Ok(row.map(|m| m.value))
}

/// Read every known key in one query and project onto [`SchoolSettings`].
/// Unknown keys in the table are ignored (forward-compatible).
pub async fn load(db: &DatabaseConnection) -> Result<SchoolSettings, sea_orm::DbErr> {
    let rows = app_setting::Entity::find().all(db).await?;
    let map: HashMap<String, String> = rows
        .into_iter()
        .map(|m| (m.key, m.value))
        .collect();
    Ok(SchoolSettings {
        school_name: map.get(KEY_SCHOOL_NAME).cloned(),
        locale: map.get(KEY_LOCALE).cloned(),
        sync_url: map.get(KEY_SYNC_URL).cloned(),
    })
}

/// Upsert one setting. Portable find-then-update-or-insert (works identically
/// on SQLite and Postgres, unlike dialect-specific `ON CONFLICT` helpers).
pub async fn set(db: &DatabaseConnection, key: &str, value: &str) -> Result<(), sea_orm::DbErr> {
    let now = chrono::Utc::now().timestamp();
    let existing = app_setting::Entity::find_by_id(key.to_string()).one(db).await?;
    match existing {
        Some(row) => {
            let mut am: app_setting::ActiveModel = row.into();
            am.value = Set(value.to_string());
            am.updated_at = Set(now);
            am.update(db).await?;
        }
        None => {
            let am = app_setting::ActiveModel {
                key: Set(key.to_string()),
                value: Set(value.to_string()),
                updated_at: Set(now),
            };
            am.insert(db).await?;
        }
    }
    Ok(())
}

/// Validate a locale code against the server's supported list.
pub fn validate_locale(locale: &str) -> Result<(), String> {
    if crate::i18n::SUPPORTED_LOCALES.iter().any(|(code, _, _)| *code == locale) {
        Ok(())
    } else {
        Err(format!("unsupported locale: {locale}"))
    }
}

/// Validate a sync-server base URL. Empty clears the setting; otherwise the
/// value must be an absolute `http(s)` URL with no whitespace. Dependency-free
/// check — the URL is consumed by the Anki client, which does the real parse.
pub fn validate_sync_url(url: &str) -> Result<(), String> {
    if url.is_empty() {
        return Ok(());
    }
    if url.len() > SYNC_URL_MAX {
        return Err(format!("sync_url exceeds {SYNC_URL_MAX} characters"));
    }
    if url.contains(char::is_whitespace) {
        return Err("sync_url must not contain whitespace".to_string());
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("sync_url must start with http:// or https://".to_string());
    }
    Ok(())
}

/// Validate a school name. Empty clears the setting; otherwise bounded length.
pub fn validate_school_name(name: &str) -> Result<(), String> {
    if name.len() > SCHOOL_NAME_MAX {
        return Err(format!("school_name exceeds {SCHOOL_NAME_MAX} characters"));
    }
    Ok(())
}

/// Apply a validated locale change to the server-side override as well, so
/// the running process reflects the new school default without a restart.
/// Best-effort: a failure here must not fail the settings write.
pub fn apply_locale_override(locale: &str) {
    crate::i18n::set_locale(locale);
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_validation_accepts_supported() {
        assert!(validate_locale("en-US").is_ok());
        assert!(validate_locale("ar").is_ok());
        assert!(validate_locale("zh").is_ok());
    }

    #[test]
    fn locale_validation_rejects_unknown() {
        assert!(validate_locale("xx-YY").is_err());
        assert!(validate_locale("").is_err());
        assert!(validate_locale("en-US ").is_err());
    }

    #[test]
    fn sync_url_validation() {
        assert!(validate_sync_url("").is_ok()); // clears
        assert!(validate_sync_url("https://sync.example.com").is_ok());
        assert!(validate_sync_url("http://10.0.0.5:27701").is_ok());
        assert!(validate_sync_url("ftp://x").is_err());
        assert!(validate_sync_url("https://a b").is_err());
        assert!(validate_sync_url(&"x".repeat(501)).is_err());
    }

    #[test]
    fn school_name_validation() {
        assert!(validate_school_name("").is_ok());
        assert!(validate_school_name("Riverside Middle School").is_ok());
        assert!(validate_school_name(&"x".repeat(201)).is_err());
    }
}
