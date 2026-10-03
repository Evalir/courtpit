//! Tenant resolution and RLS isolation.

use axum::http::StatusCode;
use courtpit_server::TenantTx;
use uuid::Uuid;

use crate::common::TestApp;

#[tokio::test]
async fn tenant_resolves_from_header() {
    let app = TestApp::spawn().await;
    let _ = app.community("demo").await;
    let body = app
        .get("/api/v1/tenant")
        .community("demo")
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(body["slug"], "demo");
    assert_eq!(body["branding"]["display_name"], "demo club");
    assert_eq!(body["branding"]["colors"], serde_json::json!({}));
    assert_eq!(body["branding"]["logo_url"], serde_json::Value::Null);
}

#[tokio::test]
async fn tenant_resolves_from_host() {
    let app = TestApp::spawn().await;
    let _ = app.community("madrid").await;
    let body = app
        .get("/api/v1/tenant")
        .header("host", "madrid.courtpit.app")
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(body["slug"], "madrid");
}

#[tokio::test]
async fn tenant_resolves_from_custom_domain() {
    let app = TestApp::spawn().await;
    let community = app.community("club").await;
    let _ =
        sqlx::query("UPDATE communities SET custom_domain = 'tennis.example.org' WHERE id = $1")
            .bind(community.id)
            .execute(&app.db)
            .await
            .unwrap();
    let body = app
        .get("/api/v1/tenant")
        .header("host", "tennis.example.org")
        .send()
        .await
        .expect(StatusCode::OK);
    assert_eq!(body["slug"], "club");
}

#[tokio::test]
async fn unknown_and_missing_tenant() {
    let app = TestApp::spawn().await;
    let body = app
        .get("/api/v1/tenant")
        .community("nope")
        .send()
        .await
        .expect(StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
    let body = app
        .get("/api/v1/tenant")
        .send()
        .await
        .expect(StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "bad_request");
}

#[tokio::test]
async fn create_community_with_owner() {
    let app = TestApp::spawn().await;
    let created = app
        .community_with_owner("owned", Some("Owner@Example.test"))
        .await;
    let player = created.owner_player_id.unwrap();
    let mut tx = TenantTx::begin(&app.db, created.community.id)
        .await
        .unwrap();
    let role: String = sqlx::query_scalar("SELECT role::text FROM players WHERE id = $1")
        .bind(player)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(role, "owner");
}

async fn add_player(app: &TestApp, community: Uuid, email: &str) -> Uuid {
    let user: Uuid =
        sqlx::query_scalar("INSERT INTO users (id, email) VALUES ($1, $2) RETURNING id")
            .bind(Uuid::now_v7())
            .bind(email)
            .fetch_one(&app.db)
            .await
            .unwrap();
    let mut tx = TenantTx::begin(&app.db, community).await.unwrap();
    let id = sqlx::query_scalar(
        "INSERT INTO players (id, community_id, user_id, display_name) VALUES ($1, $2, $3, $4)
         RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(community)
    .bind(user)
    .bind(email)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    id
}

/// The safety net: a scoped query with no WHERE clause still only sees its own community, and
/// cannot write or modify another community's rows.
#[tokio::test]
async fn rls_isolates_communities() {
    let app = TestApp::spawn().await;
    let alpha = app.community("alpha").await.id;
    let beta = app.community("beta").await.id;
    let alpha_player = add_player(&app, alpha, "a@example.test").await;
    let beta_player = add_player(&app, beta, "b@example.test").await;

    let mut tx = TenantTx::begin(&app.db, alpha).await.unwrap();
    let visible: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM players")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert_eq!(visible, vec![alpha_player]);

    let updated = sqlx::query("UPDATE players SET display_name = 'pwned' WHERE id = $1")
        .bind(beta_player)
        .execute(&mut *tx)
        .await
        .unwrap()
        .rows_affected();
    assert_eq!(updated, 0, "cannot touch another community's row");

    let user: Uuid = sqlx::query_scalar("SELECT user_id FROM players WHERE id = $1")
        .bind(alpha_player)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let insert = sqlx::query(
        "INSERT INTO players (id, community_id, user_id, display_name) VALUES ($1, $2, $3, 'x')",
    )
    .bind(Uuid::now_v7())
    .bind(beta)
    .bind(user)
    .execute(&mut *tx)
    .await;
    let err = insert.unwrap_err().to_string();
    assert!(err.contains("row-level security"), "{err}");
    drop(tx);

    let mut tx = TenantTx::begin(&app.db, beta).await.unwrap();
    let name: String = sqlx::query_scalar("SELECT display_name FROM players WHERE id = $1")
        .bind(beta_player)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(name, "b@example.test");
}
