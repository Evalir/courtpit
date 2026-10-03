//! `courtpit-server seed`: the demo club, a league in every state, and idempotence.

use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use courtpit_domain::{MatchFormat, Score, validate_score};
use courtpit_server::seed::seed;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    common::{Session, TestApp},
    league_lifecycle::league_matches,
};

const SINGLES: &str = "Autumn Singles League";
const DOUBLES: &str = "Winter Doubles League";

async fn count(app: &TestApp, community: Uuid, sql: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(community)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

async fn league_id(app: &TestApp, community: Uuid, name: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM leagues WHERE community_id = $1 AND name = $2")
        .bind(community)
        .bind(name)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

/// Row counts of every table the seed writes to (a fresh database holds nothing else).
async fn snapshot(app: &TestApp) -> Vec<i64> {
    let mut counts = Vec::new();
    for table in [
        "communities",
        "users",
        "players",
        "leagues",
        "league_divisions",
        "league_entries",
        "matches",
        "match_proposals",
        "ranking_events",
        "rankings",
        "match_requests",
        "jobs",
    ] {
        let sql = format!("SELECT count(*) FROM {table}");
        counts.push(sqlx::query_scalar(&sql).fetch_one(&app.db).await.unwrap());
    }
    counts
}

fn when(value: &Value, key: &str) -> DateTime<Utc> {
    value[key].as_str().unwrap().parse().unwrap()
}

async fn get(app: &TestApp, owner: &Session, path: &str) -> Value {
    app.get(path).as_(owner).send().await.expect(StatusCode::OK)
}

#[tokio::test]
async fn seeds_branding_people_and_profiles() {
    let app = TestApp::spawn().await;
    let summary = seed(&app.state, "demo").await.unwrap();
    let community = summary.community_id;
    assert_eq!(summary.slug, "demo");
    assert_eq!(summary.owner_email, "marcus.hale@example.com");
    let owner = app.login(&summary.owner_email, "demo").await;

    let tenant = get(&app, &owner, "/api/v1/tenant").await;
    let branding = &tenant["branding"];
    assert_eq!(branding["display_name"], "Riverside Tennis Club");
    assert!(
        branding["logo_url"]
            .as_str()
            .unwrap()
            .starts_with("https://")
    );
    assert!(branding["colors"]["primary"].is_string() && branding["colors"]["text"].is_string());
    assert!(branding["typography"].is_string());
    assert_eq!(branding["feature_flags"]["doubles"], true);

    let verified = "SELECT count(*) FROM players p JOIN users u ON u.id = p.user_id
        WHERE p.community_id = $1 AND u.email_verified_at IS NOT NULL
          AND u.email LIKE '%@example.com'";
    assert_eq!(count(&app, community, verified).await, 25);
    let owners = "SELECT count(*) FROM players WHERE community_id = $1 AND role = 'owner'";
    assert_eq!(count(&app, community, owners).await, 1);
    let spread = "SELECT count(*) FROM (SELECT min(utr) AS low, max(utr) AS high FROM players
        WHERE community_id = $1) t WHERE low = 3.0 AND high = 8.0";
    assert_eq!(
        count(&app, community, spread).await,
        1,
        "UTR spans 3.0 to 8.0"
    );
    for gender in ["female", "male", "other", "undisclosed"] {
        let sql =
            format!("SELECT count(*) FROM players WHERE community_id = $1 AND gender = '{gender}'");
        assert!(count(&app, community, &sql).await > 0, "no {gender} player");
    }
    for varied in [
        "phone_visible",
        "NOT phone_visible AND phone IS NOT NULL",
        "socials_visible",
        "NOT socials_visible AND socials <> '{}'",
        "racket IS NOT NULL AND strings IS NOT NULL",
        "racket IS NULL",
        "jsonb_array_length(preferred_locations) > 1",
    ] {
        let sql = format!("SELECT count(*) FROM players WHERE community_id = $1 AND {varied}");
        assert!(
            count(&app, community, &sql).await > 0,
            "no player with {varied}"
        );
    }
    let prefs = "SELECT count(DISTINCT play_pref) FROM players WHERE community_id = $1";
    assert_eq!(count(&app, community, prefs).await, 3);

    let directory = get(&app, &owner, "/api/v1/players?limit=100").await;
    let members = directory["items"].as_array().unwrap();
    assert_eq!(members.len(), 24, "everyone but the caller is listed");
    assert!(members.iter().any(|player| player["phone"].is_string()));
    assert!(members.iter().any(|player| player["phone"].is_null()));
}

#[tokio::test]
async fn seeds_a_singles_league_mid_season() {
    let app = TestApp::spawn().await;
    let summary = seed(&app.state, "demo").await.unwrap();
    let community = summary.community_id;
    let owner = app.login(&summary.owner_email, "demo").await;
    let league = league_id(&app, community, SINGLES).await;

    let view = get(&app, &owner, &format!("/api/v1/leagues/{league}")).await;
    assert_eq!(view["status"], "active");
    let now = app.state.clock.now();
    let (opens, closes) = (
        when(&view, "registration_opens_at"),
        when(&view, "registration_closes_at"),
    );
    let (starts, ends) = (when(&view, "starts_at"), when(&view, "ends_at"));
    assert!(opens < closes && closes <= starts && starts < now && now < ends);
    assert!(when(&view, "published_at") <= opens);

    let divisions = "SELECT count(*) FROM league_divisions WHERE community_id = $1";
    assert!(count(&app, community, divisions).await >= 2);
    let placed = "SELECT count(*) FROM league_entries
        WHERE community_id = $1 AND status = 'confirmed' AND division_id IS NOT NULL";
    assert_eq!(count(&app, community, placed).await, 14);

    let matches = league_matches(&app, &owner, &league.to_string()).await;
    let with = |status: &str| {
        matches
            .iter()
            .filter(|found| found["status"] == status)
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(matches.len(), 42);
    assert_eq!(with("confirmed").len(), 12);
    assert_eq!(with("scheduled").len(), 4);
    assert_eq!(with("proposed").len(), 24);
    for found in with("confirmed") {
        // Scores are legal under the match's own format and name the recorded winner.
        let score: Score = serde_json::from_value(found["score"].clone()).unwrap();
        let format: MatchFormat = serde_json::from_value(found["match_format"].clone()).unwrap();
        let winner = validate_score(&format, &score).unwrap().winner;
        assert_eq!(serde_json::to_value(winner).unwrap(), found["winner_side"]);
        assert!(when(&found, "scheduled_at") < now);
    }

    let reported = with("reported");
    assert_eq!(reported.len(), 1);
    assert!(
        when(&reported[0], "confirm_deadline_at") > now,
        "still awaiting confirmation"
    );
    let auto_confirm =
        "SELECT count(*) FROM jobs j JOIN matches m ON j.payload->>'match_id' = m.id::text
        WHERE j.kind = 'auto_confirm_match' AND j.completed_at IS NULL
          AND j.run_at = m.confirm_deadline_at AND m.status = 'reported'
          AND m.community_id = $1";
    assert_eq!(count(&app, community, auto_confirm).await, 1);

    let disputed = with("disputed");
    assert_eq!(disputed.len(), 1);
    let (reporter, disputer) = (&disputed[0]["reported_by"], &disputed[0]["disputed_by"]);
    assert!(disputer.is_string() && disputer != reporter);
    assert!(disputed[0]["dispute_note"].is_string());

    // Every confirmed result is on the ledger, and the boxes' tables count them.
    let ledger =
        "SELECT count(*) FROM ranking_events WHERE community_id = $1 AND source = 'league_match'";
    assert_eq!(count(&app, community, ledger).await, 24);
    let tables = get(&app, &owner, &format!("/api/v1/leagues/{league}/standings")).await;
    let played: u64 = tables
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|division| division["table"].as_array().unwrap())
        .map(|line| line["played"].as_u64().unwrap())
        .sum();
    assert_eq!(played, 24);
}

#[tokio::test]
async fn seeds_a_doubles_league_in_registration_requests_and_rankings() {
    let app = TestApp::spawn().await;
    let summary = seed(&app.state, "demo").await.unwrap();
    let community = summary.community_id;
    let owner = app.login(&summary.owner_email, "demo").await;
    let now = app.state.clock.now();

    let league = league_id(&app, community, DOUBLES).await;
    let view = get(&app, &owner, &format!("/api/v1/leagues/{league}")).await;
    assert_eq!(view["status"], "registration");
    assert!(
        when(&view, "registration_opens_at") <= now && now < when(&view, "registration_closes_at")
    );
    let starts_in = when(&view, "starts_at") - now;
    assert!(starts_in > Duration::days(13) && starts_in < Duration::days(15));
    let drawn = "SELECT count(*) FROM matches WHERE community_id = $1 AND league_id IS NOT NULL
        AND league_id = (SELECT id FROM leagues WHERE community_id = $1 AND name = 'Winter Doubles League')";
    assert_eq!(count(&app, community, drawn).await, 0);

    let entries = get(&app, &owner, &format!("/api/v1/leagues/{league}/entries")).await;
    let entries = entries.as_array().unwrap();
    let with = |status: &str| {
        entries
            .iter()
            .filter(|entry| entry["status"] == status)
            .collect::<Vec<_>>()
    };
    assert_eq!(with("confirmed").len(), 3);
    assert!(
        with("confirmed")
            .iter()
            .all(|entry| entry["player_ids"].as_array().unwrap().len() == 2)
    );
    let pending = with("pending_partner");
    assert_eq!(pending.len(), 1);
    assert!(pending[0]["invited_partner_id"].is_string());
    assert_eq!(pending[0]["player_ids"].as_array().unwrap().len(), 1);

    let requests = get(&app, &owner, "/api/v1/match-requests").await;
    let requests = requests["items"].as_array().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| when(request, "time_window_start") > now)
    );
    let singles = requests
        .iter()
        .find(|request| request["discipline"] == "singles")
        .unwrap();
    assert_eq!(singles["slots_open"], 1);
    let doubles = requests
        .iter()
        .find(|request| request["discipline"] == "doubles")
        .unwrap();
    assert_eq!(doubles["slots_open"], 2, "the creator brought a partner");
    assert_eq!(doubles["players"].as_array().unwrap().len(), 2);

    let board = get(
        &app,
        &owner,
        "/api/v1/rankings?discipline=singles&limit=100",
    )
    .await;
    let rows = board["items"].as_array().unwrap();
    assert_eq!(
        summary.counts[4],
        ("ranked players", i64::try_from(rows.len()).unwrap())
    );
    assert!(!rows.is_empty());
    assert_eq!(rows[0]["rank"], 1);
    let points: Vec<i64> = rows
        .iter()
        .map(|row| row["points_52w"].as_i64().unwrap())
        .collect();
    assert!(
        points.windows(2).all(|pair| pair[0] >= pair[1]),
        "best first"
    );
    assert!(points[0] > 0);
}

#[tokio::test]
async fn reseeding_never_duplicates_and_follows_the_clock() {
    let app = TestApp::spawn().await;
    app.clock.advance(Duration::days(40));
    let first = seed(&app.state, "demo").await.unwrap();
    let community = first.community_id;
    let now = app.state.clock.now();
    let (starts, ends): (DateTime<Utc>, DateTime<Utc>) =
        sqlx::query_as("SELECT starts_at, ends_at FROM leagues WHERE name = $1")
            .bind(SINGLES)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(
        starts < now && now < ends,
        "dates are relative to the app clock"
    );
    let counts: Vec<_> = first.counts.iter().map(|(_, count)| *count).collect();
    assert_eq!(counts[..4], [25, 2, 42, 2]);

    let before = snapshot(&app).await;
    let again = seed(&app.state, "demo").await.unwrap();
    assert_eq!(again, first);
    assert_eq!(snapshot(&app).await, before);

    // Hand edits: the seed refreshes what it owns (club, profiles, missing requests) and
    // leaves everything else, including a league's entries and a promoted player, alone.
    for edit in [
        "UPDATE communities SET name = 'Renamed', branding = '{}'",
        "UPDATE players SET racket = 'Bent frame', utr = 1.5 WHERE display_name = 'Liam Carter'",
        "UPDATE players SET role = 'admin' WHERE display_name = 'Ava Thompson'",
        "UPDATE league_entries SET status = 'withdrawn' WHERE id = (SELECT id FROM league_entries
            WHERE status = 'confirmed' AND division_id IS NOT NULL LIMIT 1)",
        "DELETE FROM match_requests WHERE discipline = 'singles'",
    ] {
        let _ = sqlx::query(edit).execute(&app.db).await.unwrap();
    }
    assert_eq!(seed(&app.state, "demo").await.unwrap(), first);
    assert_eq!(
        snapshot(&app).await,
        before,
        "the deleted request is back, nothing else new"
    );
    let name: String = sqlx::query_scalar("SELECT name FROM communities")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(name, "Riverside Tennis Club");
    let branding: Value = sqlx::query_scalar("SELECT branding FROM communities")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(branding["display_name"], "Riverside Tennis Club");
    let liam =
        "SELECT count(*) FROM players WHERE community_id = $1 AND display_name = 'Liam Carter'
        AND racket IS DISTINCT FROM 'Bent frame' AND utr = 3.2";
    assert_eq!(count(&app, community, liam).await, 1, "profile refreshed");
    let ava =
        "SELECT count(*) FROM players WHERE community_id = $1 AND display_name = 'Ava Thompson'
        AND role = 'admin'";
    assert_eq!(
        count(&app, community, ava).await,
        1,
        "roles are never demoted"
    );
    let withdrawn =
        "SELECT count(*) FROM league_entries WHERE community_id = $1 AND status = 'withdrawn'";
    assert_eq!(
        count(&app, community, withdrawn).await,
        1,
        "an existing league is left as it is"
    );
}

#[tokio::test]
async fn a_second_slug_is_a_second_club_with_the_same_people() {
    let app = TestApp::spawn().await;
    let demo = seed(&app.state, "demo").await.unwrap();
    let other = seed(&app.state, " Club-Two ").await.unwrap();
    assert_eq!(other.slug, "club-two");
    assert_ne!(other.community_id, demo.community_id);
    assert_eq!(other.counts[..3], demo.counts[..3]);
    assert_eq!(demo.counts[0], ("players", 25));
    let counts = snapshot(&app).await;
    // communities, users (shared by email), players, leagues.
    assert_eq!(counts[..4], [2, 25, 50, 4]);
}
