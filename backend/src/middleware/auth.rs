//! Auth middleware — JWT extraction and user injection.
//!
//! Extracts the `Authorization: Bearer <token>` header, decodes the JWT,
//! and injects `AuthUser` into request extensions for downstream handlers.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use crate::middleware::auth::{require_auth, AuthUser};
//!
//! async fn protected_route(
//!     Extension(user): Extension<AuthUser>,
//! ) -> Result<Json<...>> {
//!     // user.id, user.role, user.email are available
//! }
//! ```

use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
    Extension,
};
use serde::{Deserialize, Serialize};

use crate::services::auth as jwt;

/// Authenticated user information injected into request extensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUser {
    pub id: i64,
    pub role: String,
    pub email: String,
}


/// Build a 401 `Response` without ambiguity over which `StatusCode` type is
/// in scope (Loco's prelude pulls in `reqwest::StatusCode`, which shadows the
/// `http` crate's one). Constructs the response directly from the axum `http`
/// crate type.
fn unauthorized_response() -> Response {
    use axum::body::Body;
    use axum::http::{self, Response as _};
    let mut res: axum::http::Response<Body> = http::Response::builder()
        .status(401)
        .body(Body::empty())
        .expect("building a 401 response");
    res
}

/// Axum middleware: extract JWT from Authorization header, inject AuthUser.
///
/// Returns 401 if the header is missing, the token is expired, or the
/// signature is invalid.
///
/// **Important:** this returns a real `Response` for the 401 (not
/// `Err(StatusCode)`). A middleware whose error type is a `StatusCode` has
/// its rejection silently dropped in Loco's layer chain (the error is never
/// converted via `IntoResponse`), so the real handler would run
/// unauthenticated. Returning `Response` directly makes the 401 bullet-proof.
pub async fn require_auth(mut req: Request, next: Next) -> Result<Response, std::convert::Infallible> {
    // Extract Bearer token
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let token = match auth_header {
        Some(t) => t,
        None => return Ok(unauthorized_response()),
    };

    // Decode and validate
    let claims = match jwt::decode_token(token) {
        Ok(c) => c,
        Err(_) => return Ok(unauthorized_response()),
    };

    // Inject user into request extensions
    req.extensions_mut().insert(AuthUser {
        id: claims.sub,
        role: claims.role,
        email: claims.email,
    });

    Ok(next.run(req).await)
}

/// Middleware factory for **selective** management auth.
///
/// Loco's `AppRoutes::to_router` applies its default middleware stack
/// (`limit_payload`, `cors`, `catch_panic`, `etag`, `request_id`, `fallback`,
/// …) to the outer router via `Router::layer` *before* `.with_state`. A
/// separate `Router::nest` for the management routes does not survive that
/// chain — the nested routes stop matching and every `/api/v1/management/*`
/// request falls through to Loco's 200 "Welcome" fallback, with `require_auth`
/// never consulted.
///
/// The reliable pattern is therefore to register the management routes on the
/// *outer* router (so Loco's own layer chain, which is proven to work, wraps
/// them) and gate them with this middleware, which enforces JWT **only** for
/// the `/api/v1/management` prefix and passes every other path through
/// untouched. Apply it with `axum::middleware::from_fn` inside a
/// `Router::layer` call in `after_routes`.
pub async fn require_auth_for_management(
    mut req: Request,
    next: Next,
) -> Result<Response, std::convert::Infallible> {
    let p = req.uri().path();
    // Gate both the staff console AND the student self-service surface.
    // `/display/*` is intentionally NOT gated — the per-class display token is
    // its own credential (spec §8.1).
    let is_gated = p.starts_with("/api/v1/management")
        || p.starts_with("/management")
        || p.starts_with("/api/v1/student")
        || p.starts_with("/student");
    if !is_gated {
        // Public path — pass through without touching the auth header.
        return Ok(next.run(req).await);
    }
    require_auth(req, next).await
}

/// Optional auth: extracts JWT if present, but does not reject unauthenticated
/// requests. Useful for routes that behave differently for logged-in users.
pub async fn optional_auth(mut req: Request, next: Next) -> Result<Response, std::convert::Infallible> {
    if let Some(token) = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        if let Ok(claims) = jwt::decode_token(token) {
            req.extensions_mut().insert(AuthUser {
                id: claims.sub,
                role: claims.role,
                email: claims.email,
            });
        }
    }

    Ok(next.run(req).await)
}

/// Extract an `AuthUser` from request extensions (panics if missing — only
/// call this on routes protected by `require_auth`).
pub fn get_auth_user(req: &Request) -> &AuthUser {
    req.extensions()
        .get::<AuthUser>()
        .expect("AuthUser not found in request extensions — is require_auth middleware applied?")
}

/// Gate a request by role.
///
/// Call at the top of a *write* handler (create / update / delete / enroll /
/// distribute / generate / sync / install) after `require_auth` has injected
/// the user:
///
/// ```rust,ignore
/// let _ = ensure_role(&user, &["teacher", "admin"])?; // 403 for students
/// ```
///
/// - Any role present in `allowed` (case-insensitive) passes.
/// - A role *not* in the list (e.g. `"student"` when only teachers may write)
///   yields a **403 Forbidden**.
///
/// Returns a [`loco_rs::Error::CustomError`] carrying `403`, so handlers can
/// use `?` directly — the framework converts it to an HTTP 403 response.
pub fn ensure_role(user: &AuthUser, allowed: &[&str]) -> Result<(), loco_rs::Error> {
    let ok = allowed
        .iter()
        .any(|r| r.eq_ignore_ascii_case(&user.role));
    if ok {
        Ok(())
    } else {
        Err(loco_rs::Error::CustomError(
            axum::http::StatusCode::FORBIDDEN,
            loco_rs::controller::ErrorDetail {
                error: Some("forbidden".into()),
                description: Some("this action requires a teacher or admin role".into()),
            },
        ))
    }
}


