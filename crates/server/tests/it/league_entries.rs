//! League registration: singles, doubles invitations, mixed eligibility, pairing, withdrawal.

use axum::http::StatusCode;
use chrono::Duration;
use serde_json::{Value, json};

use crate::{
    common::{Session, TestApp},
    leagues::open_league,
    matches::players,
};

async fn setup(names: &[&str]) -> (TestApp, Session, Vec<Session>) {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let admin = app.login("admin@example.test", "demo").await;
    app.make_admin(&admin).await;
    let ps = players(&app, "demo", names).await;
    (app, admin, ps)
}

async fn register(
    app: &TestApp,
    session: &Session,
    league: &str,
    body: Value,
) -> crate::common::Res {
    app.post(&format!("/api/v1/leagues/{league}/entries"))
        .as_(session)
        .json(body)
        .send()
        .await
}

async fn act(
    app: &TestApp,
    session: &Session,
    league: &str,
    entry: &Value,
    action: &str,
) -> crate::common::Res {
    app.post(&format!(
        "/api/v1/leagues/{league}/entries/{}/{action}",
        entry["id"].as_str().unwrap()
    ))
    .as_(session)
    .send()
    .await
}

#[tokio::test]
async fn singles_entries_confirm_immediately_once_per_player() {
    let (app, admin, ps) = setup(&["ana", "bo"]).await;
    let (ana, bo) = (&ps[0], &ps[1]);
    let league = open_league(&app, &admin, "singles").await;
    let entry = register(&app, ana, &league, json!({}))
        .await
        .expect(StatusCode::CREATED);
    assert_eq!(entry["status"], "confirmed");
    assert_eq!(entry["player_ids"], json!([ana.player_id]));
    let _ = register(&app, ana, &league, json!({}))
        .await
        .expect(StatusCode::CONFLICT);
    let _ = register(&app, bo, &league, json!({ "partner_id": ana.player_id }))
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);

    app.clock.advance(Duration::days(8));
    let body = register(&app, bo, &league, json!({}))
        .await
        .expect(StatusCode::CONFLICT);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not open")
    );
    let listed = app
        .get(&format!("/api/v1/leagues/{league}/entries"))
        .as_(bo)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(listed.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn doubles_invitations_accept_and_decline() {
    let (app, admin, ps) = setup(&["ana", "bo", "cy", "dee"]).await;
    let (ana, bo, cy, dee) = (&ps[0], &ps[1], &ps[2], &ps[3]);
    let league = open_league(&app, &admin, "doubles").await;
    let _ = register(&app, ana, &league, json!({}))
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
    let entry = register(&app, ana, &league, json!({ "partner_id": bo.player_id }))
        .await
        .expect(StatusCode::CREATED);
    assert_eq!(entry["status"], "pending_partner");
    assert_eq!(entry["invited_partner_id"], json!(bo.player_id));

    let seen_by = |session: &Session| {
        app.get(&format!("/api/v1/leagues/{league}/entries"))
            .as_(session)
            .send()
    };
    let for_cy = seen_by(cy).await.expect(StatusCode::OK);
    assert!(
        for_cy[0]["invited_partner_id"].is_null(),
        "invitee hidden from others"
    );
    let for_bo = seen_by(bo).await.expect(StatusCode::OK);
    assert_eq!(for_bo[0]["invited_partner_id"], json!(bo.player_id));

    let _ = act(&app, cy, &league, &entry, "accept")
        .await
        .expect(StatusCode::FORBIDDEN);
    // Bo had listed himself as looking; accepting supersedes that solo entry.
    let solo = register(&app, bo, &league, json!({ "looking_for_partner": true }))
        .await
        .expect(StatusCode::CREATED);
    let done = act(&app, bo, &league, &entry, "accept")
        .await
        .expect(StatusCode::OK);
    assert_eq!(done["status"], "confirmed");
    assert_eq!(done["player_ids"], json!([ana.player_id, bo.player_id]));
    assert!(done["invited_partner_id"].is_null());
    let all = app
        .get(&format!(
            "/api/v1/leagues/{league}/entries?status=withdrawn"
        ))
        .as_(&admin)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(all[0]["id"], solo["id"]);

    let _ = register(&app, cy, &league, json!({ "partner_id": bo.player_id }))
        .await
        .expect(StatusCode::CONFLICT);
    let e2 = register(&app, cy, &league, json!({ "partner_id": dee.player_id }))
        .await
        .expect(StatusCode::CREATED);
    let declined = act(&app, dee, &league, &e2, "decline")
        .await
        .expect(StatusCode::OK);
    assert!(declined["invited_partner_id"].is_null());
    assert_eq!(declined["status"], "pending_partner");
    let _ = act(&app, dee, &league, &e2, "accept")
        .await
        .expect(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn mixed_needs_one_female_and_one_male() {
    let (app, admin, ps) = setup(&["ana", "bo", "cy", "dee"]).await;
    let (ana, bo, cy, dee) = (&ps[0], &ps[1], &ps[2], &ps[3]);
    for (session, gender) in [
        (ana, "female"),
        (bo, "male"),
        (cy, "female"),
        (dee, "undisclosed"),
    ] {
        let _ = app.patch_me(session, json!({ "gender": gender })).await;
    }
    let league = open_league(&app, &admin, "mixed").await;
    for partner in [cy, dee] {
        let body = register(
            &app,
            ana,
            &league,
            json!({ "partner_id": partner.player_id }),
        )
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("one female and one male")
        );
    }
    let body = register(&app, dee, &league, json!({ "looking_for_partner": true }))
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("your profile")
    );

    let entry = register(&app, ana, &league, json!({ "partner_id": bo.player_id }))
        .await
        .expect(StatusCode::CREATED);
    // Bo changes his gender before accepting: re-checked on accept.
    let _ = app.patch_me(bo, json!({ "gender": "other" })).await;
    let _ = act(&app, bo, &league, &entry, "accept")
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
    let _ = app.patch_me(bo, json!({ "gender": "male" })).await;
    let done = act(&app, bo, &league, &entry, "accept")
        .await
        .expect(StatusCode::OK);
    assert_eq!(done["status"], "confirmed");
}

/// Sets the community's mixed-doubles rule (the tenant cache would otherwise serve the old one).
async fn set_mixed_rule(app: &TestApp, rule: &str) {
    let _ = sqlx::query(
        "UPDATE communities SET settings = jsonb_build_object('mixed_eligibility', $1::text)",
    )
    .bind(rule)
    .execute(&app.db)
    .await
    .unwrap();
    app.state.tenants.invalidate_all();
}

fn message(res: crate::common::Res) -> String {
    res.expect(StatusCode::UNPROCESSABLE_ENTITY)["error"]["message"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn other_enters_mixed_only_under_any_two_distinct() {
    let (app, admin, ps) = setup(&["ana", "bo", "cy", "dee"]).await;
    let (ana, bo, cy, dee) = (&ps[0], &ps[1], &ps[2], &ps[3]);
    for (session, gender) in [
        (ana, "female"),
        (bo, "other"),
        (cy, "other"),
        (dee, "undisclosed"),
    ] {
        let _ = app.patch_me(session, json!({ "gender": gender })).await;
    }
    let league = open_league(&app, &admin, "mixed").await;
    let invite = async |from: &Session, to: &Session| {
        register(&app, from, &league, json!({ "partner_id": to.player_id })).await
    };

    // The default rule: exactly one female and one male.
    let why = message(invite(ana, bo).await);
    assert!(why.contains("one female and one male"), "{why}");
    let why = message(register(&app, bo, &league, json!({ "looking_for_partner": true })).await);
    assert!(
        why.contains("other and undisclosed are not eligible"),
        "{why}"
    );

    set_mixed_rule(&app, "any_two_distinct").await;
    // Undisclosed is never eligible, alone or in a pair; two of a kind are not a mixed pair.
    let why = message(invite(ana, dee).await);
    assert!(why.contains("undisclosed is not eligible"), "{why}");
    let why = message(register(&app, dee, &league, json!({ "looking_for_partner": true })).await);
    assert!(why.contains("your profile"), "{why}");
    let why = message(invite(bo, cy).await);
    assert!(why.contains("different genders"), "{why}");
    let _ = register(&app, cy, &league, json!({ "looking_for_partner": true }))
        .await
        .expect(StatusCode::CREATED);

    // Female + other is a valid pair now.
    let entry = invite(ana, bo).await.expect(StatusCode::CREATED);
    // The rule is checked again on accept: back to the default, `other` is refused.
    set_mixed_rule(&app, "female_male").await;
    let why = message(act(&app, bo, &league, &entry, "accept").await);
    assert!(why.contains("one female and one male"), "{why}");
    set_mixed_rule(&app, "any_two_distinct").await;
    let done = act(&app, bo, &league, &entry, "accept")
        .await
        .expect(StatusCode::OK);
    assert_eq!(done["status"], "confirmed");
    assert_eq!(done["player_ids"], json!([ana.player_id, bo.player_id]));
}

#[tokio::test]
async fn admin_pairing_follows_the_mixed_rule() {
    let (app, admin, ps) = setup(&["ana", "bo", "cy"]).await;
    let (ana, bo, cy) = (&ps[0], &ps[1], &ps[2]);
    for (session, gender) in [(ana, "other"), (bo, "female"), (cy, "other")] {
        let _ = app.patch_me(session, json!({ "gender": gender })).await;
    }
    set_mixed_rule(&app, "any_two_distinct").await;
    let league = open_league(&app, &admin, "mixed").await;
    let mut entries = Vec::new();
    for player in [ana, bo, cy] {
        entries.push(
            register(
                &app,
                player,
                &league,
                json!({ "looking_for_partner": true }),
            )
            .await
            .expect(StatusCode::CREATED),
        );
    }
    let pair = |first: &Value, second: &Value| {
        app.post(&format!("/api/v1/admin/leagues/{league}/pair"))
            .as_(&admin)
            .json(json!({ "entry_ids": [first["id"], second["id"]] }))
            .send()
    };
    let why = message(pair(&entries[0], &entries[2]).await);
    assert!(why.contains("different genders"), "other + other: {why}");
    set_mixed_rule(&app, "female_male").await;
    let why = message(pair(&entries[0], &entries[1]).await);
    assert!(
        why.contains("one female and one male"),
        "other + female: {why}"
    );
    set_mixed_rule(&app, "any_two_distinct").await;
    let paired = pair(&entries[0], &entries[1]).await.expect(StatusCode::OK);
    assert_eq!(paired["status"], "confirmed");
}

#[tokio::test]
async fn admins_pair_solo_entries() {
    let (app, admin, ps) = setup(&["ana", "bo", "cy"]).await;
    let (ana, bo, cy) = (&ps[0], &ps[1], &ps[2]);
    let league = open_league(&app, &admin, "doubles").await;
    let ana_entry = register(&app, ana, &league, json!({ "looking_for_partner": true }))
        .await
        .expect(StatusCode::CREATED);
    let bo_entry = register(&app, bo, &league, json!({ "looking_for_partner": true }))
        .await
        .expect(StatusCode::CREATED);
    let cy_entry = register(&app, cy, &league, json!({ "looking_for_partner": true }))
        .await
        .expect(StatusCode::CREATED);
    let looking = app
        .get(&format!(
            "/api/v1/leagues/{league}/entries?looking_for_partner=true"
        ))
        .as_(cy)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(looking.as_array().unwrap().len(), 3);

    let pair = |session: &Session, ids: Value| {
        app.post(&format!("/api/v1/admin/leagues/{league}/pair"))
            .as_(session)
            .json(json!({ "entry_ids": ids }))
            .send()
    };
    let _ = pair(ana, json!([ana_entry["id"], bo_entry["id"]]))
        .await
        .expect(StatusCode::FORBIDDEN);
    let _ = pair(&admin, json!([ana_entry["id"], ana_entry["id"]]))
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
    let paired = pair(&admin, json!([ana_entry["id"], bo_entry["id"]]))
        .await
        .expect(StatusCode::OK);
    assert_eq!(paired["status"], "confirmed");
    assert_eq!(paired["player_ids"], json!([ana.player_id, bo.player_id]));
    assert!(!paired["looking_for_partner"].as_bool().unwrap());
    let _ = pair(&admin, json!([ana_entry["id"], cy_entry["id"]]))
        .await
        .expect(StatusCode::CONFLICT);
    let left = app
        .get(&format!(
            "/api/v1/leagues/{league}/entries?looking_for_partner=true"
        ))
        .as_(cy)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(left.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn withdrawal_rules() {
    let (app, admin, ps) = setup(&["ana", "bo"]).await;
    let (ana, bo) = (&ps[0], &ps[1]);
    let league = open_league(&app, &admin, "singles").await;
    let ana_entry = register(&app, ana, &league, json!({}))
        .await
        .expect(StatusCode::CREATED);
    let bo_entry = register(&app, bo, &league, json!({}))
        .await
        .expect(StatusCode::CREATED);
    let _ = act(&app, bo, &league, &ana_entry, "withdraw")
        .await
        .expect(StatusCode::FORBIDDEN);
    let w = act(&app, ana, &league, &ana_entry, "withdraw")
        .await
        .expect(StatusCode::OK);
    assert_eq!(w["status"], "withdrawn");
    let _ = act(&app, ana, &league, &ana_entry, "withdraw")
        .await
        .expect(StatusCode::CONFLICT);
    // Withdrawn players may enter again while registration is open.
    let _ = register(&app, ana, &league, json!({}))
        .await
        .expect(StatusCode::CREATED);

    app.clock.advance(Duration::days(8));
    let _ = act(&app, bo, &league, &bo_entry, "withdraw")
        .await
        .expect(StatusCode::CONFLICT);
    let w = act(&app, &admin, &league, &bo_entry, "withdraw")
        .await
        .expect(StatusCode::OK);
    assert_eq!(w["status"], "withdrawn");
}
