//! T-114 (corrected) — HTTP surface gap closure.
//!
//! Covers the ~27 routes with zero prior test references (census:
//! `specs/harness/2026-10-13-t114-census-gap.md` §C). Contracts follow the
//! established `endpoint_coverage_test.rs` pattern: auth-gated routes answer
//! 200 + JSON on the happy path, degrade gracefully (never 5xx) where the
//! handler promises it, and reject unauthenticated callers.
//!
//! Run: `cargo nextest run -p backend --test http_surface_gap_test`
//!
//! Known-flaky: nextest (0.9.x, macOS) intermittently flags one fast test in
//! this binary `LEAK` (~1 in 5-10 runs, different test each time). Investigated
//! 2026-10-13: no child processes observable (30ms ps polling across 12 runs
//! incl. a leaky one), no spawn paths in the code under test (only
//! `anki_launcher::ensure_anki_running` spawns, unreachable from these
//! routes), test DB is in-memory SQLite. Treated as a runner false positive;
//! LEAK does not fail the gate.

use loco_rs::testing;
use serde_json::{json, Value};

/// Parse a response body as JSON, panicking with the raw body on failure.
fn json_body(text: &str) -> Value {
    match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => panic!("invalid JSON: {e}\nbody: {text:?}"),
    }
}

fn teacher_token() -> String {
    backend::services::auth::encode_token(881, "teacher", "gap-teacher@example.edu", 86400)
        .expect("mint teacher token")
}

fn student_token() -> String {
    backend::services::auth::encode_token(883, "student", "gap-student@example.edu", 86400)
        .expect("mint student token")
}

// ---------------------------------------------------------------------------
// 1. Auth — login + change-password (register/logout already covered)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn auth_login_and_change_password_flow() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        // Register a fresh user.
        let reg = server
            .post("/api/v1/auth/register")
            .json(&json!({
                "username": "gapflow",
                "email": "gapflow@example.edu",
                "password": "CorrectHorse1!",
                "role": "student"
            }))
            .await;
        assert_eq!(reg.status_code(), 200, "register failed: {}", reg.text());
        let reg_json = json_body(&reg.text());
        assert!(reg_json["token"].is_string(), "register must return a token");

        // LOGIN — happy path with the registered credentials.
        let login = server
            .post("/api/v1/auth/login")
            .json(&json!({
                "email": "gapflow@example.edu",
                "password": "CorrectHorse1!"
            }))
            .await;
        assert_eq!(login.status_code(), 200, "login failed: {}", login.text());
        let login_json = json_body(&login.text());
        assert!(login_json["token"].is_string(), "login must return a token");
        assert!(login_json["user_id"].is_i64(), "login must return user_id");
        let bearer = format!("Bearer {}", login_json["token"].as_str().unwrap_or_default());

        // CHANGE-PASSWORD — authenticated, with the current password.
        let change = server
            .post("/api/v1/auth/change-password")
            .add_header("Authorization", bearer.clone())
            .json(&json!({
                "current_password": "CorrectHorse1!",
                "new_password": "StapleTone2!"
            }))
            .await;
        assert_eq!(
            change.status_code(),
            200,
            "change-password failed: {}",
            change.text()
        );

        // Old password must now fail; new password must succeed.
        let old = server
            .post("/api/v1/auth/login")
            .json(&json!({ "email": "gapflow@example.edu", "password": "CorrectHorse1!" }))
            .await;
        assert!(
            old.status_code().as_u16() >= 400,
            "login with stale password must be rejected, got {}",
            old.status_code()
        );
        let renewed = server
            .post("/api/v1/auth/login")
            .json(&json!({ "email": "gapflow@example.edu", "password": "StapleTone2!" }))
            .await;
        assert_eq!(renewed.status_code(), 200, "login with new password failed");

        // Unauthenticated change-password must be rejected.
        let anon = server
            .post("/api/v1/auth/change-password")
            .json(&json!({ "current_password": "x", "new_password": "y" }))
            .await;
        assert!(
            anon.status_code().as_u16() >= 400,
            "anonymous change-password must be rejected, got {}",
            anon.status_code()
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 2. Probe diagnostics — health / schema / indexes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn probe_diagnostic_routes() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let auth = format!("Bearer {}", teacher_token());
        for route in [
            "/api/v1/management/probe/health",
            "/api/v1/management/probe/schema",
            "/api/v1/management/probe/indexes",
        ] {
            let resp = server
                .get(route)
                .add_header("Authorization", auth.clone())
                .await;
            assert_eq!(resp.status_code(), 200, "GET {route} failed");
            let body = json_body(&resp.text());
            assert!(body.is_object(), "{route} must return a JSON object");
        }

        // Unauthenticated callers must be rejected on the management surface.
        let anon = server.get("/api/v1/management/probe/schema").await;
        assert!(
            anon.status_code().as_u16() >= 400,
            "anonymous probe/schema must be rejected, got {}",
            anon.status_code()
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 3. Students — scan-playground + create-profiles
// ---------------------------------------------------------------------------

#[tokio::test]
async fn students_scan_playground_and_create_profiles() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let auth = format!("Bearer {}", teacher_token());

        // scan-playground against the real playground (query-param path).
        let scan = server
            .get("/api/v1/management/students/scan-playground")
            .add_header("Authorization", auth.clone())
            .await;
        assert_eq!(scan.status_code(), 200, "scan-playground failed");
        let scan_json = json_body(&scan.text());
        assert!(scan_json.is_object() || scan_json.is_array(),
            "scan-playground must return JSON, got: {}", scan.text());

        // create-profiles into an isolated temp base (never the real playground).
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let create = server
            .post("/api/v1/management/students/create-profiles")
            .add_header("Authorization", auth.clone())
            .json(&json!({
                "names": ["Gap Test Student"],
                "base_path": tmp.path().display().to_string(),
                "copy_demo_collection": false
            }))
            .await;
        assert_eq!(create.status_code(), 200, "create-profiles failed: {}", create.text());
        let created = json_body(&create.text());
        assert!(created.is_object(), "create-profiles must return a JSON object");

        // The created profile must now be visible to a scan of the same base.
        let scan2 = server
            .get(&format!(
                "/api/v1/management/students/scan-playground?path={}",
                urlencoding_escape(tmp.path().display().to_string())
            ))
            .add_header("Authorization", auth)
            .await;
        assert_eq!(scan2.status_code(), 200, "post-create scan failed");
        let scan2_text = scan2.text();
        assert!(
            scan2_text.contains("Gap Test Student"),
            "scan after create-profiles must list the new profile, got: {scan2_text}"
        );
    })
    .await;
}

/// Minimal percent-encoding for a path query param (spaces only — test paths
/// are ASCII and lack reserved characters beyond spaces).
fn urlencoding_escape(s: String) -> String {
    s.replace(' ', "%20")
}

// ---------------------------------------------------------------------------
// 4. Tracks — full CRUD (list/create covered; :id GET/PUT/DELETE were not)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tracks_id_crud_lifecycle() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let auth = format!("Bearer {}", teacher_token());

        let create = server
            .post("/api/v1/management/tracks")
            .add_header("Authorization", auth.clone())
            .json(&json!({
                "name": "Gap CRUD Track",
                "tag": "gap::crud",
                "subject_area": "testing",
                "target_retention": 0.9,
                "n_value": 3
            }))
            .await;
        assert_eq!(create.status_code(), 200, "track create failed: {}", create.text());
        let created = json_body(&create.text());
        let track_id = created["id"]
            .as_str()
            .or_else(|| created["track"]["id"].as_str())
            .unwrap_or_else(|| panic!("track create must return an id: {}", create.text()))
            .to_string();

        // GET :id
        let get = server
            .get(&format!("/api/v1/management/tracks/{track_id}"))
            .add_header("Authorization", auth.clone())
            .await;
        assert_eq!(get.status_code(), 200, "track GET :id failed: {}", get.text());

        // PUT :id
        let put = server
            .put(&format!("/api/v1/management/tracks/{track_id}"))
            .add_header("Authorization", auth.clone())
            .json(&json!({
                "name": "Gap CRUD Track v2",
                "tag": "gap::crud",
                "subject_area": "testing",
                "target_retention": 0.85,
                "n_value": 4
            }))
            .await;
        assert_eq!(put.status_code(), 200, "track PUT :id failed: {}", put.text());

        // DELETE :id
        let del = server
            .delete(&format!("/api/v1/management/tracks/{track_id}"))
            .add_header("Authorization", auth.clone())
            .await;
        assert!(
            del.status_code().as_u16() < 500,
            "track DELETE :id must not 5xx, got {}",
            del.status_code()
        );

        // GET after delete — must not 5xx (404 or graceful empty both fine).
        let gone = server
            .get(&format!("/api/v1/management/tracks/{track_id}"))
            .add_header("Authorization", auth)
            .await;
        assert!(
            gone.status_code().as_u16() < 500,
            "track GET after delete must not 5xx, got {}",
            gone.status_code()
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 5. Decks — track bind/unbind (:deck_id/track PUT/DELETE)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn decks_track_bind_unbind_contract() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let auth = format!("Bearer {}", teacher_token());

        // Create a real track so the bind target exists.
        let create = server
            .post("/api/v1/management/tracks")
            .add_header("Authorization", auth.clone())
            .json(&json!({
                "name": "Bind Target Track",
                "tag": "gap::bind",
                "subject_area": "testing"
            }))
            .await;
        assert_eq!(create.status_code(), 200);
        let created = json_body(&create.text());
        let track_id = created["id"]
            .as_str()
            .or_else(|| created["track"]["id"].as_str())
            .expect("track id")
            .to_string();

        let unknown_deck = "00000000-0000-0000-0000-00000000de01";

        // PUT bind — unknown deck must degrade (4xx), never 5xx.
        let bind = server
            .put(&format!("/api/v1/management/decks/{unknown_deck}/track"))
            .add_header("Authorization", auth.clone())
            .json(&json!({ "track_id": track_id }))
            .await;
        assert!(
            bind.status_code().as_u16() < 500,
            "deck bind on unknown deck must not 5xx, got {} — {}",
            bind.status_code(),
            bind.text()
        );

        // DELETE unbind — same contract.
        let unbind = server
            .delete(&format!("/api/v1/management/decks/{unknown_deck}/track"))
            .add_header("Authorization", auth)
            .await;
        assert!(
            unbind.status_code().as_u16() < 500,
            "deck unbind on unknown deck must not 5xx, got {}",
            unbind.status_code()
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 6. Classes — generate-sessions + code enroll
// ---------------------------------------------------------------------------

#[tokio::test]
async fn classes_generate_sessions_and_code_enroll() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let auth = format!("Bearer {}", teacher_token());

        // Create a class to obtain a real class_code.
        let create = server
            .post("/api/v1/management/classes")
            .add_header("Authorization", auth.clone())
            .json(&json!({
                "name": "Gap Session Class",
                "teacher_id": "881",
                "subject_area": "testing"
            }))
            .await;
        assert_eq!(create.status_code(), 200, "class create failed: {}", create.text());
        let created = json_body(&create.text());
        let class_id = created["id"]
            .as_str()
            .or_else(|| created["class"]["id"].as_str())
            .expect("class id")
            .to_string();
        let class_code = created["class_code"]
            .as_str()
            .or_else(|| created["class"]["class_code"].as_str())
            .expect("class_code")
            .to_string();

        // generate-sessions with a nonexistent track_profile_id — the handler
        // must degrade (empty generation / 4xx), never 5xx.
        let gen = server
            .post(&format!("/api/v1/management/classes/{class_id}/generate-sessions"))
            .add_header("Authorization", auth.clone())
            .json(&json!({
                "track_profile_id": "00000000-0000-0000-0000-00000000tp00",
                "n_value": 3
            }))
            .await;
        assert!(
            gen.status_code().as_u16() < 500,
            "generate-sessions with unknown track profile must not 5xx, got {} — {}",
            gen.status_code(),
            gen.text()
        );

        // code lookup + student self-enroll by code.
        let lookup = server
            .get(&format!("/api/v1/management/classes/code/{class_code}"))
            .add_header("Authorization", auth.clone())
            .await;
        assert_eq!(lookup.status_code(), 200, "code lookup failed: {}", lookup.text());

        let enroll = server
            .post(&format!("/api/v1/management/classes/code/{class_code}/enroll"))
            .add_header("Authorization", format!("Bearer {}", student_token()))
            .json(&json!({ "display_name": "Gap Enrolled Student" }))
            .await;
        assert!(
            enroll.status_code().as_u16() < 500,
            "enroll-by-code must not 5xx, got {} — {}",
            enroll.status_code(),
            enroll.text()
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 7. Compliance — weekly + students list
// ---------------------------------------------------------------------------

#[tokio::test]
async fn compliance_weekly_and_students_list() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let auth = format!("Bearer {}", teacher_token());

        let weekly = server
            .get("/api/v1/management/compliance/weekly")
            .add_header("Authorization", auth.clone())
            .await;
        assert_eq!(weekly.status_code(), 200, "compliance/weekly failed");
        assert!(
            json_body(&weekly.text()).is_object(),
            "compliance/weekly must return a JSON object"
        );

        let students = server
            .get("/api/v1/management/compliance/students")
            .add_header("Authorization", auth)
            .await;
        assert_eq!(students.status_code(), 200, "compliance/students failed");
        let body = json_body(&students.text());
        assert!(
            body.is_object() || body.is_array(),
            "compliance/students must return JSON"
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 8. Addons — full surface (list/install/distribute/config)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn addons_surface() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let auth = format!("Bearer {}", teacher_token());

        // GET list
        let list = server
            .get("/api/v1/management/addons")
            .add_header("Authorization", auth.clone())
            .await;
        assert_eq!(list.status_code(), 200, "addons list failed");
        assert!(
            json_body(&list.text()).is_array() || json_body(&list.text()).is_object(),
            "addons list must return JSON"
        );

        // POST install (raw bytes body)
        let install = server
            .post("/api/v1/management/addons/install")
            .add_header("Authorization", auth.clone())
            .bytes(b"fake-addon-blob".to_vec().into())
            .await;
        assert_eq!(install.status_code(), 200, "addon install failed: {}", install.text());
        let addon = json_body(&install.text());
        let addon_id = addon["id"].as_str().expect("installed addon id").to_string();

        // POST distribute
        let distribute = server
            .post(&format!("/api/v1/management/addons/{addon_id}/distribute"))
            .add_header("Authorization", auth.clone())
            .json(&json!({ "target": "class:00000000-0000-0000-0000-00000000cl00" }))
            .await;
        assert!(
            distribute.status_code().as_u16() < 500,
            "addon distribute must not 5xx, got {} — {}",
            distribute.status_code(),
            distribute.text()
        );

        // GET config
        let get_cfg = server
            .get(&format!("/api/v1/management/addons/{addon_id}/config"))
            .add_header("Authorization", auth.clone())
            .await;
        assert_eq!(get_cfg.status_code(), 200, "addon config GET failed");

        // PUT config
        let put_cfg = server
            .put(&format!("/api/v1/management/addons/{addon_id}/config"))
            .add_header("Authorization", auth)
            .json(&json!({ "setting": "value" }))
            .await;
        assert!(
            put_cfg.status_code().as_u16() < 500,
            "addon config PUT must not 5xx, got {}",
            put_cfg.status_code()
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 9. Jev — translator routes (local) + evaluate contract
// ---------------------------------------------------------------------------

#[tokio::test]
async fn jev_translator_routes() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let auth = format!("Bearer {}", teacher_token());

        // human-to-jev — local translation, no API key required.
        let h2j = server
            .post("/api/v1/management/jev/human-to-jev")
            .add_header("Authorization", auth.clone())
            .json(&json!({ "prompt": "Did the student answer correctly?" }))
            .await;
        assert_eq!(h2j.status_code(), 200, "human-to-jev failed: {}", h2j.text());
        let h2j_json = json_body(&h2j.text());
        assert!(
            h2j_json["request"].is_object(),
            "human-to-jev must return a Jev request"
        );
        assert!(
            h2j_json["used_llm"].is_boolean(),
            "human-to-jev must report used_llm"
        );

        // jev-to-human — render a well-formed Jev request.
        let j2h = server
            .post("/api/v1/management/jev/jev-to-human")
            .add_header("Authorization", auth.clone())
            .json(&json!({
                "request": {
                    "state": {"answer": "B"},
                    "model": "jev-latest",
                    "questions": {
                        "q1": {
                            "type": "choice",
                            "instructions": "Pick the correct option",
                            "criteria": {"options": ["a", "b"]}
                        }
                    }
                }
            }))
            .await;
        assert_eq!(j2h.status_code(), 200, "jev-to-human failed: {}", j2h.text());

        // evaluate — requires TYPESAFE_API_KEY; without it the handler must
        // fail loudly (4xx/5xx), never return a fake success.
        let eval = server
            .post("/api/v1/management/jev/evaluate")
            .add_header("Authorization", auth)
            .json(&json!({
                "request": {
                    "state": {"answer": "B"},
                    "model": "jev-latest",
                    "questions": {
                        "q1": {
                            "type": "choice",
                            "instructions": "Pick the correct option",
                            "criteria": {"options": ["a", "b"]}
                        }
                    }
                }
            }))
            .await;
        assert!(
            eval.status_code().as_u16() >= 400,
            "jev/evaluate without TYPESAFE_API_KEY must not succeed, got {}",
            eval.status_code()
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 10. Stats — stats + exceptions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stats_routes() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let auth = format!("Bearer {}", teacher_token());

        let stats = server
            .get("/api/v1/management/stats")
            .add_header("Authorization", auth.clone())
            .await;
        assert_eq!(stats.status_code(), 200, "stats failed: {}", stats.text());
        assert!(json_body(&stats.text()).is_object(), "stats must return an object");

        let exceptions = server
            .get("/api/v1/management/stats/exceptions")
            .add_header("Authorization", auth)
            .await;
        assert_eq!(
            exceptions.status_code(),
            200,
            "stats/exceptions failed: {}",
            exceptions.text()
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 11. Sync stream — route registration contract
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sync_stream_route_registered() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        // `/sync/stream` is a WebSocket upgrade fed by a broadcast sender that
        // is installed by the live-server bootstrap (not the test harness).
        // Contract here: the route is REGISTERED — a plain GET must not 404.
        // (Missing extension in the test harness surfaces as a 5xx rejection,
        // which still proves the route matched.)
        let resp = server.get("/api/v1/sync/stream").await;
        assert_ne!(
            resp.status_code(),
            404,
            "/api/v1/sync/stream must be registered"
        );
    })
    .await;
}

// ---------------------------------------------------------------------------
// 12. Dashboard static assets
// ---------------------------------------------------------------------------

#[tokio::test]
async fn dashboard_static_assets_served() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let assets = [
            ("/dashboard/i18n.js", "javascript"),
            ("/dashboard/rtl.css", "css"),
            ("/dashboard/locales/en-US.json", "json"),
            ("/dashboard/locales/he.json", "json"),
            ("/dashboard/locales/ar.json", "json"),
            ("/dashboard/vendor/popper.min.js", "javascript"),
            ("/dashboard/vendor/tippy.min.js", "javascript"),
            ("/dashboard/vendor/driver.min.js", "javascript"),
        ];
        for (route, kind) in assets {
            let resp = server.get(route).await;
            assert_eq!(resp.status_code(), 200, "GET {route} failed");
            let body = resp.text();
            assert!(!body.is_empty(), "{route} must not be empty");
            match kind {
                "json" => assert!(
                    serde_json::from_str::<Value>(&body).is_ok(),
                    "{route} must be valid JSON"
                ),
                _ => {}
            }
        }

        // All 13 launch locales must be served.
        for locale in [
            "en-US", "he", "ar", "fr", "es", "de", "it", "zh", "ko", "ja", "ru", "pt", "hi",
        ] {
            let resp = server
                .get(&format!("/dashboard/locales/{locale}.json"))
                .await;
            assert_eq!(resp.status_code(), 200, "locale {locale} not served");
        }
    })
    .await;
}