//! T-114 (corrected) — exhaustive scenario matrix over AnkiPlayGround collections.
//!
//! Owner dispatch 2026-10-13: P1 = exhaustive testing — T-114 corrected to
//! cover the **backend HTTP surface** + **67 AnkiPlayGround profile dirs**.
//! This suite parameterizes over **every** playground dir holding a
//! `collection.anki2` (65 collections: 28 active + 37 fresh-signature) and
//! drives the deterministic, no-Anki-process surface:
//!
//! 1. **Census pin** — the collection set matches the 2026-10-13 census
//!    (`specs/harness/2026-10-13-t114-census-gap.md`): 65 testable
//!    collections, 28 active / 37 fresh, archetype bands 6/12/10.
//! 2. **Forensic battery** — every `ForensicReader` method against every
//!    collection's real decks (cold health, retention curve, lapse,
//!    interval degradation, adherence, sporadic, gaps, suitability).
//! 3. **FSRS sort invariants** — `fetch_and_sort` over real track tags:
//!    ascending-recall ordering, track-count consistency, determinism.
//! 4. **Recall bounds on real card states** — `compute_approximate_recall`
//!    over sampled (overdue, ivl) pairs from every collection.
//!
//! Deterministic by construction: direct SQLite + pure services, no Anki
//! process, no external model (`ANKITOV_NLU_PROVIDER` untouched).
//!
//! Run: `cargo nextest run -p backend --test profile_scenario_matrix_test`
//! (needs `ANKIPLAYGROUND_PATH` or the default playground location).

mod common;

use backend::services::forensic_reader::{ForensicReader, ForensicError};
use backend::services::fsrs_sort::{compute_approximate_recall, fetch_and_sort};
use common::{
    collection_snapshot_and_stats, classify, deck_names, note_tags, sample_recall_pairs,
    CollectionSnapshot, CollectionStats, ProfileClass,
};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Census expectations (specs/harness/2026-10-13-t114-census-gap.md)
// ---------------------------------------------------------------------------

/// Total dirs holding a `collection.anki2` (35 named + 30 `h2-*`).
const EXPECTED_COLLECTIONS: usize = 65;
/// Active-signature collections (revlog >= 1000).
const EXPECTED_ACTIVE: usize = 28;
/// Fresh-signature collections (7 named + 30 `h2-*` clones).
const EXPECTED_FRESH: usize = 37;

/// One discovered collection with its snapshot + stats.
struct Discovered {
    name: String,
    class: ProfileClass,
    stats: CollectionStats,
    _snapshot: CollectionSnapshot,
    anki2: PathBuf,
}

/// Discover every playground collection, snapshot it (WAL-checkpointed) and
/// compute stats. Panics with a census-style message if the playground is
/// missing — the matrix cannot run without its fixture source.
fn discover_all() -> Vec<Discovered> {
    let base = common::playground_path()
        .expect("AnkiPlayGround not found — set ANKIPLAYGROUND_PATH (census fixture source)");
    let mut out = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&base)
        .expect("read playground")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir() && p.join("collection.anki2").exists())
        .collect();
    entries.sort();
    for dir in entries {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        let (snapshot, stats) = collection_snapshot_and_stats(&dir)
            .unwrap_or_else(|e| panic!("snapshot {name}: {e}"));
        let class = classify(stats);
        let anki2 = snapshot.anki2_path();
        out.push(Discovered {
            name,
            class,
            stats,
            _snapshot: snapshot,
            anki2,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// 1. Census pin
// ---------------------------------------------------------------------------

#[test]
fn census_pins_dispatch_scope() {
    let all = discover_all();
    let active = all.iter().filter(|d| d.class == ProfileClass::Active).count();
    let fresh = all.iter().filter(|d| d.class == ProfileClass::Fresh).count();

    assert_eq!(
        all.len(),
        EXPECTED_COLLECTIONS,
        "collection count drifted from the 2026-10-13 census (65). If AnkiPlayGround \
         legitimately changed, re-run buzz-hive/.scratch/census_profiles.sh and update \
         specs/harness/2026-10-13-t114-census-gap.md + these constants. Found: {:?}",
        all.iter().map(|d| d.name.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(active, EXPECTED_ACTIVE, "active collection count drifted");
    assert_eq!(fresh, EXPECTED_FRESH, "fresh collection count drifted");

    // Every collection must have a sane minimum shape.
    for d in &all {
        assert!(
            d.stats.cards > 0 && d.stats.notes > 0 && d.stats.decks > 0,
            "{}: empty collection (cards={} notes={} decks={})",
            d.name,
            d.stats.cards,
            d.stats.notes,
            d.stats.decks
        );
    }

    // Active profiles: 2331 cards / 6 decks per census; fresh: 1581 / 5.
    for d in &all {
        let (cards, decks) = (d.stats.cards, d.stats.decks);
        match d.class {
            ProfileClass::Active => assert!(
                cards == 2331 && decks == 6,
                "{}: active signature drifted (cards={cards}, decks={decks}, expected 2331/6)",
                d.name
            ),
            ProfileClass::Fresh => assert!(
                cards == 1581 && decks == 5,
                "{}: fresh signature drifted (cards={cards}, decks={decks}, expected 1581/5)",
                d.name
            ),
        }
    }
}

#[test]
fn archetype_bands_match_census() {
    let all = discover_all();
    let mut struggling = Vec::new();
    let mut consistent = Vec::new();
    let mut star = Vec::new();
    for d in all.iter().filter(|d| d.class == ProfileClass::Active) {
        let pct = d.stats.correct_pct;
        if pct < 90.0 {
            struggling.push((d.name.clone(), pct));
        } else if pct <= 93.0 {
            consistent.push((d.name.clone(), pct));
        } else {
            star.push((d.name.clone(), pct));
        }
    }
    // Census bands: 6 struggling / 12 consistent / 10 star.
    assert_eq!(
        struggling.len(),
        6,
        "struggling band drifted: {struggling:?}"
    );
    assert_eq!(
        consistent.len(),
        12,
        "consistent band drifted: {consistent:?}"
    );
    assert_eq!(star.len(), 10, "star band drifted: {star:?}");
}

// ---------------------------------------------------------------------------
// 2. Forensic battery over every collection
// ---------------------------------------------------------------------------

/// Tolerated error on fresh collections: decks without review data in the
/// window legitimately return `NoDataForDeck`.
fn expect_battery_ok<T>(
    profile: &str,
    deck: &str,
    what: &str,
    result: Result<T, ForensicError>,
) -> Option<T> {
    match result {
        Ok(v) => Some(v),
        Err(ForensicError::NoDataForDeck(_, _)) => None,
        Err(e) => panic!("{profile}/{deck}: {what} failed: {e}"),
    }
}

#[test]
fn forensic_battery_over_every_collection() {
    let all = discover_all();
    assert!(!all.is_empty(), "no collections discovered");

    for d in &all {
        let reader = ForensicReader::open(&d.anki2)
            .unwrap_or_else(|e| panic!("{}: ForensicReader::open: {e}", d.name));

        // Cold health — must agree with raw SQLite stats.
        let health = reader
            .cold_health_check()
            .unwrap_or_else(|e| panic!("{}: cold_health_check: {e}", d.name));
        assert_eq!(
            health.card_count as i64, d.stats.cards,
            "{}: health card_count != raw COUNT(cards)",
            d.name
        );
        assert_eq!(
            health.note_count as i64, d.stats.notes,
            "{}: health note_count != raw COUNT(notes)",
            d.name
        );
        assert_eq!(
            health.review_count as i64, d.stats.revlog_entries,
            "{}: health review_count != raw COUNT(revlog)",
            d.name
        );
        assert_eq!(
            health.schema_version, 18,
            "{}: unexpected schema version {}",
            d.name, health.schema_version
        );
        assert_eq!(
            health.orphaned_note_count, 0,
            "{}: {} orphaned notes",
            d.name, health.orphaned_note_count
        );
        assert_eq!(
            health.orphaned_card_count, 0,
            "{}: {} orphaned cards",
            d.name, health.orphaned_card_count
        );

        // Per-deck battery (up to 3 decks per collection keeps the matrix
        // bounded while still covering every collection).
        let decks = deck_names(&d.anki2)
            .unwrap_or_else(|e| panic!("{}: deck_names: {e}", d.name));
        assert!(!decks.is_empty(), "{}: no decks", d.name);
        for deck in decks.iter().take(3) {
            if let Some(curve) = expect_battery_ok(
                &d.name,
                deck,
                "retention_curve",
                reader.retention_curve(deck, 30),
            ) {
                for p in &curve {
                    assert!(
                        p.retention_rate.is_finite() && (0.0..=1.0).contains(&p.retention_rate),
                        "{}/{deck}: retention_rate {} out of [0,1]",
                        d.name,
                        p.retention_rate
                    );
                    assert!(
                        p.lapse_count <= p.review_count,
                        "{}/{deck}: lapses {} exceed reviews {}",
                        d.name,
                        p.lapse_count,
                        p.review_count
                    );
                }
            }
            if let Some(lapse) = expect_battery_ok(
                &d.name,
                deck,
                "lapse_analysis",
                reader.lapse_analysis(deck, 30),
            ) {
                assert!(
                    (0.0..=1.0).contains(&lapse.lapse_rate),
                    "{}/{deck}: lapse_rate {} out of [0,1]",
                    d.name,
                    lapse.lapse_rate
                );
                assert!(
                    lapse.total_lapses <= lapse.total_reviews,
                    "{}/{deck}: total_lapses {} exceed total_reviews {}",
                    d.name,
                    lapse.total_lapses,
                    lapse.total_reviews
                );
            }
            if let Some(interval) = expect_battery_ok(
                &d.name,
                deck,
                "interval_degradation",
                reader.interval_degradation(deck),
            ) {
                assert!(
                    interval.avg_interval_days.is_finite() && interval.avg_interval_days >= 0.0,
                    "{}/{deck}: avg_interval_days {}",
                    d.name,
                    interval.avg_interval_days
                );
                assert!(
                    interval.avg_ease_factor.is_finite() && interval.avg_ease_factor >= 0.0,
                    "{}/{deck}: avg_ease_factor {}",
                    d.name,
                    interval.avg_ease_factor
                );
                // Fresh decks have no revlog rows → ease defaults to 0; an
                // active deck's reviewed cards must always carry ease data.
                if d.class == ProfileClass::Active && interval.avg_ease_factor == 0.0 {
                    assert!(
                        interval.bucket_short_days
                            + interval.bucket_medium_days
                            + interval.bucket_long_days
                            == 0,
                        "{}/{deck}: active deck reports ease 0 with {} interval buckets",
                        d.name,
                        interval.bucket_short_days
                            + interval.bucket_medium_days
                            + interval.bucket_long_days
                    );
                }
            }
            if let Some(adh) = expect_battery_ok(
                &d.name,
                deck,
                "practice_adherence",
                reader.practice_adherence(deck, 30),
            ) {
                assert!(
                    adh.score <= 100,
                    "{}/{deck}: adherence score {} > 100",
                    d.name,
                    adh.score
                );
                assert!(
                    adh.active_days <= adh.total_days,
                    "{}/{deck}: active_days {} > total_days {}",
                    d.name,
                    adh.active_days,
                    adh.total_days
                );
            }
            if let Some(spor) = expect_battery_ok(
                &d.name,
                deck,
                "sporadic_index",
                reader.sporadic_index(deck, 30),
            ) {
                assert!(
                    (0.0..=1.0).contains(&spor.index),
                    "{}/{deck}: sporadic index {} out of [0,1]",
                    d.name,
                    spor.index
                );
                assert!(
                    (0.0..=1.0).contains(&spor.coverage),
                    "{}/{deck}: coverage {} out of [0,1]",
                    d.name,
                    spor.coverage
                );
            }
            if let Some(gaps) = expect_battery_ok(
                &d.name,
                deck,
                "gap_analysis",
                reader.gap_analysis(deck, 30),
            ) {
                assert!(
                    gaps.avg_gap_days.is_finite() && gaps.avg_gap_days >= 0.0,
                    "{}/{deck}: avg_gap_days {}",
                    d.name,
                    gaps.avg_gap_days
                );
                assert!(
                    gaps.max_gap_days as f64 >= gaps.avg_gap_days,
                    "{}/{deck}: max_gap {} < avg_gap {}",
                    d.name,
                    gaps.max_gap_days,
                    gaps.avg_gap_days
                );
            }
            if let Some(suit) = expect_battery_ok(
                &d.name,
                deck,
                "deck_suitability",
                reader.deck_suitability(deck, 30),
            ) {
                assert!(
                    (0.0..=1.0).contains(&suit.lapse_rate),
                    "{}/{deck}: suitability lapse_rate {} out of [0,1]",
                    d.name,
                    suit.lapse_rate
                );
                assert!(
                    !suit.verdict.is_empty(),
                    "{}/{deck}: empty suitability verdict",
                    d.name
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 3. FSRS sort invariants over every collection
// ---------------------------------------------------------------------------

#[test]
fn fsrs_sort_invariants_over_every_collection() {
    let all = discover_all();

    for d in &all {
        let tags = note_tags(&d.anki2)
            .unwrap_or_else(|e| panic!("{}: note_tags: {e}", d.name));
        // Bound the tag set: fetch_and_sort ORs tags together; the full
        // distinct set (30+ per collection) is fine but redundant — take a
        // stable subset covering every top-level track prefix.
        let mut chosen: Vec<String> = Vec::new();
        for t in &tags {
            let prefix = t.split("::").next().unwrap_or(t).to_string();
            if !chosen
                .iter()
                .any(|c| c.split("::").next() == Some(prefix.as_str()))
            {
                chosen.push(t.clone());
            }
            if chosen.len() >= 8 {
                break;
            }
        }

        let result = fetch_and_sort(&d.anki2, &chosen, 30)
            .unwrap_or_else(|e| panic!("{}: fetch_and_sort: {e}", d.name));

        // INVARIANT I — ascending approximate_recall.
        for w in result.ranked.windows(2) {
            assert!(
                w[0].approximate_recall <= w[1].approximate_recall,
                "{}: sort order violated: {} ({}) before {} ({})",
                d.name,
                w[0].card_id,
                w[0].approximate_recall,
                w[1].card_id,
                w[1].approximate_recall
            );
        }

        // INVARIANT N — track counts consistent with ranked set.
        let total: usize = result.track_counts.values().sum();
        assert_eq!(
            total,
            result.ranked.len(),
            "{}: track_counts sum {total} != ranked.len() {}",
            d.name,
            result.ranked.len()
        );

        // INVARIANT K — deterministic: identical inputs → identical order.
        let again = fetch_and_sort(&d.anki2, &chosen, 30)
            .unwrap_or_else(|e| panic!("{}: fetch_and_sort (2nd): {e}", d.name));
        let a: Vec<i64> = result.ranked.iter().map(|c| c.card_id).collect();
        let b: Vec<i64> = again.ranked.iter().map(|c| c.card_id).collect();
        assert_eq!(a, b, "{}: sort not deterministic", d.name);

        // Time estimate must be positive and finite.
        assert!(
            result.avg_seconds_per_card.is_finite() && result.avg_seconds_per_card > 0.0,
            "{}: avg_seconds_per_card {}",
            d.name,
            result.avg_seconds_per_card
        );
    }
}

// ---------------------------------------------------------------------------
// 4. Recall bounds on real card states
// ---------------------------------------------------------------------------

#[test]
fn recall_bounds_on_real_card_states() {
    let all = discover_all();
    let mut sampled = 0usize;

    for d in &all {
        let pairs = sample_recall_pairs(&d.anki2, 40)
            .unwrap_or_else(|e| panic!("{}: sample_recall_pairs: {e}", d.name));
        for (overdue, ivl) in &pairs {
            let r = compute_approximate_recall(*overdue, *ivl);
            assert!(
                r.is_finite() && (0.0..=1.0).contains(&r),
                "{}: recall {r} out of [0,1] for (overdue={overdue}, ivl={ivl})",
                d.name
            );
            if *overdue > 0 {
                assert!(
                    r <= 0.90,
                    "{}: due-card recall {r} exceeded the 0.90 base \
                     (overdue={overdue}, ivl={ivl})",
                    d.name
                );
            }
            sampled += 1;
        }
    }
    assert!(
        sampled >= 500,
        "expected to sample recall pairs from the active collections, got {sampled}"
    );
}