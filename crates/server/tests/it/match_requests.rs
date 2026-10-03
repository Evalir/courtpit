//! Match requests: open calls that others join until a match is created.

use axum::http::StatusCode;
use chrono::Duration;
use serde_json::{Value, json};

use crate::{
    common::{Session, TestApp},
    matches::players,
    proposals::in_days,
};

async fn create(app: &TestApp, session: &Session, body: Value) -> Value {
    app.post("/api/v1/match-requests")
        .as_(session)
        .json(body)
        .send()
        .await
        .expect(StatusCode::CREATED)
}

fn window(mut extra: Value, discipline: &str) -> Value {
    let obj = extra.as_object_mut().unwrap();
    let _ = obj.insert("discipline".into(), json!(discipline));
    let _ = obj.insert("time_window_start".into(), json!(in_days(2)));
    let _ = obj.insert(
        "time_window_end".into(),
        json!((chrono::Utc::now() + Duration::days(2) + Duration::hours(3)).to_rfc3339()),
    );
    extra
}

async fn join(app: &TestApp, session: &Session, id: &str) -> Value {
    app.post(&format!("/api/v1/match-requests/{id}/join"))
        .as_(session)
        .send()
        .await
        .expect(StatusCode::OK)
}

#[tokio::test]
async fn singles_request_fills_into_a_proposed_match() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [ana, bo] = <[Session; 2]>::try_from(players(&app, "demo", &["ana", "bo"]).await).unwrap();
    let request = create(
        &app,
        &ana,
        window(json!({ "location": "Retiro" }), "singles"),
    )
    .await;
    assert_eq!(request["slots_open"], 1);
    assert_eq!(request["status"], "open");
    let id = request["id"].as_str().unwrap();

    let _ = app
        .post(&format!("/api/v1/match-requests/{id}/join"))
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::CONFLICT);
    let request = join(&app, &bo, id).await;
    assert_eq!(request["status"], "filled");
    assert_eq!(request["slots_open"], 0);
    assert_eq!(request["players"], json!([ana.player_id, bo.player_id]));
    let match_id = request["match_id"].as_str().unwrap();

    let created = app
        .get(&format!("/api/v1/matches/{match_id}"))
        .as_(&bo)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(created["status"], "proposed");
    assert_eq!(created["side_a"], json!([ana.player_id]));
    assert_eq!(created["side_b"], json!([bo.player_id]));
    assert_eq!(created["proposals"][0]["proposed_by"], json!(ana.player_id));
    assert_eq!(created["proposals"][0]["location"], "Retiro");

    let cy = app.login("cy@example.test", "demo").await;
    let _ = app
        .post(&format!("/api/v1/match-requests/{id}/join"))
        .as_(&cy)
        .send()
        .await
        .expect(StatusCode::CONFLICT);
}

#[tokio::test]
async fn doubles_sides_creator_and_first_joiner_versus_the_rest() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [ana, bo, cy, dee] =
        <[Session; 4]>::try_from(players(&app, "demo", &["ana", "bo", "cy", "dee"]).await).unwrap();
    let request = create(&app, &ana, window(json!({}), "doubles")).await;
    assert_eq!(request["slots_open"], 3);
    let id = request["id"].as_str().unwrap();
    let _ = join(&app, &bo, id).await;
    let _ = join(&app, &cy, id).await;
    let request = app
        .post(&format!("/api/v1/match-requests/{id}/leave"))
        .as_(&cy)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(request["slots_open"], 2);
    let _ = app
        .post(&format!("/api/v1/match-requests/{id}/leave"))
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::CONFLICT);
    let _ = join(&app, &dee, id).await;
    let request = join(&app, &cy, id).await;
    assert_eq!(request["status"], "filled");
    let created = app
        .get(&format!(
            "/api/v1/matches/{}",
            request["match_id"].as_str().unwrap()
        ))
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(created["discipline"], "doubles");
    assert_eq!(created["side_a"], json!([ana.player_id, bo.player_id]));
    assert_eq!(created["side_b"], json!([dee.player_id, cy.player_id]));
}

#[tokio::test]
async fn doubles_with_own_partner_needs_two_opponents() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [ana, bo, cy, dee] =
        <[Session; 4]>::try_from(players(&app, "demo", &["ana", "bo", "cy", "dee"]).await).unwrap();
    let request = create(
        &app,
        &ana,
        window(json!({ "partner_id": bo.player_id }), "doubles"),
    )
    .await;
    assert_eq!(request["slots_open"], 2);
    let id = request["id"].as_str().unwrap();
    let _ = app
        .post(&format!("/api/v1/match-requests/{id}/join"))
        .as_(&bo)
        .send()
        .await
        .expect(StatusCode::CONFLICT);
    let _ = join(&app, &cy, id).await;
    let request = join(&app, &dee, id).await;
    let created = app
        .get(&format!(
            "/api/v1/matches/{}",
            request["match_id"].as_str().unwrap()
        ))
        .as_(&bo)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(created["side_a"], json!([ana.player_id, bo.player_id]));
    assert_eq!(created["side_b"], json!([cy.player_id, dee.player_id]));
}

#[tokio::test]
async fn utr_bands_gate_joining_and_filter_listing() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [ana, low, mid, none] =
        <[Session; 4]>::try_from(players(&app, "demo", &["ana", "low", "mid", "none"]).await)
            .unwrap();
    let _ = app.patch_me(&low, json!({ "utr": 2.5 })).await;
    let _ = app.patch_me(&mid, json!({ "utr": 5.0 })).await;
    let banded = create(
        &app,
        &ana,
        window(json!({ "utr_min": 4.0, "utr_max": 6.0 }), "singles"),
    )
    .await;
    let _ = create(&app, &ana, window(json!({}), "singles")).await;
    let id = banded["id"].as_str().unwrap();
    for session in [&low, &none] {
        let body = app
            .post(&format!("/api/v1/match-requests/{id}/join"))
            .as_(session)
            .send()
            .await
            .expect(StatusCode::UNPROCESSABLE_ENTITY);
        assert!(body["error"]["message"].as_str().unwrap().contains("UTR"));
    }
    let count = |body: Value| body["items"].as_array().unwrap().len();
    let list = |session: &Session, query: &str| {
        app.get(&format!("/api/v1/match-requests{query}"))
            .as_(session)
            .send()
    };
    assert_eq!(count(list(&low, "").await.expect(StatusCode::OK)), 2);
    assert_eq!(
        count(list(&low, "?fits_me=true").await.expect(StatusCode::OK)),
        1
    );
    assert_eq!(
        count(list(&mid, "?fits_me=true").await.expect(StatusCode::OK)),
        2
    );
    assert_eq!(
        count(
            list(&mid, "?discipline=doubles")
                .await
                .expect(StatusCode::OK)
        ),
        0
    );
    let page = list(&mid, "?limit=1").await.expect(StatusCode::OK);
    let cursor = page["next_cursor"].as_str().unwrap().to_owned();
    let rest = list(&mid, &format!("?limit=1&cursor={}", urlencode(&cursor)))
        .await
        .expect(StatusCode::OK);
    assert_eq!(count(rest.clone()), 1);
    assert_ne!(page["items"][0]["id"], rest["items"][0]["id"]);
    let _ = join(&app, &mid, id).await;
}

fn urlencode(text: &str) -> String {
    text.replace('+', "%2B")
        .replace(':', "%3A")
        .replace('|', "%7C")
}

#[tokio::test]
async fn validation_cancellation_and_expiry() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [ana, bo, admin] =
        <[Session; 3]>::try_from(players(&app, "demo", &["ana", "bo", "admin"]).await).unwrap();
    app.make_admin(&admin).await;
    for body in [
        json!({ "discipline": "singles", "time_window_start": in_days(-2), "time_window_end": in_days(-1) }),
        json!({ "discipline": "singles", "time_window_start": in_days(2), "time_window_end": in_days(1) }),
        json!({ "discipline": "singles", "time_window_start": in_days(1), "time_window_end": in_days(200) }),
        window(json!({ "partner_id": bo.player_id }), "singles"),
        window(json!({ "utr_min": 6.0, "utr_max": 4.0 }), "singles"),
        window(json!({ "utr_min": 20.0 }), "singles"),
    ] {
        let res = app
            .post("/api/v1/match-requests")
            .as_(&ana)
            .json(body.clone())
            .send()
            .await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }

    let request = create(&app, &ana, window(json!({}), "singles")).await;
    let id = request["id"].as_str().unwrap();
    let _ = app
        .post(&format!("/api/v1/match-requests/{id}/cancel"))
        .as_(&bo)
        .send()
        .await
        .expect(StatusCode::FORBIDDEN);
    let request = app
        .post(&format!("/api/v1/match-requests/{id}/cancel"))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(request["status"], "cancelled");
    let _ = app
        .post(&format!("/api/v1/match-requests/{id}/join"))
        .as_(&bo)
        .send()
        .await
        .expect(StatusCode::CONFLICT);

    let request = create(&app, &ana, window(json!({}), "singles")).await;
    let id = request["id"].as_str().unwrap();
    app.clock.advance(Duration::days(3));
    let open = app
        .get("/api/v1/match-requests")
        .as_(&bo)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(
        open["items"].as_array().unwrap().len(),
        0,
        "expired requests are hidden"
    );
    let _ = app
        .post(&format!("/api/v1/match-requests/{id}/join"))
        .as_(&bo)
        .send()
        .await
        .expect(StatusCode::CONFLICT);
}
