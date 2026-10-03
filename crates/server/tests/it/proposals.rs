//! Scheduling: proposals, counter-proposals, accept and decline.

use axum::http::StatusCode;
use chrono::{Duration, Utc};
use serde_json::{Value, json};

use crate::{
    common::{Session, TestApp},
    matches::players,
};

pub(crate) fn in_days(days: i64) -> String {
    (Utc::now() + Duration::days(days)).to_rfc3339()
}

/// Creates a singles friendly from `challenger` against `opponent` with an opening proposal.
pub(crate) async fn proposed_singles(
    app: &TestApp,
    challenger: &Session,
    opponent: &Session,
) -> Value {
    app.post("/api/v1/matches")
        .as_(challenger)
        .json(json!({
            "discipline": "singles",
            "opponent_ids": [opponent.player_id],
            "proposed_time": in_days(2),
            "location": "Retiro court 3",
        }))
        .send()
        .await
        .expect(StatusCode::CREATED)
}

pub(crate) fn open_proposal(match_view: &Value) -> String {
    match_view["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|proposal| proposal["status"] == "open")
        .expect("an open proposal")["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn propose_accept_schedules_the_match() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [ana, bo] = <[Session; 2]>::try_from(players(&app, "demo", &["ana", "bo"]).await).unwrap();
    let match_view = proposed_singles(&app, &ana, &bo).await;
    assert_eq!(match_view["status"], "proposed");
    assert_eq!(match_view["side_a"], json!([ana.player_id]));
    assert_eq!(match_view["side_b"], json!([bo.player_id]));
    assert_eq!(match_view["match_format"]["final_set"], "match_tiebreak_10");
    let id = match_view["id"].as_str().unwrap();
    let pid = open_proposal(&match_view);

    let body = app
        .post(&format!("/api/v1/matches/{id}/proposals/{pid}/accept"))
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "forbidden", "proposer can't accept");

    let match_view = app
        .post(&format!("/api/v1/matches/{id}/proposals/{pid}/accept"))
        .as_(&bo)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(match_view["status"], "scheduled");
    assert_eq!(match_view["location"], "Retiro court 3");
    assert!(match_view["scheduled_at"].is_string());
    assert_eq!(match_view["proposals"][0]["status"], "accepted");

    let _ = app
        .post(&format!("/api/v1/matches/{id}/proposals/{pid}/accept"))
        .as_(&bo)
        .send()
        .await
        .expect(StatusCode::CONFLICT);
}

#[tokio::test]
async fn counter_proposals_supersede_and_decline_keeps_the_match_open() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [ana, bo] = <[Session; 2]>::try_from(players(&app, "demo", &["ana", "bo"]).await).unwrap();
    let match_view = proposed_singles(&app, &ana, &bo).await;
    let id = match_view["id"].as_str().unwrap();
    let first = open_proposal(&match_view);

    let match_view = app
        .post(&format!("/api/v1/matches/{id}/proposals"))
        .as_(&bo)
        .json(json!({ "time": in_days(3), "location": "Chamartín" }))
        .send()
        .await
        .expect(StatusCode::CREATED);
    let statuses: Vec<&str> = match_view["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|proposal| proposal["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["superseded", "open"]);
    let counter = open_proposal(&match_view);
    let _ = app
        .post(&format!("/api/v1/matches/{id}/proposals/{first}/accept"))
        .as_(&bo)
        .send()
        .await
        .expect(StatusCode::CONFLICT);

    let match_view = app
        .post(&format!("/api/v1/matches/{id}/proposals/{counter}/decline"))
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(match_view["status"], "proposed");
    assert_eq!(match_view["proposals"][1]["status"], "declined");

    let _ = app
        .post(&format!("/api/v1/matches/{id}/proposals"))
        .as_(&ana)
        .json(json!({ "time": (Utc::now() - Duration::hours(1)).to_rfc3339() }))
        .send()
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn doubles_any_player_on_a_side_acts_for_it() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [a1, a2, b1, b2] =
        <[Session; 4]>::try_from(players(&app, "demo", &["a1", "a2", "b1", "b2"]).await).unwrap();
    let match_view = app
        .post("/api/v1/matches")
        .as_(&a1)
        .json(json!({
            "discipline": "doubles",
            "partner_id": a2.player_id,
            "opponent_ids": [b1.player_id, b2.player_id],
        }))
        .send()
        .await
        .expect(StatusCode::CREATED);
    let id = match_view["id"].as_str().unwrap();
    assert_eq!(match_view["side_a"], json!([a1.player_id, a2.player_id]));
    let match_view = app
        .post(&format!("/api/v1/matches/{id}/proposals"))
        .as_(&b2)
        .json(json!({ "time": in_days(1) }))
        .send()
        .await
        .expect(StatusCode::CREATED);
    let pid = open_proposal(&match_view);
    let _ = app
        .post(&format!("/api/v1/matches/{id}/proposals/{pid}/accept"))
        .as_(&b1)
        .send()
        .await
        .expect(StatusCode::FORBIDDEN);
    let match_view = app
        .post(&format!("/api/v1/matches/{id}/proposals/{pid}/accept"))
        .as_(&a2)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(match_view["status"], "scheduled");
}

#[tokio::test]
async fn proposal_times_must_be_ahead_and_cancelling_closes_them() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [ana, bo] = <[Session; 2]>::try_from(players(&app, "demo", &["ana", "bo"]).await).unwrap();
    for when in [in_days(-1), in_days(400)] {
        let _ = app.post("/api/v1/matches")
            .as_(&ana)
            .json(json!({ "discipline": "singles", "opponent_ids": [bo.player_id], "proposed_time": when }))
            .send()
            .await
            .expect(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let match_view = proposed_singles(&app, &ana, &bo).await;
    let id = match_view["id"].as_str().unwrap();
    let match_view = app
        .post(&format!("/api/v1/matches/{id}/cancel"))
        .as_(&ana)
        .json(json!({}))
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(match_view["proposals"][0]["status"], "superseded");
    let _ = app
        .post(&format!("/api/v1/matches/{id}/proposals"))
        .as_(&bo)
        .json(json!({ "time": in_days(1) }))
        .send()
        .await
        .expect(StatusCode::CONFLICT);
}

#[tokio::test]
async fn accepting_a_proposal_whose_time_passed_is_refused() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [ana, bo] = <[Session; 2]>::try_from(players(&app, "demo", &["ana", "bo"]).await).unwrap();
    let match_view = proposed_singles(&app, &ana, &bo).await;
    let id = match_view["id"].as_str().unwrap();
    let pid = open_proposal(&match_view);
    app.clock.advance(Duration::days(3));
    let _ = app
        .post(&format!("/api/v1/matches/{id}/proposals/{pid}/accept"))
        .as_(&bo)
        .send()
        .await
        .expect(StatusCode::CONFLICT);
}
