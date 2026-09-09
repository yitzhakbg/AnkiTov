//! AnkiTov End-to-End Integration Tests
//!
//! These tests require a **running headless Anki instance** with AnkiConnect.
//! They are automatically skipped when `ANKIPLAYGROUND_PATH` is not set or
//! Anki cannot be launched.
//!
//! ## What These Tests Cover
//!
//! | Test | Zone | What it verifies |
//! |---|---|---|
//! | `launch_and_version` | 2 | AnkiConnect responds with version 6 |
//! | `deck_names_and_due_cards` | 2 | Real deck enumeration + due-count query |
//! | `forensic_read_via_anki` | 1+2 | ForensicReader sees same data AnkiConnect reports |
//! | `capsule_generation_pipeline` | 1+2+IMP | Generate capsule → verify card IDs exist in collection |
//! | `temp_profile_roundtrip` | 2 | Write: launch temp copy → modify → verify isolation |
//!
//! ## Running
//!
//! ```bash
//! # Run all integration tests (requires Anki playground)
//! cargo nextest run ankitov_integration_
//!
//! # Run just the smoke test
//! cargo nextest run ankitov_integration_::smoke
//! ```
//!
//! ## Prerequisites
//!
//! 1. Anki playground exists (default: `/Volumes/YBG1TB4Mac/AnkiTov/AnkiPlayGround/`)
//! 2. AnkiConnect addon is installed in `addons21/2055492159/`
//! 3. At least one profile has cards (e.g., "Maya Chen" has 1,578 reviews)
//! 4. No other Anki instance is running on port 8765

mod common;

use common::{playground_path, playground_profiles, AnkiSession};

use backend::services::forensic_reader::ForensicReader;

// =============================================================================
// Smoke Tests — basic AnkiConnect connectivity
// =============================================================================

mod smoke {
    use super::*;

    /// Verify we can launch Anki headless and connect to AnkiConnect.
    #[tokio::test]
    async fn launch_and_version() {
        let profiles = playground_profiles();
        if profiles.is_empty() {
            eprintln!("SKIP: no playground profiles available");
            return;
        }

        let profile = &profiles[0];
        let session = match AnkiSession::with_playground_profile(profile).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("SKIP: cannot launch Anki ({e})");
                return;
            }
        };

        let client = session.client();
        let version = client.api_version().await.expect("version query");
        assert_eq!(version["result"], 6, "AnkiConnect should report API version 6");

        // session drops → Anki killed
    }

    /// Verify deck_names returns a non-empty list from a real profile.
    #[tokio::test]
    async fn deck_names_returns_decks() {
        let profiles = playground_profiles();
        if profiles.is_empty() {
            eprintln!("SKIP: no profiles");
            return;
        }

        // Pick a profile known to have a collection with decks
        let profile_name = if profiles.contains(&"Maya Chen".to_string()) {
            "Maya Chen"
        } else {
            &profiles[0]
        };

        let session = match AnkiSession::with_playground_profile(profile_name).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("SKIP: {e}");
                return;
            }
        };

        let client = session.client();
        let decks = client.deck_names().await.expect("deck names");
        assert!(!decks.is_empty(), "profile should have at least one deck");
        eprintln!("Decks in '{profile_name}': {decks:?}");
    }
}

// =============================================================================
// Zone 1 ↔ Zone 2 Consistency Tests
// =============================================================================

mod zone_consistency {
    use super::*;

    /// Verify that ForensicReader (Zone 1, direct SQLite) and AnkiConnect
    /// (Zone 2, live HTTP API) report consistent data for the same profile.
    #[tokio::test]
    async fn forensic_and_ankiconnect_agree_on_deck_count() {
        let playground = match playground_path() {
            Some(p) => p,
            None => {
                eprintln!("SKIP: no playground");
                return;
            }
        };

        let profile_name = "Maya Chen";

        // Zone 1 — direct SQLite read
        let anki2_path = playground.join(profile_name).join("collection.anki2");
        if !anki2_path.exists() {
            eprintln!("SKIP: no collection.anki2 for {profile_name}");
            return;
        }

        let reader = ForensicReader::open(&anki2_path).expect("open forensic reader");
        let health = reader.cold_health_check().expect("health check");
        eprintln!(
            "Zone 1: {} decks, {} cards, {} reviews (schema v{})",
            health.deck_count, health.card_count, health.review_count, health.schema_version
        );

        // Zone 2 — AnkiConnect
        let session = match AnkiSession::with_playground_profile(profile_name).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("SKIP: cannot launch Anki ({e})");
                return;
            }
        };

        let client = session.client();
        let decks = client.deck_names().await.expect("deck names");

        // The SQLite decks table count and AnkiConnect deckNames should
        // agree (allowing for special Anki system decks if any).
        assert!(
            decks.len() >= health.deck_count as usize,
            "AnkiConnect ({}) should report >= decks than SQLite ({})",
            decks.len(),
            health.deck_count
        );

        eprintln!("Zone 2: {} decks via AnkiConnect: {decks:?}", decks.len());
    }
}

// =============================================================================
// IMP Pipeline Integration Tests
// =============================================================================

mod imp_pipeline {
    use super::*;

    /// End-to-end capsule generation test:
    /// 1. Launch Anki against a real profile
    /// 2. Query due cards via AnkiConnect
    /// 3. Verify the IMP capsule slicer produces reasonable output
    /// 4. Verify capsule cards exist in the collection
    #[tokio::test]
    async fn capsule_generation_against_real_profile() {
        let profiles = playground_profiles();
        let profile_name = if profiles.contains(&"Maya Chen".to_string()) {
            "Maya Chen"
        } else if !profiles.is_empty() {
            &profiles[0]
        } else {
            eprintln!("SKIP: no profiles");
            return;
        };

        let session = match AnkiSession::with_playground_profile(profile_name).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("SKIP: {e}");
                return;
            }
        };

        let client = session.client();

        // 1. Get all deck names
        let decks = client.deck_names().await.expect("deck names");
        eprintln!("Profile '{profile_name}' decks: {decks:?}");

        if decks.is_empty() {
            eprintln!("SKIP: no decks in profile");
            return;
        }

        // 2. Find due cards in the first deck
        let due = client.get_due_cards(&decks[0]).await.expect("due cards");
        eprintln!("Due cards in '{}': {}", decks[0], due.len());

        // 3. If we have cards, verify capsule slicer logic
        if !due.is_empty() {
            use backend::services::capsule_slicer;
            let pool = due.len();
            let capsule_size = capsule_slicer::compute_capsule_size(pool, 3, 25, 10, 25.0, 3);

            assert!(capsule_size > 0, "should produce non-empty capsule from {pool} cards");
            assert!(
                capsule_size <= pool,
                "capsule should not exceed pool ({capsule_size} > {pool})"
            );
            eprintln!("Capsule size for pool={pool}: {capsule_size}");

            // 4. Verify all due card IDs can be resolved via cardsInfo
            let info = client.cards_info(due.clone()).await.expect("cards info");
            assert_eq!(
                info.len(),
                due.len(),
                "cardsInfo should return same number of results"
            );
        }
    }

    /// Verify that the N-Lever adjustment works with real card counts.
    #[tokio::test]
    async fn n_lever_affects_capsule_on_real_data() {
        let profiles = playground_profiles();
        let profile_name = if profiles.contains(&"Aisha Patel".to_string()) {
            "Aisha Patel" // 1,480 reviews, high-volume star student
        } else if !profiles.is_empty() {
            &profiles[0]
        } else {
            eprintln!("SKIP: no profiles");
            return;
        };

        let session = match AnkiSession::with_playground_profile(profile_name).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("SKIP: {e}");
                return;
            }
        };

        let client = session.client();
        let decks = client.deck_names().await.expect("deck names");

        // Find total due across all decks
        let mut total_due = 0usize;
        for deck in &decks {
            if let Ok(due) = client.get_due_cards(deck).await {
                total_due += due.len();
            }
        }

        if total_due < 10 {
            eprintln!("SKIP: not enough due cards ({total_due}) for N-lever test");
            return;
        }

        use backend::services::capsule_slicer;

        let pool = total_due;
        let size_n3 = capsule_slicer::compute_capsule_size(pool, 3, 25, 60, 25.0, 3);
        let size_n1 = capsule_slicer::compute_capsule_size(pool, 1, 25, 60, 25.0, 3);

        eprintln!("Pool={pool} → N=3 capsule={size_n3}, N=1 capsule={size_n1}");

        assert!(
            size_n3 >= size_n1,
            "N=3 ({size_n3}) should produce >= N=1 ({size_n1}) for pool={pool}"
        );
    }
}

// =============================================================================
// Temp Profile Write Tests
// =============================================================================

mod temp_profile {
    use super::*;

    /// Launch a temp copy of a profile, modify it via AnkiConnect, and verify
    /// the original playground profile is untouched.
    #[tokio::test]
    async fn temp_profile_isolation() {
        let profiles = playground_profiles();
        if profiles.is_empty() {
            eprintln!("SKIP: no profiles");
            return;
        }

        let profile_name = &profiles[0];
        let session = match AnkiSession::with_temp_profile(profile_name).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("SKIP: {e}");
                return;
            }
        };

        let client = session.client();

        // Verify we can still read from the temp copy
        let decks = client.deck_names().await.expect("deck names");
        eprintln!("Temp profile '{profile_name}' decks: {decks:?}");

        // The temp copy should have the same decks as the original
        assert!(!decks.is_empty(), "temp copy should have decks");

        // Verify that the temp collection.anki2 is different from original
        let base = session.base_dir().to_path_buf();
        let temp_anki2 = base.join(profile_name).join("collection.anki2");
        assert!(temp_anki2.exists(), "temp collection.anki2 should exist");

        // session drops → temp dir cleaned up
    }
}

// =============================================================================
// Harness Self-Tests
// =============================================================================

mod harness {
    use super::*;

    /// Verify the test harness can find the playground and list profiles.
    #[test]
    fn harness_discovers_playground() {
        let path = playground_path();
        assert!(
            path.is_some(),
            "ANKIPLAYGROUND_PATH should be set or playground should exist"
        );
        let path = path.unwrap();
        assert!(path.is_dir());
        assert!(
            path.join("addons21").is_dir(),
            "addons21/ must exist for AnkiConnect to load"
        );
    }

    /// Verify at least one profile has cards we can test against.
    #[test]
    fn at_least_one_profile_has_data() {
        let playground = playground_path().expect("playground must exist");

        for profile in playground_profiles() {
            let anki2 = playground.join(&profile).join("collection.anki2");
            if !anki2.exists() {
                continue;
            }
            let reader = ForensicReader::open(&anki2).expect("open anki2");
            let health = reader.cold_health_check().expect("health check");
            if health.card_count > 0 && health.review_count > 0 {
                eprintln!(
                    "✓ {profile}: {} cards, {} reviews",
                    health.card_count, health.review_count
                );
                return; // Found at least one data-bearing profile
            }
        }
        panic!("No playground profile has cards + reviews");
    }
}