//! T-115 — property-based scheduling invariants (proptest).
//!
//! Extends `property_fsrs_sort_test.rs` (which covers INVARIANT I on
//! `compute_approximate_recall` alone) to the DB-bound invariants L/M/N/O,
//! determinism (K) and cross-domain independence (J) over **synthetic
//! review histories**, plus FSRS recall/interval bounds and card-pair
//! interference mixing (capsule slicing, IMP-18 presence guarantee).
//!
//! Invariant source of truth: `specs/audits/fsrs-sort-invariants.txt`.
//!
//! Evidence gate: `PROPTEST_CASES=10000 cargo nextest run -p backend
//! --test property_scheduling_invariants_test` (>= 10,000 randomized
//! state transitions per property).

use backend::services::capsule_slicer::{compute_capsule_size, slice_capsule};
use backend::services::fsrs_sort::{compute_approximate_recall, fetch_and_sort};
use proptest::prelude::*;
use rusqlite::Connection;
use std::collections::BTreeSet;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Synthetic collection builder
// ---------------------------------------------------------------------------

/// One synthetic card: (id, note tags, ivl, type, due, queue).
type CardSpec = (i64, String, i64, i64, i64, i64);

/// Build a minimal schema-v18 `.anki2` (col/notes/cards/revlog/decks/
/// notetypes) with the given cards, in a temp dir. Returns the dir — the
/// caller keeps it alive for the duration of the property.
fn build_collection(crt: i64, cards: &[CardSpec]) -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("collection.anki2");
    let conn = Connection::open(&path).expect("open synthetic collection");

    conn.execute_batch(
        "CREATE TABLE col (
            id INTEGER PRIMARY KEY, crt INTEGER NOT NULL DEFAULT 0,
            mod INTEGER NOT NULL DEFAULT 0, scm INTEGER NOT NULL DEFAULT 0,
            ver INTEGER NOT NULL DEFAULT 0, dty INTEGER NOT NULL DEFAULT 0,
            usn INTEGER NOT NULL DEFAULT 0, ls INTEGER NOT NULL DEFAULT 0,
            conf TEXT NOT NULL DEFAULT '{}', models TEXT NOT NULL DEFAULT '{}',
            decks TEXT NOT NULL DEFAULT '{}', dconf TEXT NOT NULL DEFAULT '{}',
            tags TEXT NOT NULL DEFAULT '');
         CREATE TABLE notes (
            id INTEGER PRIMARY KEY, guid TEXT NOT NULL DEFAULT '',
            mid INTEGER NOT NULL DEFAULT 0, mod INTEGER NOT NULL DEFAULT 0,
            usn INTEGER NOT NULL DEFAULT 0, tags TEXT NOT NULL DEFAULT '',
            flds TEXT NOT NULL DEFAULT '', sfld TEXT NOT NULL DEFAULT '',
            csum INTEGER NOT NULL DEFAULT 0, flags INTEGER NOT NULL DEFAULT 0,
            data TEXT NOT NULL DEFAULT '');
         CREATE TABLE cards (
            id INTEGER PRIMARY KEY, nid INTEGER NOT NULL DEFAULT 0,
            did INTEGER NOT NULL DEFAULT 0, ord INTEGER NOT NULL DEFAULT 0,
            mod INTEGER NOT NULL DEFAULT 0, usn INTEGER NOT NULL DEFAULT 0,
            type INTEGER NOT NULL DEFAULT 0, queue INTEGER NOT NULL DEFAULT 0,
            due INTEGER NOT NULL DEFAULT 0, ivl INTEGER NOT NULL DEFAULT 0,
            factor INTEGER NOT NULL DEFAULT 0, reps INTEGER NOT NULL DEFAULT 0,
            lapses INTEGER NOT NULL DEFAULT 0, left INTEGER NOT NULL DEFAULT 0,
            odue INTEGER NOT NULL DEFAULT 0, odid INTEGER NOT NULL DEFAULT 0,
            flags INTEGER NOT NULL DEFAULT 0, data TEXT NOT NULL DEFAULT '');
         CREATE TABLE revlog (
            id INTEGER PRIMARY KEY, cid INTEGER NOT NULL DEFAULT 0,
            usn INTEGER NOT NULL DEFAULT 0, ease INTEGER NOT NULL DEFAULT 0,
            ivl INTEGER NOT NULL DEFAULT 0, last_ivl INTEGER NOT NULL DEFAULT 0,
            factor INTEGER NOT NULL DEFAULT 0, time INTEGER NOT NULL DEFAULT 0,
            type INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE decks (
            id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT '',
            mtime_secs INTEGER NOT NULL DEFAULT 0, usn INTEGER NOT NULL DEFAULT 0,
            common TEXT NOT NULL DEFAULT '{}', kind TEXT NOT NULL DEFAULT '{}');
         CREATE TABLE notetypes (
            id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT '',
            mtime_secs INTEGER NOT NULL DEFAULT 0, usn INTEGER NOT NULL DEFAULT 0,
            config TEXT NOT NULL DEFAULT '{}', fields TEXT NOT NULL DEFAULT '[]',
            templates TEXT NOT NULL DEFAULT '[]', type INTEGER NOT NULL DEFAULT 0);",
    )
    .expect("create synthetic schema");

    conn.execute(
        "INSERT INTO col (id, crt, mod, scm, ver, conf) VALUES (1, ?1, 0, 0, 18, '{}')",
        [crt],
    )
    .expect("insert col");
    conn.execute(
        "INSERT INTO decks (id, name, mtime_secs, usn, common, kind) VALUES (1, 'Synthetic', 0, 0, '{}', '{}')",
        [],
    )
    .expect("insert deck");

    for (cid, tags, ivl, ctype, due, queue) in cards {
        conn.execute(
            "INSERT INTO notes (id, guid, mid, mod, usn, tags, flds, sfld, csum, flags, data)
             VALUES (?1, '', 1, 0, 0, ?2, '', '', 0, 0, '')",
            rusqlite::params![cid, tags],
        )
        .expect("insert note");
        conn.execute(
            "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data)
             VALUES (?1, ?1, 1, 0, 0, 0, ?2, ?3, ?4, ?5, 2500, 0, 0, 0, 0, 0, 0, '')",
            rusqlite::params![cid, ctype, queue, due, ivl],
        )
        .expect("insert card");
    }
    dir
}

/// The collection creation date used by the generator: 100 days ago, so
/// `today_day == 100` and generated review dues (0..=80) never sit on the
/// `due <= today_day` boundary (no midnight-crossing flakiness).
fn synthetic_crt() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64;
    now - 100 * 86400
}

/// Days-since-crt the service will compute (100, modulo test-run midnight
/// crossings which the generator avoids by keeping dues well below 100).
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
}

/// Card spec WITHOUT id — ids are assigned uniquely by [`card_vec_strategy`]
/// (notes.id has a UNIQUE constraint).
type CardShape = (String, i64, i64, i64, i64);

/// Track tag pool for generated notes.
fn tag_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("alpha".to_string()),
        Just("beta".to_string()),
        Just("gamma".to_string()),
        Just("alpha beta".to_string()),
        Just("".to_string()),
    ]
}

/// Card spec strategy. Type-1 dues are kept far from `now` (±2..10 days) so
/// the `due <= now_secs` predicate cannot flip between the test's clock read
/// and the service's.
fn card_shape_strategy() -> impl Strategy<Value = CardShape> {
    (
        tag_strategy(),
        0i64..=36500,
        0i64..=2,
        prop_oneof![
            // type-2 style day dues (relative to crt), strictly < 100
            0i64..=80,
            // type-1 style epoch dues, ±2..10 days from now
            (2i64..10).prop_map(|d| now_secs() - d * 86400),
            (2i64..10).prop_map(|d| now_secs() + d * 86400),
        ],
        -3i64..=3,
    )
        .prop_map(|(tags, ivl, ctype, due, queue)| (tags, ivl, ctype, due, queue))
}

/// A vec of card specs with UNIQUE ids (1000 + index) — notes.id is UNIQUE.
fn card_vec_strategy(min: usize, max: usize) -> impl Strategy<Value = Vec<CardSpec>> {
    prop::collection::vec(card_shape_strategy(), min..max).prop_map(|shapes| {
        shapes
            .into_iter()
            .enumerate()
            .map(|(i, (tags, ivl, ctype, due, queue))| {
                (1000 + i as i64, tags, ivl, ctype, due, queue)
            })
            .collect()
    })
}

/// The service's admission predicate, recomputed in the test from the specs
/// (INVARIANT L ground truth).
fn expected_admitted(cards: &[CardSpec], tags: &[String]) -> BTreeSet<i64> {
    let crt = synthetic_crt();
    let today_day = (now_secs() - crt) / 86400;
    let now = now_secs();
    cards
        .iter()
        .filter(|(_, note_tags, _, ctype, due, queue)| {
            if *queue < 0 {
                return false;
            }
            let tag_match = tags.iter().any(|t| note_tags.contains(t.as_str()));
            if !tag_match {
                return false;
            }
            match ctype {
                2 => *due <= today_day,
                1 => *due <= now,
                0 => true,
                _ => false,
            }
        })
        .map(|(id, ..)| *id)
        .collect()
}

// ---------------------------------------------------------------------------
// INVARIANT L — due-card filter
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn due_filter_matches_spec(cards in card_vec_strategy(0, 40)) {
        let crt = synthetic_crt();
        let dir = build_collection(crt, &cards);
        let path = dir.path().join("collection.anki2");

        let result = fetch_and_sort(&path, &["alpha".to_string(), "beta".to_string()], 30)
            .expect("fetch_and_sort on synthetic collection");
        let got: BTreeSet<i64> = result.ranked.iter().map(|c| c.card_id).collect();
        let want = expected_admitted(&cards, &["alpha".to_string(), "beta".to_string()]);
        prop_assert_eq!(got, want, "admitted set diverges from the L predicate");
    }
}

// ---------------------------------------------------------------------------
// INVARIANT M — empty tags → empty result, no file I/O
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn empty_tags_short_circuits(cards in card_vec_strategy(1, 20)) {
        let dir = build_collection(synthetic_crt(), &cards);

        // Existing file: empty tags → Ok, empty, no panic.
        let result = fetch_and_sort(dir.path().join("collection.anki2"), &[], 30)
            .expect("empty tags must be Ok");
        prop_assert!(result.ranked.is_empty());
        prop_assert!(result.track_counts.is_empty());

        // Even a nonexistent path must short-circuit before any file I/O.
        let missing = fetch_and_sort("/nonexistent/t115/missing.anki2", &[], 30);
        prop_assert!(missing.is_ok(), "empty tags must not touch the filesystem");
        prop_assert!(missing.unwrap().ranked.is_empty());
    }
}

// ---------------------------------------------------------------------------
// INVARIANT O — missing database → CannotOpen, never panic
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn missing_db_is_cannot_open(tags in prop::collection::vec("[a-z]{3,8}", 1..4)) {
        let result = fetch_and_sort("/nonexistent/t115/missing.anki2", &tags, 30);
        match result {
            Err(backend::services::fsrs_sort::FsrsSortError::CannotOpen(_, _)) => {}
            other => prop_assert!(false, "expected CannotOpen, got {:?}", other.map(|r| r.ranked.len())),
        }
    }
}

// ---------------------------------------------------------------------------
// INVARIANT K + I — deterministic, urgency-ordered, tie-broken
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn deterministic_urgency_ordered_tie_broken(cards in card_vec_strategy(2, 40)) {
        let dir = build_collection(synthetic_crt(), &cards);
        let path = dir.path().join("collection.anki2");
        let tags = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];

        let a = fetch_and_sort(&path, &tags, 30).expect("first run");
        let b = fetch_and_sort(&path, &tags, 30).expect("second run");

        // K — identical order across runs.
        let ids_a: Vec<i64> = a.ranked.iter().map(|c| c.card_id).collect();
        let ids_b: Vec<i64> = b.ranked.iter().map(|c| c.card_id).collect();
        prop_assert_eq!(ids_a, ids_b, "sort order not deterministic");

        // I — ascending recall; exactly-equal recalls tie-broken by
        // overdue desc, then card id asc. A "tie" is EXACT equality —
        // the production sort compares with `partial_cmp`, so distinct
        // tiny recalls (e.g. 0.0 vs 1.4e-201 from ivl=0 vs ivl=31 at
        // ~20k days overdue) are legitimately ordered by recall, not
        // by the tie-break chain. Epsilon-proximity here misclassified
        // those as ties (regression: cards [(1000,…,31,…), (1001,…,0,…)]).
        for w in a.ranked.windows(2) {
            prop_assert!(
                !w[0].approximate_recall.is_nan() && !w[1].approximate_recall.is_nan(),
                "recall must be NaN-free (invariant I)"
            );
            prop_assert!(
                w[0].approximate_recall <= w[1].approximate_recall,
                "recall order violated: {} ({}) before {} ({})",
                w[0].card_id, w[0].approximate_recall, w[1].card_id, w[1].approximate_recall
            );
            if w[0].approximate_recall == w[1].approximate_recall {
                prop_assert!(
                    w[0].overdue_days >= w[1].overdue_days,
                    "tie not broken by overdue desc"
                );
                if w[0].overdue_days == w[1].overdue_days {
                    prop_assert!(w[0].card_id <= w[1].card_id, "tie not broken by id asc");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// INVARIANT N — track counts consistent with the ranked set
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn track_counts_match_truth(cards in card_vec_strategy(1, 40)) {
        let dir = build_collection(synthetic_crt(), &cards);
        let path = dir.path().join("collection.anki2");
        let tags = vec!["alpha".to_string(), "beta".to_string()];

        let result = fetch_and_sort(&path, &tags, 30).expect("fetch_and_sort");

        let total: usize = result.track_counts.values().sum();
        prop_assert_eq!(total, result.ranked.len(), "track_counts sum != ranked.len()");

        // Per-tag ground truth from the specs.
        let want = expected_admitted(&cards, &["alpha".to_string(), "beta".to_string()]);
        let mut per_tag: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for (id, note_tags, ..) in &cards {
            if want.contains(id) {
                let matched = ["alpha", "beta"]
                    .iter()
                    .find(|t| note_tags.contains(&t.to_string()))
                    .unwrap_or(&"alpha");
                *per_tag.entry(matched.to_string()).or_insert(0) += 1;
            }
        }
        for (tag, count) in &per_tag {
            let msg = format!("track_counts[{tag}] diverges from ground truth");
            prop_assert_eq!(result.track_counts.get(tag), Some(count), "{}", msg);
        }
    }
}

// ---------------------------------------------------------------------------
// INVARIANT J — cross-domain independence
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn cross_domain_independence(
        b_ivl in 1i64..=36500,
        b_due in 0i64..=80,
        a_ivl in 1i64..=36500,
        a_due in 0i64..=80,
    ) {
        // Card A (alpha) and card B (beta). Mutating A's scheduling state
        // must never change B's recall VALUE (J: no cross-track leakage).
        let base = vec![
            (1001i64, "alpha".to_string(), a_ivl, 2i64, a_due, 2i64),
            (1002i64, "beta".to_string(), b_ivl, 2, b_due, 2),
        ];
        let mutated = vec![
            (1001i64, "alpha".to_string(), 36500 - a_ivl, 2, 80 - a_due, 2),
            (1002i64, "beta".to_string(), b_ivl, 2, b_due, 2),
        ];

        let dir_a = build_collection(synthetic_crt(), &base);
        let dir_b = build_collection(synthetic_crt(), &mutated);

        let ra = fetch_and_sort(dir_a.path().join("collection.anki2"), &["beta".to_string()], 30)
            .expect("base run");
        let rb = fetch_and_sort(dir_b.path().join("collection.anki2"), &["beta".to_string()], 30)
            .expect("mutated run");

        prop_assert_eq!(
            ra.ranked.iter().map(|c| c.approximate_recall).collect::<Vec<_>>(),
            rb.ranked.iter().map(|c| c.approximate_recall).collect::<Vec<_>>(),
            "mutating card A changed card B's recall"
        );
    }
}

// ---------------------------------------------------------------------------
// FSRS recall bounds + monotonicity over synthetic histories
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn recall_bounds_and_monotonicity(
        overdue in -100i32..=1000,
        ivl in 0i64..=36500,
        overdue2 in -100i32..=1000,
        ivl2 in 0i64..=36500,
    ) {
        let r = compute_approximate_recall(overdue, ivl);
        prop_assert!(r.is_finite(), "non-finite recall");
        prop_assert!((0.0..=1.0).contains(&r), "recall {r} out of [0,1]");
        if overdue > 0 {
            prop_assert!(r <= 0.90, "due-card recall {r} above the 0.90 base");
            prop_assert!(r > 0.0, "due-card recall must be positive");
        } else {
            prop_assert_eq!(r, 0.95, "not-due recall must be the 0.95 base");
        }

        // Monotonic non-increasing in overdue (fixed ivl).
        let r2 = compute_approximate_recall(overdue2, ivl);
        if overdue <= overdue2 {
            prop_assert!(r >= r2, "recall increased with overdue: {r} -> {r2}");
        }

        // Monotonic non-decreasing in ivl (fixed positive overdue).
        if overdue > 0 && ivl <= ivl2 {
            let r3 = compute_approximate_recall(overdue, ivl2);
            prop_assert!(r <= r3, "recall decreased with interval: {r} -> {r3}");
        }
    }
}

// ---------------------------------------------------------------------------
// Capsule sizing bounds
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn capsule_size_bounds(
        pool in 0usize..=500,
        n in 0i32..=10,
        hard_cap in 1usize..=100,
        duration in 1i32..=60,
        per_card in 0.5f64..=60.0,
        tracks in 0usize..=10,
    ) {
        let size = compute_capsule_size(pool, n, hard_cap, duration, per_card, tracks);
        // Invariant G (documented in compute_capsule_size): the per-track
        // presence floor can LIFT the size above the hard cap, up to one
        // card per track — but never above the pool.
        let presence_floor = tracks.min(pool);
        prop_assert!(
            size <= hard_cap.max(presence_floor),
            "size {size} exceeds hard_cap {hard_cap} + presence floor {presence_floor}"
        );
        if pool == 0 || n <= 0 {
            prop_assert_eq!(size, 0, "empty pool or non-positive n must yield 0");
        } else {
            prop_assert!(size <= pool, "size {size} exceeds pool {pool}");
            prop_assert!(size > 0, "non-empty pool with n>0 must yield a positive capsule");
        }
    }
}

// ---------------------------------------------------------------------------
// Card-pair interference mixing — slice_capsule (IMP-18 presence + partition)
// ---------------------------------------------------------------------------

fn ranked_strategy() -> impl Strategy<Value = Vec<(i64, String)>> {
    prop::collection::vec((1000i64..9000, prop_oneof![Just("alpha"), Just("beta"), Just("gamma")]), 0..40)
        .prop_map(|v| {
            let mut seen = BTreeSet::new();
            v.into_iter()
                .filter(|(id, _)| seen.insert(*id))
                .map(|(id, t)| (id, t.to_string()))
                .collect()
        })
}

proptest! {
    #[test]
    fn slice_partitions_and_preserves_order(
        ranked in ranked_strategy(),
        target in 0usize..=30,
    ) {
        // Track counts consistent with the ranked pool.
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for (_, tag) in &ranked {
            *counts.entry(tag.clone()).or_insert(0) += 1;
        }

        let result = slice_capsule(&ranked, &counts, target);

        // Partition: selected ∪ remaining == input, disjoint.
        let all: BTreeSet<i64> = ranked.iter().map(|(id, _)| *id).collect();
        let sel: BTreeSet<i64> = result.selected.iter().copied().collect();
        let rem: BTreeSet<i64> = result.remaining.iter().copied().collect();
        prop_assert_eq!(sel.len(), result.selected.len(), "selected has duplicates");
        prop_assert!(sel.is_disjoint(&rem), "selected and remaining overlap");
        let mut union = sel.clone();
        union.extend(rem);
        prop_assert_eq!(union, all, "slice is not a partition of the pool");

        // Size contract.
        prop_assert_eq!(result.actual_size, result.selected.len());
        prop_assert!(result.actual_size <= target, "actual_size exceeds target");

        // Urgency order preserved: selected is a subsequence of ranked.
        let mut it = ranked.iter().map(|(id, _)| *id);
        for id in &result.selected {
            loop {
                match it.next() {
                    Some(next) if next == *id => break,
                    Some(_) => continue,
                    None => prop_assert!(false, "selected {id} breaks ranked order"),
                }
            }
        }

        // IMP-18 presence guarantee: when the target can hold every track,
        // every track must appear in the selection.
        let num_tracks = counts.len();
        if target >= num_tracks && !ranked.is_empty() {
            let sel_tags: BTreeSet<String> = result
                .selected
                .iter()
                .map(|id| ranked.iter().find(|(rid, _)| *rid == *id).unwrap().1.clone())
                .collect();
            let msg = format!(
                "presence guarantee violated: tracks {num_tracks}, selected tags {sel_tags:?}"
            );
            prop_assert_eq!(sel_tags.len(), num_tracks, "{}", msg);
        }
    }
}