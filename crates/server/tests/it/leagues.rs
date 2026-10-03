//! League setup: create, edit, publish, cancel, visibility.

use axum::http::StatusCode;
use chrono::{Duration, Utc};
use serde_json::{Value, json};

use crate::common::{Session, TestApp};

pub(crate) fn at(days: i64) -> String {
    (Utc::now() + Duration::days(days)).to_rfc3339()
}

/// League body: registration from `opens` (days from now) to +7d, play +8d..+60d.
pub(crate) fn league_body(discipline: &str, opens: i64) -> Value {
    json!({
        "name": format!("Autumn {discipline}"),
        "discipline": discipline,
        "registration_opens_at": at(opens),
        "registration_closes_at": at(7),
        "starts_at": at(8),
        "ends_at": at(60),
    })
}

pub(crate) async fn create_league(app: &TestApp, admin: &Session, body: Value) -> Value {
    app.post("/api/v1/admin/leagues")
        .as_(admin)
        .json(body)
        .send()
        .await
        .expect(StatusCode::CREATED)
}

/// A published league with registration open now.
pub(crate) async fn open_league(app: &TestApp, admin: &Session, discipline: &str) -> String {
    let league = create_league(app, admin, league_body(discipline, -1)).await;
    let id = league["id"].as_str().unwrap().to_owned();
    let league = app
        .post(&format!("/api/v1/admin/leagues/{id}/publish"))
        .as_(admin)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(league["status"], "registration");
    id
}

async fn setup() -> (TestApp, Session, Session) {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let admin = app.login("admin@example.test", "demo").await;
    app.make_admin(&admin).await;
    let ana = app.login("ana@example.test", "demo").await;
    (app, admin, ana)
}

#[tokio::test]
async fn admins_create_and_validate_leagues() {
    let (app, admin, ana) = setup().await;
    let _ = app
        .post("/api/v1/admin/leagues")
        .as_(&ana)
        .json(league_body("singles", 1))
        .send()
        .await
        .expect(StatusCode::FORBIDDEN);
    let league = create_league(&app, &admin, league_body("singles", 1)).await;
    assert_eq!(league["status"], "draft");
    assert!(league["published_at"].is_null());
    assert!(league["entry_fee_minor"].is_null());
    assert_eq!(
        league["match_format"]["final_set"], "match_tiebreak_10",
        "community default"
    );
    assert_eq!(
        (
            league["box_min_size"].clone(),
            league["box_max_size"].clone()
        ),
        (json!(6), json!(8))
    );

    let mut bad_dates = league_body("singles", 1);
    bad_dates["starts_at"] = json!(at(3));
    let mut bad_format = league_body("singles", 1);
    bad_format["match_format"] =
        json!({ "sets_to_win": 5, "games_per_set": 6, "final_set": "full_set" });
    let mut bad_scoring = league_body("singles", 1);
    bad_scoring["scoring_overrides"] = json!({ "league_match": { "win_big": 9 } });
    let mut bad_boxes = league_body("singles", 1);
    bad_boxes["box_min_size"] = json!(9);
    let mut bad_prev = league_body("doubles", 1);
    bad_prev["previous_league_id"] = league["id"].clone();
    for (body, why) in [
        (bad_dates, "dates"),
        (bad_format, "format"),
        (bad_scoring, "scoring"),
        (bad_boxes, "boxes"),
        (bad_prev, "previous league of another discipline"),
    ] {
        let res = app
            .post("/api/v1/admin/leagues")
            .as_(&admin)
            .json(body)
            .send()
            .await;
        assert_eq!(
            res.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{why}: {:#}",
            res.body
        );
    }

    let mut good = league_body("singles", 1);
    good["match_format"] =
        json!({ "sets_to_win": 1, "games_per_set": 6, "tiebreak_at": 6, "final_set": "pro_set_8" });
    good["scoring_overrides"] = json!({ "league_match": { "win_straight": 4 } });
    good["previous_league_id"] = league["id"].clone();
    let l2 = create_league(&app, &admin, good).await;
    assert_eq!(l2["match_format"]["final_set"], "pro_set_8");
    assert_eq!(l2["scoring_overrides"]["league_match"]["win_straight"], 4);
}

#[tokio::test]
async fn drafts_are_editable_and_hidden_until_published() {
    let (app, admin, ana) = setup().await;
    let mut body = league_body("doubles", 2);
    body["match_format"] =
        json!({ "sets_to_win": 2, "games_per_set": 6, "tiebreak_at": 6, "final_set": "full_set" });
    let league = create_league(&app, &admin, body).await;
    let id = league["id"].as_str().unwrap();

    let count = |page: Value| page["items"].as_array().unwrap().len();
    let list = |session: &Session| app.get("/api/v1/leagues").as_(session).send();
    assert_eq!(count(list(&ana).await.expect(StatusCode::OK)), 0);
    assert_eq!(count(list(&admin).await.expect(StatusCode::OK)), 1);
    let _ = app
        .get(&format!("/api/v1/leagues/{id}"))
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::NOT_FOUND);

    let league = app
        .patch(&format!("/api/v1/admin/leagues/{id}"))
        .as_(&admin)
        .json(json!({ "name": "Winter doubles", "match_format": null }))
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(league["name"], "Winter doubles");
    assert_eq!(
        league["match_format"]["final_set"], "match_tiebreak_10",
        "override cleared"
    );
    let _ = app
        .patch(&format!("/api/v1/admin/leagues/{id}"))
        .as_(&admin)
        .json(json!({ "ends_at": at(5) }))
        .send()
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);

    // Registration opens in 2 days: published but still draft until then.
    let league = app
        .post(&format!("/api/v1/admin/leagues/{id}/publish"))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(league["status"], "draft");
    assert!(league["published_at"].is_string());
    assert_eq!(count(list(&ana).await.expect(StatusCode::OK)), 1);
    let _ = app
        .post(&format!("/api/v1/admin/leagues/{id}/publish"))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::CONFLICT);

    let filtered = app
        .get("/api/v1/leagues?discipline=singles")
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(count(filtered), 0);
}

#[tokio::test]
async fn publishing_opens_registration_and_cancel_ends_it() {
    let (app, admin, ana) = setup().await;
    let id = open_league(&app, &admin, "singles").await;
    let _ = app
        .patch(&format!("/api/v1/admin/leagues/{id}"))
        .as_(&admin)
        .json(json!({ "name": "Renamed" }))
        .send()
        .await
        .expect(StatusCode::CONFLICT);
    let league = app
        .get(&format!("/api/v1/leagues/{id}"))
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(league["status"], "registration");
    let _ = app
        .post(&format!("/api/v1/admin/leagues/{id}/cancel"))
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::FORBIDDEN);
    let league = app
        .post(&format!("/api/v1/admin/leagues/{id}/cancel"))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(league["status"], "cancelled");
    let _ = app
        .post(&format!("/api/v1/admin/leagues/{id}/cancel"))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::CONFLICT);

    let mut closed = league_body("singles", -10);
    closed["registration_closes_at"] = json!(at(-1));
    let league = create_league(&app, &admin, closed).await;
    let _ = app
        .post(&format!(
            "/api/v1/admin/leagues/{}/publish",
            league["id"].as_str().unwrap()
        ))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::CONFLICT);
}
