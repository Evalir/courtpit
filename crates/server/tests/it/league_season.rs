//! A full league season: play, standings, finishing (season points, promotion/relegation)
//! and seeding the next season from the results.

use std::collections::HashMap;

use axum::http::StatusCode;
use chrono::Duration;
use courtpit_server::{clock::Clock, jobs};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    common::{Session, TestApp},
    league_lifecycle::{at_day, league, league_matches, setup},
    leagues::{at, create_league, open_league},
};

/// Plays a scheduled-from-scratch league match: the higher UTR wins 6-2 6-2.
async fn play(app: &TestApp, match_row: &Value, by_id: &HashMap<Uuid, (&Session, f64)>) {
    let id = match_row["id"].as_str().unwrap();
    let side = |key: &str| -> Uuid { match_row[key][0].as_str().unwrap().parse().unwrap() };
    let (home, away) = (by_id[&side("side_a")], by_id[&side("side_b")]);
    let time = (app.clock.now() + Duration::days(1)).to_rfc3339();
    let proposal = app
        .post(&format!("/api/v1/matches/{id}/proposals"))
        .as_(home.0)
        .json(json!({ "time": time }))
        .send()
        .await
        .expect(StatusCode::CREATED);
    let pid = proposal["proposals"].as_array().unwrap().last().unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let _ = app
        .post(&format!("/api/v1/matches/{id}/proposals/{pid}/accept"))
        .as_(away.0)
        .send()
        .await
        .expect(StatusCode::OK);
    let a_wins = home.1 > away.1;
    let set = if a_wins {
        json!({ "a": 6, "b": 2 })
    } else {
        json!({ "a": 2, "b": 6 })
    };
    let _ = app
        .post(&format!("/api/v1/matches/{id}/score"))
        .as_(home.0)
        .json(json!({ "sets": [set, set] }))
        .send()
        .await
        .expect(StatusCode::OK);
    let _ = app
        .post(&format!("/api/v1/matches/{id}/confirm"))
        .as_(away.0)
        .send()
        .await
        .expect(StatusCode::OK);
}

async fn register_all(app: &TestApp, league: &str, ps: &[Session]) {
    for player in ps {
        let _ = app
            .post(&format!("/api/v1/leagues/{league}/entries"))
            .as_(player)
            .json(json!({}))
            .send()
            .await
            .expect(StatusCode::CREATED);
    }
}

async fn standings(app: &TestApp, viewer: &Session, league: &str) -> Value {
    app.get(&format!("/api/v1/leagues/{league}/standings"))
        .as_(viewer)
        .send()
        .await
        .expect(StatusCode::OK)
}

fn box_players(table: &Value) -> Vec<Vec<Uuid>> {
    table
        .as_array()
        .unwrap()
        .iter()
        .map(|division| {
            division["table"]
                .as_array()
                .unwrap()
                .iter()
                .map(|line| line["player_ids"][0].as_str().unwrap().parse().unwrap())
                .collect()
        })
        .collect()
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "one long end-to-end season scenario")]
async fn a_season_finishes_with_points_and_seeds_the_next_one() {
    let (app, admin, ps) = setup(12).await;
    let first = open_league(&app, &admin, "singles").await;
    let mut by_id = HashMap::new();
    for (i, player) in ps.iter().enumerate() {
        let utr = 2.0 + i as f64 * 0.5;
        let _ = app.patch_me(player, json!({ "utr": utr })).await;
        let _ = by_id.insert(player.player_id, (player, utr));
    }
    register_all(&app, &first, &ps).await;
    at_day(&app, 8).await;
    let matches = league_matches(&app, &admin, &first).await;
    assert_eq!(matches.len(), 30);
    for match_row in &matches {
        play(&app, match_row, &by_id).await;
    }
    // Strongest first: p11..p06 in box 1, p05..p00 in box 2.
    let table = standings(&app, &ps[0], &first).await;
    let order = box_players(&table);
    let ids = |range: std::ops::RangeInclusive<usize>| -> Vec<Uuid> {
        range.rev().map(|i| ps[i].player_id).collect()
    };
    assert_eq!(order, vec![ids(6..=11), ids(0..=5)]);
    let top = &table[0]["table"][0];
    assert_eq!(
        (
            top["played"].clone(),
            top["won"].clone(),
            top["points"].clone()
        ),
        (json!(5), json!(5), json!(15))
    );
    assert_eq!(
        (top["sets_won"].clone(), top["games_won"].clone()),
        (json!(10), json!(60))
    );
    assert_eq!(table[0]["tier"], 1);

    at_day(&app, 60).await;
    assert_eq!(league(&app, &admin, &first).await["status"], "finished");
    let events = |player: &Session| {
        app.get(&format!(
            "/api/v1/rankings/events?player_id={}",
            player.player_id
        ))
        .as_(&admin)
        .send()
    };
    let season = |events_json: Value| -> i64 {
        events_json
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["source"] == "league_season")
            .map(|event| event["points"].as_i64().unwrap())
            .unwrap()
    };
    assert_eq!(
        season(events(&ps[11]).await.expect(StatusCode::OK)),
        100,
        "1st, tier 1"
    );
    assert_eq!(
        season(events(&ps[6]).await.expect(StatusCode::OK)),
        15,
        "6th, tier 1"
    );
    assert_eq!(
        season(events(&ps[5]).await.expect(StatusCode::OK)),
        70,
        "1st, tier 2: 100 x 0.7"
    );
    assert_eq!(
        season(events(&ps[0]).await.expect(StatusCode::OK)),
        11,
        "6th, tier 2: 10.5"
    );
    // The refresh job ran with the season-end events: match points + season points.
    let ranking = app
        .get("/api/v1/rankings?discipline=singles&limit=1")
        .as_(&ps[0])
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(ranking["items"][0]["player_id"], json!(ps[11].player_id));
    assert_eq!(ranking["items"][0]["points_52w"], 15 + 100);

    // Next season: same players, seeded by last season's movement. Box 2's top two (p05,
    // p04) go up, box 1's bottom two (p07, p06) go down.
    app.clock.set(chrono::Utc::now());
    let mut body = json!({
        "name": "Spring singles",
        "discipline": "singles",
        "registration_opens_at": at(-1),
        "registration_closes_at": at(7),
        "starts_at": at(8),
        "ends_at": at(60),
        "previous_league_id": first,
    });
    let second = create_league(&app, &admin, body.take()).await;
    let second = second["id"].as_str().unwrap().to_owned();
    let _ = app
        .post(&format!("/api/v1/admin/leagues/{second}/publish"))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::OK);
    register_all(&app, &second, &ps).await;
    at_day(&app, 8).await;
    let table = standings(&app, &ps[0], &second).await;
    let mut boxes = box_players(&table);
    for division_box in &mut boxes {
        division_box.sort_unstable();
    }
    let mut want_top: Vec<Uuid> = [11, 10, 9, 8, 5, 4]
        .iter()
        .map(|&i| ps[i].player_id)
        .collect();
    want_top.sort_unstable();
    assert_eq!(boxes[0], want_top);
}

#[tokio::test]
async fn season_end_cancels_unplayed_matches() {
    let (app, admin, ps) = setup(2).await;
    let league_id = open_league(&app, &admin, "singles").await;
    register_all(&app, &league_id, &ps).await;
    at_day(&app, 8).await;
    let matches = league_matches(&app, &ps[0], &league_id).await;
    assert_eq!(matches.len(), 1);
    at_day(&app, 61).await;
    let matches = league_matches(&app, &ps[0], &league_id).await;
    assert_eq!(matches[0]["status"], "cancelled");
    assert_eq!(matches[0]["resolution_note"], "season ended");
    let table = standings(&app, &ps[0], &league_id).await;
    assert!(
        table[0]["table"]
            .as_array()
            .unwrap()
            .iter()
            .all(|line| line["played"] == 0)
    );
    // Nothing left to do: further runs are no-ops.
    assert_eq!(jobs::run_due(&app.state, "t").await.unwrap(), 0);
}
