//! Property tests — FSRS urgency-sort invariants.
//!
//! Source of truth: `specs/audits/fsrs-sort-invariants.txt` (INVARIANT I—O).
//! Spec: `specs/harness/2026-10-04-test-gates.md` §4.
//!
//! Covers the computable core — `compute_approximate_recall` — which is where
//! the urgency ordering actually comes from. Invariants L, M, N and O are
//! DB-bound (`fetch_and_sort` against a real `.anki2`) and are deferred to
//! M2; the pattern for that already exists in `tests/anki_zones_test.rs`.
//!
//! Run: `cargo nextest run property_fsrs_sort`
//! Stress: `PROPTEST_CASES=10000 cargo nextest run property_fsrs_sort`

use backend::services::fsrs_sort::compute_approximate_recall;
use proptest::prelude::*;

/// The base recall returned for a card that is not yet due (`overdue_days <= 0`).
const NOT_DUE_RECALL: f64 = 0.95;

proptest! {
    // ── INVARIANT I — never NaN ────────────────────────────────────────────
    #[test]
    fn recall_is_never_nan(overdue_days in any::<i32>(), ivl in any::<i64>()) {
        let r = compute_approximate_recall(overdue_days, ivl);
        prop_assert!(!r.is_nan(), "NaN recall for (overdue={}, ivl={})", overdue_days, ivl);
        prop_assert!(r.is_finite(), "non-finite recall for (overdue={}, ivl={})", overdue_days, ivl);
    }

    // ── INVARIANT I — inside the documented 0.0–1.0 range ──────────────────
    #[test]
    fn recall_within_unit_range(overdue_days in any::<i32>(), ivl in any::<i64>()) {
        let r = compute_approximate_recall(overdue_days, ivl);
        prop_assert!(
            (0.0..=1.0).contains(&r),
            "recall {} outside [0,1] for (overdue={}, ivl={})", r, overdue_days, ivl
        );
        // A due card decays from the FSRS 0.90 base retention, never above it.
        if overdue_days > 0 {
            prop_assert!(r <= 0.90, "due-card recall {} exceeded the 0.90 base", r);
        }
    }

    // ── INVARIANT I — monotonic: more overdue ⇒ no higher recall ────────────
    #[test]
    fn recall_non_increasing_in_overdue_days(
        earlier in any::<i32>(),
        gap in any::<u32>(),
        ivl in any::<i64>(),
    ) {
        // No `prop_assume!(earlier <= later)`. Filtering is not free: proptest
        // gives a rejection budget of 1024 for the whole test, and a ~50 %
        // reject rate blows it once PROPTEST_CASES is raised — the failure
        // surfaces as "Test aborted: Too many global rejects", which reads like
        // a flaky assertion but is really the reject budget running out.
        // Generating the gap instead of filtering makes the precondition hold by
        // construction, so this test scales to any case count.
        let later = earlier.saturating_add(gap.min(i32::MAX as u32) as i32);
        let a = compute_approximate_recall(earlier, ivl);
        let b = compute_approximate_recall(later, ivl);
        prop_assert!(
            a >= b,
            "recall increased with overdue_days: f({})={} > f({})={}",
            earlier, a, later, b
        );
    }

    // ── INVARIANT I — strictly decreasing once a card is due ───────────────
    // This is what makes the sort total: two due cards can never tie on recall
    // unless their overdue days are equal.
    //
    // Domain note (measured, not assumed): recall = 0.90 * 0.5^(days/ivl), and
    // `0.5^x` falls off the bottom of f64 at x > ~1074. The strict claim is
    // therefore asserted over the domain the urgency sort actually operates in —
    // up to 1000x the card's interval — and the saturation boundary gets its own
    // test (`recall_saturates_at_zero_past_the_f64_floor`). Over `any::<i64>()`
    // intervals two adjacent days can differ by 1e-18 in the exponent, which is
    // far below f64 resolution, so "strictly" would be false by rounding alone.
    #[test]
    fn recall_strictly_decreasing_when_overdue(
        earlier in 1i32..=1_000,
        gap in 1u32..=1_000,
        ivl in 1i64..=100,
    ) {
        // `gap` starts at 1 so `later > earlier` holds by construction — see the
        // note in `recall_non_increasing_in_overdue_days` about reject budgets.
        let later = earlier.saturating_add(gap as i32);
        let a = compute_approximate_recall(earlier, ivl);
        let b = compute_approximate_recall(later, ivl);
        prop_assert!(a > b, "recall not strictly decreasing: f({})={} <= f({})={}", earlier, a, later, b);
    }

    // ── The saturation boundary, pinned so a future change to the formula is
    // noticed. A card 2000x past its interval reads 0.0, never a negative or
    // NaN "extra urgent" value. This is benign — INVARIANT K's tie-break
    // (overdue_days desc, then card_id asc) keeps the sort deterministic once
    // deeply-overdue cards tie at 0.0 — but it IS a real precision limit, so it
    // is written down here rather than left as folklore.
    #[test]
    fn recall_saturates_at_zero_past_the_f64_floor(ratio in 1_100i32..=20_000, ivl in 1i64..=1_000) {
        // The floor lives in the EXPONENT (0.5^ratio), not in overdue_days, so
        // the strategy drives the ratio and derives the day count from it.
        let days = ratio as i64 * ivl;

        let r = compute_approximate_recall(days as i32, ivl);
        prop_assert!(r.is_finite(), "recall became non-finite: {}", r);
        prop_assert!(r >= 0.0, "recall went negative (saturation is the floor): {}", r);
        prop_assert_eq!(r, 0.0, "expected saturation at overdue/ivl > ~1074, got {}", r);

        // ...and pin the boundary from the other side: one step below the floor
        // the value must still be representable and strictly positive.
        let just_under = compute_approximate_recall((1_073i64 * ivl) as i32, ivl);
        prop_assert!(just_under > 0.0,
            "recall saturated early at overdue/ivl = 1073 (got {})", just_under);
    }

    // ── INVARIANT J — cross-domain independence (purity) ───────────────────
    // Urgency must depend ONLY on this card's own (overdue_days, ivl). If the
    // function carried hidden state — a global, a cache, a counter — interleaving
    // calls from "another domain" would perturb the result. Bitwise comparison
    // makes the check exact.
    #[test]
    fn recall_is_pure(
        a_days in any::<i32>(), a_ivl in any::<i64>(),
        b_days in any::<i32>(), b_ivl in any::<i64>(),
        c_days in any::<i32>(), c_ivl in any::<i64>(),
    ) {
        let solo_a = compute_approximate_recall(a_days, a_ivl);
        let solo_c = compute_approximate_recall(c_days, c_ivl);

        // Interleave a call from a "different domain" between the two.
        let _noise = compute_approximate_recall(b_days, b_ivl);
        let inter_a = compute_approximate_recall(a_days, a_ivl);
        let _noise = compute_approximate_recall(b_days, b_ivl);
        let inter_c = compute_approximate_recall(c_days, c_ivl);

        prop_assert_eq!(solo_a.to_bits(), inter_a.to_bits(), "recall for ({} ,{}) changed with interleaved calls", a_days, a_ivl);
        prop_assert_eq!(solo_c.to_bits(), inter_c.to_bits(), "recall for ({} ,{}) changed with interleaved calls", c_days, c_ivl);
    }

    // ── INVARIANT J — domains order independently of each other ─────────────
    // A math card (few days overdue, short interval) must outrank a vocabulary
    // card (barely overdue, long interval): lower recall ⇒ more urgent.
    #[test]
    fn recall_orders_across_domains(
        calm_days in 1i32..=1_000,
        extra in 1i32..=1_000,
        calm_ivl in 1i64..=30,
        ivl_bias in any::<u64>(),
    ) {
        // Constructed, not filtered — see the reject-budget note above.
        // `urgent_days = calm_days + extra` with `extra >= 1` gives
        // urgent_days > calm_days and urgent_days >= 2; folding the interval
        // gives 1..=calm_ivl with no rejection.
        let urgent_days = calm_days.saturating_add(extra);
        let urgent_ivl = 1 + (ivl_bias % calm_ivl as u64) as i64;

        // "More overdue per unit interval" is the whole comparison, so assert it
        // on the exponent and let recall follow. Asserting strictness on recall
        // directly would be wrong: 10d/1 and 30d/3 are the same exponent, so
        // those two cards legitimately tie.
        let urgent_exponent = urgent_days as f64 / urgent_ivl as f64;
        let calm_exponent = calm_days as f64 / calm_ivl as f64;
        prop_assert!(urgent_exponent >= calm_exponent,
            "urgent card ({}/{}) has a lower decay exponent than calm card ({}/{})",
            urgent_days, urgent_ivl, calm_days, calm_ivl);

        let urgent = compute_approximate_recall(urgent_days, urgent_ivl);
        let calm = compute_approximate_recall(calm_days, calm_ivl);

        if urgent_exponent > calm_exponent {
            prop_assert!(urgent < calm,
                "urgent card ({}d/{}) recall {} did not outrank calm card ({}d/{}) recall {}",
                urgent_days, urgent_ivl, urgent, calm_days, calm_ivl, calm);
        } else {
            prop_assert_eq!(urgent.to_bits(), calm.to_bits(),
                "equal decay exponents must produce equal recall");
        }
    }

    // ── Interval clamping: no division by zero for degenerate intervals ─────
    #[test]
    fn recall_clamps_ivl_at_or_below_one(overdue_days in any::<i32>(), ivl in -1_000_000i64..=1) {
        let clamped = compute_approximate_recall(overdue_days, ivl);
        let one = compute_approximate_recall(overdue_days, 1);
        prop_assert_eq!(
            clamped.to_bits(), one.to_bits(),
            "ivl={} should behave exactly like ivl=1", ivl
        );
    }

    // ── INVARIANT K — determinism ───────────────────────────────────────────
    #[test]
    fn recall_is_deterministic(overdue_days in any::<i32>(), ivl in any::<i64>()) {
        let first = compute_approximate_recall(overdue_days, ivl);
        for _ in 0..4 {
            let again = compute_approximate_recall(overdue_days, ivl);
            prop_assert_eq!(first.to_bits(), again.to_bits(), "recall is not deterministic");
        }
    }

    // ── A not-yet-due card is pinned above every due card ──────────────────
    #[test]
    fn not_yet_due_outranks_due(overdue_days in 1i32..100_000, ivl in any::<i64>()) {
        let due = compute_approximate_recall(overdue_days, ivl);
        prop_assert!(NOT_DUE_RECALL > due, "due card recall {} was not below {}", due, NOT_DUE_RECALL);

        // The not-due branch is `overdue_days <= 0`, so 0 is the boundary and
        // 1 is the first due day. Both ends are pinned so the branch cannot
        // creep by one without a counterexample here.
        prop_assert_eq!(compute_approximate_recall(0, ivl), NOT_DUE_RECALL, "day 0 must be not-due");
        prop_assert_eq!(compute_approximate_recall(-1, ivl), NOT_DUE_RECALL, "day -1 must be not-due");
        prop_assert!(compute_approximate_recall(1, ivl) < NOT_DUE_RECALL,
            "day 1 is already due and must fall below the not-due pin");
    }
}