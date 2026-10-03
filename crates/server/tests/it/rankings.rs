//! The ranking ledger: league results append events, the refresh job materialises rankings.

use axum::http::StatusCode;
use chrono::Duration;
use courtpit_server::jobs;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    common::{Session, TestApp},
    leagues::open_league,
    match_results::{report, straight_sets_a},
    matches::players,
    proposals::{in_days, open_proposal},
};

/// Inserts a `proposed` league match, as the lifecycle job would; nothing is scheduled yet.
pub(crate) async fn insert_league_match(
    app: &TestApp,
    league: &str,
    discipline: &str,
    side_a: &[&Session],
    side_b: &[&Session],
) -> String {
    let id = Uuid::now_v7();
    let ids = |sessions: &[&Session]| {
        sessions
            .iter()
            .map(|session| session.player_id)
            .collect::<Vec<_>>()
    };
    let _ = sqlx::query(
        "INSERT INTO matches (id, community_id, discipline, league_id, round, side_a_players,
            side_b_players, match_format)
         SELECT $1, community_id, $2::discipline, id, 1, $3, $4, $5 FROM leagues WHERE id = $6",
    )
    .bind(id)
    .bind(discipline)
    .bind(ids(side_a))
    .bind(ids(side_b))
    .bind(json!({ "sets_to_win": 2, "games_per_set": 6, "tiebreak_at": 6, "final_set": "match_tiebreak_10" }))
    .bind(league.parse::<Uuid>().unwrap())
    .execute(&app.db)
    .await
    .unwrap();
    id.to_string()
}

/// Inserts a league match and schedules it via a proposal.
async fn league_match(
    app: &TestApp,
    league: &str,
    discipline: &str,
    side_a: &[&Session],
    side_b: &[&Session],
) -> String {
    let id = insert_league_match(app, league, discipline, side_a, side_b).await;
    let match_view = app
        .post(&format!("/api/v1/matches/{id}/proposals"))
        .as_(side_a[0])
        .json(json!({ "time": in_days(1) }))
        .send()
        .await
        .expect(StatusCode::CREATED);
    let pid = open_proposal(&match_view);
    let _ = app
        .post(&format!("/api/v1/matches/{id}/proposals/{pid}/accept"))
        .as_(side_b[0])
        .send()
        .await
        .expect(StatusCode::OK);
    id
}

async fn confirm(app: &TestApp, session: &Session, id: &str) {
    let _ = app
        .post(&format!("/api/v1/matches/{id}/confirm"))
        .as_(session)
        .send()
        .await
        .expect(StatusCode::OK);
}

async fn rankings(app: &TestApp, session: &Session, discipline: &str) -> Vec<(Uuid, i64, i64)> {
    let body = app
        .get(&format!("/api/v1/rankings?discipline={discipline}"))
        .as_(session)
        .send()
        .await
        .expect(StatusCode::OK);
    body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["player_id"].as_str().unwrap().parse().unwrap(),
                row["rank"].as_i64().unwrap(),
                row["points_52w"].as_i64().unwrap(),
            )
        })
        .collect()
}

pub(crate) async fn ledger(app: &TestApp, viewer: &Session, player: &Session) -> Value {
    app.get(&format!(
        "/api/v1/rankings/events?player_id={}",
        player.player_id
    ))
    .as_(viewer)
    .send()
    .await
    .expect(StatusCode::OK)
}

async fn setup(names: &[&str]) -> (TestApp, Session, Vec<Session>) {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let admin = app.login("admin@example.test", "demo").await;
    app.make_admin(&admin).await;
    let ps = players(&app, "demo", names).await;
    (app, admin, ps)
}

#[tokio::test]
async fn league_results_feed_the_ledger_and_rankings() {
    let (app, admin, ps) = setup(&["ana", "bo", "cy"]).await;
    let (ana, bo, cy) = (&ps[0], &ps[1], &ps[2]);
    let league = open_league(&app, &admin, "singles").await;

    let m1 = league_match(&app, &league, "singles", &[ana], &[bo]).await;
    let _ = report(&app, ana, &m1, straight_sets_a()).await;
    assert_eq!(
        ledger(&app, cy, ana).await.as_array().unwrap().len(),
        0,
        "not yet confirmed"
    );
    confirm(&app, bo, &m1).await;
    let events = ledger(&app, cy, ana).await;
    assert_eq!(events[0]["points"], 3);
    assert_eq!(events[0]["source"], "league_match");
    assert_eq!(events[0]["source_id"], json!(m1));
    assert_eq!(ledger(&app, cy, bo).await[0]["points"], 0);

    // Deciding-set win for cy over ana: 2 / 1.
    let m2 = league_match(&app, &league, "singles", &[cy], &[ana]).await;
    let _ = report(
        &app,
        cy,
        &m2,
        json!({ "sets": [{ "a": 6, "b": 4 }, { "a": 4, "b": 6 }, { "a": 10, "b": 5, "match_tiebreak": true }] }),
    )
    .await;
    confirm(&app, ana, &m2).await;

    // A friendly earns nothing.
    let friendly = crate::match_results::scheduled(&app, &[bo], &[cy]).await;
    let _ = report(&app, bo, &friendly, straight_sets_a()).await;
    confirm(&app, cy, &friendly).await;
    assert_eq!(ledger(&app, cy, bo).await.as_array().unwrap().len(), 1);

    assert!(
        rankings(&app, ana, "singles").await.is_empty(),
        "materialised by the job"
    );
    let _ = jobs::run_due(&app.state, "t").await.unwrap();
    assert_eq!(
        rankings(&app, ana, "singles").await,
        vec![
            (ana.player_id, 1, 4),
            (cy.player_id, 2, 2),
            (bo.player_id, 3, 0)
        ]
    );
    assert!(rankings(&app, ana, "doubles").await.is_empty());
}

#[tokio::test]
async fn walkovers_disputes_and_auto_confirm_score_too() {
    let (app, admin, ps) = setup(&["a1", "a2", "b1", "b2"]).await;
    let (a1, a2, b1, b2) = (&ps[0], &ps[1], &ps[2], &ps[3]);
    let league = open_league(&app, &admin, "doubles").await;

    let wo = league_match(&app, &league, "doubles", &[a1, a2], &[b1, b2]).await;
    let _ = app
        .post(&format!("/api/v1/admin/matches/{wo}/walkover"))
        .as_(&admin)
        .json(json!({ "winner_side": "b" }))
        .send()
        .await
        .expect(StatusCode::OK);

    let disputed = league_match(&app, &league, "doubles", &[a1, a2], &[b1, b2]).await;
    let _ = report(&app, a1, &disputed, straight_sets_a()).await;
    let _ = app
        .post(&format!("/api/v1/matches/{disputed}/dispute"))
        .as_(b2)
        .json(json!({}))
        .send()
        .await
        .expect(StatusCode::OK);
    let _ = app.post(&format!("/api/v1/admin/matches/{disputed}/resolve"))
        .as_(&admin)
        .json(json!({ "resolution": "score", "score": { "sets": [{ "a": 6, "b": 3 }, { "a": 3, "b": 6 }, { "a": 10, "b": 8, "match_tiebreak": true }] } }))
        .send()
        .await
        .expect(StatusCode::OK);

    let silent = league_match(&app, &league, "doubles", &[a1, a2], &[b1, b2]).await;
    let _ = report(
        &app,
        b1,
        &silent,
        json!({ "sets": [{ "a": 0, "b": 6 }, { "a": 0, "b": 6 }] }),
    )
    .await;
    app.clock.advance(Duration::days(3) + Duration::minutes(1));
    let _ = jobs::run_due(&app.state, "t").await.unwrap();

    // a: walkover 0 + deciding win 2 + straight loss 0; b: 2 + 1 + 3. Each partner in full.
    let table = rankings(&app, a1, "doubles").await;
    let points = |player: &Session| {
        table
            .iter()
            .find(|row| row.0 == player.player_id)
            .unwrap()
            .2
    };
    assert_eq!((points(a1), points(a2)), (2, 2));
    assert_eq!((points(b1), points(b2)), (6, 6));
    assert_eq!(table[0].1, 1);
    assert_eq!(table[1].1, 1, "partners tie and share the rank");
    assert_eq!(table[2].1, 3);
}

#[tokio::test]
async fn points_decay_after_52_weeks_and_mixed_pooling_folds_into_doubles() {
    let (app, admin, ps) = setup(&["ana", "bo", "cy", "dee"]).await;
    let (ana, bo, cy, dee) = (&ps[0], &ps[1], &ps[2], &ps[3]);
    let _ = sqlx::query("UPDATE communities SET scoring_config = jsonb_set(scoring_config, '{mixed_pooling}', '\"doubles\"')")
        .execute(&app.db)
        .await
        .unwrap();
    for (session, gender) in [(ana, "female"), (bo, "male"), (cy, "female"), (dee, "male")] {
        let _ = app.patch_me(session, json!({ "gender": gender })).await;
    }
    let league = open_league(&app, &admin, "mixed").await;
    let match_view = league_match(&app, &league, "mixed", &[ana, bo], &[cy, dee]).await;
    let _ = report(&app, ana, &match_view, straight_sets_a()).await;
    confirm(&app, cy, &match_view).await;
    assert_eq!(ledger(&app, ana, ana).await[0]["discipline"], "mixed");
    let _ = jobs::run_due(&app.state, "t").await.unwrap();
    assert!(rankings(&app, ana, "mixed").await.is_empty());
    assert_eq!(rankings(&app, ana, "doubles").await.len(), 4);

    // The refresh reschedules itself daily, so points drop out without new results.
    app.clock.advance(Duration::weeks(52) + Duration::days(2));
    let _ = jobs::run_due(&app.state, "t").await.unwrap();
    assert!(rankings(&app, ana, "doubles").await.is_empty(), "decayed");
    assert_eq!(
        ledger(&app, ana, ana).await.as_array().unwrap().len(),
        1,
        "ledger keeps it"
    );
}
