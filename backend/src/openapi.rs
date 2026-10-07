//! Top-level OpenAPI aggregation and live serving of the API reference.
//!
//! Every controller route carries a `#[utoipa::path]` annotation; this module
//! stitches them into one spec and serves it:
//!
//! - `GET /api/v1/openapi.json` — machine-readable OpenAPI 3.0 spec
//! - `GET /scalar` — interactive Scalar API reference (CDN embed)
//!
//! The spec is always current: it is generated from the compiled annotations
//! at request time, so it cannot drift from the running server. Registered as
//! public routes in `App::after_routes` (outside the management JWT gate —
//! docs are meant to be browsable without credentials).

use axum::response::IntoResponse;
use utoipa::OpenApi;

use crate::controllers;

/// OpenAPI spec for the endpoints outside the Management Console: auth,
/// locale, student self-service, and the public display wall.
///
/// Paths are controller-relative (`/auth/logout`, `/locale`, …) — exactly how
/// the controllers register their routes in `app.rs`; [`ApiDoc`] nests this
/// doc under the app-wide `/api/v1` prefix.
#[derive(OpenApi)]
#[openapi(
    paths(
        controllers::auth::logout,
        controllers::locale::get_locale,
        controllers::locale::get_locale_info,
        controllers::locale::list_locales,
        controllers::locale::set_locale,
        controllers::student::leaderboard::my_board,
        controllers::student::leaderboard::my_stats,
        controllers::student::leaderboard::patch_identity,
        controllers::display::leaderboard::display_class_board,
    ),
    tags(
        (name = "Auth", description = "Registration, login, and session teardown"),
        (name = "Locale", description = "i18n data served to the frontend"),
        (name = "Student", description = "JWT-gated, self-scoped student endpoints"),
        (name = "Display", description = "Public classroom display wall (token in the path is the credential)"),
    )
)]
pub struct PublicOpenApi;

/// The complete AnkiTov API spec: public routes plus the Management Console.
///
/// Both groups are nested under `/api/v1` (the annotations carry
/// controller-relative paths like `/management/decks` or `/locale`), which is
/// exactly how Loco mounts them in `app.rs` and how the Caddyfile proxies
/// them in production.
#[derive(OpenApi)]
#[openapi(
    nest(
        (path = "/api/v1", api = PublicOpenApi),
        (path = "/api/v1", api = controllers::management::ManagementOpenApi),
    ),
    info(
        title = "AnkiTov API",
        description = "Zero-intervention knowledge retention infrastructure — management console, IMP capsule pipeline, telemetry, and student surfaces.",
        contact(name = "AnkiTov", url = "https://ankitov.com"),
        license(name = "AGPL-3.0-only"),
    )
)]
pub struct ApiDoc;

/// `GET /api/v1/openapi.json` — the live OpenAPI 3.0 spec.
pub async fn openapi_json() -> impl IntoResponse {
    axum::Json(ApiDoc::openapi())
}

/// `GET /scalar` — interactive API reference (Scalar, loaded from CDN).
pub async fn scalar_ui() -> impl IntoResponse {
    axum::response::Html(
        r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>AnkiTov API Reference</title>
  </head>
  <body>
    <!-- Scalar single-script embed (https://scalar.com) pointing at the live spec -->
    <script id="api-reference" data-url="/api/v1/openapi.json"></script>
    <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
  </body>
</html>"#,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The served spec must expose the real URL shapes: management under
    /// `/api/v1/management`, public routes under `/api/v1` — matching both
    /// the Loco route table and the Caddyfile rewrite.
    #[test]
    fn spec_paths_match_mounted_routes() {
        let doc = ApiDoc::openapi();
        let json = serde_json::to_value(&doc).expect("spec serializes");
        let paths = json["paths"].as_object().expect("paths object");

        for expected in [
            "/api/v1/management/decks",
            "/api/v1/management/compliance/weekly",
            "/api/v1/auth/logout",
            "/api/v1/locale",
            "/api/v1/locale/set",
            "/api/v1/student/leaderboard",
            "/api/v1/display/class/{token}",
        ] {
            assert!(
                paths.contains_key(expected),
                "spec missing `{expected}` (spec has {} paths total)",
                paths.len()
            );
        }
    }
}