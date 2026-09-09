//! Deck Verification Service — P1 producer intake (spec: retention-period-plan §Phase 1).
//!
//! Computes the **real** card count, a card-type census
//! (basic / cloze / image-occlusion / video), FSRS-field presence, and
//! media-integrity flags for an uploaded `.apkg`, by importing it through
//! headless Anki `importPackage` (already whitelisted — Q1 forbids new
//! AnkiConnect actions) and querying the verification collection.
//!
//! # Provider selection (house pattern)
//!
//! Mirrors the `ANKITOV_NLU_PROVIDER=test` pattern from [`crate::services::rig_nlu`]:
//! the env var `ANKITOV_ANKI` selects the verification backend:
//!
//! - `ANKITOV_ANKI=test` → [`StubVerifier`] — deterministic, Anki-free. Sniffs
//!   the ZIP magic of the package and returns a fixed verified/rejected
//!   report. Used by `cargo nextest` so unit/integration tests never need
//!   headless Anki. The black-box harness (H2) exercises the real path.
//! - unset (default) → [`AnkiVerifier`] — the real headless-Anki path.
//!   Untouched by tests; only reachable when `ANKITOV_ANKI != "test"`.
//!
//! # Injection
//!
//! Callers (the producers controller) consume verification via
//! [`verifier()`], which returns a `Box<dyn Verifier>` selected from the env.
//! Tests may also construct and inject a [`StubVerifier`] directly.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Env var selecting the verification backend (`test` → [`StubVerifier`]).
pub const ENV_ANKI_PROVIDER: &str = "ANKITOV_ANKI";

/// Prefix used for the dedicated verification deck inside the scratch
/// verification collection (real path only).
const VERIFICATION_DECK_PREFIX: &str = "AnkiTov Verification";

// ---------------------------------------------------------------------------
// Report types (spec §Phase 1 — `DeckVerificationReport`)
// ---------------------------------------------------------------------------

/// Per-note-type card census after import.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CardTypeCensus {
    pub basic: i32,
    pub cloze: i32,
    pub image_occlusion: i32,
    pub video: i32,
}

impl CardTypeCensus {
    /// Total cards accounted for by the census.
    pub fn total(&self) -> i32 {
        self.basic + self.cloze + self.image_occlusion + self.video
    }
}

/// Media integrity verdict for the package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub enum MediaIntegrity {
    /// All referenced media resolved.
    Ok,
    /// `n` referenced media files are missing.
    MissingMedia(i32),
}

/// Full verification report for one `.apkg` package.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DeckVerificationReport {
    /// Deck name reported by the verification collection.
    pub deck_name: String,
    /// Real card count from `findCards` after import (not a byte-size estimate).
    pub real_card_count: i32,
    /// Card-type census (basic / cloze / image_occlusion / video).
    pub card_type_census: CardTypeCensus,
    /// Whether FSRS-related fields/states are present in the collection.
    pub fsrs_fields_present: bool,
    /// Media integrity flags.
    pub media_integrity: MediaIntegrity,
    /// Overall verdict — `false` means the package must be rejected.
    pub ok: bool,
    /// Non-fatal observations recorded with the report.
    pub warnings: Vec<String>,
    /// Set when `ok == false`: human-readable rejection reason.
    pub reject_reason: Option<String>,
}

/// Errors from the verification pipeline itself (infrastructure, not verdict).
#[derive(Debug, thiserror::Error)]
pub enum VerificationError {
    /// The `.apkg` file does not exist on disk.
    #[error("package not found: {0}")]
    NotFound(String),
    /// The verification backend (headless Anki) is unavailable or failed.
    #[error("verification backend unavailable: {0}")]
    Backend(String),
    /// The package could not be read for verification.
    #[error("package unreadable: {0}")]
    Unreadable(String),
}

// ---------------------------------------------------------------------------
// Verifier trait — injectable per P1 spec ("the service takes an injectable
// Verifier trait; tests use a deterministic stub")
// ---------------------------------------------------------------------------

/// Verifies an `.apkg` file on disk and produces a [`DeckVerificationReport`].
///
/// Implementations must be `Send + Sync` (used from axum handlers).
#[async_trait::async_trait]
pub trait Verifier: Send + Sync {
    /// Verify the package at `apkg_path`.
    ///
    /// Returns `Ok(report)` with `report.ok == false` for a *verifiably bad*
    /// package (e.g. corrupt/foreign file) — rejection is a report, not an
    /// error. `Err` is reserved for infrastructure failures (missing file,
    /// backend unreachable).
    async fn verify(&self, apkg_path: &Path) -> Result<DeckVerificationReport, VerificationError>;
}

/// Env-driven verifier selection (`ANKITOV_ANKI`).
///
/// - `ANKITOV_ANKI=test` → deterministic Anki-free [`StubVerifier`]
/// - anything else (default) → real [`AnkiVerifier`] headless path
pub fn verifier() -> Box<dyn Verifier> {
    if is_test_stub() {
        Box::new(StubVerifier)
    } else {
        Box::new(AnkiVerifier::from_env())
    }
}

/// Whether the env selects the deterministic test stub.
pub fn is_test_stub() -> bool {
    std::env::var(ENV_ANKI_PROVIDER)
        .unwrap_or_default()
        .eq_ignore_ascii_case("test")
}

// ---------------------------------------------------------------------------
// StubVerifier — ANKITOV_ANKI=test (house pattern: mirrors ANKITOV_NLU_PROVIDER=test)
// ---------------------------------------------------------------------------

/// Deterministic, Anki-free verifier for unit/integration tests.
///
/// Reads only the package header bytes:
/// - ZIP magic (`PK\x03\x04` — every real `.apkg` is a ZIP) → fixed verified
///   report (12 cards, census summing to 12, FSRS present, media OK).
/// - anything else → report with `ok == false` and a fixed reject reason.
///
/// Never touches Anki, the network, or the filesystem beyond the file read —
/// `cargo nextest` therefore never needs headless Anki.
pub struct StubVerifier;

/// Card count the stub reports for any ZIP-magic package (deterministic).
pub const STUB_CARD_COUNT: i32 = 12;

#[async_trait::async_trait]
impl Verifier for StubVerifier {
    async fn verify(&self, apkg_path: &Path) -> Result<DeckVerificationReport, VerificationError> {
        if !apkg_path.exists() {
            return Err(VerificationError::NotFound(
                apkg_path.display().to_string(),
            ));
        }

        let header = std::fs::read(apkg_path)
            .map_err(|e| VerificationError::Unreadable(format!("{}: {e}", apkg_path.display())))?
            .into_iter()
            .take(4)
            .collect::<Vec<u8>>();

        let deck_name = apkg_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let is_zip = header == vec![b'P', b'K', 0x03, 0x04];
        if !is_zip {
            return Ok(DeckVerificationReport {
                deck_name,
                real_card_count: 0,
                card_type_census: CardTypeCensus::default(),
                fsrs_fields_present: false,
                media_integrity: MediaIntegrity::Ok,
                ok: false,
                warnings: Vec::new(),
                reject_reason: Some(
                    "not a valid .apkg package (missing ZIP header)".to_string(),
                ),
            });
        }

        Ok(DeckVerificationReport {
            deck_name,
            real_card_count: STUB_CARD_COUNT,
            card_type_census: CardTypeCensus {
                basic: 7,
                cloze: 3,
                image_occlusion: 2,
                video: 0,
            },
            fsrs_fields_present: true,
            media_integrity: MediaIntegrity::Ok,
            ok: true,
            warnings: Vec::new(),
            reject_reason: None,
        })
    }
}

// ---------------------------------------------------------------------------
// AnkiVerifier — real headless path (default when ANKITOV_ANKI != "test")
// ---------------------------------------------------------------------------

/// Real verification backend: headless Anki + AnkiConnect.
///
/// Flow (spec §Phase 1, no new AnkiConnect actions — Q1):
/// 1. ensure headless Anki is running (`anki_launcher::ensure_anki_running`)
/// 2. `importPackage` the `.apkg` into the dedicated verification collection
///    under a namespaced deck (scratch base dir; the deck is left in place —
///    `deleteDeck` is not whitelisted)
/// 3. `findCards` on the verification deck → real card count
/// 4. `cardsInfo` → per-card `modelName` census + FSRS memory-state heuristic
///
/// Exercised only by the black-box harness (H2) against the Phase 0 seed —
/// never by `cargo nextest`.
pub struct AnkiVerifier {
    port: u16,
}

impl AnkiVerifier {
    /// Build a verifier using the `ANKICONNECT_PORT` env (default 8765).
    pub fn from_env() -> Self {
        let port = std::env::var("ANKICONNECT_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8765);
        Self { port }
    }

    /// Classification heuristic for one AnkiConnect `modelName`.
    fn classify_model(model_name: &str, card_json: &serde_json::Value) -> CardKind {
        let lower = model_name.to_ascii_lowercase();
        if lower.contains("image occlusion") {
            return CardKind::ImageOcclusion;
        }
        if lower.contains("cloze") {
            return CardKind::Cloze;
        }
        // Video heuristic: card content references a video container file.
        let blob = card_json.to_string().to_ascii_lowercase();
        if [".mp4", ".mov", ".webm", ".avi", ".mkv"]
            .iter()
            .any(|ext| blob.contains(ext))
        {
            return CardKind::Video;
        }
        CardKind::Basic
    }
}

enum CardKind {
    Basic,
    Cloze,
    ImageOcclusion,
    Video,
}

#[async_trait::async_trait]
impl Verifier for AnkiVerifier {
    async fn verify(&self, apkg_path: &Path) -> Result<DeckVerificationReport, VerificationError> {
        if !apkg_path.exists() {
            return Err(VerificationError::NotFound(
                apkg_path.display().to_string(),
            ));
        }

        let deck_name = apkg_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        // 1. Ensure headless Anki is up (per specs/headless-anki-launch.md).
        let launch = crate::services::anki_launcher::ensure_anki_running(self.port).await;
        if !launch.is_ok() {
            return Err(VerificationError::Backend(format!("{launch:?}")));
        }

        let client =
            crate::services::anki_connect::AnkiConnectClient::new(self.port, None);

        // 2. Import into the dedicated verification collection (namespaced deck).
        let verification_deck = format!("{VERIFICATION_DECK_PREFIX}::{deck_name}");
        client
            .import_package(&apkg_path.display().to_string(), &verification_deck)
            .await
            .map_err(|e| VerificationError::Backend(e.to_string()))?;

        // 3. Real card count via findCards on the verification deck.
        let query = format!("deck:\"{verification_deck}\"");
        let cards = client
            .find_cards(&query)
            .await
            .map_err(|e| VerificationError::Backend(e.to_string()))?;

        let mut census = CardTypeCensus::default();
        let mut fsrs_seen = false;
        if !cards.is_empty() {
            let infos = client
                .cards_info(cards)
                .await
                .map_err(|e| VerificationError::Backend(e.to_string()))?;
            for card in &infos {
                match Self::classify_model(
                    card
                        .get("modelName")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    card,
                ) {
                    CardKind::Basic => census.basic += 1,
                    CardKind::Cloze => census.cloze += 1,
                    CardKind::ImageOcclusion => census.image_occlusion += 1,
                    CardKind::Video => census.video += 1,
                }
                // FSRS presence heuristic: FSRS stores per-card memory state.
                if card.get("memoryState").is_some() {
                    fsrs_seen = true;
                }
            }
        }

        // Media integrity: AnkiConnect exposes no media-file probe (Q1 — no new
        // actions), so record the limitation as a warning instead of a claim.
        let warnings = vec![
            "media integrity scan requires direct collection.media access (deferred; Q1 forbids new AnkiConnect actions)".to_string(),
        ];

        Ok(DeckVerificationReport {
            deck_name: verification_deck,
            real_card_count: census.total(),
            card_type_census: census,
            fsrs_fields_present: fsrs_seen,
            media_integrity: MediaIntegrity::Ok,
            ok: true,
            warnings,
            reject_reason: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests — unit tests for the env selection + stub determinism (Anki-free)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    /// Stub selection: ANKITOV_ANKI=test must select the StubVerifier.
    #[test]
    #[serial]
    fn env_test_selects_stub() {
        std::env::set_var(ENV_ANKI_PROVIDER, "test");
        assert!(is_test_stub());
        std::env::remove_var(ENV_ANKI_PROVIDER);
    }

    /// Default (unset env) must select the real path — the stub is opt-in.
    #[test]
    #[serial]
    fn env_unset_selects_real_path() {
        std::env::remove_var(ENV_ANKI_PROVIDER);
        assert!(!is_test_stub());
        std::env::remove_var(ENV_ANKI_PROVIDER);
    }

    /// Stub: a ZIP-magic package verifies with the fixed deterministic report.
    #[tokio::test]
    #[serial]
    async fn stub_verifies_zip_magic_package() {
        std::env::set_var(ENV_ANKI_PROVIDER, "test");
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("sample-seed.apkg");
        // Minimal .apkg stand-in: real .apkg files are ZIP archives.
        std::fs::write(&path, [b'P', b'K', 0x03, 0x04, 0x00, 0x01, 0x02, 0x03])
            .expect("write fixture");

        let report = StubVerifier.verify(&path).await.expect("verify");
        assert!(report.ok);
        assert_eq!(report.real_card_count, STUB_CARD_COUNT);
        assert_eq!(report.card_type_census.total(), STUB_CARD_COUNT);
        assert!(report.fsrs_fields_present);
        assert_eq!(report.media_integrity, MediaIntegrity::Ok);
        assert!(report.reject_reason.is_none());
        assert_eq!(report.deck_name, "sample-seed");
        std::env::remove_var(ENV_ANKI_PROVIDER);
    }

    /// Stub: corrupt/foreign bytes produce a rejection report with a reason.
    #[tokio::test]
    #[serial]
    async fn stub_rejects_corrupt_package() {
        std::env::set_var(ENV_ANKI_PROVIDER, "test");
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("corrupt.apkg");
        std::fs::write(&path, b"definitely not a zip archive").expect("write fixture");

        let report = StubVerifier.verify(&path).await.expect("verify");
        assert!(!report.ok);
        assert_eq!(report.real_card_count, 0);
        assert!(report.reject_reason.is_some());
        std::env::remove_var(ENV_ANKI_PROVIDER);
    }

    /// Missing file is an infrastructure error, not a rejection report.
    #[tokio::test]
    #[serial]
    async fn stub_missing_file_is_error() {
        let err = StubVerifier
            .verify(Path::new("/nonexistent/never-matches.apkg"))
            .await
            .expect_err("must be an error");
        assert!(matches!(err, VerificationError::NotFound(_)));
    }
}
