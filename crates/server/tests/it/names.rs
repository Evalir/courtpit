//! Views that list player ids also carry their display names, so clients need no request per
//! player.

use std::collections::{HashMap, HashSet};

use axum::http::StatusCode;
use courtpit_server::seed::seed;
use serde_json::Value;
use uuid::Uuid;

use crate::common::{Session, TestApp};

async fn get(app: &TestApp, viewer: &Session, path: &str) -> Value {
    app.get(path)
        .as_(viewer)
        .send()
        .await
        .expect(StatusCode::OK)
}

/// Every player's display name in `community`, by id.
async fn display_names(app: &TestApp, community: Uuid) -> HashMap<String, String> {
    let rows: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, display_name FROM players WHERE community_id = $1")
            .bind(community)
            .fetch_all(&app.db)
            .await
            .unwrap();
    rows.into_iter()
        .map(|(id, name)| (id.to_string(), name))
        .collect()
}

/// The ids a view lists under `keys` (arrays or single ids; nulls skipped), in order.
fn ids(view: &Value, keys: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for key in keys {
        match &view[*key] {
            Value::Array(items) => {
                out.extend(items.iter().map(|item| item.as_str().unwrap().to_owned()));
            }
            Value::String(id) => out.push(id.clone()),
            _ => {}
        }
    }
    out
}

/// Asserts `view.names` names exactly `expected` (deduplicated, in first-seen order) with the
/// players' current display names.
fn assert_names(view: &Value, expected: &[String], display: &HashMap<String, String>) {
    let mut seen = HashSet::new();
    let expected: Vec<&String> = expected.iter().filter(|id| seen.insert(*id)).collect();
    let names = view["names"].as_array().unwrap();
    let listed: Vec<&str> = names
        .iter()
        .map(|name| name["id"].as_str().unwrap())
        .collect();
    assert_eq!(listed, expected, "names follow the view's ids: {view}");
    for name in names {
        let id = name["id"].as_str().unwrap();
        assert_eq!(
            name["display_name"].as_str(),
            display.get(id).map(String::as_str)
        );
    }
}

#[tokio::test]
async fn match_lists_and_details_name_everyone_they_mention() {
    let app = TestApp::spawn().await;
    let summary = seed(&app.state, "demo").await.unwrap();
    let owner = app.login(&summary.owner_email, "demo").await;
    let display = display_names(&app, summary.community_id).await;
    let keys = [
        "side_a",
        "side_b",
        "reported_by",
        "disputed_by",
        "resolved_by",
    ];

    let page = get(&app, &owner, "/api/v1/matches?all=true&limit=100").await;
    let matches = page["items"].as_array().unwrap();
    assert!(matches.len() > 20);
    for view in matches {
        assert_names(view, &ids(view, &keys), &display);
    }

    // A single match also names whoever proposed a time.
    let reported = matches
        .iter()
        .find(|view| view["status"] == "reported")
        .unwrap();
    let detail = get(
        &app,
        &owner,
        &format!("/api/v1/matches/{}", reported["id"].as_str().unwrap()),
    )
    .await;
    let mut expected = ids(&detail, &keys);
    expected.extend(
        detail["proposals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|proposal| proposal["proposed_by"].as_str().unwrap().to_owned()),
    );
    assert!(!detail["proposals"].as_array().unwrap().is_empty());
    assert_names(&detail, &expected, &display);
}

#[tokio::test]
async fn standings_entries_and_requests_name_their_players() {
    let app = TestApp::spawn().await;
    let summary = seed(&app.state, "demo").await.unwrap();
    let owner = app.login(&summary.owner_email, "demo").await;
    let display = display_names(&app, summary.community_id).await;
    let leagues = get(&app, &owner, "/api/v1/leagues?limit=100").await;
    let league = |status: &str| {
        leagues["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|league| league["status"] == status)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };

    let boxes = get(
        &app,
        &owner,
        &format!("/api/v1/leagues/{}/standings", league("active")),
    )
    .await;
    let lines: Vec<&Value> = boxes
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|division| division["table"].as_array().unwrap())
        .collect();
    assert!(lines.len() >= 14);
    for line in lines {
        assert_names(line, &ids(line, &["player_ids"]), &display);
    }

    // An admin sees the invited partner, and their name.
    let path = format!("/api/v1/leagues/{}/entries", league("registration"));
    let entries = get(&app, &owner, &path).await;
    let entry_keys = ["player_ids", "created_by", "invited_partner_id"];
    for entry in entries.as_array().unwrap() {
        assert_names(entry, &ids(entry, &entry_keys), &display);
    }
    let pending = entries
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["status"] == "pending_partner")
        .unwrap();
    let invited = pending["invited_partner_id"].as_str().unwrap().to_owned();

    // A bystander sees neither the invitee nor their name.
    let bystander_email: String = sqlx::query_scalar(
        "SELECT u.email FROM players p JOIN users u ON u.id = p.user_id
         WHERE p.community_id = $1 AND p.role = 'player' AND p.id <> ALL($2)
         ORDER BY u.email LIMIT 1",
    )
    .bind(summary.community_id)
    .bind(
        ids(pending, &entry_keys)
            .iter()
            .map(|id| id.parse::<Uuid>().unwrap())
            .collect::<Vec<_>>(),
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    let bystander = app.login(&bystander_email, "demo").await;
    let seen = get(&app, &bystander, &path).await;
    let seen = seen
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == pending["id"])
        .unwrap();
    assert!(seen["invited_partner_id"].is_null());
    assert!(
        !seen["names"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name["id"] == invited.as_str())
    );

    let requests = get(&app, &owner, "/api/v1/match-requests").await;
    for request in requests["items"].as_array().unwrap() {
        assert_names(request, &ids(request, &["players"]), &display);
    }
}
