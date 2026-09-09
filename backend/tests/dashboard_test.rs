//! Integration tests for the dashboard UI.

use loco_rs::testing;

#[tokio::test]
async fn dashboard_index_returns_html() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server.get("/dashboard/").await;
        assert_eq!(resp.status_code(), 200, "dashboard index should return 200");

        let body = resp.text();
        assert!(body.contains("AnkiTov"), "dashboard should contain AnkiTov branding");
        assert!(
            body.contains("Management Console"),
            "dashboard should contain Management Console title"
        );
        assert!(body.contains("Dashboard"), "dashboard should contain Dashboard nav");
        assert!(body.contains("Decks"), "dashboard should contain Decks section");
    })
    .await;
}

#[tokio::test]
async fn dashboard_health_returns_ok() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server.get("/dashboard/health").await;
        assert_eq!(resp.status_code(), 200, "dashboard health should return 200");
        assert!(resp.text().contains("OK"), "health endpoint should return OK");
    })
    .await;
}

#[tokio::test]
async fn api_health_returns_json() {
    testing::request::<backend::App, _, _>(|server, _ctx| async move {
        let resp = server.get("/api/v1/health").await;
        assert_eq!(resp.status_code(), 200, "API health should return 200");

        let body = resp.text();
        assert!(body.contains(r#""ok""#), "should return status ok");
        assert!(body.contains("ankitov"), "should mention service name");
    })
    .await;
}
