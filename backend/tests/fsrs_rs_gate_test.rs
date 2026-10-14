//! T-116 fsrs-rs adoption gate — state-transition correctness (specs/tasks.md).
//!
//! Owner directive 2026-10-13: adoption of `open-spaced-repetition/fsrs-rs`
//! (BSD-3-Clause) is the intended path; this suite is the correctness half of
//! the gate. The in-house `services/fsrs_sort.rs` approximate-recall sorter
//! stays as the fallback behind `property_fsrs_sort_test.rs` /
//! `property_scheduling_invariants_test.rs` until the swap lands.
//!
//! Anchors:
//! - Reference values for the first transition come from the fsrs-rs 6.6.2
//!   (crates.io release) `FSRS::next_states` doc example: default FSRS-6
//!   parameters, desired_retention 0.9, no prior state, days_elapsed 0.
//! - Invariants mirror the property suite that guards the in-house sorter, so
//!   the swap boundary is defined as "fsrs-rs satisfies every ordering
//!   invariant the in-house model guarantees, plus real memory-state dynamics".

use fsrs::{current_retrievability, FSRS6_DEFAULT_DECAY, FSRS, MemoryState, NextStates};

/// Tolerance for doc-example anchors: fsrs-rs docs round to 4-7 decimal places.
const ANCHOR_TOL: f32 = 5e-4;
/// Relative tolerance for interval anchors.
const INTERVAL_RTOL: f32 = 1e-3;

fn assert_anchor(actual: f32, expected: f32, what: &str) {
    let tol = if expected.abs() > 1.0 { expected.abs() * INTERVAL_RTOL } else { ANCHOR_TOL };
    assert!(
        (actual - expected).abs() <= tol,
        "{what}: expected {expected}, got {actual} (tol {tol})"
    );
}

/// Reference anchor: first transition of a new card under default FSRS-6
/// parameters must match the fsrs-rs 6.6.2 documented values exactly (within
/// doc rounding). This pins the crate version to reference behavior.
#[test]
fn reference_anchor_first_transition_matches_fsrs_docs() {
    let fsrs = FSRS::default();
    let ns: NextStates = fsrs.next_states(None, 0.9, 0).unwrap();

    // again: stability 0.212, difficulty 6.4133, interval 0.212
    assert_anchor(ns.again.memory.stability, 0.212, "again.stability");
    assert_anchor(ns.again.memory.difficulty, 6.4133, "again.difficulty");
    assert_anchor(ns.again.interval, 0.212, "again.interval");
    // hard: stability 1.2931, difficulty 5.1121707, interval 1.2931
    assert_anchor(ns.hard.memory.stability, 1.2931, "hard.stability");
    assert_anchor(ns.hard.memory.difficulty, 5.1121707, "hard.difficulty");
    assert_anchor(ns.hard.interval, 1.2931, "hard.interval");
    // good: stability 2.3065, difficulty 2.118104, interval 2.3065
    assert_anchor(ns.good.memory.stability, 2.3065, "good.stability");
    assert_anchor(ns.good.memory.difficulty, 2.118104, "good.difficulty");
    assert_anchor(ns.good.interval, 2.3065, "good.interval");
    // easy: stability 8.2956, difficulty 1.0, interval 8.2956
    assert_anchor(ns.easy.memory.stability, 8.2956, "easy.stability");
    assert_anchor(ns.easy.memory.difficulty, 1.0, "easy.difficulty");
    assert_anchor(ns.easy.interval, 8.2956, "easy.interval");
}

/// Learned states to exercise transitions from (span the difficulty/stability
/// space: new-ish, mature, struggling, strong).
fn learned_states() -> Vec<MemoryState> {
    vec![
        MemoryState { stability: 0.5, difficulty: 7.0 },
        MemoryState { stability: 2.3065, difficulty: 2.118104 },
        MemoryState { stability: 12.0, difficulty: 5.5 },
        MemoryState { stability: 45.0, difficulty: 2.5 },
        MemoryState { stability: 120.0, difficulty: 8.5 },
    ]
}

/// Ordering invariant: for every learned state, higher rating ⇒ higher next
/// stability, lower next difficulty, longer next interval. This is the
/// invariant the in-house sorter's ordering relies on.
///
/// Same-day nuance (elapsed == 0): fsrs-rs short-term stability clamps the
/// multiplier at 1.0 for ratings >= 2 (`sinc.max(1.0)`, model.rs), and with
/// default FSRS-6 parameters (w18 = w19 = 0) both hard and good leave
/// stability exactly unchanged — so hard == good is correct upstream behavior
/// same-day. Strict ordering is asserted for elapsed >= 1 (long-term formula
/// with hard_penalty < 1, easy_bonus > 1); elapsed == 0 asserts
/// again < hard <= good < easy.
#[test]
fn invariant_rating_ordering_stability_difficulty_interval() {
    let fsrs = FSRS::default();
    for state in learned_states() {
        for elapsed in [0u32, 1, 3, 7, 30] {
            let ns = fsrs.next_states(Some(state), 0.9, elapsed).unwrap();
            if elapsed == 0 {
                // Same-day: again < hard <= good < easy (hard/good may tie —
                // short-term clamp leaves both at the pre-review stability).
                assert!(ns.again.memory.stability < ns.hard.memory.stability, "again < hard (stability): {state:?}");
                assert!(ns.hard.memory.stability <= ns.good.memory.stability, "hard <= good (stability): {state:?}");
                assert!(ns.good.memory.stability < ns.easy.memory.stability, "good < easy (stability): {state:?}");
                assert!(ns.again.interval < ns.hard.interval, "again < hard (interval): {state:?}");
                assert!(ns.hard.interval <= ns.good.interval, "hard <= good (interval): {state:?}");
                assert!(ns.good.interval < ns.easy.interval, "good < easy (interval): {state:?}");
            } else {
                // Long-term formula: strict ordering across all four ratings.
                let quad = [&ns.again, &ns.hard, &ns.good, &ns.easy];
                for w in quad.windows(2) {
                    assert!(
                        w[0].memory.stability < w[1].memory.stability,
                        "stability must increase with rating: state {state:?} elapsed {elapsed}: {} < {}",
                        w[0].memory.stability,
                        w[1].memory.stability
                    );
                    assert!(
                        w[0].interval < w[1].interval,
                        "interval must increase with rating: state {state:?} elapsed {elapsed}"
                    );
                }
            }
            // Difficulty ordering is strict at every elapsed (difficulty update
            // has no same-day clamp).
            let quad = [&ns.again, &ns.hard, &ns.good, &ns.easy];
            for w in quad.windows(2) {
                assert!(
                    w[0].memory.difficulty > w[1].memory.difficulty,
                    "difficulty must decrease with rating: state {state:?} elapsed {elapsed}"
                );
            }
        }
    }
}

/// Success monotonicity: 'good'/'easy' never decrease stability of a learned
/// state; 'again' always drops it below the pre-review stability.
#[test]
fn invariant_success_never_loses_stability_again_always_drops() {
    let fsrs = FSRS::default();
    for state in learned_states() {
        let ns = fsrs.next_states(Some(state), 0.9, 1).unwrap();
        assert!(ns.good.memory.stability >= state.stability, "good must not lose stability: {state:?} -> {}", ns.good.memory.stability);
        assert!(ns.easy.memory.stability >= state.stability, "easy must not lose stability: {state:?} -> {}", ns.easy.memory.stability);
        assert!(ns.again.memory.stability < state.stability, "again must drop stability: {state:?} -> {}", ns.again.memory.stability);
    }
}

/// Difficulty stays in the FSRS-6 [1, 10] band across a long deterministic
/// rating walk (no panic, no NaN, no drift outside bounds).
#[test]
fn invariant_difficulty_bounded_on_long_walk() {
    let fsrs = FSRS::default();
    // Deterministic LCG walk — 2000 transitions, mixed ratings.
    let mut x: u64 = 0x2545F4914F6CDD1D;
    let mut state: Option<MemoryState> = None;
    for step in 0..2000 {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let rating = 1 + (x >> 33) % 4; // 1..=4
        let elapsed = (x >> 20) % 14; // 0..=13 days
        let ns = fsrs
            .next_states(state, 0.9, elapsed as u32)
            .unwrap_or_else(|e| panic!("step {step} rating {rating}: {e}"));
        let next = match rating {
            1 => ns.again,
            2 => ns.hard,
            3 => ns.good,
            _ => ns.easy,
        };
        let m = next.memory;
        assert!(m.stability.is_finite() && m.difficulty.is_finite(), "non-finite state at step {step}: {m:?}");
        assert!(
            (1.0..=10.0).contains(&m.difficulty),
            "difficulty out of [1,10] at step {step}: {}",
            m.difficulty
        );
        assert!(m.stability > 0.0, "stability must stay positive at step {step}: {}", m.stability);
        assert!(next.interval.is_finite() && next.interval > 0.0, "bad interval at step {step}: {}", next.interval);
        state = Some(m);
    }
}

/// Recall curve: retrievability is 1.0 at elapsed 0, strictly decreasing in
/// elapsed days, stays in (0, 1], and recall at t = stability is 0.9 (the
/// half-life definition the interval scheduler is built on).
#[test]
fn invariant_retrievability_monotone_decay_and_half_life() {
    for state in learned_states() {
        let r0 = current_retrievability(state, 0.0, FSRS6_DEFAULT_DECAY);
        assert!((r0 - 1.0).abs() < 1e-6, "recall at elapsed 0 must be 1.0, got {r0}");
        let mut prev = r0;
        for d in [1.0f32, 2.0, 5.0, 10.0, 30.0, 90.0, 365.0] {
            let r = current_retrievability(state, d, FSRS6_DEFAULT_DECAY);
            assert!(r > 0.0 && r <= 1.0, "recall out of (0,1] at {d}d: {r}");
            assert!(r < prev, "recall must decrease with elapsed: {d}d {r} <= prev {prev}");
            prev = r;
        }
        // Half-life: R(stability) must sit at 0.9 under the FSRS-6 decay.
        let r_at_s = current_retrievability(state, state.stability, FSRS6_DEFAULT_DECAY);
        assert!(
            (r_at_s - 0.9).abs() < 1e-3,
            "R(stability={}) should be 0.9, got {r_at_s}",
            state.stability
        );
    }
}

/// Desired-retention monotonicity: higher desired retention ⇒ shorter
/// scheduled interval for the same state (tighter safety margin).
#[test]
fn invariant_higher_desired_retention_shortens_interval() {
    let fsrs = FSRS::default();
    for state in learned_states() {
        let loose = fsrs.next_states(Some(state), 0.80, 1).unwrap();
        let tight = fsrs.next_states(Some(state), 0.95, 1).unwrap();
        assert!(
            tight.good.interval < loose.good.interval,
            "0.95 retention interval {} must be shorter than 0.80 interval {}",
            tight.good.interval,
            loose.good.interval
        );
    }
}

/// Determinism / Copy semantics: the same state transitioned twice yields
/// identical results — no hidden mutation of the input state.
#[test]
fn invariant_transition_is_deterministic_and_copy_safe() {
    let fsrs = FSRS::default();
    let state = learned_states()[2];
    let before = state;
    let a = fsrs.next_states(Some(state), 0.9, 3).unwrap();
    let b = fsrs.next_states(Some(state), 0.9, 3).unwrap();
    assert_eq!(a, b, "same input must give identical NextStates");
    assert_eq!(state, before, "input MemoryState must not be mutated");
}

/// Swap-boundary compatibility: fsrs-rs recall ordering agrees with the
/// in-house half-life ordering on due cards. The in-house model ranks by
/// `0.90 * 0.5^(overdue/ivl)`; fsrs-rs ranks by the power forgetting curve
/// with stability ≈ ivl. For any pair where in-house says A is safer than B,
/// fsrs-rs must agree — this is what makes the sorter swap order-preserving.
#[test]
fn recall_ordering_agrees_with_in_house_half_life_model() {
    // (overdue_days, ivl) pairs spanning the sorter's operating range.
    let pairs: Vec<(i32, i64)> = vec![
        (1, 1), (1, 3), (2, 3), (1, 7), (5, 7), (3, 10), (10, 10),
        (7, 21), (20, 21), (15, 30), (40, 30), (30, 90), (100, 90), (200, 365),
    ];
    let recall_fsrs = |overdue: i32, ivl: i64| -> f32 {
        current_retrievability(
            MemoryState { stability: ivl as f32, difficulty: 5.0 },
            overdue as f32,
            FSRS6_DEFAULT_DECAY,
        )
    };
    let recall_in_house = |overdue: i32, ivl: i64| -> f64 {
        0.90 * 0.5f64.powf(overdue as f64 / ivl.max(1) as f64)
    };
    for i in 0..pairs.len() {
        for j in (i + 1)..pairs.len() {
            let (a, b) = (pairs[i], pairs[j]);
            let (fa, fb) = (recall_fsrs(a.0, a.1), recall_fsrs(b.0, b.1));
            let (ia, ib) = (recall_in_house(a.0, a.1), recall_in_house(b.0, b.1));
            // Same strict ordering (ties on either side are fine).
            let fsrs_order = fa.partial_cmp(&fb).unwrap();
            let in_house_order = ia.partial_cmp(&ib).unwrap();
            assert_eq!(
                fsrs_order, in_house_order,
                "ordering disagreement on {a:?} vs {b:?}: fsrs {fa}/{fb}, in-house {ia}/{ib}"
            );
        }
    }
}