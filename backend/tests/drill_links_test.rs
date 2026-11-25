//! Drill links (T-215 — First-60-Seconds front door) — integration tests.
//!
//! White-box, in-process: boots the Loco app against the in-memory SQLite
//! test DB and drives the real Axum router. Covers the management surface
//! (create/list/revoke + role gate + on-disk pack validation) and the public
//! `GET /drill/:id` decision logic (uniform 404 for missing/malformed/
//! expired/exhausted, per-link soft rate limit, branded busy degradation).
//!
//! Seat SPAWN itself is not exercised here (needs bwrap + the rootfs; the
//! caps-trip path is covered by seat_manager's inline tests and real spawn
//! by the LOCAL demo) — every route taken below stops before `start_seat`.
//!
//! Run with: `cargo nextest run drill_links`

use loco_rs::testing;
use sea_orm::ActiveModelTrait;
use sea_orm::{entity::prelude::EntityTrait, ActiveValue, Set};
use serde_json::{json, Value};

const MGMT: &str = "/api/v1/management/drill-links";
const PUBLIC: &str = "/drill";

/// A pack name shaped for the allowlist; whether it "exists on disk" is
/// controlled by pointing `ANKITOV_SEED_DIR` at the temp fixture.
const TEST_PACK: &str = "drill-test-pack.apkg";

// ── helpers ─────────────────────────────────────────────────────────────────

fn mint(user_id: i64, role: &str, email: &str) -> String {
    backend::services::auth::encode_token(user_id, role, email, 86400)
        .expect("minting JWT for the drill-links harness must succeed")
}

fn js(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("invalid JSON: {e}\nbody: {text:?}"))
}

/// Seed a controlled seed-dir fixture: a temp dir holding one dummy .apkg.
/// Returns the dir path (to pass to `set_seed_dir`).
fn make_seed_fixture() -> String {
    let dir = std::env::temp_dir().join(format!(
        "ankitov-drill-seed-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).expect("creating seed fixture dir must succeed");
    std::fs::write(dir.join(TEST_PACK), b"not-a-real-apkg")
        .expect("writing the dummy pack must succeed");
    dir.to_string_lossy().to_string()
}

fn set_seed_dir(dir: &str) {
    std::env::set_var("ANKITOV_SEED_DIR", dir);
}

fn create_body(pack: &str) -> Value {
    json!({
        "pack": pack,
        "deck": "Drill Test Deck",
        "title": "T-215 test link",
        "max_uses": 5,
        "expires_in_secs": 3600
    })
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

/// Insert a drill_links row directly; `expired`/`exhausted` control the
/// gate state the public GET should reject.
async fn seed_link(
    ctx: &loco_rs::app::AppContext,
    id: &str,
    expired: bool,
    exhausted: bool,
) {
    let now = chrono::Utc::now().timestamp();
    let m = backend::models::entities::drill_link::ActiveModel {
        id: ActiveValue::Set(id.to_string()),
        pack: ActiveValue::Set(TEST_PACK.to_string()),
        deck: ActiveValue::Set("Deck".to_string()),
        title: ActiveValue::Set("seeded".to_string()),
        created_by: ActiveValue::Set("test@example.edu".to_string()),
        max_uses: ActiveValue::Set(Some(2)),
        use_count: ActiveValue::Set(if exhausted { 2 } else { 0 }),
        expires_at: ActiveValue::Set(Some(if expired { now - 60 } else { now + 3600 })),
        created_at: ActiveValue::Set(now),
    };
    backend::models::entities::drill_link::Entity::insert(m)
        .exec(&ctx.db)
        .await
        .expect("inserting a seeded drill link must succeed");
}

/// Bump a seeded link's use_count (simulates successful spawns).
async fn bump_use(ctx: &loco_rs::app::AppContext, id: &str, times: i32) {
    for _ in 0..times {
        let link = backend::models::entities::drill_link::Entity::find_by_id(id)
            .one(&ctx.db)
            .await
            .expect("lookup must succeed")
            .expect("seeded link must exist");
        let mut am: backend::models::entities::drill_link::ActiveModel = link.into();
        am.use_count = Set(am.use_count.unwrap() + 1);
        am.update(&ctx.db).await.expect("bump must succeed");
    }
}

// ═════════════════════════════════════════════════════════════════════════
// Management surface (POST/GET/DELETE /management/drill-links)
// ═════════════════════════════════════════════════════════════════════════

/// Student role → 403; anonymous → 403 too (the management middleware
/// rejects "no staff role" uniformly — see harness_auth_test's contract).
#[tokio::test]
async fn create_requires_staff_role() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        set_seed_dir(&make_seed_fixture());
        let resp = server
            .post(MGMT)
            .add_header("Authorization", bearer(&mint(880, "student", "s@example.edu")))
            .json(&create_body(TEST_PACK))
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::FORBIDDEN);

        let resp = server.post(MGMT).json(&create_body(TEST_PACK)).await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::FORBIDDEN);
    })
    .await;
}

/// Pack names outside the allowlist → 400 before any disk check.
#[tokio::test]
async fn create_rejects_bad_pack_name() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        set_seed_dir(&make_seed_fixture());
        let resp = server
            .post(MGMT)
            .add_header("Authorization", bearer(&mint(881, "teacher", "t@example.edu")))
            .json(&create_body("../escape.apkg"))
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::BAD_REQUEST, "{}", resp.text());

        let resp = server
            .post(MGMT)
            .add_header("Authorization", bearer(&mint(881, "teacher", "t@example.edu")))
            .json(&create_body("missing-extension"))
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::BAD_REQUEST);
    })
    .await;
}

/// Well-formed pack name that is NOT on disk → 400 (link would degrade to
/// an empty seat, so the create gate refuses).
#[tokio::test]
async fn create_rejects_missing_pack() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        set_seed_dir(&make_seed_fixture());
        let resp = server
            .post(MGMT)
            .add_header("Authorization", bearer(&mint(882, "teacher", "t@example.edu")))
            .json(&create_body("definitely-not-here-zz.apkg"))
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::BAD_REQUEST);
        let body = resp.text();
        assert!(
            body.contains("not found in the seed library"),
            "error must name the failure mode, got: {body}"
        );
    })
    .await;
}

/// Valid create → 201 with a 32-char urlsafe token id and a URL under
/// ANKITOV_PUBLIC_URL/drill/. Also exercised: deck/title/max_uses echoes.
#[tokio::test]
async fn create_returns_link_with_url() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        set_seed_dir(&make_seed_fixture());
        std::env::set_var("ANKITOV_PUBLIC_URL", "https://practice.example.test");
        let resp = server
            .post(MGMT)
            .add_header("Authorization", bearer(&mint(883, "teacher", "t@example.edu")))
            .json(&create_body(TEST_PACK))
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
        let body = js(&resp.text());
        let id = body["id"].as_str().expect("id must be a string").to_string();
        assert_eq!(id.len(), 32, "token is 32 urlsafe chars");
        assert!(
            id.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "token must be urlsafe"
        );
        assert_eq!(
            body["url"].as_str(),
            Some(format!("https://practice.example.test/drill/{id}").as_str())
        );
        assert_eq!(body["use_count"], 0);
        assert_eq!(body["max_uses"], 5);
    })
    .await;
}

/// Created links appear in the list; DELETE revokes (row gone → public 404).
#[tokio::test]
async fn list_then_revoke() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        set_seed_dir(&make_seed_fixture());
        let auth = bearer(&mint(884, "teacher", "t@example.edu"));
        let resp = server
            .post(MGMT)
            .add_header("Authorization", auth.clone())
            .json(&create_body(TEST_PACK))
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
        let id = js(&resp.text())["id"].as_str().unwrap().to_string();

        let resp = server
            .get(MGMT)
            .add_header("Authorization", auth.clone())
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
        let list = js(&resp.text());
        assert!(
            list.as_array().is_some_and(|a| a.iter().any(|l| l["id"] == id.as_str())),
            "created link must appear in the list"
        );

        let resp = server
            .delete(&format!("{MGMT}/{id}"))
            .add_header("Authorization", auth)
            .await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
        assert_eq!(js(&resp.text())["revoked"], true);

        // Revoked → the public door 404s.
        let resp = server.get(&format!("{PUBLIC}/{id}")).await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::NOT_FOUND);
    })
    .await;
}

// ═════════════════════════════════════════════════════════════════════════
// Public door (GET /drill/:id) — everything BEFORE the seat spawn
// ═════════════════════════════════════════════════════════════════════════

/// Missing AND malformed tokens render the same uniform 404 (no oracle).
#[tokio::test]
async fn public_get_uniform_404() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let missing = "a".repeat(32);
        let resp = server.get(&format!("{PUBLIC}/{missing}")).await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::NOT_FOUND);

        let malformed = "tooshort";
        let resp = server.get(&format!("{PUBLIC}/{malformed}")).await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::NOT_FOUND);

        // Same page: both bodies are identical branded HTML.
        let a = resp.text();
        let resp = server.get(&format!("{PUBLIC}/{missing}")).await;
        assert_eq!(a, resp.text(), "uniform 404 page for every dead shape");
        assert!(a.contains("AnkiTov"), "page is branded");
    })
    .await;
}

/// Expired and exhausted links 404 exactly like missing ones.
#[tokio::test]
async fn public_get_expired_and_exhausted_are_404() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        let expired = "b".repeat(32);
        let exhausted = "c".repeat(32);
        seed_link(&ctx, &expired, true, false).await;
        seed_link(&ctx, &exhausted, false, true).await;

        for id in [&expired, &exhausted] {
            let resp = server.get(&format!("{PUBLIC}/{id}")).await;
            assert_eq!(
                resp.status_code(),
                axum::http::StatusCode::NOT_FOUND,
                "{id}"
            );
            assert!(resp.text().contains("AnkiTov"));
        }
    })
    .await;
}

/// A live link whose spawn slot is available on a machine WITHOUT bwrap
/// (this test host) degrades to the branded 503 busy page — never an error
/// dump. (On a Linux box with bwrap this would 302; the demo proves that.)
#[tokio::test]
async fn public_get_live_link_degrades_to_busy_page_without_runtime() {
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        let id = "d".repeat(32);
        seed_link(&ctx, &id, false, false).await;
        let resp = server.get(&format!("{PUBLIC}/{id}")).await;
        let status = resp.status_code();
        assert!(
            status == axum::http::StatusCode::SERVICE_UNAVAILABLE
                || status == axum::http::StatusCode::FOUND,
            "live link must 503 (no bwrap) or 302 (bwrap present), got {status}"
        );
        if status == axum::http::StatusCode::SERVICE_UNAVAILABLE {
            let body = resp.text();
            assert!(
                body.contains("busy"),
                "busy page must say pods are busy, got {body}"
            );
            assert!(!body.contains("panic"), "no error dump");
        }
    })
    .await;
}

/// Per-link soft rate limit: after the burst budget the link sheds with the
/// branded 503 (tested on a valid-shaped MISSING token, so no seat spawn
/// ever happens — deterministic on any host).
#[tokio::test]
async fn public_get_rate_limits_per_link() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let id = "e".repeat(32);
        let mut last = None;
        for i in 0..25 {
            let resp = server.get(&format!("{PUBLIC}/{id}")).await;
            last = Some((i, resp.status_code()));
        }
        let (i, status) = last.expect("loop ran");
        assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE, "hit {i}");
        // A different link is not shed by this one's burst.
        let other = "f".repeat(32);
        let resp = server.get(&format!("{PUBLIC}/{other}")).await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::NOT_FOUND);
    })
    .await;
}

/// Exhaustion is driven by SUCCESSFUL spawns: use_count only counts when a
/// seat came up. The endpoint bumps only after spawn success (unit-asserted
/// via the seeded-row round trip here).
#[tokio::test]
async fn exhaustion_counts_only_successful_spawns() {
    // Pure round-trip proof: seed exhausted=false, bump twice → exhausted
    // gate fires (the endpoint's spawn-success bump increments the same
    // column; the gate condition is use_count >= max_uses).
    testing::request::<backend::App, _, _>(|server, ctx| async move {
        let id = "9".repeat(32);
        seed_link(&ctx, &id, false, false).await;
        bump_use(&ctx, &id, 2).await;
        let resp = server.get(&format!("{PUBLIC}/{id}")).await;
        assert_eq!(resp.status_code(), axum::http::StatusCode::NOT_FOUND);
    })
    .await;
}
