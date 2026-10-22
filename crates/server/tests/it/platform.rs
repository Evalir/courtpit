//! `POST /api/v1/platform/communities`: the platform operator creates communities with the
//! platform token, before any community (and so any session) exists.

use axum::http::StatusCode;
use racquetcollective_server::Config;
use serde_json::json;

use crate::common::{TestApp, test_config};

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

async fn spawn_with_token() -> TestApp {
    TestApp::spawn_with(Config { platform_token: Some(TOKEN.parse().unwrap()), ..test_config() })
        .await
}

#[tokio::test]
async fn without_a_configured_token_the_route_does_not_exist() {
    let app = TestApp::spawn().await;
    let body = app
        .post("/api/v1/platform/communities")
        .bearer(TOKEN)
        .json(json!({ "slug": "club", "name": "Club" }))
        .send()
        .await
        .expect(StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}

#[tokio::test]
async fn a_missing_or_wrong_token_is_refused() {
    let app = spawn_with_token().await;
    let create = json!({ "slug": "club", "name": "Club" });
    for request in [
        app.post("/api/v1/platform/communities"),
        app.post("/api/v1/platform/communities").bearer("0123456789abcdef0123456789abcdeX"),
    ] {
        let body = request.json(create.clone()).send().await.expect(StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "unauthorized");
    }
}

#[tokio::test]
async fn the_operator_creates_a_community_its_owner_can_sign_in_to() {
    let app = spawn_with_token().await;
    let created = app
        .post("/api/v1/platform/communities")
        .bearer(TOKEN)
        .json(json!({
            "slug": "riverside",
            "name": "Riverside Tennis",
            "custom_domain": "Courts.Example.org",
            "owner_email": "owner@example.test",
        }))
        .send()
        .await
        .expect(StatusCode::CREATED);
    assert_eq!(created["slug"], "riverside");
    assert_eq!(created["custom_domain"], "courts.example.org");

    let tenant = app
        .get("/api/v1/tenant")
        .header("host", "courts.example.org")
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(tenant["name"], "Riverside Tennis");

    let owner = app.login("owner@example.test", "riverside").await;
    assert_eq!(owner.player_id.to_string(), created["owner_player_id"].as_str().unwrap());
    let me = app.get("/api/v1/me").as_(&owner).send().await.expect(StatusCode::OK);
    assert_eq!(me["player"]["role"], "owner");
}

#[tokio::test]
async fn taken_and_malformed_slugs_are_refused() {
    let app = spawn_with_token().await;
    let create = |slug: &str| {
        app.post("/api/v1/platform/communities")
            .bearer(TOKEN)
            .json(json!({ "slug": slug, "name": "Club" }))
    };
    let _ = create("club").send().await.expect(StatusCode::CREATED);
    let taken = create("club").send().await.expect(StatusCode::CONFLICT);
    assert_eq!(taken["error"]["code"], "conflict");
    let malformed = create("-bad-").send().await.expect(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(malformed["error"]["code"], "validation_failed");
}
