//! Shared on-disk package-library resolvers (T-234).
//!
//! Both the controllers (deck library, deck upload, drill links) and the
//! service layer ([`crate::services::seat_manager`] staging at seat spawn)
//! need to resolve `.apkg` package files against the same directories, so
//! the resolvers live here — services cannot import from controllers.
//!
//! Two libraries exist:
//!
//! * **Seed library** — packages shipped with the product
//!   (`content/seed/packages`, overridable via `ANKITOV_SEED_DIR`).
//! * **Upload library** — packages teachers uploaded through the
//!   management console (`uploads/decks` under the data dir, overridable
//!   via `ANKITOV_UPLOAD_DIR`). Rows in the `decks` table record the
//!   persisted file name in `decks.package_file` (migration
//!   `m20261126_add_deck_package_file`).

use std::path::PathBuf;

/// Resolve the seed-packages directory.
///
/// Order: `ANKITOV_SEED_DIR` env override → `/app/ankitov/content/seed`
/// (container layout) → `content/seed` relative to the working directory
/// (local `cargo run` development).
pub fn seed_packages_dir() -> PathBuf {
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

/// Resolve the directory uploaded `.apkg` packages are persisted to.
///
/// Order: `ANKITOV_UPLOAD_DIR` env override → `uploads/decks` relative to
/// the working directory (the backend's CWD is the data dir in container
/// and release layouts, matching the seed-dir convention).
pub fn upload_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ANKITOV_UPLOAD_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from("uploads/decks")
}

/// Validate a requested package filename: strict allowlist, no traversal.
pub fn valid_package_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.ends_with(".apkg")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !name.contains("..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_package_name_accepts_sane_names() {
        assert!(valid_package_name("english-vocab-he.apkg"));
        assert!(valid_package_name("a1b2c3d4-my_deck.apkg"));
    }

    #[test]
    fn valid_package_name_rejects_traversal_and_junk() {
        assert!(!valid_package_name("../escape.apkg"));
        assert!(!valid_package_name("missing-extension"));
        assert!(!valid_package_name(""));
        assert!(!valid_package_name("has space.apkg"));
        assert!(!valid_package_name("a..b.apkg"));
    }

    #[test]
    fn upload_dir_honors_env_override() {
        // nextest runs each test in its own process — env mutation is safe.
        std::env::set_var("ANKITOV_UPLOAD_DIR", "/tmp/ankitov-deck-paths-test");
        assert_eq!(upload_dir(), PathBuf::from("/tmp/ankitov-deck-paths-test"));
        std::env::remove_var("ANKITOV_UPLOAD_DIR");
    }
}
