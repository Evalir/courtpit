//! Player directory, contact redaction and moderation.

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
async fn directory_filters_and_paginates() {
    let (app, ana, bo) = setup().await;
    let _ = patch(
        &app,
        &bo,
        json!({ "utr": 5.0, "play_pref": "singles", "preferred_locations": ["Retiro"] }),
    )
    .await;
    let cy = app.login("cy@example.test", "demo").await;
    let _ = patch(
        &app,
        &cy,
        json!({ "utr": 8.0, "play_pref": "any", "preferred_locations": ["Chamartín"] }),
    )
    .await;
    let dee = app.login("dee@example.test", "demo").await;
    let _ = patch(&app, &dee, json!({ "utr": 3.0, "play_pref": "doubles" })).await;

    let names = |body: &serde_json::Value| -> Vec<String> {
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|player| player["display_name"].as_str().unwrap().to_owned())
            .collect()
    };
    let all = app
        .get("/api/v1/players")
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(names(&all), ["bo", "cy", "dee"], "caller excluded");
    let band = app
        .get("/api/v1/players?utr_min=4&utr_max=8")
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(names(&band), ["bo", "cy"]);
    let singles = app
        .get("/api/v1/players?play_pref=singles")
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(names(&singles), ["bo", "cy"], "`any` matches singles");
    let loc = app
        .get("/api/v1/players?location=retiro")
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(names(&loc), ["bo"]);

    let p1 = app
        .get("/api/v1/players?limit=2")
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(names(&p1), ["bo", "cy"]);
    let cursor = p1["next_cursor"].as_str().unwrap();
    let p2 = app
        .get(&format!("/api/v1/players?limit=2&cursor={cursor}"))
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(names(&p2), ["dee"]);
    assert!(p2["next_cursor"].is_null());
    let _ = app
        .get("/api/v1/players?cursor=nope")
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn unverified_players_are_hidden_and_gated() {
    let (app, ana, bo) = setup().await;
    let _ = sqlx::query("UPDATE users SET email_verified_at = NULL WHERE id = $1")
        .bind(bo.user_id)
        .execute(&app.db)
        .await
        .unwrap();
    let all = app
        .get("/api/v1/players")
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(all["items"].as_array().unwrap().len(), 0);
    let _ = app
        .get(&format!("/api/v1/players/{}", bo.player_id))
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn contact_details_need_opt_in_and_verified_viewer() {
    let (app, ana, bo) = setup().await;
    let _ = patch(
        &app,
        &bo,
        json!({ "phone": "+34 611", "socials": { "instagram": "@bo" }, "gender": "male" }),
    )
    .await;
    let url = format!("/api/v1/players/{}", bo.player_id);
    let hidden = app.get(&url).as_(&ana).send().await.expect(StatusCode::OK);
    assert!(hidden["phone"].is_null() && hidden["socials"].is_null());
    assert!(hidden.get("gender").is_none(), "gender is never exposed");

    let _ = patch(
        &app,
        &bo,
        json!({ "phone_visible": true, "socials_visible": true }),
    )
    .await;
    let shown = app.get(&url).as_(&ana).send().await.expect(StatusCode::OK);
    assert_eq!(shown["phone"], "+34 611");
    assert_eq!(shown["socials"]["instagram"], "@bo");

    let _ = sqlx::query("UPDATE users SET email_verified_at = NULL WHERE id = $1")
        .bind(ana.user_id)
        .execute(&app.db)
        .await
        .unwrap();
    let viewer_unverified = app.get(&url).as_(&ana).send().await.expect(StatusCode::OK);
    assert!(viewer_unverified["phone"].is_null());
}

#[tokio::test]
async fn admin_ban_and_unban() {
    let (app, ana, bo) = setup().await;
    let url = format!("/api/v1/admin/players/{}/ban", bo.player_id);
    let _ = app
        .post(&url)
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::FORBIDDEN);
    let _ = app
        .get("/api/v1/players?status=banned")
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::FORBIDDEN);
    app.make_admin(&ana).await;
    let _ = app
        .post(&url)
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::NO_CONTENT);
    let _ = app
        .get("/api/v1/me")
        .as_(&bo)
        .send()
        .await
        .expect(StatusCode::FORBIDDEN);
    let listed = app
        .get("/api/v1/players")
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(listed["items"].as_array().unwrap().len(), 0);
    // Admins find banned members to lift the ban; the profile says so.
    let banned = app
        .get("/api/v1/players?status=banned")
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(banned["items"][0]["id"], json!(bo.player_id));
    assert_eq!(banned["items"][0]["status"], "banned");
    let profile = app
        .get(&format!("/api/v1/players/{}", bo.player_id))
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(profile["status"], "banned");
    // Admins cannot ban themselves or other admins (only owners can).
    let _ = app
        .post(&format!("/api/v1/admin/players/{}/ban", ana.player_id))
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
    let _ = app
        .post(&format!("/api/v1/admin/players/{}/unban", bo.player_id))
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::NO_CONTENT);
    let _ = app
        .get("/api/v1/me")
        .as_(&bo)
        .send()
        .await
        .expect(StatusCode::OK);
}

#[tokio::test]
async fn deleted_accounts_leave_the_directory() {
    let (app, ana, bo) = setup().await;
    let _ = app
        .delete("/api/v1/me")
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::NO_CONTENT);
    let listed = app
        .get("/api/v1/players")
        .as_(&bo)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(listed["items"].as_array().unwrap().len(), 0);
}
