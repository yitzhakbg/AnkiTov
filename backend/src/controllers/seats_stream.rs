//! Seat stream authorization — the Caddy `forward_auth` target.
//!
//! Owner decision 2026-11-23: proxy-level auth for the public seat surface
//! (`https://practice.ankitov.com/seat/<port>/`). Caddy validates EVERY
//! `/seat/*` request against this endpoint before proxying to the seat;
//! non-2xx responses (401 deny, 302 exchange) are copied back to the client
//! verbatim by Caddy's forward_auth.
//!
//! Why not JWT: the backend authenticates APIs via `Authorization: Bearer`
//! (see `middleware::auth`), but a student reaching the seat is a plain
//! browser NAVIGATION — no header injection possible, and the Selkies
//! client's websocket upgrades cannot replay a query param either. So the
//! credential is a per-seat random token (`SeatInfo.token`) with a
//! token→cookie exchange:
//!
//! 1. First navigation opens `…/seat/<port>/?seat_token=<token>` (the URL
//!    the teacher/API hands over).
//! 2. This endpoint validates token ↔ port against the LIVE seat table and
//!    answers `302` to the same path minus the token, with
//!    `Set-Cookie: ankitov_seat_token=…; Path=/seat; HttpOnly`.
//! 3. The browser follows the redirect; every subsequent request — page,
//!    assets, websocket upgrades — carries the cookie and authenticates.
//!
//! The seat table is the source of truth on every request: a cookie for a
//! stopped/reaped seat is just a 401, and the token↔port binding means one
//! student's token never opens another student's seat. Teachers previewing
//! a seat use the same token (it is in the start/list API response).

use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use loco_rs::prelude::*;

use crate::services::seat_manager::SeatManager;

const COOKIE_NAME: &str = "ankitov_seat_token";
/// Cookie lifetime. Seats are reaped after `ANKITOV_SEAT_IDLE_SECS` (300s
/// default) and every request re-validates against the live seat table, so
/// this only bounds how long a stale cookie lingers in the browser.
const COOKIE_MAX_AGE_SECS: i64 = 3600;

pub fn routes() -> Routes {
    Routes::new().prefix("/seats").add("/authz", get(authz))
}

/// GET /seats/authz — forward_auth target. NOT JWT-gated: the caller is
/// Caddy relaying the student's browser request (no Authorization header on
/// navigations). Access is granted purely on the per-seat token.
#[utoipa::path(
    get,
    path = "/seats/authz",
    responses((status = 200, description = "Seat access granted")),
    tag = "Seats"
)]
pub async fn authz(headers: HeaderMap) -> Response {
    // 1. The ORIGINAL request URI, relayed by Caddy as X-Forwarded-Uri.
    let orig = headers
        .get("x-forwarded-uri")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let Some(port) = seat_port_from_uri(orig) else {
        return deny("missing or malformed seat path");
    };

    // 2. A live seat must exist on exactly this port (token↔port binding).
    let Some(seat) = SeatManager::global()
        .list()
        .into_iter()
        .find(|s| s.port == port)
    else {
        return deny("no live seat on this port");
    };

    // 3. Credential: cookie or X-Seat-Token header → direct verdict;
    //    ?seat_token= query → exchange for a cookie (302).
    let presented = cookie_value(&headers, COOKIE_NAME)
        .or_else(|| header_token(&headers));
    if presented.as_deref() == Some(seat.token.as_str()) {
        return StatusCode::OK.into_response();
    }
    if query_param(orig, "seat_token").as_deref() == Some(seat.token.as_str()) {
        return exchange_for_cookie(&headers, orig, &seat.token);
    }
    deny("seat token required")
}

/// 302 back to the same path minus the token, setting the scoped cookie.
/// Caddy copies this response to the client verbatim (non-2xx forward_auth
/// responses are relayed), so the browser lands on the seat already
/// authenticated and the token never stays in the address bar.
fn exchange_for_cookie(headers: &HeaderMap, orig_uri: &str, token: &str) -> Response {
    let (path, query) = orig_uri.split_once('?').unwrap_or((orig_uri, ""));
    let kept: Vec<&str> = query
        .split('&')
        .filter(|p| !p.is_empty() && !p.starts_with("seat_token="))
        .collect();
    let location = if kept.is_empty() {
        path.to_owned()
    } else {
        format!("{path}?{}", kept.join("&"))
    };
    // Secure only when the client actually came in over TLS (Caddy sets
    // X-Forwarded-Proto); local http testing must keep working.
    let secure = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("https"))
        .unwrap_or(false);
    let cookie = format!(
        "{COOKIE_NAME}={token}; Path=/seat; HttpOnly; SameSite=Lax; Max-Age={COOKIE_MAX_AGE_SECS}{}",
        if secure { "; Secure" } else { "" }
    );
    axum::http::Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .header(header::SET_COOKIE, cookie)
        .body(axum::body::Body::empty())
        .expect("building seat-auth 302")
}

/// 401 with a human-readable page (students land here on stale links).
fn deny(msg: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::CONTENT_TYPE,
            "text/html; charset=utf-8",
        )],
        format!(
            "<html><body style=\"font-family:sans-serif;text-align:center;padding-top:8vh\">\
             <h2>Seat access denied</h2>\
             <p>{msg}.</p>\
             <p>Open the practice link your teacher gave you, or ask them to restart your seat.</p>\
             </body></html>"
        ),
    )
        .into_response()
}

/// Seat port from an original URI like `/seat/8100/` or `/seat/8100/ws`.
fn seat_port_from_uri(uri: &str) -> Option<u16> {
    let path = uri.split('?').next().unwrap_or("");
    let mut segs = path.split('/').filter(|s| !s.is_empty());
    if segs.next()? != "seat" {
        return None;
    }
    segs.next()?.parse().ok()
}

/// One query param's value from a raw URI (no url crate needed).
fn query_param(uri: &str, name: &str) -> Option<String> {
    let q = uri.split_once('?')?.1;
    q.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == name).then(|| v.to_owned())
    })
}

/// One cookie's value from the request headers.
fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|c| {
        let (k, v) = c.trim().split_once('=')?;
        (k == name).then(|| v.to_owned())
    })
}

fn header_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-seat-token")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seat_port_parses() {
        assert_eq!(seat_port_from_uri("/seat/8100/"), Some(8100));
        assert_eq!(seat_port_from_uri("/seat/8100/ws?x=1"), Some(8100));
        assert_eq!(seat_port_from_uri("/seat/8100"), Some(8100));
        assert_eq!(seat_port_from_uri("/seat/abc/"), None);
        assert_eq!(seat_port_from_uri("/other/8100/"), None);
        assert_eq!(seat_port_from_uri(""), None);
    }

    #[test]
    fn query_param_extracts() {
        assert_eq!(
            query_param("/seat/8100/?seat_token=abc&x=2", "seat_token"),
            Some("abc".into())
        );
        assert_eq!(query_param("/seat/8100/", "seat_token"), None);
        assert_eq!(query_param("/seat/8100/?a=1", "seat_token"), None);
    }
}
