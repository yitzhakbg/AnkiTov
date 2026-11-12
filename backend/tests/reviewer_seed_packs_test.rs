//! T-204: `GET /api/v1/reviewer/seed-packs` — the student-accessible seed
//! catalog. The reviewer SPA previously fetched a nonexistent
//! `/api/v1/decks/available` (teacher-gated anyway) and silently rendered
//! zero packs. These tests pin: (1) the endpoint is JWT-gated, (2) a
//! student JWT lists the packages in the seed dir with manifest-style
//! metadata.
//!
//! ANKITOV_SEED_DIR is process-global — safe under nextest's
//! one-process-per-test isolation only; do NOT run this suite under plain
//! `cargo test` with default threading.

use backend::app::App;
use loco_rs::testing;

fn student_token() -> String {
    backend::services::auth::encode_token(1001, "student", "maya.chen@example.edu", 86400)
        .expect("mint student token")
}

/// Point the seed dir at a unique temp dir containing one fake package.
fn set_seed_dir_with_one_package() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ankitov-seedpacks-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create seed dir");
    let pkg = dir.join("math-definitions-en.apkg");
    std::fs::write(&pkg, b"PK\x03\x04fake-apkg-bytes").expect("write fake package");
    std::env::set_var("ANKITOV_SEED_DIR", &dir);
    dir
}

#[tokio::test]
async fn seed_packs_requires_auth() {
    testing::request::<App, _, _>(|server, _ctx| async move {
        let res = server.get("/api/v1/reviewer/seed-packs").await;
        assert_eq!(res.status_code(), 401, "no bearer token must be rejected");
    })
    .await;
}

#[tokio::test]
async fn seed_packs_lists_packages_for_student() {
    let dir = set_seed_dir_with_one_package();
    testing::request::<App, _, _>(|server, _ctx| async move {
        let res = server
            .get("/api/v1/reviewer/seed-packs")
            .add_header("Authorization", format!("Bearer {}", student_token()))
            .await;
        assert_eq!(res.status_code(), 200);
        let body: serde_json::Value = serde_json::from_slice(res.as_bytes()).expect("json body");
        let packs = body.as_array().expect("seed-packs must be a JSON array");
        assert_eq!(packs.len(), 1, "one fake package in the seed dir");
        let p = &packs[0];
        assert_eq!(p["package"], "math-definitions-en.apkg");
        assert_eq!(p["file_size_bytes"], 19);
        // name/language fall back to filename-derived values
        assert!(p["name"].as_str().is_some());
        assert_eq!(p["language"], "en");
    })
    .await;
    let _ = std::fs::remove_dir_all(&dir);
}
