//! Public drill-link front door (T-215 — First-60-Seconds).
//!
//! A drill link is a token URL a teacher/prospect can open WITHOUT any
//! account: `GET /drill/<token>` resolves the link, spawns an ephemeral
//! preview-tier seat pre-loaded with the link's pack (and auto-opened deck),
//! and 302s the browser straight into the Selkies stream. The visitor lands
//! on Card #1 in kiosk mode — no Anki menus, no sync dialogs, no login.
//!
//! Design: `specs/design/2026-11-23-first-60-seconds-drill-link.md` §3.2.
//! Degradation is honest and branded: missing/expired/exhausted links render
//! the same 404 page (no existence oracle), and no-free-slot / spawn failure
//! / rate-limit render the same "pods are busy" 503 page — never an error
//! dump. Seat spawn is on-demand (owner GO): no pool, cold start measured
//! against the 60-second budget.
//!
//! Auth posture: intentionally public — the token IS the credential, the
//! spawned seat is ephemeral (scratch collection wiped on stop/reap, 120 s
//! idle reap, hard preview cap) and the Selkies surface is still gated by
//! the per-seat token cookie (`seats_stream`).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use loco_rs::prelude::*;
use rand::Rng;
use sea_orm::entity::prelude::EntityTrait;
use sea_orm::{ActiveModelTrait, Set};

use crate::models::entities::drill_link;
use crate::services::seat_manager::{SeatLaunch, SeatManager, SeatTier};

/// Drill seats are preview-tier: scratch collection wiped on stop/reap.
///
/// Soft per-link rate limit: generous enough that a human clicking through
/// (or a page refresh) never trips it, tight enough that a token hammered
/// by a bot gets the busy page instead of burning spawn slots.
const RATE_WINDOW: Duration = Duration::from_secs(60);
const RATE_MAX_HITS: u32 = 20;
/// After a spawn failure give the (likely misconfigured) backend a breather
/// before the next anonymous click retries the spawn.
const BUSY_RETRY_HINT_SECS: u64 = 30;

static RATE_LIMITER: OnceLock<Mutex<HashMap<String, (Instant, u32)>>> = OnceLock::new();

pub fn routes() -> Routes {
    Routes::new().prefix("/drill").add("/:id", get(drill))
}

/// GET /drill/:id — the drill link (public; token is the credential).
#[utoipa::path(
    get,
    path = "/drill/{id}",
    params(("id" = String, Path, description = "Drill-link token")),
    responses(
        (status = 302, description = "Redirect to the live seat stream"),
        (status = 404, description = "Link missing/expired/exhausted (uniform page)"),
        (status = 503, description = "No seat slot available / rate limited"),
    ),
    tag = "Drill"
)]
pub async fn drill(
    State(ctx): State<AppContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    // Malformed tokens are simply not links — same 404 as unknown ones.
    if !valid_link_token(&id) {
        return not_found_page();
    }
    if rate_limited(&id) {
        return busy_page();
    }

    let link = match drill_link::Entity::find_by_id(&id).one(&ctx.db).await {
        Ok(Some(link)) => link,
        Ok(None) => return not_found_page(),
        Err(e) => {
            tracing::error!("drill link lookup failed: {e}");
            return busy_page();
        }
    };

    // Expiry / exhaustion — rendered identically to a missing link so a
    // dead link is indistinguishable from a wrong one (spec §3.2: no
    // existence oracle beyond the 404 itself).
    let now = chrono::Utc::now().timestamp();
    let expired = link.expires_at.map(|t| t < now).unwrap_or(false);
    let exhausted = link
        .max_uses
        .map(|m| link.use_count >= m)
        .unwrap_or(false);
    if expired || exhausted {
        return not_found_page();
    }

    // Per-click seat: synthetic id derived from the link prefix + fresh
    // randomness — a page refresh gets a fresh seat, and the preview caps
    // (ANKITOV_SEAT_PREVIEW_MAX / ANKITOV_SEAT_MAX) bound total seats.
    let seat_id = format!("drill-{}-{}", &id[..8], rand_suffix(4));
    let launch = SeatLaunch {
        // Preview tier forces kiosk 0 inside the service (clamped there
        // too — the controller never gets to loosen it).
        tier: SeatTier::Preview,
        kiosk_level: 0,
        // Telemetry attribution: card answers attribute to the LINK, not
        // "unknown" (spec §3.2).
        student_email: Some(format!("drill-{}@drill.local", &id[..8])),
        deck: Some(link.deck.clone()),
        packs: Some(link.pack.clone()),
    };
    match SeatManager::global().start_seat(&seat_id, &launch).await {
        Ok(info) => {
            // Count the use only once the seat is actually up — a failed
            // spawn should not burn a limited link's budget.
            let new_count = link.use_count + 1;
            let mut am: drill_link::ActiveModel = link.into();
            am.use_count = Set(new_count);
            if let Err(e) = am.update(&ctx.db).await {
                tracing::error!("drill use_count bump failed (seat already up): {e}");
            }
            redirect_to_seat(&info.stream_url(), &info.token)
        }
        Err(e) => {
            tracing::warn!("drill seat spawn failed for link …{}: {e}", &id[..8]);
            busy_page()
        }
    }
}

// ── helpers ─────────────────────────────────────────────────────────────────

/// Drill-link tokens are 192-bit urlsafe (32 chars of base64url). Anything
/// outside that shape is not a link we minted.
fn valid_link_token(id: &str) -> bool {
    id.len() == 32
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Soft per-link rate limit (in-process; single-process backend).
/// Returns true when THIS hit should be shed.
fn rate_limited(id: &str) -> bool {
    let map = RATE_LIMITER.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().unwrap_or_else(|p| p.into_inner());
    let now = Instant::now();
    // Bound memory: prune stale windows once the map grows past a page.
    if guard.len() > 512 {
        guard.retain(|_, (start, _)| now.duration_since(*start) < 2 * RATE_WINDOW);
    }
    let entry = guard.entry(id.to_string()).or_insert((now, 0));
    if now.duration_since(entry.0) >= RATE_WINDOW {
        *entry = (now, 0);
    }
    entry.1 += 1;
    entry.1 > RATE_MAX_HITS
}

/// Random alphanumeric suffix for the per-click seat id.
fn rand_suffix(len: usize) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}

/// 302 to the seat stream, handing the per-seat token for the cookie
/// exchange (`seats_stream` swaps it for `ankitov_seat_token` on first load).
fn redirect_to_seat(stream_url: &str, token: &str) -> Response {
    let location = format!("{}?seat_token={}", stream_url, token);
    axum::http::Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .body(axum::body::Body::empty())
        .unwrap_or_else(|_| busy_page().into_response())
}

/// Uniform "this link is gone" page — missing, malformed, expired AND
/// exhausted all render this (no oracle).
fn not_found_page() -> Response {
    html_page(
        StatusCode::NOT_FOUND,
        "Link not found",
        "This practice link is invalid, expired, or has been used up. \
         Ask your teacher for a fresh link.",
    )
}

/// Branded degradation page — no free slot, spawn failure, or rate limit.
/// Never an internal error dump (spec §3.5).
fn busy_page() -> Response {
    html_page(
        StatusCode::SERVICE_UNAVAILABLE,
        "All practice pods are busy",
        &format!(
            "All practice pods are busy right now — try again in about \
             {BUSY_RETRY_HINT_SECS} seconds."
        ),
    )
}

fn html_page(status: StatusCode, title: &str, body: &str) -> Response {
    let html = format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{title} — AnkiTov</title>\
         <style>body{{font-family:system-ui,sans-serif;display:flex;min-height:100vh;\
         align-items:center;justify-content:center;margin:0;background:#f7f5f0;}}\
         main{{text-align:center;padding:2rem;max-width:32rem;}}\
         h1{{color:#3d3d3d;font-size:1.4rem;}}p{{color:#6b6b6b;line-height:1.5;}}\
         .brand{{color:#2f6f4f;font-weight:700;letter-spacing:.02em;}}</style></head>\
         <body><main><div class=\"brand\">AnkiTov</div><h1>{title}</h1><p>{body}</p>\
         </main></body></html>"
    );
    (
        status,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_link_token_accepts_only_urlsafe_32() {
        assert!(valid_link_token(
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef"
        ));
        assert!(valid_link_token("a-b_c-d-e-f-g-h-i-j-k-l-m-n-o-pq"));
        assert!(!valid_link_token("short"));
        assert!(!valid_link_token("has.dot.in.it.000000000000000000"));
        assert!(!valid_link_token(
            "with+plus/andslash__________xxxxx"
        )); // 32 chars but +/ not urlsafe
        assert!(!valid_link_token(""));
    }

    #[test]
    fn rate_limiter_sheds_after_limit_and_resets_per_window() {
        let id = "ratetest-token-token-token-tokens";
        // Fresh id: first RATE_MAX_HITS passes.
        for _ in 0..RATE_MAX_HITS {
            assert!(!rate_limited(id));
        }
        assert!(rate_limited(id));
        // A different id is unaffected.
        assert!(!rate_limited("othertoken-token-token-token-tokn"));
    }

    #[test]
    fn rand_suffix_shape() {
        let s = rand_suffix(4);
        assert_eq!(s.len(), 4);
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
