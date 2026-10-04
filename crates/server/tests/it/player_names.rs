//! Player names embedded in match, standings, entry and match-request views, under the
//! visibility rule of `GET /players/{id}`.

use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    common::{Session, TestApp},
    league_lifecycle::{at_day, setup},
    leagues::open_league,
    matches::players,
    proposals::in_days,
};

async fn name(app: &TestApp, session: &Session, display_name: &str) {
    let _ = app
        .patch_me(session, json!({ "display_name": display_name }))
        .await;
}

async fn get(app: &TestApp, session: &Session, path: &str) -> Value {
    app.get(path)
        .as_(session)
        .send()
        .await
        .expect(StatusCode::OK)
}

async fn hide(app: &TestApp, sql: &str, id: Uuid) {
    let _ = sqlx::query(sql).bind(id).execute(&app.db).await.unwrap();
}

async fn ban(app: &TestApp, session: &Session) {
    hide(
        app,
        "UPDATE players SET status = 'banned' WHERE id = $1",
        session.player_id,
    )
    .await;
}

async fn unverify(app: &TestApp, session: &Session) {
    hide(
        app,
        "UPDATE users SET email_verified_at = NULL WHERE id = $1",
        session.user_id,
    )
    .await;
}

/// The names in `view[names_key]`, after checking they follow `view[ids_key]` one for one
/// and carry nothing but the id and the name.
fn names_of(view: &Value, ids_key: &str, names_key: &str) -> Vec<Option<String>> {
    let ids = view[ids_key].as_array().unwrap();
    let named = view[names_key].as_array().unwrap();
    assert_eq!(
        named.len(),
        ids.len(),
        "{names_key} follows {ids_key}: {view:#}"
    );
    ids.iter()
        .zip(named)
        .map(|(id, player)| {
            assert_eq!(&player["id"], id, "{view:#}");
            let mut keys: Vec<&str> = player
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            assert_eq!(
                keys,
                ["display_name", "id"],
                "nothing else, gender least of all"
            );
            player["display_name"].as_str().map(str::to_owned)
        })
        .collect()
}

fn sides(view: &Value) -> [Vec<Option<String>>; 2] {
    [
        names_of(view, "side_a", "side_a_names"),
        names_of(view, "side_b", "side_b_names"),
    ]
}

fn named(names: &[&str]) -> Vec<Option<String>> {
    names.iter().map(|&name| Some(name.to_owned())).collect()
}

#[tokio::test]
async fn matches_name_the_players_the_viewer_may_see() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [ana, bo, cy, di, admin] =
        <[Session; 5]>::try_from(players(&app, "demo", &["ana", "bo", "cy", "di", "admin"]).await)
            .unwrap();
    app.make_admin(&admin).await;
    for (session, display_name) in [
        (&ana, "Ana Ruiz"),
        (&bo, "Bo Chen"),
        (&cy, "Cy Park"),
        (&di, "Di Okafor"),
    ] {
        name(&app, session, display_name).await;
    }
    let created = app
        .post("/api/v1/matches")
        .as_(&ana)
        .json(json!({
            "discipline": "doubles",
            "partner_id": bo.player_id,
            "opponent_ids": [cy.player_id, di.player_id],
        }))
        .send()
        .await
        .expect(StatusCode::CREATED);
    assert_eq!(
        sides(&created),
        [
            named(&["Ana Ruiz", "Bo Chen"]),
            named(&["Cy Park", "Di Okafor"])
        ]
    );
    let id = created["id"].as_str().unwrap();

    // Di is banned, Cy never verified their email, and neither did Ana; she still sees her
    // own name, admins see everyone's, as with `GET /players/{id}`.
    ban(&app, &di).await;
    unverify(&app, &cy).await;
    unverify(&app, &ana).await;
    let hidden = [named(&["Ana Ruiz", "Bo Chen"]), vec![None, None]];
    let one = get(&app, &ana, &format!("/api/v1/matches/{id}")).await;
    assert_eq!(sides(&one), hidden);
    let list = get(&app, &ana, "/api/v1/matches").await;
    assert_eq!(sides(&list["items"][0]), hidden);
    for hidden_player in [&cy, &di] {
        let _ = app
            .get(&format!("/api/v1/players/{}", hidden_player.player_id))
            .as_(&ana)
            .send()
            .await
            .expect(StatusCode::NOT_FOUND);
    }
    let everyone = get(&app, &admin, &format!("/api/v1/matches/{id}")).await;
    assert_eq!(
        sides(&everyone),
        [
            named(&["Ana Ruiz", "Bo Chen"]),
            named(&["Cy Park", "Di Okafor"])
        ]
    );
    let all = get(&app, &admin, "/api/v1/matches?all=true").await;
    assert_eq!(sides(&all["items"][0]), sides(&everyone));

    // Writes answer with the same view, named for the caller.
    let proposed = app
        .post(&format!("/api/v1/matches/{id}/proposals"))
        .as_(&bo)
        .json(json!({ "time": in_days(3) }))
        .send()
        .await
        .expect(StatusCode::CREATED);
    assert_eq!(
        sides(&proposed),
        [vec![None, Some("Bo Chen".to_owned())], vec![None, None]],
        "Bo may not see Ana now either"
    );
}

#[tokio::test]
async fn standings_and_entries_name_their_players() {
    let (app, admin, ps) = setup(4).await;
    let names = ["Ana Ruiz", "Bo Chen", "Cy Park", "Di Okafor"];
    for (session, display_name) in ps.iter().zip(names) {
        name(&app, session, display_name).await;
    }
    let league = open_league(&app, &admin, "singles").await;
    for session in &ps {
        let _ = app
            .post(&format!("/api/v1/leagues/{league}/entries"))
            .as_(session)
            .json(json!({}))
            .send()
            .await
            .expect(StatusCode::CREATED);
    }
    let entries = get(&app, &ps[0], &format!("/api/v1/leagues/{league}/entries")).await;
    let entry_names: Vec<_> = entries
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|entry| names_of(entry, "player_ids", "player_names"))
        .collect();
    assert_eq!(entry_names, named(&names), "registration order");

    at_day(&app, 8).await;
    ban(&app, &ps[3]).await;
    let line_names = |table: &Value| -> Vec<(Uuid, Option<String>)> {
        let mut out: Vec<_> = table
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|division| division["table"].as_array().unwrap())
            .map(|line| {
                let player = line["player_ids"][0].as_str().unwrap().parse().unwrap();
                let [display_name] =
                    <[_; 1]>::try_from(names_of(line, "player_ids", "player_names")).unwrap();
                (player, display_name)
            })
            .collect();
        out.sort_unstable();
        out
    };
    let expected = |banned_name: Option<&str>| {
        let mut out: Vec<_> = ps
            .iter()
            .zip(names)
            .map(|(session, display_name)| {
                let shown = if session.player_id == ps[3].player_id {
                    banned_name
                } else {
                    Some(display_name)
                };
                (session.player_id, shown.map(str::to_owned))
            })
            .collect();
        out.sort_unstable();
        out
    };
    let path = format!("/api/v1/leagues/{league}/standings");
    assert_eq!(line_names(&get(&app, &ps[0], &path).await), expected(None));
    assert_eq!(
        line_names(&get(&app, &admin, &path).await),
        expected(Some("Di Okafor"))
    );
    let entries = get(&app, &ps[0], &format!("/api/v1/leagues/{league}/entries")).await;
    let banned = entries
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["player_ids"][0] == json!(ps[3].player_id))
        .unwrap();
    assert_eq!(names_of(banned, "player_ids", "player_names"), [None]);
}

#[tokio::test]
async fn match_requests_name_their_players() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [ana, bo, cy] =
        <[Session; 3]>::try_from(players(&app, "demo", &["ana", "bo", "cy"]).await).unwrap();
    name(&app, &ana, "Ana Ruiz").await;
    name(&app, &bo, "Bo Chen").await;
    name(&app, &cy, "Cy Park").await;
    let window_end = (chrono::Utc::now() + chrono::Duration::days(3)).to_rfc3339();
    let request = app
        .post("/api/v1/match-requests")
        .as_(&ana)
        .json(json!({
            "discipline": "doubles",
            "partner_id": bo.player_id,
            "time_window_start": in_days(2),
            "time_window_end": window_end,
        }))
        .send()
        .await
        .expect(StatusCode::CREATED);
    assert_eq!(
        names_of(&request, "players", "player_names"),
        named(&["Ana Ruiz", "Bo Chen"])
    );
    let id = request["id"].as_str().unwrap();

    ban(&app, &bo).await;
    let joined = app
        .post(&format!("/api/v1/match-requests/{id}/join"))
        .as_(&cy)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(
        names_of(&joined, "players", "player_names"),
        [
            Some("Ana Ruiz".to_owned()),
            None,
            Some("Cy Park".to_owned())
        ]
    );
    let listed = get(&app, &cy, "/api/v1/match-requests").await;
    assert_eq!(
        names_of(&listed["items"][0], "players", "player_names"),
        names_of(&joined, "players", "player_names")
    );
}
