//! Apple / Google sign-in against an in-process fake JWKS endpoint.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{Json, Router, http::StatusCode, routing::get};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::{Value, json};

use crate::common::{TestApp, test_config};

const KEY_PEM: &[u8] = include_bytes!("fixtures/oidc_test_key.pem");
const JWKS: &str = include_str!("fixtures/oidc_test_jwks.json");
const GOOGLE_AUD: &str = "test-google-client";
const APPLE_AUD: &str = "app.racquetcollective.demo";

struct Oidc {
    app: TestApp,
    fetches: Arc<AtomicUsize>,
}

async fn spawn() -> Oidc {
    let fetches = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&fetches);
    let jwks: Value = serde_json::from_str(JWKS).unwrap();
    let router = Router::new().route(
        "/jwks",
        get(move || {
            let _ = counter.fetch_add(1, Ordering::SeqCst);
            let jwks = jwks.clone();
            async move { Json(jwks) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(tokio::spawn(async move { axum::serve(listener, router).await }));
    let url = format!("http://{addr}/jwks");
    let app = TestApp::spawn_with(racquetcollective_server::Config {
        google_client_ids: vec![GOOGLE_AUD.to_owned()],
        apple_client_ids: vec![APPLE_AUD.to_owned()],
        google_jwks_url: url.clone(),
        apple_jwks_url: url,
        ..test_config()
    })
    .await;
    let _ = app.community("demo").await;
    Oidc { app, fetches }
}

fn token(claims: &Value) -> String {
    token_with_kid(claims, "test-key-1")
}

fn token_with_kid(claims: &Value, kid: &str) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(kid.to_owned());
    encode(&header, claims, &EncodingKey::from_rsa_pem(KEY_PEM).unwrap()).unwrap()
}

fn exp() -> i64 {
    chrono::Utc::now().timestamp() + 600
}

fn google(sub: &str, email: &str, verified: bool) -> String {
    token(&json!({
        "iss": "https://accounts.google.com", "aud": GOOGLE_AUD, "sub": sub,
        "email": email, "email_verified": verified, "exp": exp(), "iat": exp() - 600,
    }))
}

async fn sign_in(oidc: &Oidc, provider: &str, id_token: &str) -> crate::common::Res {
    oidc.app
        .post(&format!("/api/v1/auth/oidc/{provider}"))
        .community("demo")
        .json(json!({ "id_token": id_token }))
        .send()
        .await
}

#[tokio::test]
async fn google_sign_in_creates_then_reuses_user() {
    let oidc = spawn().await;
    let first = sign_in(&oidc, "google", &google("g-1", "ana@example.test", true))
        .await
        .expect(StatusCode::OK);
    let second = sign_in(&oidc, "google", &google("g-1", "ana@example.test", true))
        .await
        .expect(StatusCode::OK);
    assert_eq!(first["user_id"], second["user_id"]);
    assert_eq!(first["player_id"], second["player_id"]);
    let token = first["token"].as_str().unwrap();
    let session = oidc
        .app
        .get("/api/v1/auth/session")
        .bearer(token)
        .community("demo")
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(session["email_verified"], true);
    assert_eq!(oidc.fetches.load(Ordering::SeqCst), 1, "JWKS is cached");
}

#[tokio::test]
async fn verified_email_links_to_existing_account() {
    let oidc = spawn().await;
    let otp = oidc.app.login("bo@example.test", "demo").await;
    let body = sign_in(&oidc, "google", &google("g-2", "BO@example.test", true))
        .await
        .expect(StatusCode::OK);
    assert_eq!(body["user_id"], otp.user_id.to_string());
}

#[tokio::test]
async fn unverified_email_never_takes_over_an_account() {
    let oidc = spawn().await;
    let _ = oidc.app.login("cy@example.test", "demo").await;
    let _ = sign_in(&oidc, "google", &google("g-3", "cy@example.test", false))
        .await
        .expect(StatusCode::CONFLICT);
}

#[tokio::test]
async fn apple_string_email_verified_and_nonce() {
    let oidc = spawn().await;
    let id_token = token(&json!({
        "iss": "https://appleid.apple.com", "aud": APPLE_AUD, "sub": "apple-1",
        "email": "dee@privaterelay.appleid.com", "email_verified": "true",
        "nonce": "n0nce", "exp": exp(),
    }));
    let _ = oidc
        .app
        .post("/api/v1/auth/oidc/apple")
        .community("demo")
        .json(json!({ "id_token": id_token, "nonce": "other" }))
        .send()
        .await
        .expect(StatusCode::UNAUTHORIZED);
    let _ = oidc
        .app
        .post("/api/v1/auth/oidc/apple")
        .community("demo")
        .json(json!({ "id_token": id_token, "nonce": "n0nce" }))
        .send()
        .await
        .expect(StatusCode::OK);
}

#[tokio::test]
async fn bad_tokens_are_rejected() {
    let oidc = spawn().await;
    let base = json!({
        "iss": "https://accounts.google.com", "aud": GOOGLE_AUD, "sub": "g-9",
        "email": "x@example.test", "email_verified": true, "exp": exp(),
    });
    let mut wrong_aud = base.clone();
    wrong_aud["aud"] = json!("someone-else");
    let mut wrong_iss = base.clone();
    wrong_iss["iss"] = json!("https://evil.example");
    let mut expired = base.clone();
    expired["exp"] = json!(chrono::Utc::now().timestamp() - 3600);
    let good = token(&base);
    let (head, rest) = good.split_once('.').unwrap();
    let (_, sig) = rest.split_once('.').unwrap();
    let forged_payload = {
        use base64::Engine;
        let mut forged = base.clone();
        forged["sub"] = json!("g-admin");
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(forged.to_string())
    };
    let tampered = format!("{head}.{forged_payload}.{sig}");
    for bad in [token(&wrong_aud), token(&wrong_iss), token(&expired), tampered, "garbage".into()] {
        let body = sign_in(&oidc, "google", &bad).await.expect(StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "invalid_credentials");
    }
    // Unknown kid triggers exactly one refetch, then fails.
    let before = oidc.fetches.load(Ordering::SeqCst);
    let _ = sign_in(&oidc, "google", &token_with_kid(&base, "rotated-away"))
        .await
        .expect(StatusCode::UNAUTHORIZED);
    assert_eq!(oidc.fetches.load(Ordering::SeqCst), before + 1);
}

#[tokio::test]
async fn link_identity_to_signed_in_user() {
    let oidc = spawn().await;
    let session = oidc.app.login("eve@example.test", "demo").await;
    let id_token = google("g-eve", "eve.other@gmail.example", true);
    let _ = oidc
        .app
        .post("/api/v1/auth/oidc/google/link")
        .as_(&session)
        .json(json!({ "id_token": id_token }))
        .send()
        .await
        .expect(StatusCode::NO_CONTENT);
    let body = sign_in(&oidc, "google", &id_token).await.expect(StatusCode::OK);
    assert_eq!(body["user_id"], session.user_id.to_string());
    // Another user cannot claim the same identity.
    let other = oidc.app.login("fay@example.test", "demo").await;
    let _ = oidc
        .app
        .post("/api/v1/auth/oidc/google/link")
        .as_(&other)
        .json(json!({ "id_token": id_token }))
        .send()
        .await
        .expect(StatusCode::CONFLICT);
}

#[tokio::test]
async fn unknown_or_disabled_provider() {
    let oidc = spawn().await;
    let _ = sign_in(&oidc, "facebook", "x").await.expect(StatusCode::NOT_FOUND);
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let _ = app
        .post("/api/v1/auth/oidc/google")
        .community("demo")
        .json(json!({ "id_token": "x" }))
        .send()
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY);
}
