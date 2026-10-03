//! The date-driven league lifecycle: registration opening, activation (placement and
//! round-robin schedule), driven by advancing the clock and running due jobs.

use std::collections::{HashMap, HashSet};

use axum::http::StatusCode;
use chrono::Duration;
use courtpit_server::jobs;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    common::{Session, TestApp},
    leagues::{create_league, league_body, open_league},
};

pub(crate) async fn setup(n: usize) -> (TestApp, Session, Vec<Session>) {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let admin = app.login("admin@example.test", "demo").await;
    app.make_admin(&admin).await;
    let mut ps = Vec::new();
    for i in 0..n {
        ps.push(app.login(&format!("p{i:02}@example.test"), "demo").await);
    }
    (app, admin, ps)
}

pub(crate) async fn league(app: &TestApp, admin: &Session, id: &str) -> Value {
    app.get(&format!("/api/v1/leagues/{id}"))
        .as_(admin)
        .send()
        .await
        .expect(StatusCode::OK)
}

pub(crate) async fn league_matches(app: &TestApp, session: &Session, id: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let url = cursor.as_ref().map_or_else(
            || format!("/api/v1/matches?league_id={id}&limit=100"),
            |token| format!("/api/v1/matches?league_id={id}&limit=100&cursor={token}"),
        );
        let page = app
            .get(&url)
            .as_(session)
            .send()
            .await
            .expect(StatusCode::OK);
        out.extend(page["items"].as_array().unwrap().iter().cloned());
        match page["next_cursor"].as_str() {
            Some(token) => cursor = Some(token.to_owned()),
            None => return out,
        }
    }
}

/// Moves the clock to `days` from the real now and runs every due job.
pub(crate) async fn at_day(app: &TestApp, days: i64) {
    app.clock.set(chrono::Utc::now() + Duration::days(days));
    let _ = jobs::run_due(&app.state, "t").await.unwrap();
}

#[tokio::test]
async fn publishing_schedules_registration_opening() {
    let (app, admin, ps) = setup(1).await;
    let created = create_league(&app, &admin, league_body("singles", 2)).await;
    let id = created["id"].as_str().unwrap();
    let _ = app
        .post(&format!("/api/v1/admin/leagues/{id}/publish"))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::OK);
    let _ = jobs::run_due(&app.state, "t").await.unwrap();
    assert_eq!(league(&app, &admin, id).await["status"], "draft");
    let _ = app
        .post(&format!("/api/v1/leagues/{id}/entries"))
        .as_(&ps[0])
        .json(json!({}))
        .send()
        .await
        .expect(StatusCode::CONFLICT);
    at_day(&app, 2).await;
    assert_eq!(league(&app, &admin, id).await["status"], "registration");
    let _ = app
        .post(&format!("/api/v1/leagues/{id}/entries"))
        .as_(&ps[0])
        .json(json!({}))
        .send()
        .await
        .expect(StatusCode::CREATED);
}

#[tokio::test]
async fn activation_places_entries_by_utr_and_schedules_round_robins() {
    let (app, admin, ps) = setup(13).await;
    let id = open_league(&app, &admin, "singles").await;
    let mut utr_of = HashMap::new();
    for (i, player) in ps.iter().enumerate() {
        let utr = 3.0 + i as f64 * 0.5;
        let _ = app.patch_me(player, json!({ "utr": utr })).await;
        let _ = utr_of.insert(player.player_id, utr);
        let _ = app
            .post(&format!("/api/v1/leagues/{id}/entries"))
            .as_(player)
            .json(json!({}))
            .send()
            .await
            .expect(StatusCode::CREATED);
    }
    at_day(&app, 7).await;
    assert_eq!(
        league(&app, &admin, &id).await["status"],
        "registration",
        "closed, not started"
    );
    at_day(&app, 8).await;
    assert_eq!(league(&app, &admin, &id).await["status"], "active");

    let entries = app
        .get(&format!("/api/v1/leagues/{id}/entries"))
        .as_(&ps[0])
        .send()
        .await
        .expect(StatusCode::OK);
    let mut by_division: HashMap<String, Vec<Uuid>> = HashMap::new();
    for entry in entries.as_array().unwrap() {
        let player: Uuid = entry["player_ids"][0].as_str().unwrap().parse().unwrap();
        by_division
            .entry(entry["division_id"].as_str().expect("placed").to_owned())
            .or_default()
            .push(player);
    }
    let mut sizes: Vec<usize> = by_division.values().map(Vec::len).collect();
    sizes.sort_unstable();
    assert_eq!(sizes, vec![6, 7]);
    let top = by_division.values().find(|group| group.len() == 7).unwrap();
    assert!(
        top.iter().all(|player| utr_of[player] >= 6.0),
        "top box holds the 7 highest UTRs"
    );

    let matches = league_matches(&app, &ps[0], &id).await;
    assert_eq!(matches.len(), 7 * 6 / 2 + 6 * 5 / 2);
    let mut pairs = HashSet::new();
    for item in &matches {
        assert_eq!(item["status"], "proposed");
        assert!(item["round"].as_i64().unwrap() >= 1);
        let (side_a, side_b) = (
            item["side_a"][0].as_str().unwrap(),
            item["side_b"][0].as_str().unwrap(),
        );
        let division = item["division_id"].as_str().unwrap();
        let members = &by_division[division];
        assert!(
            members.contains(&side_a.parse().unwrap())
                && members.contains(&side_b.parse().unwrap())
        );
        assert!(
            pairs.insert((side_a.min(side_b).to_owned(), side_a.max(side_b).to_owned())),
            "pair repeated"
        );
    }
    // League matches are visible to every member and appear in players' own lists.
    let mine = app
        .get("/api/v1/matches?limit=100")
        .as_(&ps[12])
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(mine["items"].as_array().unwrap().len(), 6);
}

#[tokio::test]
async fn doubles_activation_drops_incomplete_entries_and_copies_pairs() {
    let (app, admin, ps) = setup(5).await;
    let id = open_league(&app, &admin, "doubles").await;
    let register = |session: &Session, body: Value| {
        app.post(&format!("/api/v1/leagues/{id}/entries"))
            .as_(session)
            .json(body)
            .send()
    };
    for (player, partner) in [(0, 1), (2, 3)] {
        let entry = register(&ps[player], json!({ "partner_id": ps[partner].player_id }))
            .await
            .expect(StatusCode::CREATED);
        let _ = app
            .post(&format!(
                "/api/v1/leagues/{id}/entries/{}/accept",
                entry["id"].as_str().unwrap()
            ))
            .as_(&ps[partner])
            .send()
            .await
            .expect(StatusCode::OK);
    }
    let solo = register(&ps[4], json!({ "looking_for_partner": true }))
        .await
        .expect(StatusCode::CREATED);
    at_day(&app, 8).await;
    let withdrawn = app
        .get(&format!("/api/v1/leagues/{id}/entries?status=withdrawn"))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(withdrawn[0]["id"], solo["id"]);
    let matches = league_matches(&app, &ps[0], &id).await;
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["discipline"], "doubles");
    assert_eq!(
        matches[0]["side_a"],
        json!([ps[0].player_id, ps[1].player_id])
    );
    assert_eq!(
        matches[0]["side_b"],
        json!([ps[2].player_id, ps[3].player_id])
    );
}

#[tokio::test]
async fn a_late_job_catches_up_and_cancelled_leagues_stay_put() {
    let (app, admin, ps) = setup(2).await;
    let created = create_league(&app, &admin, league_body("singles", 1)).await;
    let id = created["id"].as_str().unwrap().to_owned();
    let _ = app
        .post(&format!("/api/v1/admin/leagues/{id}/publish"))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::OK);
    // Nobody ran the jobs until after the start date: one run does both steps.
    at_day(&app, 9).await;
    assert_eq!(league(&app, &admin, &id).await["status"], "active");

    at_day(&app, 0).await;
    let other = open_league(&app, &admin, "singles").await;
    for player in &ps {
        let _ = app
            .post(&format!("/api/v1/leagues/{other}/entries"))
            .as_(player)
            .json(json!({}))
            .send()
            .await
            .expect(StatusCode::CREATED);
    }
    let _ = app
        .post(&format!("/api/v1/admin/leagues/{other}/cancel"))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::OK);
    at_day(&app, 30).await;
    assert_eq!(league(&app, &admin, &other).await["status"], "cancelled");
    assert!(league_matches(&app, &admin, &other).await.is_empty());
}
