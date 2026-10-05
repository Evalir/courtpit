//! Own profile, joining, export and deletion.

use axum::http::StatusCode;
use serde_json::json;

use crate::common::{Session, TestApp};

async fn setup() -> (TestApp, Session, Session) {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let ana = app.login("ana@example.test", "demo").await;
    let bo = app.login("bo@example.test", "demo").await;
    (app, ana, bo)
}

async fn patch(app: &TestApp, session: &Session, body: serde_json::Value) -> serde_json::Value {
    app.patch_me(session, body).await
}

#[tokio::test]
async fn me_patch_and_clear_fields() {
    let (app, ana, _) = setup().await;
    let me = app.get("/api/v1/me").as_(&ana).send().await.expect(StatusCode::OK);
    assert_eq!(me["account"]["email"], "ana@example.test");
    assert_eq!(me["player"]["display_name"], "ana");
    assert_eq!(me["player"]["gender"], "undisclosed");

    let me = patch(
        &app,
        &ana,
        json!({
            "display_name": " Ana P ", "utr": 6.25, "gender": "female", "play_pref": "doubles",
            "phone": "+34 600 000 000", "racket": "Pure Aero", "tension_kg": 23.5,
            "preferred_locations": ["Retiro", " Casa de Campo "]
        }),
    )
    .await;
    assert_eq!(me["player"]["display_name"], "Ana P");
    assert_eq!(me["player"]["utr"], 6.25);
    assert_eq!(me["player"]["preferred_locations"], json!(["Retiro", "Casa de Campo"]));

    let me = patch(&app, &ana, json!({ "utr": null, "racket": null })).await;
    assert!(me["player"]["utr"].is_null());
    assert!(me["player"]["racket"].is_null());
    assert_eq!(me["player"]["phone"], "+34 600 000 000", "absent fields untouched");
}

#[tokio::test]
async fn patch_validation() {
    let (app, ana, _) = setup().await;
    for bad in [
        json!({ "utr": 17 }),
        json!({ "utr": 0.5 }),
        json!({ "utr": 5.555 }),
        json!({ "display_name": "  " }),
        json!({ "tension_kg": 50 }),
        json!({ "gender": "robot" }),
        json!({ "socials": ["not", "an", "object"] }),
        json!({ "unknown_field": 1 }),
    ] {
        let body = app.patch("/api/v1/me").as_(&ana).json(bad.clone()).send().await;
        assert_eq!(body.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad} -> {:#}", body.body);
        assert_eq!(body.body["error"]["code"], "validation_failed", "{bad}");
    }
}

#[tokio::test]
async fn join_another_community() {
    let (app, ana, _) = setup().await;
    let _ = app.community("other").await;
    let _ = app
        .get("/api/v1/me")
        .bearer(&ana.token)
        .community("other")
        .send()
        .await
        .expect(StatusCode::FORBIDDEN);
    let joined = app
        .post("/api/v1/me/join")
        .bearer(&ana.token)
        .community("other")
        .send()
        .await
        .expect(StatusCode::OK);
    assert_ne!(joined["player_id"], ana.player_id.to_string());
    let _ = app
        .get("/api/v1/me")
        .bearer(&ana.token)
        .community("other")
        .send()
        .await
        .expect(StatusCode::OK);
}

#[tokio::test]
async fn export_and_delete_account() {
    let (app, ana, _) = setup().await;
    let _ = app.community("other").await;
    let _ = app
        .post("/api/v1/me/join")
        .bearer(&ana.token)
        .community("other")
        .send()
        .await
        .expect(StatusCode::OK);
    let _ = patch(&app, &ana, json!({ "phone": "+34 600" })).await;

    let export = app.get("/api/v1/me/export").as_(&ana).send().await.expect(StatusCode::OK);
    assert_eq!(export["user"]["email"], "ana@example.test");
    assert_eq!(export["memberships"].as_array().unwrap().len(), 2);

    let _ = app.delete("/api/v1/me").as_(&ana).send().await.expect(StatusCode::NO_CONTENT);
    let _ = app.get("/api/v1/me").as_(&ana).send().await.expect(StatusCode::UNAUTHORIZED);
    let (email, phones): (String, i64) = sqlx::query_as(
        "SELECT u.email, (SELECT count(*) FROM players WHERE user_id = u.id AND phone IS NOT NULL)
         FROM users u WHERE u.id = $1",
    )
    .bind(ana.user_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(email.ends_with("@deleted.invalid"));
    assert_eq!(phones, 0);
    // The address is free to sign up again as a new account.
    let again = app.login("ana@example.test", "demo").await;
    assert_ne!(again.user_id, ana.user_id);
}
