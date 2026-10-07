//! Student-surface controllers — **JWT-gated, self-scoped** (spec §8.2).
//!
//! Every route here requires a valid student (or any authenticated) bearer
//! token. The board scope is *always* derived from the caller's own
//! `class_enrollments` row — never from a query parameter — so a student can
//! only ever see their own class's board and their own row.
//!
//! Registered on the outer router and wrapped with `require_auth` scoped to
//! the `/student` prefix in `app.rs` (mirroring the management gate).

pub mod leaderboard;

use loco_rs::prelude::*;

/// Aggregate `/student` routes (mounted under the app's `/api/v1` prefix).
pub fn routes() -> Routes {
    leaderboard::routes()
}
