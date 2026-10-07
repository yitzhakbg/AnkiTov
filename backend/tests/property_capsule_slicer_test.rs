//! Property tests — capsule slicing invariants.
//!
//! Source of truth: `specs/audits/capsule-slicing-invariants.txt`
//! (INVARIANT A—H). Spec: `specs/harness/2026-10-04-test-gates.md` §4.
//!
//! The existing `imp_pipeline_test.rs` covers these rules with hand-picked
//! examples (e.g. pool=90, N=3 → 24). These suites generalise each rule over
//! the whole input domain, which is what catches the *interactions* — the
//! per-track floor versus the time bound versus the hard cap.
//!
//! Pure functions only — no DB, no Anki, no server. Fast enough for pre-merge.
//!
//! Run: `cargo nextest run property_capsule_slicer`
//! Stress: `PROPTEST_CASES=10000 cargo nextest run property_capsule_slicer`

use backend::services::capsule_slicer::{compute_capsule_size, slice_capsule, TrackCounts};
use proptest::prelude::*;

/// Track tags used by the generators. Small, closed set so per-track assertions
/// stay meaningful.
fn track_tag() -> impl Strategy<Value = String> {
    prop::sample::select(vec!["math", "vocab", "physics"]).prop_map(String::from)
}

/// The track floor applied by `compute_capsule_size`, restated here so the
/// property states the contract independently of the implementation.
fn track_floor(num_tracks: usize, pool_size: usize) -> usize {
    num_tracks.min(pool_size)
}

/// The time bound as documented in INVARIANT E, with the implementation's
/// cold-start fallback (25 s/card when the estimate is non-positive).
fn time_bound(session_minutes: i32, per_card_seconds: f64) -> usize {
    let per_card = if per_card_seconds > 0.0 {
        per_card_seconds
    } else {
        capsule_slicer_cold_start()
    };
    ((session_minutes as f64 * 60.0) / per_card).floor() as usize
}

fn capsule_slicer_cold_start() -> f64 {
    backend::services::capsule_slicer::COLD_START_SECONDS_PER_CARD
}

proptest! {
    // ── INVARIANT A — the documented worked example (regression guard) ──────
    #[test]
    fn documented_example_holds(pool in 0usize..5000, n in 1i32..12, cap in 1usize..40) {
        // The spec's example: pool=90, N=3, hard_cap=25, 10 min, 25 s/card, 3 tracks → 24.
        prop_assert_eq!(compute_capsule_size(90, 3, 25, 10, 25.0, 3), 24);

        // Generalised: the function must be total — never panic, never saturate
        // on any input in the domain, regardless of the reference example.
        let _ = compute_capsule_size(pool, n, cap, 10, 25.0, 3);
    }

    // ── INVARIANT B — zero pool or non-positive N yields zero ───────────────
    #[test]
    fn zero_pool_or_non_positive_n_yields_zero(
        pool in 0usize..200,
        n in -5i32..=0,
        cap in 0usize..50,
        minutes in -10i32..120,
        per_card in 0.0f64..120.0,
        tracks in 0usize..30,
    ) {
        prop_assert_eq!(compute_capsule_size(pool, n, cap, minutes, per_card, tracks), 0);
        prop_assert_eq!(compute_capsule_size(0, 3, 25, 10, 25.0, 3), 0);
    }

    // ── INVARIANT C — never exceeds the pool ────────────────────────────────
    #[test]
    fn never_exceeds_pool(
        pool in 0usize..4000,
        n in 1i32..12,
        cap in 0usize..60,
        minutes in 0i32..180,
        per_card in 0.0f64..300.0,
        tracks in 0usize..30,
    ) {
        let size = compute_capsule_size(pool, n, cap, minutes, per_card, tracks);
        prop_assert!(size <= pool, "capsule_size {} exceeded pool {}", size, pool);
    }

    // ── INVARIANT D — per-track minimum presence floor ──────────────────────
    #[test]
    fn meets_per_track_floor(
        pool in 1usize..4000,
        n in 1i32..12,
        cap in 1usize..60,
        minutes in 1i32..180,
        per_card in 1.0f64..300.0,
        tracks in 1usize..30,
    ) {
        let size = compute_capsule_size(pool, n, cap, minutes, per_card, tracks);
        let floor = track_floor(tracks, pool);
        prop_assert!(
            size >= floor,
            "capsule_size {} below per-track floor {} (pool={} tracks={})",
            size, floor, pool, tracks
        );
    }

    // ── INVARIANT E — time bound, WEAKENED (see spec §4.2) ──────────────────
    //
    // The audit doc states `size <= floor(minutes*60 / per_card)`. That is NOT
    // universally true, by deliberate design: `compute_capsule_size` applies
    // `.max(track_floor)` AFTER the time bound, so covering every assigned
    // track wins over strict session duration (module header, accepted
    // 2026-08-07). Concretely (pool=90, N=3, cap=25, 10min, 25s, 30 tracks)
    // returns 30 — above both the hard cap and the time bound of 24.
    //
    // The executable contract is therefore:
    //     size <= time_bound.max(track_floor)
    #[test]
    fn respects_time_bound_or_track_floor(
        pool in 1usize..4000,
        n in 1i32..12,
        cap in 1usize..60,
        minutes in 1i32..180,
        per_card in 1.0f64..300.0,
        tracks in 1usize..30,
    ) {
        let size = compute_capsule_size(pool, n, cap, minutes, per_card, tracks);
        let tb = time_bound(minutes, per_card);
        let tf = track_floor(tracks, pool);
        prop_assert!(
            size <= tb.max(tf),
            "capsule_size {} exceeds max(time_bound={}, track_floor={})",
            size, tb, tf
        );
    }

    // ── Never below 1 when there is work to do ─────────────────────────────
    #[test]
    fn at_least_one_card_when_pool_is_non_empty(
        pool in 1usize..4000,
        n in 1i32..12,
        cap in 0usize..60,
        minutes in 0i32..180,
        per_card in 0.0f64..300.0,
        tracks in 0usize..30,
    ) {
        let size = compute_capsule_size(pool, n, cap, minutes, per_card, tracks);
        prop_assert!(size >= 1, "capsule_size 0 with a non-empty pool of {}", pool);
    }
}

// ── slice_capsule ────────────────────────────────────────────────────────────

proptest! {
    // ── INVARIANT F — completeness: nothing is lost or duplicated ───────────
    #[test]
    fn slice_is_complete(
        ranked in prop::collection::vec((any::<i64>(), track_tag()), 0..60),
        target in 0usize..40,
    ) {
        let mut counts: TrackCounts = TrackCounts::new();
        for (_, tag) in &ranked {
            *counts.entry(tag.clone()).or_insert(0) += 1;
        }
        let result = slice_capsule(&ranked, &counts, target);
        prop_assert_eq!(
            result.selected.len() + result.remaining.len(),
            ranked.len(),
            "completeness violated (selected={} remaining={} ranked={})",
            result.selected.len(), result.remaining.len(), ranked.len()
        );
    }

    // ── INVARIANT H — actual size never exceeds the target ─────────────────
    #[test]
    fn slice_respects_target_size(
        ranked in prop::collection::vec((any::<i64>(), track_tag()), 0..60),
        target in 0usize..40,
    ) {
        let mut counts: TrackCounts = TrackCounts::new();
        for (_, tag) in &ranked {
            *counts.entry(tag.clone()).or_insert(0) += 1;
        }
        let result = slice_capsule(&ranked, &counts, target);
        prop_assert!(result.actual_size <= target);
        prop_assert_eq!(result.actual_size, result.selected.len());
        prop_assert_eq!(result.target_size, target);
    }

    // ── No card is both selected and spilled ───────────────────────────────
    #[test]
    fn no_card_is_selected_and_remaining(
        ranked in prop::collection::vec((any::<i64>(), track_tag()), 0..60),
        target in 0usize..40,
    ) {
        let mut counts: TrackCounts = TrackCounts::new();
        for (_, tag) in &ranked {
            *counts.entry(tag.clone()).or_insert(0) += 1;
        }
        let result = slice_capsule(&ranked, &counts, target);
        for id in &result.selected {
            prop_assert!(
                !result.remaining.contains(id),
                "card {} appears in both selected and remaining", id
            );
        }
    }

    // ── INVARIANT G — per-track presence, given enough capacity ────────────
    //
    // Each track that actually has due cards must contribute at least one
    // card, PROVIDED the capsule has room for one card per such track. Without
    // that precondition the property is simply false (a target of 2 cannot
    // represent 5 tracks) — so the precondition is part of the contract.
    #[test]
    fn slice_gives_each_track_a_card(
        ranked in prop::collection::vec((any::<i64>(), track_tag()), 1..60),
        extra_target in 0usize..20,
    ) {
        let mut counts: TrackCounts = TrackCounts::new();
        for (_, tag) in &ranked {
            *counts.entry(tag.clone()).or_insert(0) += 1;
        }
        let target = counts.len() + extra_target; // >= one slot per live track

        let result = slice_capsule(&ranked, &counts, target);

        let mut selected_tags: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (id, _) in &ranked {
            if result.selected.contains(id) {
                selected_tags.insert(
                    ranked.iter().find(|(i, _)| i == id).map(|(_, t)| t.as_str()).unwrap_or(""),
                );
            }
        }
        for (tag, count) in &counts {
            if *count > 0 {
                prop_assert!(
                    selected_tags.contains(tag.as_str()),
                    "track {:?} has {} due cards but none were selected", tag, count
                );
            }
        }
    }

    // ── Empty input is total: no panic, nothing selected, all spilled ──────
    #[test]
    fn empty_input_is_total(target in 0usize..40) {
        let result = slice_capsule(&[], &TrackCounts::new(), target);
        prop_assert!(result.selected.is_empty());
        prop_assert!(result.remaining.is_empty());
        prop_assert_eq!(result.actual_size, 0);
    }
}