//! A full league season: play, standings, finishing (season points, promotion/relegation)
//! and seeding the next season from the results.

use std::collections::HashMap;

use axum::http::StatusCode;
use chrono::Duration;
use racquetcollective_server::{clock::Clock, jobs};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    common::{Session, TestApp},
    league_lifecycle::{at_day, league, league_matches, setup},
    leagues::{at, create_league, open_league},
};

/// Schedules a league match and reports the higher UTR's 6-2 6-2 win, leaving it `reported`
/// for the other side to answer.
async fn report(app: &TestApp, match_row: &Value, by_id: &HashMap<Uuid, (&Session, f64)>) {
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
    let set = if a_wins { json!({ "a": 6, "b": 2 }) } else { json!({ "a": 2, "b": 6 }) };
    let _ = app
        .post(&format!("/api/v1/matches/{id}/score"))
        .as_(home.0)
        .json(json!({ "sets": [set, set] }))
        .send()
        .await
        .expect(StatusCode::OK);
}

/// Plays a league match to a confirmed result.
async fn play(app: &TestApp, match_row: &Value, by_id: &HashMap<Uuid, (&Session, f64)>) {
    report(app, match_row, by_id).await;
    let id = match_row["id"].as_str().unwrap();
    let away = by_id[&match_row["side_b"][0].as_str().unwrap().parse::<Uuid>().unwrap()];
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
async fn a_season_finishes_with_points_and_seeds_the_next_one() {
    let (app, admin, ps) = setup(12).await;
    let first = open_league(&app, &admin, "singles").await;
    let mut by_id = HashMap::new();
    for (i, player) in ps.iter().enumerate() {
        let utr = (i as f64).mul_add(0.5, 2.0);
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
        (top["played"].clone(), top["won"].clone(), top["points"].clone()),
        (json!(5), json!(5), json!(15))
    );
    assert_eq!((top["sets_won"].clone(), top["games_won"].clone()), (json!(10), json!(60)));
    assert_eq!(table[0]["tier"], 1);

    at_day(&app, 60).await;
    assert_eq!(league(&app, &admin, &first).await["status"], "finished");
    let events = |player: &Session| {
        app.get(&format!("/api/v1/rankings/events?player_id={}", player.player_id))
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
    assert_eq!(season(events(&ps[11]).await.expect(StatusCode::OK)), 100, "1st, tier 1");
    assert_eq!(season(events(&ps[6]).await.expect(StatusCode::OK)), 15, "6th, tier 1");
    assert_eq!(season(events(&ps[5]).await.expect(StatusCode::OK)), 70, "1st, tier 2: 100 x 0.7");
    assert_eq!(season(events(&ps[0]).await.expect(StatusCode::OK)), 11, "6th, tier 2: 10.5");
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
    let mut want_top: Vec<Uuid> = [11, 10, 9, 8, 5, 4].iter().map(|&i| ps[i].player_id).collect();
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
    assert!(table[0]["table"].as_array().unwrap().iter().all(|line| line["played"] == 0));
    // Nothing left to do: further runs are no-ops.
    assert_eq!(jobs::run_due(&app.state, "t").await.unwrap(), 0);
}

/// A started singles league of `n` players with UTRs 2.0, 3.0, ..., that nobody has played in.
async fn started(n: usize) -> (TestApp, Session, Vec<Session>, String) {
    let (app, admin, ps) = setup(n).await;
    let league_id = open_league(&app, &admin, "singles").await;
    for (i, player) in ps.iter().enumerate() {
        let _ = app.patch_me(player, json!({ "utr": 2.0 + i as f64 })).await;
    }
    register_all(&app, &league_id, &ps).await;
    at_day(&app, 8).await;
    (app, admin, ps, league_id)
}

/// Players by id with the UTRs `started` gave them.
fn ratings(ps: &[Session]) -> HashMap<Uuid, (&Session, f64)> {
    ps.iter().enumerate().map(|(i, player)| (player.player_id, (player, 2.0 + i as f64))).collect()
}

async fn unresolved(app: &TestApp, admin: &Session, league: &str, query: &str) -> Value {
    app.get(&format!("/api/v1/admin/leagues/{league}/unresolved{query}"))
        .as_(admin)
        .send()
        .await
        .expect(StatusCode::OK)
}

/// `run_at` of the league's waiting lifecycle job.
async fn next_run(app: &TestApp, league: &str) -> chrono::DateTime<chrono::Utc> {
    sqlx::query_scalar(
        "SELECT run_at FROM jobs WHERE kind = 'advance_league' AND payload->>'league_id' = $1
           AND completed_at IS NULL AND failed_at IS NULL",
    )
    .bind(league)
    .fetch_one(&app.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn a_reported_match_defers_the_finish_until_it_auto_confirms() {
    let (app, admin, ps, league_id) = started(2).await;
    let by_ref = ratings(&ps);
    // Reported a day before the season ends: its 3-day window outlasts the season.
    at_day(&app, 59).await;
    report(&app, &league_matches(&app, &admin, &league_id).await[0], &by_ref).await;

    at_day(&app, 61).await;
    assert_eq!(
        league(&app, &admin, &league_id).await["status"],
        "active",
        "the season is over but a score is unanswered"
    );
    let open = unresolved(&app, &admin, &league_id, "").await;
    let items = open["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["status"], "reported");
    assert_eq!(
        items[0]["division_id"],
        league_matches(&app, &admin, &league_id).await[0]["division_id"]
    );
    assert_eq!(items[0]["round"], 1);
    assert!(items[0]["reported_by"].is_string() && items[0]["reported_at"].is_string());
    let deadline =
        chrono::DateTime::parse_from_rfc3339(items[0]["confirm_deadline_at"].as_str().unwrap())
            .unwrap();
    assert_eq!(
        next_run(&app, &league_id).await,
        deadline + Duration::minutes(1),
        "looks again just after the auto-confirm deadline"
    );
    assert_eq!(jobs::run_due(&app.state, "t").await.unwrap(), 0);

    // The deadline passes: the confirmation runs first, then the finish.
    at_day(&app, 63).await;
    assert_eq!(league(&app, &admin, &league_id).await["status"], "finished");
    assert!(unresolved(&app, &admin, &league_id, "").await["items"].as_array().unwrap().is_empty());
    let table = standings(&app, &ps[0], &league_id).await;
    assert_eq!(table[0]["table"][0]["played"], 1, "the confirmed result counts");
    assert_eq!(jobs::run_due(&app.state, "t").await.unwrap(), 0);
}

#[tokio::test]
async fn a_dispute_defers_the_finish_daily_until_an_admin_resolves_it() {
    let (app, admin, ps, league_id) = started(2).await;
    let by_ref = ratings(&ps);
    let match_row = league_matches(&app, &admin, &league_id).await.remove(0);
    report(&app, &match_row, &by_ref).await;
    let match_id = match_row["id"].as_str().unwrap();
    let (reporter, opponent) = (match_row["side_a"][0].clone(), match_row["side_b"][0].clone());
    assert_eq!(match_row["status"], "proposed");
    let disputer = ps.iter().find(|player| json!(player.player_id) == opponent).unwrap();
    let _ = app
        .post(&format!("/api/v1/matches/{match_id}/dispute"))
        .as_(disputer)
        .json(json!({ "note": "that was 6-4" }))
        .send()
        .await
        .expect(StatusCode::OK);

    at_day(&app, 61).await;
    assert_eq!(league(&app, &admin, &league_id).await["status"], "active");
    let open = unresolved(&app, &admin, &league_id, "").await;
    assert_eq!(open["items"][0]["status"], "disputed");
    assert_eq!(open["items"][0]["dispute_note"], "that was 6-4");
    assert_eq!(open["items"][0]["disputed_by"], opponent);
    assert_eq!(open["items"][0]["reported_by"], reporter);
    let wait = next_run(&app, &league_id).await - app.clock.now();
    assert!(wait > Duration::hours(23) && wait <= Duration::hours(24), "retries in a day: {wait}");

    // Nothing changes by waiting; a day later it is still blocked and still queued.
    app.clock.advance(Duration::hours(25));
    let _ = jobs::run_due(&app.state, "t").await.unwrap();
    assert_eq!(league(&app, &admin, &league_id).await["status"], "active");
    assert!(next_run(&app, &league_id).await > app.clock.now());

    let _ = app
        .post(&format!("/api/v1/admin/matches/{match_id}/resolve"))
        .as_(&admin)
        .json(json!({ "resolution": "score", "score": { "sets": [{ "a": 6, "b": 4 }, { "a": 6, "b": 4 }] } }))
        .send()
        .await
        .expect(StatusCode::OK);
    app.clock.advance(Duration::hours(25));
    let _ = jobs::run_due(&app.state, "t").await.unwrap();
    assert_eq!(league(&app, &admin, &league_id).await["status"], "finished");
}

#[tokio::test]
async fn unresolved_lists_pages_and_is_admin_only() {
    let (app, admin, ps, league_id) = started(3).await;
    let by_ref = ratings(&ps);
    let matches = league_matches(&app, &admin, &league_id).await;
    assert_eq!(matches.len(), 3);
    for match_row in &matches[..2] {
        report(&app, match_row, &by_ref).await;
    }
    let first = unresolved(&app, &admin, &league_id, "?limit=1").await;
    assert_eq!(first["items"].as_array().unwrap().len(), 1);
    let cursor = first["next_cursor"].as_str().unwrap();
    let second = unresolved(&app, &admin, &league_id, &format!("?limit=1&cursor={cursor}")).await;
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert!(second["next_cursor"].is_null());
    assert_ne!(first["items"][0]["id"], second["items"][0]["id"]);
    let all = unresolved(&app, &admin, &league_id, "").await;
    assert_eq!(all["items"].as_array().unwrap().len(), 2, "unplayed ones are not listed");

    let _ = app
        .get(&format!("/api/v1/admin/leagues/{league_id}/unresolved"))
        .as_(&ps[0])
        .send()
        .await
        .expect(StatusCode::FORBIDDEN);
    let _ = app
        .get(&format!("/api/v1/admin/leagues/{}/unresolved", Uuid::now_v7()))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn finishing_by_hand_is_refused_until_the_end_and_while_matches_are_open() {
    let (app, admin, ps, league_id) = started(2).await;
    let by_ref = ratings(&ps);
    let finish = |session: &Session, query: &str| {
        app.post(&format!("/api/v1/admin/leagues/{league_id}/finish{query}")).as_(session).send()
    };
    let _ = finish(&ps[0], "").await.expect(StatusCode::FORBIDDEN);
    let body = finish(&admin, "").await.expect(StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "season_not_over");
    let body = finish(&admin, "?force=true").await.expect(StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "season_not_over", "force does not skip the end date");

    at_day(&app, 59).await;
    report(&app, &league_matches(&app, &admin, &league_id).await[0], &by_ref).await;
    // Past the end, before the job ran: the endpoint applies the same rule.
    app.clock.set(chrono::Utc::now() + Duration::days(61));
    let body = finish(&admin, "").await.expect(StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "unresolved_matches");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(message.contains("1 league match is"), "{message}");
    assert!(message.contains("/unresolved") && message.contains("force=true"), "{message}");
    assert_eq!(league(&app, &admin, &league_id).await["status"], "active");

    let done = finish(&admin, "?force=true").await.expect(StatusCode::OK);
    assert_eq!(done["status"], "finished");
    let table = standings(&app, &ps[0], &league_id).await;
    assert_eq!(table[0]["table"][0]["played"], 0, "the unanswered score is left out");
    let again = finish(&admin, "?force=true").await.expect(StatusCode::CONFLICT);
    assert_eq!(again["error"]["code"], "conflict", "already finished");

    // The score still auto-confirms later and earns match points; the finished season stays.
    let _ = jobs::run_due(&app.state, "t").await.unwrap();
    app.clock.advance(Duration::days(2));
    let _ = jobs::run_due(&app.state, "t").await.unwrap();
    let match_row = &league_matches(&app, &admin, &league_id).await[0];
    assert_eq!(match_row["status"], "confirmed");
    assert_eq!(league(&app, &admin, &league_id).await["status"], "finished");
}

#[tokio::test]
async fn finishing_by_hand_works_without_open_matches_and_only_when_active() {
    let (app, admin, _, league_id) = started(2).await;
    app.clock.set(chrono::Utc::now() + Duration::days(61));
    let done = app
        .post(&format!("/api/v1/admin/leagues/{league_id}/finish"))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(done["status"], "finished");
    let matches = league_matches(&app, &admin, &league_id).await;
    assert_eq!(matches[0]["resolution_note"], "season ended");
    // The lifecycle job still queued for the end date finds nothing to do.
    let _ = jobs::run_due(&app.state, "t").await.unwrap();
    assert_eq!(league(&app, &admin, &league_id).await["status"], "finished");

    app.clock.set(chrono::Utc::now());
    let upcoming = open_league(&app, &admin, "singles").await;
    let _ = app
        .post(&format!("/api/v1/admin/leagues/{upcoming}/finish"))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::CONFLICT);
    let _ = app
        .post(&format!("/api/v1/admin/leagues/{}/finish", Uuid::now_v7()))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn openapi_documents_the_season_endpoints() {
    let app = TestApp::spawn().await;
    let doc = app.get("/api/v1/openapi.json").send().await.expect(StatusCode::OK);
    let finish = &doc["paths"]["/api/v1/admin/leagues/{id}/finish"]["post"];
    assert_eq!(finish["tags"], json!(["admin"]));
    let params = finish["parameters"].as_array().unwrap();
    assert!(params.iter().any(|param| param["name"] == "force"));
    for status in ["200", "403", "404", "409"] {
        assert!(finish["responses"][status].is_object(), "finish {status}");
    }
    let list = &doc["paths"]["/api/v1/admin/leagues/{id}/unresolved"]["get"];
    let params = list["parameters"].as_array().unwrap();
    assert!(params.iter().any(|param| param["name"] == "cursor"));
    assert!(params.iter().any(|param| param["name"] == "limit"));
    assert!(list["responses"]["200"].is_object());
}
