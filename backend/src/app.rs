use std::path::Path;

use axum::Router as AxumRouter;
use async_trait::async_trait;
use tower_http::cors::{Any, CorsLayer};
use chrono::Utc;
use loco_rs::{
    app::{AppContext, Hooks},
    boot::{create_app, BootResult, StartMode},
    controller::AppRoutes,
    environment::Environment,
    prelude::*,
    task::Tasks,
};
use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};

use crate::controllers;
use crate::migrations::Migrator;
use crate::models::entities::user;
use crate::services::auth as auth_svc;

/// The central application configuration for the AnkiTov system.
pub struct App;

/// Startup boot guard for the JWT signing secret.
///
/// AnkiTov issues self-contained JWTs for every authenticated request. If the
/// hard-coded development secret is used in **production**, anyone who knows
/// it can mint valid tokens for *any* role (including `admin`) and read or
/// write every multi-tenant record — effectively a full authentication
/// bypass. This guard makes that failure mode loud:
///
/// * **Production** (or anything not `test`/`development`) with the default
///   secret → hard `Err`; the process refuses to boot.
/// * **Development / test** with the default secret → `warn!` once (so local
///   `loco start` and the test harness keep working without env setup) but the
///   insecure default is visible in the log.
///
/// Call this at the very start of [`App::boot`] *before* `create_app` so a
/// misconfigured production build never serves a single request.
fn enforce_jwt_secret(environment: &Environment) -> Result<()> {
    if !auth_svc::is_default_secret() {
        // A real secret is configured — nothing to enforce.
        return Ok(());
    }

    match environment {
        Environment::Production => {
            let msg = "ANKITOV_JWT_SECRET is not set (or is the hard-coded dev default).                        Refusing to boot in production — export a strong random secret,                        e.g.:  openssl rand -hex 32";
            tracing::error!(
                "JWT boot guard: production refuses the insecure default secret. {msg}"
            );
            Err(loco_rs::Error::Message(msg.to_string()))
        }
        Environment::Development | Environment::Test => {
            tracing::warn!(
                "ANKITOV_JWT_SECRET is the insecure development default. Set a real                  secret in production (e.g. `openssl rand -hex 32`). Continuing in                  `{}` mode.",
                environment.to_string()
            );
            Ok(())
        }
        // Any other (custom / Any) environment is treated like production.
        _ => Err(loco_rs::Error::Message(
            "JWT boot guard: non-production environment detected with the default              ANKITOV_JWT_SECRET; refusing to start. Export a real secret.".into(),
        )),
    }
}

#[async_trait]
impl Hooks for App {
    fn app_name() -> &'static str {
        "AnkiTov"
    }

    fn app_version() -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    async fn boot(mode: StartMode, environment: &Environment) -> Result<BootResult> {
        enforce_jwt_secret(environment)?;
        create_app::<Self, Migrator>(mode, environment).await
    }

    fn routes(_ctx: &AppContext) -> AppRoutes {
        // Register the full `/api/v1` table on the outer router. Loco's
        // `to_router` flattens each controller `Routes` into top-level
        // `Router::route()` calls and applies its default middleware chain
        // (limit_payload, cors, catch_panic, etag, request_id, fallback, …)
        // via `Router::layer` *before* `.with_state`.
        //
        // We deliberately do NOT split management out into a separate
        // `Router::nest`: a nested router does not survive Loco's layer chain
        // (the nested routes stop matching and every request falls through to
        // Loco's 200 "Welcome" fallback, with `require_auth` never consulted).
        //
        // Instead, `after_routes` applies `require_auth_for_management` as a
        // `Router::layer` on this outer router. That middleware enforces JWT
        // *only* for the `/api/v1/management` prefix and passes every other
        // path (health, auth, telemetry, dashboard, …) through untouched —
        // so the public endpoints stay open while management is gated, all
        // inside Loco's proven layer mechanism.
        AppRoutes::empty()
            .prefix("/api/v1")
            .add_route(controllers::health::routes())
            .add_route(controllers::auth::routes())
            .add_route(controllers::telemetry::routes())
            .add_route(controllers::sync::routes())
            .add_route(controllers::management::decks::routes())
            .add_route(controllers::management::stats::routes())
            .add_route(controllers::management::addons::routes())
            .add_route(controllers::management::students::routes())
            .add_route(controllers::management::sync::routes())
            .add_route(controllers::management::anki_ops::routes())
            .add_route(controllers::management::probe::routes())
            .add_route(controllers::management::jev::routes())
            .add_route(controllers::management::tracks::routes())
            .add_route(controllers::management::track_profiles::routes())
            .add_route(controllers::management::capsule_sessions::routes())
            .add_route(controllers::management::classes::routes())
            .add_route(controllers::management::compliance::routes())
            .add_route(controllers::management::leaderboard::routes())
            .add_route(controllers::management::profile_assignments::routes())
            .add_route(controllers::management::ask::routes())
            .add_route(controllers::management::producers::routes())
            .add_route(controllers::management::teachers::routes())
            .add_route(controllers::student::routes())
            .add_route(controllers::display::routes())
            .add_route(controllers::locale::routes())
    }

    async fn after_routes(router: AxumRouter, ctx: &AppContext) -> Result<AxumRouter> {
        use axum::Router;
        use axum::routing::{get, post};
        use crate::middleware::auth::require_auth_for_management;

        // CORS: allow the Management Console GUI loaded from file:// or local dev.
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        // Re-register the dashboard on the (public) outer router. It was never
        // part of the `/api/v1` table, so it has no auth layer and stays public.
        let dashboard = router
            // OpenAPI spec + Scalar API reference — public, self-documenting
            // (docs/src/reference/openapi.md). The JSON endpoint is what the
            // Scalar page and any external tooling consume.
            .route("/api/v1/openapi.json", get(crate::openapi::openapi_json))
            .route("/scalar", get(crate::openapi::scalar_ui))
            .route("/", get(controllers::dashboard::imp_console))
            .route("/health", get(controllers::dashboard::health))
            .route("/dashboard", get(controllers::dashboard::imp_console))
            .route("/dashboard/", get(controllers::dashboard::imp_console))
            .route("/imp-console", get(controllers::dashboard::imp_console))
            // Display wall — public; the token in the path is the credential
            // (spec §8.1). The page polls the public display API client-side.
            .route("/b/:token", get(controllers::dashboard::imp_wall))
            .route("/dashboard/health", get(controllers::dashboard::health))
            .route("/dashboard/i18n.js", get(controllers::dashboard::i18n_js))
            .route("/dashboard/rtl.css", get(controllers::dashboard::rtl_css))
            .route("/dashboard/locales/en-US.json", get(controllers::dashboard::locale_en_us))
            .route("/dashboard/locales/ar.json", get(controllers::dashboard::locale_ar))
            .route("/dashboard/locales/he.json", get(controllers::dashboard::locale_he))
            .route("/dashboard/locales/fr.json", get(controllers::dashboard::locale_fr))
            .route("/dashboard/locales/es.json", get(controllers::dashboard::locale_es))
            .route("/dashboard/locales/de.json", get(controllers::dashboard::locale_de))
            .route("/dashboard/locales/it.json", get(controllers::dashboard::locale_it))
            .route("/dashboard/locales/zh.json", get(controllers::dashboard::locale_zh))
            .route("/dashboard/locales/ko.json", get(controllers::dashboard::locale_ko))
            .route("/dashboard/locales/ja.json", get(controllers::dashboard::locale_ja))
            .route("/dashboard/locales/ru.json", get(controllers::dashboard::locale_ru))
            .route("/dashboard/locales/pt.json", get(controllers::dashboard::locale_pt))
            .route("/dashboard/locales/hi.json", get(controllers::dashboard::locale_hi))
            .route("/dashboard/vendor/popper.min.js", get(controllers::dashboard::popper_js))
            .route("/dashboard/vendor/tippy.min.js", get(controllers::dashboard::tippy_js))
            .route("/dashboard/vendor/driver.min.js", get(controllers::dashboard::driver_js))
            // Prong 8 — public first-login wizard (NOT auth-gated; the token in the
            // path is the credential). Served before the require_auth_for_management layer.
            //
            // The two first-login *API* routes (`first-login-status` + `first-login`) are
            // registered on the `first_login_api` sub-router below, also under the
            // non-gated `/d/` prefix — NOT here, and NOT under `/api/v1/management/*`
            // (that prefix is JWT-gated and the wizard holds no token yet).
            .route("/d/:invite_id/:token", get(controllers::dashboard::first_login));

        // The first-login handlers need `State<AppContext>` but the dashboard router is
        // `Router<()>`, so we attach state to a nested sub-router. Their absolute `/d/...`
        // paths land on the public (non-gated) branch of the outer router.
        let first_login_api = Router::new()
            .route(
                "/d/teachers/first-login-status/:invite_id/:token",
                get(controllers::management::teachers::first_login_status),
            )
            .route(
                "/d/teachers/first-login/:invite_id/:token",
                post(controllers::management::teachers::first_login),
            )
            .with_state(ctx.clone());

        // Gate `/api/v1/management/*` with the JWT middleware, applied as a
        // `Router::layer` on the outer router (the pattern that works inside Loco's
        // chain). It is path-scoped: it only enforces JWT for the management prefix and
        // lets everything else through — so the public dashboard, `/api/v1/health`,
        // `/api/v1/auth/*`, `/api/v1/telemetry/*`, and the `/d/...` first-login routes
        // stay accessible without a token.
        Ok(dashboard
            .nest("/", first_login_api)
            .layer(axum::middleware::from_fn(require_auth_for_management))
            .layer(cors))
    }

    async fn connect_workers(_ctx: &AppContext, _queue: &Queue) -> Result<()> {
        Ok(())
    }

    fn register_tasks(_tasks: &mut Tasks) {}

    async fn truncate(_db: &DatabaseConnection) -> Result<()> {
        Ok(())
    }

    async fn seed(db: &DatabaseConnection, _base: &Path) -> Result<()> {
        let now = Utc::now().timestamp();

        let students: Vec<(&str, &str, Option<&str>, &str, Option<&str>)> = vec![
            // Grade 7-A
            ("alice.chen",     "alice.chen@example.edu",      Some("Alice Chen"),      "student", Some("grade7-a")),
            ("ben.carter",     "ben.carter@example.edu",      Some("Ben Carter"),      "student", Some("grade7-a")),
            ("carlos.diaz",    "carlos.diaz@example.edu",     Some("Carlos Diaz"),     "student", Some("grade7-a")),
            ("diana.evans",    "diana.evans@example.edu",     Some("Diana Evans"),     "student", Some("grade7-a")),
            ("ethan.foster",   "ethan.foster@example.edu",    Some("Ethan Foster"),    "student", Some("grade7-a")),
            // Grade 7-B
            ("fatima.garcia",  "fatima.garcia@example.edu",   Some("Fatima Garcia"),   "student", Some("grade7-b")),
            ("george.harris",  "george.harris@example.edu",   Some("George Harris"),   "student", Some("grade7-b")),
            ("hannah.ito",     "hannah.ito@example.edu",      Some("Hannah Ito"),      "student", Some("grade7-b")),
            ("ivan.jackson",   "ivan.jackson@example.edu",    Some("Ivan Jackson"),    "student", Some("grade7-b")),
            ("julia.kim",      "julia.kim@example.edu",       Some("Julia Kim"),       "student", Some("grade7-b")),
            // Grade 8-A
            ("kevin.liu",      "kevin.liu@example.edu",       Some("Kevin Liu"),       "student", Some("grade8-a")),
            ("luna.martinez",  "luna.martinez@example.edu",   Some("Luna Martinez"),   "student", Some("grade8-a")),
            ("marcus.nguyen",  "marcus.nguyen@example.edu",   Some("Marcus Nguyen"),   "student", Some("grade8-a")),
            ("nina.okonkwo",   "nina.okonkwo@example.edu",    Some("Nina Okonkwo"),    "student", Some("grade8-a")),
            ("oliver.patel",   "oliver.patel@example.edu",    Some("Oliver Patel"),    "student", Some("grade8-a")),
            // Grade 8-B
            ("priya.quinn",    "priya.quinn@example.edu",     Some("Priya Quinn"),     "student", Some("grade8-b")),
            ("quincy.rivera",  "quincy.rivera@example.edu",   Some("Quincy Rivera"),   "student", Some("grade8-b")),
            ("rachel.suzuki",  "rachel.suzuki@example.edu",   Some("Rachel Suzuki"),   "student", Some("grade8-b")),
            ("sam.tanaka",     "sam.tanaka@example.edu",      Some("Sam Tanaka"),      "student", Some("grade8-b")),
            ("tara.uribe",     "tara.uribe@example.edu",      Some("Tara Uribe"),      "student", Some("grade8-b")),
            // Grade 9-A
            ("uma.vasquez",    "uma.vasquez@example.edu",     Some("Uma Vasquez"),     "student", Some("grade9-a")),
            ("victor.wang",    "victor.wang@example.edu",     Some("Victor Wang"),     "student", Some("grade9-a")),
            ("wendy.xu",       "wendy.xu@example.edu",        Some("Wendy Xu"),        "student", Some("grade9-a")),
            ("xander.young",   "xander.young@example.edu",    Some("Xander Young"),    "student", Some("grade9-a")),
            ("yara.zawadi",    "yara.zawadi@example.edu",     Some("Yara Zawadi"),     "student", Some("grade9-a")),
            // Grade 9-B
            ("zach.ali",       "zach.ali@example.edu",        Some("Zach Ali"),        "student", Some("grade9-b")),
            ("zoe.brighton",   "zoe.brighton@example.edu",    Some("Zoe Brighton"),    "student", Some("grade9-b")),
        ];

        for (i, (username, email, full_name, role, subsection_id)) in students.iter().enumerate() {
            let id = (i + 1) as i64;
            let model = user::ActiveModel {
                id: Set(id),
                username: Set(username.to_string()),
                email: Set(email.to_string()),
                full_name: Set(full_name.map(|s| s.to_string())),
                role: Set(role.to_string()),
                subsection_id: Set(subsection_id.map(|s| s.to_string())),
                password_hash: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
            };
            model.insert(db).await?;
        }

        tracing::info!("Seeded {} students across grade7–grade9", students.len());
        Ok(())
    }
}
