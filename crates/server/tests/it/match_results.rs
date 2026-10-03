//! Score reporting, confirmation, disputes, admin decisions and auto-confirmation.

use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use courtpit_server::jobs;
use serde_json::{Value, json};

use crate::{
    common::{Session, TestApp},
    matches::players,
    proposals::{in_days, open_proposal},
};

/// A scheduled match between `side_a` and `side_b` (one or two players each).
pub(crate) async fn scheduled(app: &TestApp, side_a: &[&Session], side_b: &[&Session]) -> String {
    let discipline = if side_a.len() == 1 {
        "singles"
    } else {
        "doubles"
    };
    let view = app
        .post("/api/v1/matches")
        .as_(side_a[0])
        .json(json!({
            "discipline": discipline,
            "partner_id": side_a.get(1).map(|member| member.player_id),
            "opponent_ids": side_b.iter().map(|member| member.player_id).collect::<Vec<_>>(),
            "proposed_time": in_days(1),
        }))
        .send()
        .await
        .expect(StatusCode::CREATED);
    let id = view["id"].as_str().unwrap().to_owned();
    let pid = open_proposal(&view);
    let _ = app
        .post(&format!("/api/v1/matches/{id}/proposals/{pid}/accept"))
        .as_(side_b[0])
        .send()
        .await
        .expect(StatusCode::OK);
    id
}

pub(crate) fn straight_sets_a() -> Value {
    json!({ "sets": [{ "a": 6, "b": 3 }, { "a": 6, "b": 4 }] })
}

pub(crate) async fn report(app: &TestApp, session: &Session, id: &str, score: Value) -> Value {
    app.post(&format!("/api/v1/matches/{id}/score"))
        .as_(session)
        .json(score)
        .send()
        .await
        .expect(StatusCode::OK)
}

#[tokio::test]
async fn report_then_confirm() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [ana, bo] = <[Session; 2]>::try_from(players(&app, "demo", &["ana", "bo"]).await).unwrap();
    let id = scheduled(&app, &[&ana], &[&bo]).await;

    let bad = app
        .post(&format!("/api/v1/matches/{id}/score"))
        .as_(&ana)
        .json(json!({ "sets": [{ "a": 6, "b": 5 }, { "a": 6, "b": 4 }] }))
        .send()
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        bad["error"]["message"]
            .as_str()
            .unwrap()
            .contains("set 1: 6-5"),
        "{bad}"
    );
    let view = report(
        &app,
        &bo,
        &id,
        json!({ "sets": [{ "a": 6, "b": 4 }, { "a": 3, "b": 6 }, { "a": 7, "b": 10, "match_tiebreak": true }] }),
    )
    .await;
    assert_eq!(view["status"], "reported");
    assert_eq!(view["winner_side"], "b");
    assert_eq!(view["reported_by"], json!(bo.player_id));
    let reported_at: DateTime<Utc> = view["reported_at"].as_str().unwrap().parse().unwrap();
    let deadline: DateTime<Utc> = view["confirm_deadline_at"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(deadline - reported_at, Duration::days(3));

    let _ = app
        .post(&format!("/api/v1/matches/{id}/confirm"))
        .as_(&bo)
        .send()
        .await
        .expect(StatusCode::FORBIDDEN);
    let view = app
        .post(&format!("/api/v1/matches/{id}/confirm"))
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(view["status"], "confirmed");
    let _ = app
        .post(&format!("/api/v1/matches/{id}/score"))
        .as_(&ana)
        .json(straight_sets_a())
        .send()
        .await
        .expect(StatusCode::CONFLICT);
}

#[tokio::test]
async fn unscheduled_matches_cannot_be_reported() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [ana, bo] = <[Session; 2]>::try_from(players(&app, "demo", &["ana", "bo"]).await).unwrap();
    let created = crate::matches::singles(&app, &ana, &bo).await;
    let id = created["id"].as_str().unwrap();
    let body = app
        .post(&format!("/api/v1/matches/{id}/score"))
        .as_(&ana)
        .json(straight_sets_a())
        .send()
        .await
        .expect(StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "conflict");
}

#[tokio::test]
async fn doubles_partner_cannot_confirm_but_any_opponent_can() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [a1, a2, b1, b2] =
        <[Session; 4]>::try_from(players(&app, "demo", &["a1", "a2", "b1", "b2"]).await).unwrap();
    let id = scheduled(&app, &[&a1, &a2], &[&b1, &b2]).await;
    let _ = report(&app, &a1, &id, straight_sets_a()).await;
    let _ = app
        .post(&format!("/api/v1/matches/{id}/confirm"))
        .as_(&a2)
        .send()
        .await
        .expect(StatusCode::FORBIDDEN);
    let view = app
        .post(&format!("/api/v1/matches/{id}/confirm"))
        .as_(&b2)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(view["status"], "confirmed");
    assert_eq!(view["winner_side"], "a");
}

#[tokio::test]
async fn disputes_are_resolved_by_an_admin() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [ana, bo, ref_] =
        <[Session; 3]>::try_from(players(&app, "demo", &["ana", "bo", "ref"]).await).unwrap();
    app.make_admin(&ref_).await;
    let id = scheduled(&app, &[&ana], &[&bo]).await;
    let _ = report(&app, &ana, &id, straight_sets_a()).await;
    let view = app
        .post(&format!("/api/v1/matches/{id}/dispute"))
        .as_(&bo)
        .json(json!({ "note": "it was 6-3 3-6 10-8 to me" }))
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(view["status"], "disputed");
    assert_eq!(view["disputed_by"], json!(bo.player_id));

    let resolve = |actor: &Session, body: Value| {
        app.post(&format!("/api/v1/admin/matches/{id}/resolve"))
            .as_(actor)
            .json(body)
            .send()
    };
    let _ = resolve(
        &bo,
        json!({ "resolution": "score", "score": straight_sets_a() }),
    )
    .await
    .expect(StatusCode::FORBIDDEN);
    let _ = resolve(&ref_, json!({ "resolution": "score" }))
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
    let _ = resolve(
        &ref_,
        json!({ "resolution": "void", "score": straight_sets_a() }),
    )
    .await
    .expect(StatusCode::UNPROCESSABLE_ENTITY);
    let view = resolve(
        &ref_,
        json!({
            "resolution": "score",
            "score": { "sets": [{ "a": 6, "b": 3 }, { "a": 3, "b": 6 }, { "a": 8, "b": 10, "match_tiebreak": true }] },
            "note": "confirmed with both players",
        }),
    )
    .await
    .expect(StatusCode::OK);
    assert_eq!(view["status"], "resolved");
    assert_eq!(view["winner_side"], "b");
    assert_eq!(view["resolved_by"], json!(ref_.player_id));
    assert_eq!(view["resolution_note"], "confirmed with both players");
}

#[tokio::test]
async fn replay_reopens_and_void_cancels() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [ana, bo, ref_] =
        <[Session; 3]>::try_from(players(&app, "demo", &["ana", "bo", "ref"]).await).unwrap();
    app.make_admin(&ref_).await;
    let id = scheduled(&app, &[&ana], &[&bo]).await;
    for (resolution, status) in [("replay", "scheduled"), ("void", "cancelled")] {
        let _ = report(&app, &ana, &id, straight_sets_a()).await;
        let _ = app
            .post(&format!("/api/v1/matches/{id}/dispute"))
            .as_(&bo)
            .json(json!({}))
            .send()
            .await
            .expect(StatusCode::OK);
        let view = app
            .post(&format!("/api/v1/admin/matches/{id}/resolve"))
            .as_(&ref_)
            .json(json!({ "resolution": resolution }))
            .send()
            .await
            .expect(StatusCode::OK);
        assert_eq!(view["status"], status);
        assert!(view["score"].is_null() && view["winner_side"].is_null());
        assert_eq!(view["reported_by"].is_null(), (status == "scheduled"));
    }
}

#[tokio::test]
async fn walkovers_are_admin_only_and_not_for_your_own_match() {
    let app = TestApp::spawn().await;
    let owner = "owner@example.test";
    let _ = app.community_with_owner("demo", Some(owner)).await;
    let [ana, bo, adm] =
        <[Session; 3]>::try_from(players(&app, "demo", &["ana", "bo", "adm"]).await).unwrap();
    let boss = app.login(owner, "demo").await;
    app.make_admin(&adm).await;
    let id = scheduled(&app, &[&adm], &[&bo]).await;
    let walkover = |actor: &Session| {
        app.post(&format!("/api/v1/admin/matches/{id}/walkover"))
            .as_(actor)
            .json(json!({ "winner_side": "b", "note": "no-show" }))
            .send()
    };
    let _ = walkover(&ana).await.expect(StatusCode::FORBIDDEN);
    let _ = walkover(&adm).await.expect(StatusCode::FORBIDDEN);
    let view = walkover(&boss).await.expect(StatusCode::OK);
    assert_eq!(view["status"], "walkover");
    assert_eq!(view["winner_side"], "b");
    assert!(view["score"].is_null());
    let _ = walkover(&boss).await.expect(StatusCode::CONFLICT);
}

#[tokio::test]
async fn unanswered_reports_auto_confirm_after_the_window() {
    let app = TestApp::spawn().await;
    let community = app.community("demo").await;
    let _ = sqlx::query(
        "UPDATE communities SET settings = '{\"confirm_window_days\": 2}' WHERE id = $1",
    )
    .bind(community.id)
    .execute(&app.db)
    .await
    .unwrap();
    let [ana, bo] = <[Session; 2]>::try_from(players(&app, "demo", &["ana", "bo"]).await).unwrap();
    let id = scheduled(&app, &[&ana], &[&bo]).await;
    let other = scheduled(&app, &[&ana], &[&bo]).await;
    let _ = report(&app, &ana, &id, straight_sets_a()).await;
    let _ = report(&app, &ana, &other, straight_sets_a()).await;
    let _ = app
        .post(&format!("/api/v1/matches/{other}/dispute"))
        .as_(&bo)
        .json(json!({}))
        .send()
        .await
        .expect(StatusCode::OK);

    let status = |id: String| {
        let app = &app;
        let bo = bo.clone();
        async move {
            app.get(&format!("/api/v1/matches/{id}"))
                .as_(&bo)
                .send()
                .await
                .expect(StatusCode::OK)["status"]
                .clone()
        }
    };
    assert_eq!(jobs::run_due(&app.state, "t").await.unwrap(), 0);
    app.clock.advance(Duration::days(1));
    assert_eq!(jobs::run_due(&app.state, "t").await.unwrap(), 0);
    assert_eq!(status(id.clone()).await, "reported");
    app.clock.advance(Duration::days(1) + Duration::minutes(1));
    assert_eq!(jobs::run_due(&app.state, "t").await.unwrap(), 2);
    assert_eq!(status(id).await, "confirmed");
    assert_eq!(
        status(other).await,
        "disputed",
        "disputes are not auto-confirmed"
    );
}
