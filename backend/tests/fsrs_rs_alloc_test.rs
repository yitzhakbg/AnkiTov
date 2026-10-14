//! T-116 fsrs-rs adoption gate — zero-allocation check (specs/tasks.md).
//!
//! The on-device SRS core must not allocate on the hot path. This test counts
//! allocations through a counting GlobalAlloc while exercising the fsrs-rs
//! 6.6.2 API surface the scheduler swap would use.
//!
//! IMPORTANT: this file must contain EXACTLY ONE #[test]. Rust runs tests in
//! parallel threads by default; any second test's allocations would race the
//! counter and produce flaky failures. Run it serially (it is a single test,
//! so plain `cargo nextest run` / `cargo test` is fine).

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
static ALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);

struct CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(new_size.saturating_sub(layout.size()), Ordering::Relaxed);
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

#[test]
fn fsrs_hot_path_is_zero_allocation() {
    use fsrs::{current_retrievability, FSRS6_DEFAULT_DECAY, FSRS, MemoryState};

    // Sanity probe: the counter must actually count, or the assertion below
    // would pass vacuously.
    let probe: Vec<u8> = Vec::with_capacity(64);
    assert!(ALLOC_COUNT.load(Ordering::Relaxed) > 0, "allocator counter is not counting");
    drop(probe);

    let fsrs = FSRS::default();
    let state = MemoryState { stability: 12.0, difficulty: 5.5 };

    // Warmup: touch every code path once (lazy init, caches, first-call setup)
    // so the measured section captures steady-state behavior only.
    for _ in 0..10 {
        let _ = fsrs.next_states(Some(state), 0.9, 1);
        let _ = current_retrievability(state, 5.0, FSRS6_DEFAULT_DECAY);
        let _ = fsrs.next_interval(Some(state.stability), 0.9, 3);
    }
    let warm_count = ALLOC_COUNT.load(Ordering::Relaxed);
    let warm_bytes = ALLOC_BYTES.load(Ordering::Relaxed);

    // Measured section: the hot-path mix the on-device scheduler would run —
    // 1000 transitions, 10k recall evaluations, 10k interval computations.
    for _ in 0..1000 {
        let _ = fsrs.next_states(Some(state), 0.9, 1);
    }
    for _ in 0..10_000 {
        let _ = current_retrievability(state, 5.0, FSRS6_DEFAULT_DECAY);
    }
    for _ in 0..10_000 {
        let _ = fsrs.next_interval(Some(state.stability), 0.9, 3);
    }

    let count = ALLOC_COUNT.load(Ordering::Relaxed) - warm_count;
    let bytes = ALLOC_BYTES.load(Ordering::Relaxed) - warm_bytes;
    assert_eq!(count, 0, "hot path allocated {count} times ({bytes} bytes)");
    assert_eq!(bytes, 0, "hot path allocated {bytes} bytes");
}