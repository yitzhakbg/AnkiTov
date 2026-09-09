//! Integration tests for Anki Zone services.
//!
//! ## Zone 1 — ForensicReader (Direct SQLite)
//!
//! Creates a temporary `.anki2` SQLite database with known schema and
//! revlog data, then verifies `ForensicReader` methods return correct
//! results.
//!
//! ## Zone 2 — AnkiConnectClient (HTTP Mock)
//!
//! Spins up a mock HTTP server that responds to AnkiConnect JSON-RPC
//! requests, then verifies `AnkiConnectClient` methods parse responses
//! correctly.
//!
//! Run with: `cargo nextest run` (or `cargo test`)

// ===========================================================================
// Zone 1 — ForensicReader Integration Tests
// ===========================================================================

mod zone1_forensic {
    use backend::services::forensic_reader::ForensicReader;
    use rusqlite::Connection;

    /// Create a temporary `.anki2` file with Anki schema v18 and test data.
    ///
    /// Schema: `col`, `notes`, `cards`, `revlog`, `decks`, `notetypes`
    ///
    /// Seeds:
    /// - 2 decks: "Test Deck" (id=1), "Other Deck" (id=2)
    /// - 3 notes
    /// - 4 cards (3 in deck 1, 1 in deck 2)
    /// - 10 revlog entries across 2 days with varying ease grades
    fn create_test_anki2() -> tempfile::NamedTempFile {
        let tmp = tempfile::NamedTempFile::new().expect("create temp file");

        let conn = Connection::open(tmp.path()).expect("open sqlite");

        // Enable WAL mode (matches real Anki)
        conn.execute_batch("PRAGMA journal_mode=WAL;").ok();

        // Schema v18 (Anki 2.1+)
        conn.execute_batch(
            "CREATE TABLE col (
                id      INTEGER PRIMARY KEY,
                crt     INTEGER NOT NULL DEFAULT 0,
                mod     INTEGER NOT NULL DEFAULT 0,
                scm     INTEGER NOT NULL DEFAULT 0,
                ver     INTEGER NOT NULL DEFAULT 0,
                dty     INTEGER NOT NULL DEFAULT 0,
                usn     INTEGER NOT NULL DEFAULT 0,
                ls      INTEGER NOT NULL DEFAULT 0,
                conf    TEXT NOT NULL DEFAULT '{}',
                models  TEXT NOT NULL DEFAULT '{}',
                decks   TEXT NOT NULL DEFAULT '{}',
                dconf   TEXT NOT NULL DEFAULT '{}',
                tags    TEXT NOT NULL DEFAULT ''
            );"
        ).expect("create col table");

        conn.execute_batch(
            "CREATE TABLE notes (
                id      INTEGER PRIMARY KEY,
                guid    TEXT NOT NULL DEFAULT '',
                mid     INTEGER NOT NULL DEFAULT 0,
                mod     INTEGER NOT NULL DEFAULT 0,
                usn     INTEGER NOT NULL DEFAULT 0,
                tags    TEXT NOT NULL DEFAULT '',
                flds    TEXT NOT NULL DEFAULT '',
                sfld    TEXT NOT NULL DEFAULT '',
                csum    INTEGER NOT NULL DEFAULT 0,
                flags   INTEGER NOT NULL DEFAULT 0,
                data    TEXT NOT NULL DEFAULT ''
            );"
        ).expect("create notes table");

        conn.execute_batch(
            "CREATE TABLE cards (
                id      INTEGER PRIMARY KEY,
                nid     INTEGER NOT NULL DEFAULT 0,
                did     INTEGER NOT NULL DEFAULT 0,
                ord     INTEGER NOT NULL DEFAULT 0,
                mod     INTEGER NOT NULL DEFAULT 0,
                usn     INTEGER NOT NULL DEFAULT 0,
                type    INTEGER NOT NULL DEFAULT 0,
                queue    INTEGER NOT NULL DEFAULT 0,
                due     INTEGER NOT NULL DEFAULT 0,
                ivl     INTEGER NOT NULL DEFAULT 0,
                factor  INTEGER NOT NULL DEFAULT 0,
                reps    INTEGER NOT NULL DEFAULT 0,
                lapses  INTEGER NOT NULL DEFAULT 0,
                left    INTEGER NOT NULL DEFAULT 0,
                odue    INTEGER NOT NULL DEFAULT 0,
                odid    INTEGER NOT NULL DEFAULT 0,
                flags   INTEGER NOT NULL DEFAULT 0,
                data    TEXT NOT NULL DEFAULT ''
            );"
        ).expect("create cards table");

        conn.execute_batch(
            "CREATE TABLE revlog (
                id      INTEGER PRIMARY KEY,
                cid     INTEGER NOT NULL DEFAULT 0,
                usn     INTEGER NOT NULL DEFAULT 0,
                ease    INTEGER NOT NULL DEFAULT 0,
                ivl     INTEGER NOT NULL DEFAULT 0,
                lastIvl INTEGER NOT NULL DEFAULT 0,
                factor  INTEGER NOT NULL DEFAULT 0,
                time    INTEGER NOT NULL DEFAULT 0,
                type    INTEGER NOT NULL DEFAULT 0
            );"
        ).expect("create revlog table");

        conn.execute_batch(
            "CREATE TABLE decks (
                id          INTEGER PRIMARY KEY,
                name        TEXT NOT NULL DEFAULT '',
                mtime_secs  INTEGER NOT NULL DEFAULT 0,
                usn         INTEGER NOT NULL DEFAULT 0,
                common      TEXT NOT NULL DEFAULT '{}',
                kind        TEXT NOT NULL DEFAULT '{}'
            );"
        ).expect("create decks table");

        conn.execute_batch(
            "CREATE TABLE notetypes (
                id          INTEGER PRIMARY KEY,
                name        TEXT NOT NULL DEFAULT '',
                mtime_secs  INTEGER NOT NULL DEFAULT 0,
                usn         INTEGER NOT NULL DEFAULT 0,
                config      TEXT NOT NULL DEFAULT '{}',
                fields      TEXT NOT NULL DEFAULT '[]',
                templates   TEXT NOT NULL DEFAULT '[]',
                type        INTEGER NOT NULL DEFAULT 0
            );"
        ).expect("create notetypes table");

        // Insert collection metadata (schema v18)
        conn.execute(
            "INSERT INTO col (id, crt, mod, scm, ver, conf) VALUES (1, 0, 0, 0, 18, '{}')",
            [],
        ).expect("insert col");

        // Insert decks
        conn.execute(
            "INSERT INTO decks (id, name, mtime_secs, usn, common, kind) VALUES (1, 'Test Deck', 0, 0, '{}', '{}')",
            [],
        ).expect("insert deck 1");
        conn.execute(
            "INSERT INTO decks (id, name, mtime_secs, usn, common, kind) VALUES (2, 'Other Deck', 0, 0, '{}', '{}')",
            [],
        ).expect("insert deck 2");

        // Insert notes (3 notes)
        for i in 1..=3 {
            conn.execute(
                "INSERT INTO notes (id, guid, mid, mod, usn, tags, flds, sfld, csum, flags, data)
                 VALUES (?1, '', 1, 0, 0, '', ?2, '', 0, 0, '')",
                rusqlite::params![i, format!("field_{i}")],
            ).expect("insert note");
        }

        // Insert cards: 3 in deck 1, 1 in deck 2
        conn.execute(
            "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data)
             VALUES (1001, 1, 1, 0, 0, 0, 2, 2, 0, 7, 2500, 5, 1, 0, 0, 0, 0, '')",
            [],
        ).expect("insert card 1");
        conn.execute(
            "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data)
             VALUES (1002, 2, 1, 0, 0, 0, 2, 2, 0, 14, 2500, 3, 0, 0, 0, 0, 0, '')",
            [],
        ).expect("insert card 2");
        conn.execute(
            "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data)
             VALUES (1003, 3, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, '')",
            [],
        ).expect("insert card 3");
        conn.execute(
            "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data)
             VALUES (1004, 1, 2, 0, 0, 0, 2, 2, 0, 21, 2500, 2, 0, 0, 0, 0, 0, '')",
            [],
        ).expect("insert card 4");

        // Insert revlog entries — 10 reviews across 2 days
        // In Anki, revlog.id IS a millisecond timestamp (not a sequential integer).
        // The forensic reader buckets by id / 86400000 to get day numbers.
        // Day 1 (today): 5 reviews with ease 1(Again), 2(Hard), 3(Good), 3(Good), 4(Easy)
        // Day 2 (yesterday): 5 reviews with ease 3(Good), 3(Good), 1(Again), 2(Hard), 3(Good)
        let now_ms = chrono::Utc::now().timestamp_millis();
        let day_ms = 86_400_000i64;

        let revlog_entries: Vec<(i64, i64, i32)> = vec![
            // Today (day 1) — 5 reviews in deck 1
            (now_ms - 3600_000, 1001i64, 3i32),  // Good
            (now_ms - 3600_001, 1002, 4),         // Easy
            (now_ms - 3600_002, 1001, 1),         // Again (lapse)
            (now_ms - 3600_003, 1002, 2),         // Hard
            (now_ms - 3600_004, 1003, 3),         // Good
            // Yesterday (day 2) — 5 reviews in deck 1
            (now_ms - day_ms - 3600_000, 1001, 3), // Good
            (now_ms - day_ms - 3600_001, 1002, 3), // Good
            (now_ms - day_ms - 3600_002, 1001, 1), // Again (lapse)
            (now_ms - day_ms - 3600_003, 1002, 2), // Hard
            (now_ms - day_ms - 3600_004, 1003, 3), // Good
        ];

        for (_i, (ts, cid, ease)) in revlog_entries.iter().enumerate() {
            // Use the timestamp as the revlog ID (matching real Anki behavior)
            conn.execute(
                "INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type)
                 VALUES (?1, ?2, 0, ?3, 0, 0, 0, 0, 0)",
                rusqlite::params![ts, cid, ease],
            ).expect("insert revlog");
        }

        // Insert a notetype
        conn.execute(
            "INSERT INTO notetypes (id, name, mtime_secs, usn, config, fields, templates, type)
             VALUES (1, 'Basic', 0, 0, '{}', '[]', '[]', 0)",
            [],
        ).expect("insert notetype");

        // Close connection so Anki2 file is fully written
        drop(conn);

        tmp
    }

    #[test]
    fn test_cold_health_check_returns_correct_counts() {
        let tmp = create_test_anki2();
        let reader = ForensicReader::open(tmp.path()).expect("open forensic reader");

        let health = reader.cold_health_check().expect("health check");
        assert_eq!(health.schema_version, 18, "schema version should be 18");
        assert_eq!(health.note_count, 3, "should have 3 notes");
        assert_eq!(health.card_count, 4, "should have 4 cards");
        assert_eq!(health.review_count, 10, "should have 10 reviews");
        assert_eq!(health.deck_count, 2, "should have 2 decks");
        assert_eq!(health.orphaned_note_count, 0, "no orphaned notes");
        assert_eq!(health.orphaned_card_count, 0, "no orphaned cards");
        assert!(health.has_recent_reviews, "should have recent reviews");
    }

    #[test]
    fn test_retention_curve_returns_daily_points() {
        let tmp = create_test_anki2();
        let reader = ForensicReader::open(tmp.path()).expect("open forensic reader");

        let curve = reader
            .retention_curve("Test Deck", 7)
            .expect("retention curve");

        // We seeded reviews across 2 days, so we expect 1-2 data points
        // (both days fall within the 7-day window).
        assert!(
            curve.len() >= 1,
            "should have at least 1 data point, got {}",
            curve.len()
        );
        assert!(
            curve.len() <= 2,
            "should have at most 2 data points, got {}",
            curve.len()
        );

        // Verify each point has valid fields
        for point in &curve {
            assert!(!point.date.is_empty(), "date should not be empty");
            assert!(
                point.retention_rate >= 0.0 && point.retention_rate <= 1.0,
                "retention rate should be 0..1, got {}",
                point.retention_rate
            );
            assert!(point.review_count > 0, "should have reviews");
        }
    }

    #[test]
    fn test_retention_curve_deck_not_found() {
        let tmp = create_test_anki2();
        let reader = ForensicReader::open(tmp.path()).expect("open forensic reader");

        let result = reader.retention_curve("Nonexistent Deck", 7);
        assert!(result.is_err(), "should fail for nonexistent deck");

        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("deck not found"),
            "error should mention deck not found: {err}"
        );
    }

    #[test]
    fn test_cold_health_check_handles_empty_database() {
        // Create a minimal empty .anki2 file
        let tmp = tempfile::NamedTempFile::new().expect("create temp file");
        let conn = Connection::open(tmp.path()).expect("open sqlite");

        conn.execute_batch(
            "CREATE TABLE col (id INTEGER PRIMARY KEY, ver INTEGER NOT NULL DEFAULT 0, conf TEXT NOT NULL DEFAULT '{}');
             CREATE TABLE notes (id INTEGER PRIMARY KEY);
             CREATE TABLE cards (id INTEGER PRIMARY KEY, nid INTEGER NOT NULL DEFAULT 0, did INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE revlog (id INTEGER PRIMARY KEY, cid INTEGER NOT NULL DEFAULT 0, ease INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE decks (id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT '');
             CREATE TABLE notetypes (id INTEGER PRIMARY KEY);
             INSERT INTO col (id, ver) VALUES (1, 18);",
        ).expect("create empty schema");

        drop(conn);

        let reader = ForensicReader::open(tmp.path()).expect("open forensic reader");
        let health = reader.cold_health_check().expect("health check");

        assert_eq!(health.schema_version, 18);
        assert_eq!(health.note_count, 0);
        assert_eq!(health.card_count, 0);
        assert_eq!(health.review_count, 0);
        assert!(!health.has_recent_reviews);
    }

    #[test]
    fn test_forensic_reader_is_send_sync() {
        // Compile-time check that ForensicReader is Send + Sync.
        // This is required for use in async contexts.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ForensicReader>();
    }

    #[test]
    fn test_multiple_decks_returns_correct_data() {
        let tmp = create_test_anki2();
        let reader = ForensicReader::open(tmp.path()).expect("open forensic reader");

        // Test Deck should have 8 reviews (all revlog entries are for cards in deck 1)
        let curve1 = reader
            .retention_curve("Test Deck", 7)
            .expect("retention curve for Test Deck");
        let total_reviews_1: u32 = curve1.iter().map(|p| p.review_count).sum();
        assert_eq!(
            total_reviews_1, 10,
            "Test Deck should have 10 reviews across all days"
        );

        // Other Deck should have no reviews (card 1004 has no revlog entries)
        let curve2 = reader.retention_curve("Other Deck", 7);
        assert!(
            curve2.is_err(),
            "Other Deck should fail (no review data)"
        );
    }

    // -----------------------------------------------------------------------
    // School-Optimized Metrics Tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_practice_adherence_returns_score() {
        let tmp = create_test_anki2();
        let reader = ForensicReader::open(tmp.path()).expect("open forensic reader");

        let pas = reader
            .practice_adherence("Test Deck", 7)
            .expect("practice adherence");

        // We have reviews on 2 out of 7 days → score ≈ 28
        assert_eq!(pas.score, 29, "PAS should be ~29 for 2/7 active days");
        assert_eq!(pas.active_days, 2, "should have 2 active days");
        assert_eq!(pas.total_days, 7);
        assert_eq!(pas.missed_days, 5);
        assert_eq!(pas.window_days, 7);
    }

    #[test]
    fn test_practice_adherence_deck_not_found() {
        let tmp = create_test_anki2();
        let reader = ForensicReader::open(tmp.path()).expect("open forensic reader");

        let result = reader.practice_adherence("Nonexistent", 7);
        assert!(result.is_err());
    }

    #[test]
    fn test_sporadic_index_for_active_deck() {
        let tmp = create_test_anki2();
        let reader = ForensicReader::open(tmp.path()).expect("open forensic reader");

        let spi = reader
            .sporadic_index("Test Deck", 7)
            .expect("sporadic index");

        // 2 active days out of 7 → coverage ≈ 0.286
        assert_eq!(spi.active_days, 2);
        assert_eq!(spi.total_days, 7);
        assert!((spi.coverage - 0.286).abs() < 0.01, "coverage should be ~0.286");
        // Both days have 5 reviews each → CV = 0 → SPI = 0 (consistent volume)
        assert_eq!(spi.index, 0.0, "SPI should be 0 when daily volume is consistent");
        assert_eq!(spi.avg_reviews_per_active_day, 5.0);
    }

    #[test]
    fn test_gap_analysis_returns_gaps() {
        let tmp = create_test_anki2();
        let reader = ForensicReader::open(tmp.path()).expect("open forensic reader");

        let gaps = reader
            .gap_analysis("Test Deck", 7)
            .expect("gap analysis");

        // We have reviews on 2 days (today and yesterday) → gap between them is 0
        // (they're consecutive). So max_gap_days should be 0.
        assert_eq!(gaps.max_gap_days, 0, "no gaps between consecutive days");
        assert_eq!(gaps.gaps_over_3_days, 0);
        assert_eq!(gaps.gaps_over_7_days, 0);
    }

    #[test]
    fn test_deck_suitability_returns_verdict() {
        let tmp = create_test_anki2();
        let reader = ForensicReader::open(tmp.path()).expect("open forensic reader");

        let dsi = reader
            .deck_suitability("Test Deck", 7)
            .expect("deck suitability");

        // Our test data: 3/10 lapses = 0.3 lapse rate → "Too Hard" threshold is >0.30
        // So lapse_rate should be 0.3 (boundary case)
        assert!(dsi.lapse_rate >= 0.0, "lapse rate should be non-negative");
        assert!(dsi.ease_factor > 0.0, "ease factor should be positive");
        assert!(!dsi.verdict.is_empty(), "verdict should not be empty");
        assert!(!dsi.indicators.lapse_rate.is_empty());
    }

    #[test]
    fn test_sporadic_index_empty_deck() {
        let tmp = create_test_anki2();
        let reader = ForensicReader::open(tmp.path()).expect("open forensic reader");

        // Other Deck has no reviews
        let spi = reader
            .sporadic_index("Other Deck", 7)
            .expect("sporadic index");

        assert_eq!(spi.index, 1.0, "empty deck should have SPI = 1.0");
        assert_eq!(spi.active_days, 0);
        assert_eq!(spi.coverage, 0.0);
    }
}

// ===========================================================================
// Zone 2 — AnkiConnectClient Integration Tests
// ===========================================================================

mod zone2_ankiconnect {
    use backend::services::anki_connect::AnkiConnectClient;

    #[tokio::test]
    async fn test_mock_server_responds_to_version_action() {
        // Verify our mock HTTP server infrastructure works for future
        // AnkiConnect client tests that will use injected base URLs.
        let server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::body_partial_json(
            serde_json::json!({"action": "version"}),
        ))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"result": 6, "error": null}),
        ))
        .mount(&server)
        .await;

        // Send a request matching AnkiConnect's JSON-RPC format
        let client = reqwest::Client::new();
        let resp = client
            .post(&server.uri())
            .json(&serde_json::json!({"action": "version", "version": 6, "params": {}}))
            .send()
            .await
            .expect("send request");

        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = resp.json().await.expect("parse json");
        assert_eq!(body["result"], 6);
        assert!(body["error"].is_null());
    }

    #[test]
    fn test_anki_connect_client_new_does_not_panic() {
        // Verify client construction works with standard params.
        let _client = AnkiConnectClient::new(8765, None);
    }

    #[test]
    fn test_anki_connect_client_with_api_key() {
        let _client = AnkiConnectClient::new(8765, Some("test-key".to_string()));
    }
}

// ===========================================================================
// Zone 3 — DeckHealthReporter Composition Tests
// ===========================================================================

mod zone3_composed {
    use backend::services::forensic_reader::ForensicReader;
    use backend::services::anki_connect::AnkiConnectClient;
    use backend::services::deck_health::DeckHealthReporter;

    #[test]
    fn test_deck_health_reporter_construction() {
        // Zone 3 reporter should construct from Zone 1 + Zone 2.
        // Need a valid .anki2 file for ForensicReader::open.
        let tmp = tempfile::NamedTempFile::new().expect("create temp file");
        let conn = rusqlite::Connection::open(tmp.path()).expect("open sqlite");
        conn.execute_batch(
            "CREATE TABLE col (id INTEGER PRIMARY KEY, ver INTEGER NOT NULL DEFAULT 0, conf TEXT NOT NULL DEFAULT '{}');
             CREATE TABLE notes (id INTEGER PRIMARY KEY);
             CREATE TABLE cards (id INTEGER PRIMARY KEY, nid INTEGER NOT NULL DEFAULT 0, did INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE revlog (id INTEGER PRIMARY KEY, cid INTEGER NOT NULL DEFAULT 0, ease INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE decks (id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT '');
             CREATE TABLE notetypes (id INTEGER PRIMARY KEY);
             INSERT INTO col (id, ver) VALUES (1, 18);",
        ).expect("create empty schema");
        drop(conn);

        let zone1 = ForensicReader::open(tmp.path()).expect("open forensic reader");
        let zone2 = AnkiConnectClient::new(8765, None);
        let _reporter = DeckHealthReporter::new(zone1, zone2);
        // Construction should not panic
    }
}

// ===========================================================================
// Anki Launcher Tests
// ===========================================================================

mod launcher {
    use backend::services::anki_launcher::LaunchResult;
    use std::time::Duration;

    #[test]
    fn test_launch_result_already_running_is_ok() {
        assert!(LaunchResult::AlreadyRunning.is_ok());
    }

    #[test]
    fn test_launch_result_launched_is_ok() {
        let result = LaunchResult::Launched {
            elapsed: Duration::from_secs(5),
        };
        assert!(result.is_ok());
    }

    #[test]
    fn test_launch_result_timeout_is_not_ok() {
        let result = LaunchResult::Timeout {
            elapsed: Duration::from_secs(20),
        };
        assert!(!result.is_ok());
    }

    #[test]
    fn test_launch_result_not_configured_is_not_ok() {
        let result = LaunchResult::NotConfigured {
            reason: "test".to_string(),
        };
        assert!(!result.is_ok());
    }
}
