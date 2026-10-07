//! Endpoint coverage harness — H1 (white-box) counterpart for every remaining
//! uncovered `/api/v1/*` route, so the double-harness done-gate
//! (`scripts/harness2/run_all.sh`) is airtight before P1–P5 build on it.
//!
//! Mirrors the H2 black-box scenarios in `scripts/harness2/run.py`, asserted
//! in-process via `loco_rs::testing` against an in-memory SQLite DB:
//!
//! 1. `/management/anki/*` — ALL 17 routes (Zone 1/2/3 surface). Every route
//!    must answer `200` + a JSON object whether or not AnkiConnect is
//!    reachable (handlers degrade to `success:false` instead of 5xx).
//! 2. `/management/probe/deck-insert` + `/deck-insert-minimal` — diagnostic
//!    DB write paths must insert and report `success:true`.
//! 3. `/management/sync/status`, `/users`, `/users/{id}`, `/users/{id}/trigger`,
//!    `/full` — per-user status stub + role-gated triggers.
//! 4. `/locale`, `/locale/info`, `/locale/list`, `/locale/set` — public i18n
//!    surface; setting `he` must report RTL direction (launch-wave invariant).
//! 5. `/management/capsule-sessions/{id}` GET, `/{id}/deliver` POST,
//!    `/capsule-sessions/generate-all`, `/generation-jobs/latest` —
//!    inspection/delivery contract + background job lifecycle.
//! 6. `/management/students/bulk-enroll`, `/populate-decks`, `/import-file` —
//!    enrollment write paths incl. multipart CSV happy path.
//!
//! Run with: `cargo nextest run -p backend --test endpoint_coverage_test`

use loco_rs::testing;
use serde_json::{json, Value};

/// Parse a response body as JSON, panicking with the raw body on failure.
fn json_body(text: &str) -> Value {
    match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => panic!("invalid JSON: {e}\nbody: {text:?}"),
    }
}

/// Mint a valid `teacher` JWT — the management surface is auth-gated.
fn teacher_token() -> String {
    backend::services::auth::encode_token(997, "teacher", "endpoint-coverage@example.edu", 86400)
        .expect("minting teacher token for endpoint coverage test must succeed")
}

/// Mint a valid `admin` JWT — the Prong-8 teacher-management routes
/// (list / create / get_detail / resend) are admin-gated.
fn admin_token() -> String {
    backend::services::auth::encode_token(998, "admin", "admin@endpoint-coverage.example.edu", 86400)
        .expect("minting admin token for endpoint coverage test must succeed")
}

/// Build a multipart body for `/management/students/import-file` with a
/// `class_id` field plus a CSV `file` field (or without `class_id` to test
/// the BadRequest path).
fn multipart_import_body(class_id: Option<&str>, csv: &str) -> (String, Vec<u8>) {
    let boundary = "ankitov-test-bnd-998";
    let mut body = Vec::new();
    if let Some(cid) = class_id {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"class_id\"\r\n\r\n{cid}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"students.csv\"\r\nContent-Type: text/csv\r\n\r\n{csv}\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. anki_ops — full 17-route surface
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_anki_ops_full_endpoint_surface() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let auth = format!("Bearer {}", teacher_token());

        // Health probe is informational (AnkiConnect may be absent in CI).
        let health = server
            .get("/api/v1/management/anki/health")
            .add_header("Authorization", auth.clone())
            .await;
        assert_eq!(health.status_code(), 200);
        let health_json = json_body(&health.text());
        assert!(health_json.is_object(), "health must be a JSON object");

        // ALL GET routes — airtight contract: 200 + JSON object, never 5xx.
        let get_routes = [
            "/api/v1/management/anki/decks",
            "/api/v1/management/anki/profiles",
            "/api/v1/management/anki/active-profile",
            "/api/v1/management/anki/collection-health",
            "/api/v1/management/anki/deck-health/EndpointCoverageProbe",
            "/api/v1/management/anki/full-health/EndpointCoverageProbe",
            "/api/v1/management/anki/due/EndpointCoverageProbe",
            "/api/v1/management/anki/retention-curve/EndpointCoverageProbe",
            "/api/v1/management/anki/lapse/EndpointCoverageProbe",
            "/api/v1/management/anki/interval/EndpointCoverageProbe",
            "/api/v1/management/anki/adherence/EndpointCoverageProbe",
            "/api/v1/management/anki/sporadic/EndpointCoverageProbe",
            "/api/v1/management/anki/gaps/EndpointCoverageProbe",
            "/api/v1/management/anki/suitability/EndpointCoverageProbe",
        ];
        for route in get_routes {
            let resp = server
                .get(route)
                .add_header("Authorization", auth.clone())
                .await;
            assert_eq!(resp.status_code(), 200, "GET {route} must return 200");
            let body = json_body(&resp.text());
            assert!(body.is_object(), "GET {route} must return a JSON object, got: {body:?}");
        }

        // POST routes — backup + sync must also degrade gracefully (200).
        for route in ["/api/v1/management/anki/backup", "/api/v1/management/anki/sync"] {
            let resp = server
                .post(route)
                .add_header("Authorization", auth.clone())
                .await;
            assert_eq!(resp.status_code(), 200, "POST {route} must return 200");
            let body = json_body(&resp.text());
            assert!(body.is_object(), "POST {route} must return a JSON object");
        }
    })
    .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. probe — deck-insert / deck-insert-minimal write paths
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_probe_deck_insert_endpoints() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let auth = format!("Bearer {}", teacher_token());

        // Full insert (payload defaults apply for omitted fields)
        let full = server
            .post("/api/v1/management/probe/deck-insert")
            .add_header("Authorization", auth.clone())
            .json(&json!({
                "name": "endpoint-coverage-probe-full",
                "card_count": 3,
                "file_size_bytes": 128,
                "checksum_sha256": "aaaa",
                "created_at": 1751078400i64,
                "updated_at": 1751078400i64
            }))
            .await;
        assert_eq!(full.status_code(), 200);
        let body = json_body(&full.text());
        assert_eq!(body["success"], true, "deck-insert must succeed: {body:?}");

        // Minimal insert (id + name only, everything else DEFAULT)
        let minimal = server
            .post("/api/v1/management/probe/deck-insert-minimal")
            .add_header("Authorization", auth.clone())
            .json(&json!({ "name": "endpoint-coverage-probe-minimal" }))
            .await;
        assert_eq!(minimal.status_code(), 200);
        let body = json_body(&minimal.text());
        assert_eq!(body["success"], true, "deck-insert-minimal must succeed: {body:?}");

        // Both rows must now be visible via the standard deck list
        let decks = server
            .get("/api/v1/management/decks")
            .add_header("Authorization", auth)
            .await;
        assert_eq!(decks.status_code(), 200);
        let decks_body = json_body(&decks.text());
        let names = decks_body
            .as_array()
            .expect("decks list must be a JSON array")
            .iter()
            .filter_map(|d| d["name"].as_str().map(str::to_string))
            .collect::<Vec<_>>();
        assert!(
            names.iter().any(|n| n == "endpoint-coverage-probe-full"),
            "deck-insert row must appear in /management/decks: {names:?}"
        );
        assert!(
            names.iter().any(|n| n == "endpoint-coverage-probe-minimal"),
            "deck-insert-minimal row must appear in /management/decks: {names:?}"
        );
    })
    .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. sync — status / users / users/{id} / trigger / full
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_sync_endpoint_surface() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let auth = format!("Bearer {}", teacher_token());

        let status = server
            .get("/api/v1/management/sync/status")
            .add_header("Authorization", auth.clone())
            .await;
        assert_eq!(status.status_code(), 200);

        let users = server
            .get("/api/v1/management/sync/users")
            .add_header("Authorization", auth.clone())
            .await;
        assert_eq!(users.status_code(), 200);

        // Per-user status stub — idle for any id
        let user = server
            .get("/api/v1/management/sync/users/endpoint-cov-user")
            .add_header("Authorization", auth.clone())
            .await;
        assert_eq!(user.status_code(), 200);
        let body = json_body(&user.text());
        assert_eq!(body["user_id"], "endpoint-cov-user");
        assert_eq!(body["status"], "idle");

        // Trigger per-user sync (teacher role allowed)
        let trigger = server
            .post("/api/v1/management/sync/users/endpoint-cov-user/trigger")
            .add_header("Authorization", auth.clone())
            .await;
        assert_eq!(trigger.status_code(), 200);
        let body = json_body(&trigger.text());
        assert_eq!(body["success"], true, "trigger must succeed: {body:?}");

        // Full sync (teacher/admin allowed)
        let full = server
            .post("/api/v1/management/sync/full")
            .add_header("Authorization", auth)
            .await;
        assert_eq!(full.status_code(), 200);
        let body = json_body(&full.text());
        assert_eq!(body["success"], true, "full sync must succeed: {body:?}");
    })
    .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. locale — public i18n surface incl. RTL invariant
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_locale_endpoint_surface() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        // Full translation map for the current locale
        let map = server.get("/api/v1/locale").await;
        assert_eq!(map.status_code(), 200);
        assert!(json_body(&map.text()).is_object(), "locale map must be an object");

        // Locale metadata
        let info = server.get("/api/v1/locale/info").await;
        assert_eq!(info.status_code(), 200);
        let body = json_body(&info.text());
        for key in ["locale", "direction", "native_name", "english_name"] {
            assert!(body.get(key).is_some(), "locale/info missing key {key}: {body:?}");
        }

        // Supported locales list
        let list = server.get("/api/v1/locale/list").await;
        assert_eq!(list.status_code(), 200);
        let body = json_body(&list.text());
        assert!(body.is_array(), "locale/list must be an array");

        // Set he → RTL (launch-wave invariant), then restore en-US
        let set_he = server
            .post("/api/v1/locale/set")
            .json(&json!({ "locale": "he" }))
            .await;
        assert_eq!(set_he.status_code(), 200);
        let body = json_body(&set_he.text());
        assert_eq!(body["locale"], "he");
        assert_eq!(body["direction"], "rtl", "he must be RTL: {body:?}");

        let set_en = server
            .post("/api/v1/locale/set")
            .json(&json!({ "locale": "en-US" }))
            .await;
        assert_eq!(set_en.status_code(), 200);
        let body = json_body(&set_en.text());
        assert_eq!(body["locale"], "en-US");
        assert_eq!(body["direction"], "ltr");
    })
    .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. capsule-sessions — inspect / deliver / generate-all / generation-jobs
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_capsule_sessions_inspect_deliver_and_generation_jobs() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let auth = format!("Bearer {}", teacher_token());

        // List sessions (may be empty)
        let list = server
            .get("/api/v1/management/capsule-sessions")
            .add_header("Authorization", auth.clone())
            .await;
        assert_eq!(list.status_code(), 200);

        // GET unknown session → graceful 200 + success:false
        let unknown = "00000000-0000-0000-0000-000000000000";
        let get_unknown = server
            .get(&format!("/api/v1/management/capsule-sessions/{unknown}"))
            .add_header("Authorization", auth.clone())
            .await;
        assert_eq!(get_unknown.status_code(), 200);
        let body = json_body(&get_unknown.text());
        assert_eq!(body["success"], false, "unknown session must be graceful: {body:?}");

        // Deliver unknown session → graceful (404 or 200+success:false), never 5xx
        let deliver_unknown = server
            .post(&format!("/api/v1/management/capsule-sessions/{unknown}/deliver"))
            .add_header("Authorization", auth.clone())
            .await;
        let status = deliver_unknown.status_code().as_u16();
        assert!(
            status == 404 || status == 400 || status == 200,
            "deliver unknown session must be graceful, got {status}"
        );
        if status == 200 {
            let body = json_body(&deliver_unknown.text());
            assert_eq!(body["success"], false);
        }

        // Generate for an unknown student → graceful error, not 5xx
        let generate = server
            .post("/api/v1/management/capsule-sessions/generate")
            .add_header("Authorization", auth.clone())
            .json(&json!({
                "student_id": "endpoint-cov-nosuch-student",
                "anki2_path": "",
                "track_profile_id": ""
            }))
            .await;
        let status = generate.status_code().as_u16();
        assert!(
            (200..500).contains(&status),
            "generate unknown student must be graceful, got {status}"
        );

        // generate-all → 200 with a background job {id, status}
        // (redirect the playground scan to a nonexistent dir so the spawned
        // batch worker finishes instantly with zero students)
        std::env::set_var("ANKIPLAYGROUND_PATH", "/tmp/endpoint-cov-no-playground");
        let generate_all = server
            .post("/api/v1/management/capsule-sessions/generate-all")
            .add_header("Authorization", auth.clone())
            .await;
        assert_eq!(generate_all.status_code(), 200);
        let body = json_body(&generate_all.text());
        assert!(body["id"].is_string(), "generate-all must return job id: {body:?}");
        assert_eq!(body["status"], "pending", "fresh job must be pending: {body:?}");

        // generation-jobs/latest must now surface the (or a) job
        let latest = server
            .get("/api/v1/management/generation-jobs/latest")
            .add_header("Authorization", auth)
            .await;
        assert_eq!(latest.status_code(), 200);
        let body = json_body(&latest.text());
        assert!(body["id"].is_string(), "latest job must have an id: {body:?}");
    })
    .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. students — bulk-enroll / populate-decks / import-file
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_students_bulk_enroll_populate_decks_and_import_file() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let auth = format!("Bearer {}", teacher_token());

        // Create a class to enroll into
        let create = server
            .post("/api/v1/management/classes")
            .add_header("Authorization", auth.clone())
            .json(&json!({
                "name": "Endpoint Coverage Bulk Class",
                "subject_area": "math",
                "teacher_id": "endpoint-cov",
                "period": "1",
                "academic_year": "2026-2027"
            }))
            .await;
        assert_eq!(create.status_code(), 200);
        let class_id = json_body(&create.text())["id"]
            .as_str()
            .expect("class create must return id")
            .to_string();

        // bulk-enroll → 200 {enrolled: 2, total: 2}
        let bulk = server
            .post("/api/v1/management/students/bulk-enroll")
            .add_header("Authorization", auth.clone())
            .json(&json!({
                "class_id": class_id,
                "student_ids": ["endpoint-cov-stu-a", "endpoint-cov-stu-b"],
                "display_names": ["Coverage A", "Coverage B"]
            }))
            .await;
        assert_eq!(bulk.status_code(), 200);
        let body = json_body(&bulk.text());
        assert_eq!(body["enrolled"], 2, "bulk-enroll must enroll 2: {body:?}");
        assert_eq!(body["total"], 2);

        // populate-decks → enrollments exist but no profile dirs → populated: 0
        let populate = server
            .post("/api/v1/management/students/populate-decks")
            .add_header("Authorization", auth.clone())
            .json(&json!({ "class_id": class_id }))
            .await;
        assert_eq!(populate.status_code(), 200);
        let body = json_body(&populate.text());
        assert_eq!(body["populated"], 0, "no profile dirs → populated:0: {body:?}");
        assert_eq!(body["total_students"], 2, "roster of 2 must be counted: {body:?}");

        // import-file happy path → 200 {enrolled: 2, errors: [], students: 2}
        let csv = "student_id,display_name\nendpoint-cov-imp-a,Import A\nendpoint-cov-imp-b,Import B\n";
        let (content_type, body_bytes) = multipart_import_body(Some(&class_id), csv);
        let import = server
            .post("/api/v1/management/students/import-file")
            .add_header("Authorization", auth.clone())
            .content_type(&content_type)
            .bytes(axum::body::Bytes::from(body_bytes))
            .await;
        assert_eq!(import.status_code(), 200);
        let body = json_body(&import.text());
        assert_eq!(body["enrolled"], 2, "import-file must enroll 2: {body:?}");
        assert_eq!(body["errors"], json!([]), "valid CSV must have no errors: {body:?}");
        assert_eq!(body["students"].as_array().map(Vec::len), Some(2));

        // import-file without class_id field → clean 400 BadRequest
        let (ct_no_class, body_no_class) = multipart_import_body(None, csv);
        let missing = server
            .post("/api/v1/management/students/import-file")
            .add_header("Authorization", auth.clone())
            .content_type(&ct_no_class)
            .bytes(axum::body::Bytes::from(body_no_class))
            .await;
        assert_eq!(
            missing.status_code(),
            400,
            "import-file without class_id must be a clean 400: {}",
            missing.text()
        );

        // Cleanup class
        let cleanup = server
            .delete(&format!("/api/v1/management/classes/{class_id}"))
            .add_header("Authorization", auth)
            .await;
        assert!(
            cleanup.status_code() == 200 || cleanup.status_code() == 204,
            "class cleanup must succeed, got {}",
            cleanup.status_code()
        );
    })
    .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. teachers (Prong 8) — list / create / get_detail / resend
//    All four are ADMIN-gated; a plain `teacher` token must be rejected with 403.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_teachers_management_surface() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let admin = format!("Bearer {}", admin_token());
        let teacher = format!("Bearer {}", teacher_token());

        // ── 1. Role gate: a non-admin teacher token must be 403 on every
        //      admin-gated route (proves the authz boundary, not just 5xx).
        let gate_list = server
            .get("/api/v1/management/teachers")
            .add_header("Authorization", teacher.clone())
            .await;
        assert_eq!(gate_list.status_code(), 403, "list as teacher must be 403");

        let gate_create = server
            .post("/api/v1/management/teachers")
            .add_header("Authorization", teacher.clone())
            .json(&json!({
                "full_name": "Gate Probe",
                "email": "gate-probe@endpoint-coverage.example.edu",
                "school": "Gate School",
                "locale": "en-US"
            }))
            .await;
        assert_eq!(gate_create.status_code(), 403, "create as teacher must be 403");

        // ── 2. list (admin) → 200 + JSON array (may be empty before create).
        let list = server
            .get("/api/v1/management/teachers")
            .add_header("Authorization", admin.clone())
            .await;
        assert_eq!(list.status_code(), 200, "list as admin must be 200");
        let list_body = json_body(&list.text());
        assert!(
            list_body.is_array(),
            "list must return a JSON array, got: {list_body:?}"
        );

        // ── 3. create (admin) → 200/201 + user + invite(token, invite_url) + audit.
        //      A fresh unique email avoids the 409 duplicate-email / active-invite paths.
        let created_email = "coverage-teacher-1@endpoint-coverage.example.edu";
        let create = server
            .post("/api/v1/management/teachers")
            .add_header("Authorization", admin.clone())
            .json(&json!({
                "full_name": "Coverage Teacher One",
                "email": created_email,
                "school": "Coverage High School",
                "community": "pilot",
                "role": "teacher",
                "locale": "en-US"
            }))
            .await;
        assert!(
            create.status_code() == 200 || create.status_code() == 201,
            "create as admin must be 2xx, got {}",
            create.status_code()
        );
        let created = json_body(&create.text());
        assert!(
            created["user"]["id"].is_i64(),
            "create must return user.id (i64): {created:?}"
        );
        assert_eq!(created["user"]["role"], "teacher");
        assert_eq!(created["user"]["email"], created_email);
        assert_eq!(created["user"]["locale"], "en-US");
        // The invite must carry a 256-bit hex token (64 hex chars) and a relative /d/ URL.
        assert_eq!(
            created["invite"]["token"].as_str().map(|t| t.len()),
            Some(64),
            "invite.token must be a 256-bit (64-char) hex string: {created:?}"
        );
        let invite_url = created["invite"]["invite_url"].as_str().unwrap_or("");
        assert!(
            invite_url.starts_with("/d/"),
            "invite_url must be a relative /d/ link, got: {invite_url:?}"
        );
        assert_eq!(created["audit"]["event_type"], "teacher_enrolled");

        let new_user_id = created["user"]["id"].as_i64().expect("user.id must be i64");

        // ── 4. get_detail (admin) → 200 + user + invite; a bogus id → 404.
        let detail = server
            .get(&format!("/api/v1/management/teachers/{new_user_id}"))
            .add_header("Authorization", admin.clone())
            .await;
        assert_eq!(detail.status_code(), 200, "get_detail as admin must be 200");
        let detail_body = json_body(&detail.text());
        assert_eq!(
            detail_body["user"]["id"],
            serde_json::json!(new_user_id),
            "detail.user.id must match the created id: {detail_body:?}"
        );
        assert_eq!(detail_body["user"]["password_hash_set"], false, "fresh teacher has no password yet");
        assert!(
            detail_body["invite"].is_object(),
            "detail.invite must be an object (a live invite exists): {detail_body:?}"
        );

        // Bogus id → graceful 404 (never 5xx) for an admin.
        let detail_missing = server
            .get("/api/v1/management/teachers/99999999999")
            .add_header("Authorization", admin.clone())
            .await;
        assert_eq!(detail_missing.status_code(), 404, "unknown user_id must be 404");

        // ── 5. resend (admin) → 200 + a ROTATED token that differs from the original.
        let resend = server
            .post(&format!("/api/v1/management/teachers/{new_user_id}/resend"))
            .add_header("Authorization", admin)
            .await;
        assert_eq!(resend.status_code(), 200, "resend as admin must be 200");
        let resent = json_body(&resend.text());
        let original = created["invite"]["token"].as_str().unwrap_or("");
        let rotated = resent["token"].as_str().expect("resend must return a token");
        assert_eq!(rotated.len(), 64, "rotated token must be 256-bit hex: {rotated:?}");
        assert_ne!(
            rotated, original,
            "resend must rotate the invite token (old: {original}, new: {rotated})"
        );
        assert!(
            ["pending", "active"].contains(&resent["status"].as_str().unwrap_or("")),
            "resent invite status must be pending/active, got: {resent:?}"
        );
    })
    .await;
}
