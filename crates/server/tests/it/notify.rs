//! Push notifications: devices, preferences, and who is told what about a match.

use axum::http::StatusCode;
use chrono::{Duration, Utc};
use racquetcollective_server::{clock::Clock, jobs};
use serde_json::{Value, json};

use crate::{
    common::{Session, TestApp},
    league_lifecycle::at_day,
    leagues::open_league,
    match_results::{report, straight_sets_a},
    matches::{players, singles},
    proposals::in_days,
};

fn token(name: &str) -> String {
    format!("ExponentPushToken[{name}]")
}

async fn register(app: &TestApp, session: &Session, name: &str) {
    let _ = app
        .req(
            axum::http::Method::PUT,
            &format!("/api/v1/me/devices/{}", token(name)),
        )
        .as_(session)
        .json(json!({ "platform": "ios" }))
        .send()
        .await
        .expect(StatusCode::NO_CONTENT);
}

/// Two named players, each with a device.
async fn setup() -> (TestApp, Session, Session) {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let [ana, bo] = <[Session; 2]>::try_from(players(&app, "demo", &["ana", "bo"]).await).unwrap();
    let _ = app.patch_me(&ana, json!({ "display_name": "Ana" })).await;
    let _ = app.patch_me(&bo, json!({ "display_name": "Bo" })).await;
    register(&app, &ana, "ana").await;
    register(&app, &bo, "bo").await;
    (app, ana, bo)
}

/// Runs due jobs and returns the (to, title, body, url) of every push sent since `seen`.
async fn pushes(app: &TestApp, seen: &mut usize) -> Vec<(String, String, String, String)> {
    let _ = jobs::run_due(&app.state, "t").await.unwrap();
    let sent = app.pusher.sent();
    let new = sent[*seen..]
        .iter()
        .map(|message| {
            (
                message.to.clone(),
                message.title.clone(),
                message.body.clone(),
                message.data["url"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    *seen = sent.len();
    new
}

async fn post(app: &TestApp, session: &Session, path: &str, body: Value) -> Value {
    app.post(path)
        .as_(session)
        .json(body)
        .send()
        .await
        .expect(StatusCode::OK)
}

#[tokio::test]
async fn devices_register_move_between_members_and_are_forgotten() {
    let (app, ana, bo) = setup().await;
    let _ = app
        .req(axum::http::Method::PUT, "/api/v1/me/devices/not-a-token")
        .as_(&ana)
        .json(json!({ "platform": "ios" }))
        .send()
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
    let owner = |name: &str| {
        let db = app.db.clone();
        let token = token(name);
        async move {
            sqlx::query_scalar::<_, uuid::Uuid>(
                "SELECT player_id FROM device_tokens WHERE expo_push_token = $1",
            )
            .bind(token)
            .fetch_optional(&db)
            .await
            .unwrap()
        }
    };
    assert_eq!(owner("ana").await, Some(ana.player_id));
    // Bo signs in on Ana's phone: the token is his now.
    register(&app, &bo, "ana").await;
    assert_eq!(owner("ana").await, Some(bo.player_id));
    let _ = app
        .delete(&format!("/api/v1/me/devices/{}", token("ana")))
        .as_(&bo)
        .send()
        .await
        .expect(StatusCode::NO_CONTENT);
    assert_eq!(owner("ana").await, None);
}

#[tokio::test]
async fn preferences_default_on_and_silence_a_category() {
    let (app, ana, bo) = setup().await;
    let prefs = app
        .get("/api/v1/me/notifications")
        .as_(&bo)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(
        prefs,
        json!({ "match_updates": true, "league_updates": true, "reminders": true })
    );
    let off = json!({ "match_updates": false, "league_updates": true, "reminders": true });
    let saved = app
        .req(axum::http::Method::PUT, "/api/v1/me/notifications")
        .as_(&bo)
        .json(off.clone())
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(saved, off);
    let mut seen = 0;
    let _ = singles(&app, &ana, &bo).await;
    assert!(
        pushes(&app, &mut seen).await.is_empty(),
        "Bo turned match updates off"
    );
}

#[tokio::test]
async fn each_step_of_a_match_tells_the_other_side() {
    let (app, ana, bo) = setup().await;
    let mut seen = 0;
    let created = singles(&app, &ana, &bo).await;
    let id = created["id"].as_str().unwrap().to_owned();
    let url = format!("/matches/{id}");
    assert_eq!(
        pushes(&app, &mut seen).await,
        [(
            token("bo"),
            "Ana challenged you".to_owned(),
            "Propose a time, or answer theirs.".to_owned(),
            url.clone()
        )]
    );

    let proposed = app
        .post(&format!("/api/v1/matches/{id}/proposals"))
        .as_(&bo)
        .json(json!({ "time": in_days(3), "location": "Court 1" }))
        .send()
        .await
        .expect(StatusCode::CREATED);
    let sent = pushes(&app, &mut seen).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (sent[0].0.as_str(), sent[0].1.as_str()),
        (token("ana").as_str(), "Bo proposed a time")
    );

    let proposal = proposed["proposals"][0]["id"].as_str().unwrap();
    let _ = post(
        &app,
        &ana,
        &format!("/api/v1/matches/{id}/proposals/{proposal}/accept"),
        json!({}),
    )
    .await;
    let sent = pushes(&app, &mut seen).await;
    assert_eq!(sent[0].0, token("bo"));
    assert_eq!(sent[0].1, "Ana accepted your time");
    assert_eq!(sent[0].2, "Your match against Ana is on.");

    let _ = report(&app, &bo, &id, straight_sets_a()).await;
    let sent = pushes(&app, &mut seen).await;
    assert_eq!(sent[0].0, token("ana"));
    assert_eq!(sent[0].1, "Bo reported a score");
    // Read from Ana's side: she won 6–3 6–4.
    assert_eq!(sent[0].2, "6–3 6–4 against Bo: confirm it or dispute it.");

    let _ = post(
        &app,
        &ana,
        &format!("/api/v1/matches/{id}/dispute"),
        json!({}),
    )
    .await;
    let sent = pushes(&app, &mut seen).await;
    assert_eq!(sent[0].0, token("bo"));
    assert_eq!(sent[0].1, "Ana disputed your score");

    let admin = app.login("admin@example.test", "demo").await;
    app.make_admin(&admin).await;
    let _ = post(
        &app,
        &admin,
        &format!("/api/v1/admin/matches/{id}/resolve"),
        json!({ "resolution": "score", "score": straight_sets_a(), "note": "Checked with both" }),
    )
    .await;
    let mut sent = pushes(&app, &mut seen).await;
    sent.sort();
    assert_eq!(sent.len(), 2, "both players hear the ruling");
    assert_eq!(sent[0].0, token("ana"));
    assert_eq!(sent[0].1, "Your disputed match was decided");
    assert_eq!(sent[0].2, "6–3 6–4 against Bo. “Checked with both”");
    assert_eq!(sent[1].2, "3–6 4–6 against Ana. “Checked with both”");
}

#[tokio::test]
async fn cancelling_and_filled_requests_notify_the_others() {
    let (app, ana, bo) = setup().await;
    let mut seen = 0;
    let created = singles(&app, &ana, &bo).await;
    let id = created["id"].as_str().unwrap();
    let _ = pushes(&app, &mut seen).await;
    let _ = post(
        &app,
        &ana,
        &format!("/api/v1/matches/{id}/cancel"),
        json!({ "note": "Rain" }),
    )
    .await;
    let sent = pushes(&app, &mut seen).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (sent[0].1.as_str(), sent[0].2.as_str()),
        ("Ana cancelled your match", "Rain")
    );

    let request = app
        .post("/api/v1/match-requests")
        .as_(&ana)
        .json(json!({
            "discipline": "singles",
            "time_window_start": in_days(2),
            "time_window_end": in_days(3),
        }))
        .send()
        .await
        .expect(StatusCode::CREATED);
    let request_id = request["id"].as_str().unwrap();
    let filled = post(
        &app,
        &bo,
        &format!("/api/v1/match-requests/{request_id}/join"),
        json!({}),
    )
    .await;
    let sent = pushes(&app, &mut seen).await;
    assert_eq!(sent.len(), 1, "the joiner who filled it already knows");
    assert_eq!(sent[0].0, token("ana"));
    assert_eq!(sent[0].1, "Your match request is full");
    assert_eq!(
        sent[0].3,
        format!("/matches/{}", filled["match_id"].as_str().unwrap())
    );
}

#[tokio::test]
async fn players_without_devices_get_scores_to_confirm_by_email() {
    let (app, ana, bo) = setup().await;
    let _ = app
        .delete(&format!("/api/v1/me/devices/{}", token("ana")))
        .as_(&ana)
        .send()
        .await
        .expect(StatusCode::NO_CONTENT);
    let created = singles(&app, &bo, &ana).await;
    let id = created["id"].as_str().unwrap();
    let mut seen = 0;
    assert!(pushes(&app, &mut seen).await.is_empty());
    assert!(
        app.mailer
            .last_to(&ana.email)
            .unwrap()
            .subject
            .contains("sign-in code"),
        "a challenge is not worth an email"
    );
    let _ = report(&app, &bo, id, straight_sets_a()).await;
    let _ = pushes(&app, &mut seen).await;
    let mail = app.mailer.last_to(&ana.email).unwrap();
    assert_eq!(mail.subject, "demo club: Bo reported a score");
    assert!(
        mail.text
            .contains("3–6 4–6 against Bo: confirm it or dispute it.")
    );
    assert!(mail.text.contains("confirms itself after 3 days"));
    assert!(
        mail.text
            .contains(&format!("https://demo.racquetcollective.app/matches/{id}"))
    );
}

#[tokio::test]
async fn unregistered_devices_are_forgotten_and_banned_players_not_told() {
    let (app, ana, bo) = setup().await;
    register(&app, &ana, "unregistered-old-phone").await;
    let mut seen = 0;
    let _ = singles(&app, &bo, &ana).await;
    let sent = pushes(&app, &mut seen).await;
    assert_eq!(sent.len(), 2, "both of Ana's devices are tried");
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM device_tokens WHERE player_id = $1")
        .bind(ana.player_id)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(left, 1);

    let created = singles(&app, &ana, &bo).await;
    let _ = sqlx::query("UPDATE players SET status = 'banned' WHERE id = $1")
        .bind(bo.player_id)
        .execute(&app.db)
        .await
        .unwrap();
    assert!(created["id"].is_string());
    assert!(pushes(&app, &mut seen).await.is_empty());
}

async fn entry_action(app: &TestApp, session: &Session, league: &str, entry: &Value, action: &str) {
    let _ = post(
        app,
        session,
        &format!(
            "/api/v1/leagues/{league}/entries/{}/{action}",
            entry["id"].as_str().unwrap()
        ),
        json!({}),
    )
    .await;
}

#[tokio::test]
async fn partner_invitations_and_their_answers_are_told() {
    let (app, ana, bo) = setup().await;
    let cy = app.login("cy@example.test", "demo").await;
    let _ = app.patch_me(&cy, json!({ "display_name": "Cy" })).await;
    let admin = app.login("admin@example.test", "demo").await;
    app.make_admin(&admin).await;
    let league = open_league(&app, &admin, "doubles").await;
    let url = format!("/leagues/{league}");
    let mut seen = 0;

    let entry = app
        .post(&format!("/api/v1/leagues/{league}/entries"))
        .as_(&ana)
        .json(json!({ "partner_id": bo.player_id }))
        .send()
        .await
        .expect(StatusCode::CREATED);
    let sent = pushes(&app, &mut seen).await;
    assert_eq!(
        sent,
        [(
            token("bo"),
            "Ana invited you to partner them".to_owned(),
            "Autumn doubles: accept or decline before registration closes.".to_owned(),
            url.clone()
        )]
    );

    entry_action(&app, &bo, &league, &entry, "decline").await;
    let sent = pushes(&app, &mut seen).await;
    assert_eq!(sent[0].0, token("ana"));
    assert_eq!(sent[0].1, "Bo declined your invitation");

    // Cy has no device: the invitation comes by email.
    let _ = post(
        &app,
        &ana,
        &format!(
            "/api/v1/leagues/{league}/entries/{}/partner",
            entry["id"].as_str().unwrap()
        ),
        json!({ "partner_id": cy.player_id }),
    )
    .await;
    assert!(pushes(&app, &mut seen).await.is_empty());
    let mail = app.mailer.last_to(&cy.email).unwrap();
    assert_eq!(mail.subject, "demo club: Ana invited you to partner them");
    assert!(
        mail.text
            .contains(&format!("https://demo.racquetcollective.app{url}"))
    );

    entry_action(&app, &cy, &league, &entry, "accept").await;
    let sent = pushes(&app, &mut seen).await;
    assert_eq!(sent[0].0, token("ana"));
    assert_eq!(sent[0].1, "Cy accepted your invitation");
    assert_eq!(sent[0].2, "You’re entered in Autumn doubles together.");
}

#[tokio::test]
async fn entrants_hear_when_their_league_starts() {
    let (app, ana, bo) = setup().await;
    let admin = app.login("admin@example.test", "demo").await;
    app.make_admin(&admin).await;
    let league = open_league(&app, &admin, "singles").await;
    for player in [&ana, &bo] {
        let _ = app
            .post(&format!("/api/v1/leagues/{league}/entries"))
            .as_(player)
            .json(json!({}))
            .send()
            .await
            .expect(StatusCode::CREATED);
    }
    let mut seen = 0;
    at_day(&app, 8).await;
    let mut sent = pushes(&app, &mut seen).await;
    sent.sort();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].0, token("ana"));
    assert_eq!(sent[0].1, "Autumn singles has started");
    assert_eq!(sent[0].2, "You’re in Box 1 with 1 match to arrange.");
    assert_eq!(sent[0].3, format!("/leagues/{league}"));
}

#[tokio::test]
async fn players_are_reminded_the_day_before_and_reschedules_move_it() {
    let (app, ana, bo) = setup().await;
    let created = singles(&app, &ana, &bo).await;
    let id = created["id"].as_str().unwrap().to_owned();
    let schedule = |days: i64| {
        let app = &app;
        let (ana, bo, id) = (ana.clone(), bo.clone(), id.clone());
        async move {
            let proposed = app
                .post(&format!("/api/v1/matches/{id}/proposals"))
                .as_(&ana)
                .json(json!({ "time": (Utc::now() + Duration::days(days)).to_rfc3339(), "location": "Court 2" }))
                .send()
                .await
                .expect(StatusCode::CREATED);
            let open = proposed["proposals"]
                .as_array()
                .unwrap()
                .iter()
                .find(|proposal| proposal["status"] == "open")
                .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_owned();
            let _ = post(
                app,
                &bo,
                &format!("/api/v1/matches/{id}/proposals/{open}/accept"),
                json!({}),
            )
            .await;
        }
    };
    let mut seen = 0;
    schedule(3).await;
    // Moved to five days out before the first reminder was due.
    schedule(5).await;
    let _ = pushes(&app, &mut seen).await;

    app.clock
        .set(Utc::now() + Duration::days(2) + Duration::minutes(1));
    assert!(
        pushes(&app, &mut seen).await.is_empty(),
        "the old reminder moved"
    );
    app.clock
        .set(Utc::now() + Duration::days(4) + Duration::minutes(1));
    let mut sent = pushes(&app, &mut seen).await;
    sent.sort();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].0, token("ana"));
    assert_eq!(sent[0].1, "Match tomorrow");
    assert_eq!(sent[0].2, "You play Bo at Court 2. Good luck!");

    // A match arranged for later today gets no reminder at all.
    let other = singles(&app, &ana, &bo).await;
    let other_id = other["id"].as_str().unwrap();
    let proposed = app
        .post(&format!("/api/v1/matches/{other_id}/proposals"))
        .as_(&bo)
        .json(json!({ "time": (app.clock.now() + Duration::hours(6)).to_rfc3339() }))
        .send()
        .await
        .expect(StatusCode::CREATED);
    let open = proposed["proposals"][0]["id"].as_str().unwrap();
    let _ = post(
        &app,
        &ana,
        &format!("/api/v1/matches/{other_id}/proposals/{open}/accept"),
        json!({}),
    )
    .await;
    let _ = pushes(&app, &mut seen).await;
    app.clock.set(app.clock.now() + Duration::hours(5));
    assert!(pushes(&app, &mut seen).await.is_empty());
}
