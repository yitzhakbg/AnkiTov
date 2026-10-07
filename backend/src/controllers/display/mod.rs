//! Display-surface controllers — **public, token-gated, no JWT** (spec §8.1).
//!
//! These routes are served to the classroom projector (and the student wall
//! app) and are deliberately *outside* the `require_auth_for_management`
//! gate: the per-class display token *is* the credential, minted by staff.
//!
//! Registered on the outer router so Loco's own layer chain wraps them
//! (the `nest`-into-a-subrouter pattern does not survive Loco's layer
//! application — see `middleware::auth::require_auth_for_management`).

pub mod leaderboard;

use loco_rs::prelude::*;

/// Aggregate `/display` routes (mounted under the app's `/api/v1` prefix).
pub fn routes() -> Routes {
    leaderboard::routes()
}
