//! Capsule Slicer — compute target capsule size and slice ranked card lists.
//!
//! Pure computation module (no I/O). Takes pool metadata and returns
//! the optimal capsule size, then slices a pre-sorted card list to that size
//! while enforcing per-track presence guarantees.
//!
//! ## Formula
//!
//! ```text
//! capsule_size = min(
//!     ceil(pool_size / N),                 // N-spread across sessions
//!     hard_cap,                            // absolute ceiling
//!     floor(duration_seconds / avg_time)   // time-bound
//! )
//! capsule_size = max(capsule_size, min_track_presence)
//! ```
//!
//! ## Design Tradeoff (Reasonix Invariant E)
//!
//! When `num_tracks > floor(duration_seconds / avg_time)`, the per-track
//! minimum presence floor can push `capsule_size` above the time-bound.
//! This is intentional: instructors prioritize covering every assigned track
//! over strict session duration. The tradeoff is accepted as of 2026-08-07.
//!
//! Cards not selected remain in the overdue pool for subsequent sessions.
//! The caller is responsible for persistence — this module only computes
//! which cards to include.

/// The result of slicing a ranked card list: the selected cards and metadata.
#[derive(Debug, Clone)]
pub struct SliceResult {
    /// Card IDs selected for this capsule.
    pub selected: Vec<i64>,
    /// Card IDs left in the pool (spill to next session).
    pub remaining: Vec<i64>,
    /// Computed capsule size before per-track clamping.
    pub target_size: usize,
    /// Actual number of cards selected.
    pub actual_size: usize,
}

/// Per-track card counts (track_tag → count of due cards).
pub type TrackCounts = std::collections::HashMap<String, usize>;

/// Compute the target capsule size from pool metadata.
///
/// # Arguments
/// * `pool_size` - Total number of overdue cards across all assigned tracks.
/// * `n_value` - Sessions per week (from TrackProfile or cohort override).
/// * `hard_cap` - Absolute ceiling (default 25, adjustable).
/// * `session_duration_minutes` - Session length from TrackProfile (default 10).
/// * `estimated_per_card_time_seconds` - Historical average or cold-start default.
/// * `num_tracks` - Number of distinct tracks in the profile.
pub fn compute_capsule_size(
    pool_size: usize,
    n_value: i32,
    hard_cap: usize,
    session_duration_minutes: i32,
    estimated_per_card_time_seconds: f64,
    num_tracks: usize,
) -> usize {
    // Guard: zero pool means zero capsule
    if pool_size == 0 || n_value <= 0 {
        return 0;
    }

    // N-spread: divide pool across N sessions
    let n_spread = (pool_size as f64 / n_value as f64).ceil() as usize;

    // Time-bound: how many cards fit in the session window
    let session_seconds = session_duration_minutes as f64 * 60.0;
    let time_per_card = if estimated_per_card_time_seconds > 0.0 {
        estimated_per_card_time_seconds
    } else {
        25.0 // cold-start fallback
    };
    let time_bound = (session_seconds / time_per_card).floor() as usize;

    // Take the minimum of all constraints
    let mut capsule_size = n_spread.min(hard_cap).min(time_bound);

    // Min per-track presence: ≥ 1 card per track.
    // Also guarantee capsule can hold at least one card per track
    // when pool supplies enough, so slice_capsule's per-track minimum
    // is always satisfiable (Invariant G).
    let track_floor = num_tracks.min(pool_size);
    capsule_size = capsule_size.max(track_floor);

    // Never exceed pool_size
    capsule_size = capsule_size.min(pool_size);

    // Never fall below 1 (we got here → there's at least 1 due card)
    capsule_size = capsule_size.max(1);

    capsule_size
}

/// Slice a pre-sorted (most urgent first) list of card+track entries into
/// a capsule, enforcing per-track presence.
///
/// Each entry is `(card_id, track_tag)`. Sorted by FSRS urgency (lowest
/// recall first) by the caller.
///
/// # Guarantees
/// - At least 1 card per track (unless the track has no due cards).
/// - Total selected ≤ target_size.
/// - Remaining cards returned for spill.
pub fn slice_capsule(
    ranked: &[(i64, String)],       // (card_id, track_tag), sorted most-urgent first
    track_counts: &TrackCounts,     // total due cards per track
    target_size: usize,
) -> SliceResult {
    if ranked.is_empty() || target_size == 0 {
        return SliceResult {
            selected: Vec::new(),
            remaining: ranked.iter().map(|(id, _)| *id).collect(),
            target_size,
            actual_size: 0,
        };
    }

    let mut selected = Vec::with_capacity(target_size);
    let mut used_from_track: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut remaining = Vec::new();

    // Compute per-track quota: at least 1, up to proportional share
    let num_tracks = track_counts.len();
    let min_per_track = if num_tracks > 0 { 1 } else { 0 };
    let max_per_track = (target_size / num_tracks.max(1)).max(min_per_track);

    // Pass 1: select cards, respecting per-track min and overall max
    for (card_id, track_tag) in ranked {
        if selected.len() >= target_size {
            remaining.push(*card_id);
            continue;
        }

        let used = used_from_track.get(track_tag).copied().unwrap_or(0);

        // If we haven't met min-per-track for this track, always include
        if used < min_per_track {
            selected.push(*card_id);
            used_from_track.insert(track_tag.clone(), used + 1);
            continue;
        }

        // If we're under the proportional cap, include
        if used < max_per_track {
            selected.push(*card_id);
            used_from_track.insert(track_tag.clone(), used + 1);
            continue;
        }

        // Otherwise, check if we still have room and all tracks have min
        if selected.len() < target_size {
            // Allow overflow if all tracks have their minimum
            let all_have_min = track_counts.keys().all(|t| {
                used_from_track.get(t).copied().unwrap_or(0) >= min_per_track
            });
            if all_have_min {
                selected.push(*card_id);
                used_from_track.insert(track_tag.clone(), used + 1);
                continue;
            }
        }

        remaining.push(*card_id);
    }

    let actual_size = selected.len();
    SliceResult {
        selected,
        remaining,
        target_size,
        actual_size,
    }
}

/// Cold-start default: 25 seconds per card.
/// Used when no historical timing data exists for a student+track combination.
pub const COLD_START_SECONDS_PER_CARD: f64 = 25.0;

/// Default hard cap for capsule size.
pub const DEFAULT_HARD_CAP: usize = 25;

/// Default session duration in minutes.
pub const DEFAULT_SESSION_DURATION_MINUTES: i32 = 10;

/// Default N value (sessions per week).
pub const DEFAULT_N_VALUE: i32 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_capsule_size_basic() {
        // pool=90, N=3, hard_cap=25, 10min session, 25s/card, 3 tracks
        // n_spread = ceil(90/3) = 30
        // time_bound = floor(600/25) = 24
        // min(n_spread=30, hard_cap=25, time=24) = 24
        // min_presence = min(3, ceil(24*0.2)=5) = 3
        // max(24, 3) = 24 → clamp to pool (90) → 24
        let size = compute_capsule_size(90, 3, 25, 10, 25.0, 3);
        assert_eq!(size, 24);
    }

    #[test]
    fn test_compute_capsule_size_small_pool() {
        // pool=10, N=3, hard_cap=25, 10min, 25s/card, 3 tracks
        // n_spread = ceil(10/3) = 4
        // time_bound = 24
        // min(4, 25, 24) = 4
        // min_presence = min(3, ceil(4*0.2)=1) = 1
        // max(4, 1) = 4
        let size = compute_capsule_size(10, 3, 25, 10, 25.0, 3);
        assert_eq!(size, 4);
    }

    #[test]
    fn test_compute_capsule_size_time_bound_dominates() {
        // pool=500, N=3, hard_cap=25, 5min session, 25s/card, 1 track
        // n_spread = ceil(500/3) = 167
        // time_bound = floor(300/25) = 12
        // min(167, 25, 12) = 12
        // min_presence = min(1, ceil(12*0.2)=3) = 1
        // max(12, 1) = 12
        let size = compute_capsule_size(500, 3, 25, 5, 25.0, 1);
        assert_eq!(size, 12);
    }

    #[test]
    fn test_compute_capsule_size_zero_pool() {
        let size = compute_capsule_size(0, 3, 25, 10, 25.0, 3);
        assert_eq!(size, 0);
    }

    #[test]
    fn test_compute_capsule_size_n1() {
        // N=1 (all cards in one session)
        // pool=60, hard_cap=25, 10min, 25s, 3 tracks
        // n_spread = ceil(60/1) = 60
        // time_bound = 24
        // min(60, 25, 24) = 24
        let size = compute_capsule_size(60, 1, 25, 10, 25.0, 3);
        assert_eq!(size, 24);
    }

    #[test]
    fn test_slice_capsule_basic() {
        let ranked = vec![
            (1, "math".into()),
            (2, "vocab".into()),
            (3, "math".into()),
            (4, "science".into()),
            (5, "vocab".into()),
            (6, "math".into()),
        ];
        let mut counts = TrackCounts::new();
        counts.insert("math".into(), 3);
        counts.insert("vocab".into(), 2);
        counts.insert("science".into(), 1);

        let result = slice_capsule(&ranked, &counts, 4);

        // Should include at least 1 from each track
        let track_ids: std::collections::HashMap<String, Vec<i64>> = {
            let mut map = std::collections::HashMap::new();
            for (id, tag) in &ranked {
                if result.selected.contains(id) {
                    map.entry(tag.clone()).or_insert_with(Vec::new).push(*id);
                }
            }
            map
        };

        assert_eq!(result.actual_size, 4);
        assert_eq!(result.target_size, 4);
        assert!(track_ids.contains_key("math"));
        assert!(track_ids.contains_key("vocab"));
        assert!(track_ids.contains_key("science"));
        assert_eq!(result.remaining.len(), 2);
    }

    #[test]
    fn test_slice_capsule_empty_pool() {
        let ranked: Vec<(i64, String)> = vec![];
        let counts = TrackCounts::new();
        let result = slice_capsule(&ranked, &counts, 10);
        assert_eq!(result.selected.len(), 0);
        assert_eq!(result.actual_size, 0);
    }

    #[test]
    fn test_slice_capsule_smaller_than_min_tracks() {
        // Only 2 cards but 3 tracks → can only select what exists
        let ranked = vec![(1, "math".into()), (2, "vocab".into())];
        let mut counts = TrackCounts::new();
        counts.insert("math".into(), 1);
        counts.insert("vocab".into(), 1);
        counts.insert("science".into(), 0); // no cards

        let result = slice_capsule(&ranked, &counts, 5);
        // Should include both available cards (science has none, so no min to meet)
        assert_eq!(result.actual_size, 2);
        assert_eq!(result.selected, vec![1, 2]);
    }
}