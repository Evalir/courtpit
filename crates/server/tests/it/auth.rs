//! Email OTP, password and session flows.

use axum::http::StatusCode;
use serde_json::json;

use crate::common::{TestApp, test_config};

#[tokio::test]
async fn otp_login_creates_verified_membership() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let session = app.login("ana@example.test", "demo").await;
    let body = app
        .get("/api/v1/auth/session")
        .as_(&session)
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(body["email"], "ana@example.test");
    assert_eq!(body["email_verified"], true);
    assert_eq!(body["role"], "player");
    assert_eq!(body["player_id"], session.player_id.to_string());
    // Codes are stored hashed, never in plaintext.
    let code = app.last_code("ana@example.test");
    let plaintext: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM email_codes WHERE encode(code_hash, 'escape') LIKE '%' || $1 || '%'",
    )
    .bind(code)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(plaintext, 0);
}

#[tokio::test]
async fn wrong_code_burns_after_five_attempts() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let email = "bo@example.test";
    let _ = app
        .post("/api/v1/auth/otp/request")
        .community("demo")
        .json(json!({ "email": email }))
        .send()
        .await
        .expect(StatusCode::ACCEPTED);
    let code = app.last_code(email);
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..5 {
        let body = app
            .post("/api/v1/auth/otp/verify")
            .community("demo")
            .json(json!({ "email": email, "code": wrong }))
            .send()
            .await
            .expect(StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "invalid_credentials");
    }
    let _ = app
        .post("/api/v1/auth/otp/verify")
        .community("demo")
        .json(json!({ "email": email, "code": code }))
        .send()
        .await
        .expect(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn expired_and_superseded_codes_fail() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let email = "cy@example.test";
    let request = async || {
        let _ = app
            .post("/api/v1/auth/otp/request")
            .community("demo")
            .json(json!({ "email": email }))
            .send()
            .await
            .expect(StatusCode::ACCEPTED);
        app.last_code(email)
    };
    let first = request().await;
    let second = request().await;
    if first != second {
        let _ = app
            .post("/api/v1/auth/otp/verify")
            .community("demo")
            .json(json!({ "email": email, "code": first }))
            .send()
            .await
            .expect(StatusCode::UNAUTHORIZED);
    }
    let _ = sqlx::query("UPDATE email_codes SET expires_at = now() - interval '1 second'")
        .execute(&app.db)
        .await
        .unwrap();
    let _ = app
        .post("/api/v1/auth/otp/verify")
        .community("demo")
        .json(json!({ "email": email, "code": second }))
        .send()
        .await
        .expect(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn code_requests_are_rate_limited_per_email_and_ip() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    for _ in 0..5 {
        let _ = app
            .post("/api/v1/auth/otp/request")
            .community("demo")
            .json(json!({ "email": "dee@example.test" }))
            .send()
            .await
            .expect(StatusCode::ACCEPTED);
    }
    let body = app
        .post("/api/v1/auth/otp/request")
        .community("demo")
        .json(json!({ "email": "dee@example.test" }))
        .send()
        .await
        .expect(StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["code"], "rate_limited");

    let app = TestApp::spawn_with(courtpit_server::Config {
        auth_ip_limit_per_hour: 2,
        ..test_config()
    })
    .await;
    let _ = app.community("demo").await;
    for (i, expected) in [
        StatusCode::ACCEPTED,
        StatusCode::ACCEPTED,
        StatusCode::TOO_MANY_REQUESTS,
    ]
    .into_iter()
    .enumerate()
    {
        let _ = app
            .post("/api/v1/auth/otp/request")
            .community("demo")
            .json(json!({ "email": format!("u{i}@example.test") }))
            .send()
            .await
            .expect(expected);
    }
}

#[tokio::test]
async fn invalid_email_is_rejected() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let _ = app
        .post("/api/v1/auth/otp/request")
        .community("demo")
        .json(json!({ "email": "not-an-email" }))
        .send()
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn password_set_and_login() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let session = app.login("eve@example.test", "demo").await;
    let _ = app
        .req(axum::http::Method::PUT, "/api/v1/auth/password")
        .as_(&session)
        .json(json!({ "password": "short" }))
        .send()
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
    let _ = app
        .req(axum::http::Method::PUT, "/api/v1/auth/password")
        .as_(&session)
        .json(json!({ "password": "a long enough password" }))
        .send()
        .await
        .expect(StatusCode::NO_CONTENT);
    let _ = app
        .post("/api/v1/auth/password/login")
        .community("demo")
        .json(json!({ "email": "EVE@example.test", "password": "wrong password!" }))
        .send()
        .await
        .expect(StatusCode::UNAUTHORIZED);
    let body = app
        .post("/api/v1/auth/password/login")
        .community("demo")
        .json(json!({ "email": "EVE@example.test", "password": "a long enough password" }))
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(body["user_id"], session.user_id.to_string());
    assert!(body["token"].is_string());
}

#[tokio::test]
async fn web_clients_get_an_httponly_cookie() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let email = "fay@example.test";
    let _ = app
        .post("/api/v1/auth/otp/request")
        .community("demo")
        .json(json!({ "email": email }))
        .send()
        .await
        .expect(StatusCode::ACCEPTED);
    let res = app
        .post("/api/v1/auth/otp/verify")
        .community("demo")
        .header("x-courtpit-client", "web")
        .json(json!({ "email": email, "code": app.last_code(email) }))
        .send()
        .await;
    let cookie = res.headers["set-cookie"].to_str().unwrap().to_owned();
    let body = res.expect(StatusCode::OK);
    assert!(body["token"].is_null());
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax"));
    let pair = cookie.split(';').next().unwrap();
    let _ = app
        .get("/api/v1/auth/session")
        .community("demo")
        .header("cookie", pair)
        .send()
        .await
        .expect(StatusCode::OK);
}

#[tokio::test]
async fn logout_revokes_the_session() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let session = app.login("gus@example.test", "demo").await;
    let _ = app
        .post("/api/v1/auth/logout")
        .as_(&session)
        .send()
        .await
        .expect(StatusCode::NO_CONTENT);
    let _ = app
        .get("/api/v1/auth/session")
        .as_(&session)
        .send()
        .await
        .expect(StatusCode::UNAUTHORIZED);
    let _ = app
        .post("/api/v1/auth/logout")
        .as_(&session)
        .send()
        .await
        .expect(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn membership_is_per_community_and_bans_apply() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let _ = app.community("other").await;
    let session = app.login("hal@example.test", "demo").await;
    let _ = app
        .get("/api/v1/auth/session")
        .bearer(&session.token)
        .community("other")
        .send()
        .await
        .expect(StatusCode::FORBIDDEN);
    let _ = sqlx::query("UPDATE players SET status = 'banned' WHERE id = $1")
        .bind(session.player_id)
        .execute(&app.db)
        .await
        .unwrap();
    let _ = app
        .get("/api/v1/auth/session")
        .as_(&session)
        .send()
        .await
        .expect(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn expired_sessions_are_rejected() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let session = app.login("ida@example.test", "demo").await;
    let _ = sqlx::query("UPDATE sessions SET expires_at = now() - interval '1 second'")
        .execute(&app.db)
        .await
        .unwrap();
    let _ = app
        .get("/api/v1/auth/session")
        .as_(&session)
        .send()
        .await
        .expect(StatusCode::UNAUTHORIZED);
}
