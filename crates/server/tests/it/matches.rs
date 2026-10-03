//! Friendly matches: creation, visibility, cancellation.

use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::common::{Session, TestApp};

pub(crate) async fn players(app: &TestApp, community: &str, names: &[&str]) -> Vec<Session> {
    let mut out = Vec::new();
    for n in names {
        out.push(app.login(&format!("{n}@example.test"), community).await);
    }
    out
}

/// Creates a singles friendly from `challenger` against `opponent`.
pub(crate) async fn singles(app: &TestApp, challenger: &Session, opponent: &Session) -> Value {
    app.post("/api/v1/matches")
        .as_(challenger)
        .json(json!({ "discipline": "singles", "opponent_ids": [opponent.player_id] }))
        .send()
        .await
        .expect(StatusCode::CREATED)
}

#[tokio::test]
async fn creation_is_validated() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let _ = app.community("other").await;
    let [ana, bo, cy] =
        <[Session; 3]>::try_from(players(&app, "demo", &["ana", "bo", "cy"]).await).unwrap();
    let stranger = app.login("zed@example.test", "other").await;
    for (body, why) in [
        (
            json!({ "discipline": "singles", "opponent_ids": [] }),
            "no opponent",
        ),
        (
            json!({ "discipline": "singles", "opponent_ids": [bo.player_id, cy.player_id] }),
            "two opponents in singles",
        ),
        (
            json!({ "discipline": "singles", "partner_id": cy.player_id, "opponent_ids": [bo.player_id] }),
            "partner in singles",
        ),
        (
            json!({ "discipline": "doubles", "opponent_ids": [bo.player_id, cy.player_id] }),
            "doubles without partner",
        ),
        (
            json!({ "discipline": "doubles", "partner_id": bo.player_id, "opponent_ids": [bo.player_id, cy.player_id] }),
            "duplicate player",
        ),
        (
            json!({ "discipline": "singles", "opponent_ids": [ana.player_id] }),
            "playing yourself",
        ),
        (
            json!({ "discipline": "singles", "opponent_ids": [stranger.player_id] }),
            "member of another community",
        ),
    ] {
        let res = app
            .post("/api/v1/matches")
            .as_(&ana)
            .json(body)
            .send()
            .await;
        assert_eq!(
            res.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{why}: {:#}",
            res.body
        );
        assert_eq!(res.body["error"]["code"], "validation_failed", "{why}");
    }

    let _ = sqlx::query("UPDATE players SET status = 'banned' WHERE id = $1")
        .bind(bo.player_id)
        .execute(&app.db)
        .await
        .unwrap();
    let _ = app
        .post("/api/v1/matches")
        .as_(&ana)
        .json(json!({ "discipline": "singles", "opponent_ids": [bo.player_id] }))
        .send()
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn friendlies_are_private_to_their_players_and_admins() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let _ = app.community("other").await;
    let [ana, bo, cy, admin] =
        <[Session; 4]>::try_from(players(&app, "demo", &["ana", "bo", "cy", "admin"]).await)
            .unwrap();
    app.make_admin(&admin).await;
    let created = singles(&app, &ana, &bo).await;
    let id = created["id"].as_str().unwrap();
    let _ = singles(&app, &bo, &cy).await;

    let _ = app
        .get(&format!("/api/v1/matches/{id}"))
        .as_(&cy)
        .send()
        .await
        .expect(StatusCode::NOT_FOUND);
    let _ = app
        .get(&format!("/api/v1/matches/{id}"))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::OK);

    let count = |page: &Value| page["items"].as_array().unwrap().len();
    let mine = |session: &Session| app.get("/api/v1/matches").as_(session).send();
    assert_eq!(count(&mine(&ana).await.expect(StatusCode::OK)), 1);
    assert_eq!(count(&mine(&bo).await.expect(StatusCode::OK)), 2);
    assert_eq!(count(&mine(&admin).await.expect(StatusCode::OK)), 0);
    let _ = app
        .get("/api/v1/matches?all=true")
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::FORBIDDEN);
    let all = app
        .get("/api/v1/matches?all=true&limit=1")
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(count(&all), 1);
    let cursor = all["next_cursor"].as_str().unwrap();
    let rest = app
        .get(&format!("/api/v1/matches?all=true&cursor={cursor}"))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(count(&rest), 1);
    assert!(rest["next_cursor"].is_null());

    // Another community never sees it, even with the id.
    let zed = app.login("zed@example.test", "other").await;
    app.make_admin(&zed).await;
    let _ = app
        .get(&format!("/api/v1/matches/{id}"))
        .as_(&zed)
        .send()
        .await
        .expect(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn players_cancel_friendlies_outsiders_cannot() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [ana, bo, cy] =
        <[Session; 3]>::try_from(players(&app, "demo", &["ana", "bo", "cy"]).await).unwrap();
    let match_view = singles(&app, &ana, &bo).await;
    let id = match_view["id"].as_str().unwrap();
    let _ = app
        .post(&format!("/api/v1/matches/{id}/cancel"))
        .as_(&cy)
        .json(json!({}))
        .send()
        .await
        .expect(StatusCode::NOT_FOUND);
    let match_view = app
        .post(&format!("/api/v1/matches/{id}/cancel"))
        .as_(&bo)
        .json(json!({ "note": "injured" }))
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(match_view["status"], "cancelled");
    assert_eq!(match_view["resolution_note"], "injured");
    let _ = app
        .post(&format!("/api/v1/matches/{id}/cancel"))
        .as_(&ana)
        .json(json!({}))
        .send()
        .await
        .expect(StatusCode::CONFLICT);
}
