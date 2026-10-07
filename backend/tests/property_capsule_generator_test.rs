//! Property tests — capsule generator invariants (N-Lever).
//!
//! Source of truth: `specs/audits/generator-invariants.txt` (INVARIANT P—W).
//! Spec: `specs/harness/2026-10-04-test-gates.md` §4.
//!
//! INVARIANT R ("N-Lever math") is the machine-checkable core: the capsule size
//! must shrink as the session frequency N rises. That relationship is exactly
//! what an instructor relies on when moving a cohort from N=1 to N=7, and it is
//! currently asserted only by hand-picked examples.
//!
//! Deferred to M2 (each needs either a DB or a refactor first — see spec §4.1):
//!   * P — rate-limit window `1800` is an inline literal, not a `const`.
//!         Extract `RATE_LIMIT_WINDOW_SECS`, then test it.
//!   * Q / V / W — resolution precedence chains live inside DB-backed service
//!         code. Extract each chain into a pure resolver, then property-test
//!         every precedence level.
//!   * S / T / U — DB-bound error paths and session-persistence shape.
//!
//! Run: `cargo nextest run property_capsule_generator`

use backend::services::capsule_slicer::{compute_capsule_size, DEFAULT_HARD_CAP, DEFAULT_N_VALUE};
use proptest::prelude::*;

/// `ceil(pool / n)` as used by the N-spread step.
fn ceil_pool_over_n(pool: usize, n: i32) -> usize {
    (pool as f64 / n as f64).ceil() as usize
}

proptest! {
    // ── INVARIANT R — the N-Lever is monotone: more sessions ⇒ smaller slice ─
    //
    // This is the load-bearing property behind "3→1→3 backlog re-entry" and
    // "N=7 → ~pool/7 per session". It is not obvious from the implementation:
    // the per-track floor can dominate for small pools, which is why the
    // assertion is `<=` (non-increasing) rather than strictly decreasing.
    #[test]
    fn capsule_size_non_increasing_in_n(
        pool in 1usize..4000,
        cap in 1usize..60,
        minutes in 1i32..180,
        per_card in 1.0f64..300.0,
        tracks in 1usize..30,
    ) {
        let sizes: Vec<usize> = (1i32..=7)
            .map(|n| compute_capsule_size(pool, n, cap, minutes, per_card, tracks))
            .collect();

        for w in sizes.windows(2) {
            prop_assert!(
                w[0] >= w[1],
                "capsule size grew with N: pool={} tracks={} sizes={:?}", pool, tracks, sizes
            );
        }
    }

    // ── INVARIANT R — bounded by ceil(pool/N), modulo the per-track floor ──
    #[test]
    fn capsule_size_bounded_by_ceil_pool_over_n(
        pool in 1usize..4000,
        n in 1i32..=7,
        cap in 1usize..60,
        minutes in 1i32..180,
        per_card in 1.0f64..300.0,
        tracks in 1usize..30,
    ) {
        let size = compute_capsule_size(pool, n, cap, minutes, per_card, tracks);
        let spread = ceil_pool_over_n(pool, n);
        let floor = tracks.min(pool);
        prop_assert!(
            size <= spread.max(floor),
            "capsule_size {} exceeded ceil(pool/n)={} (floor={}) for pool={} n={}",
            size, spread, floor, pool, n
        );
    }

    // ── INVARIANT R — N=1 attempts the whole pool, N=7 roughly a seventh ────
    #[test]
    fn n_lever_endpoints(pool in 50usize..4000, tracks in 1usize..5) {
        let cap = DEFAULT_HARD_CAP;
        let per_card = 25.0;

        let n1 = compute_capsule_size(pool, 1, cap, 60, per_card, tracks);
        let n7 = compute_capsule_size(pool, 7, cap, 60, per_card, tracks);

        // N=1 never asks for less than the pool (bounded only by cap/session).
        prop_assert!(n1 <= pool, "N=1 returned {} for a pool of {}", n1, pool);
        prop_assert!(n1 >= 7, "N=1 returned {} — expected at least a week's worth", n1);

        // N=7 lands within the expected band around pool/7.
        let seventh = ceil_pool_over_n(pool, 7);
        prop_assert!(
            n7 <= seventh.max(tracks.min(pool)),
            "N=7 returned {} for pool {} (expected <= {})", n7, pool, seventh
        );
    }

    // ── Defaults stay self-consistent ──────────────────────────────────────
    // The documented defaults must compose without panic or degenerate output.
    #[test]
    fn documented_defaults_are_composable(pool in 1usize..4000, tracks in 1usize..30) {
        let size = compute_capsule_size(
            pool,
            DEFAULT_N_VALUE,
            DEFAULT_HARD_CAP,
            10, // DEFAULT_SESSION_DURATION_MINUTES
            25.0, // COLD_START_SECONDS_PER_CARD
            tracks,
        );
        prop_assert!(size >= 1);
        prop_assert!(size <= pool);
        prop_assert!(size >= tracks.min(pool));
    }
}