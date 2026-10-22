//! Smoke tests for migration 0001, the RLS runtime role and forced RLS.

use racquetcollective_server::{GlobalTx, db::RlsMode};

use crate::{common::TestApp, tenancy::add_player};

#[tokio::test]
async fn app_role_cannot_bypass_rls() {
    let app = TestApp::spawn().await;
    let (superuser, bypass): (bool, bool) = sqlx::query_as(
        "SELECT rolsuper, rolbypassrls FROM pg_roles WHERE rolname = 'courtpit_app'",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(!superuser);
    assert!(!bypass);
}

#[tokio::test]
async fn tenant_tables_have_rls_enabled() {
    let app = TestApp::spawn().await;
    let rls: bool =
        sqlx::query_scalar("SELECT relrowsecurity FROM pg_class WHERE relname = 'players'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(rls);
}

#[tokio::test]
async fn unscoped_app_role_sees_no_players() {
    let app = TestApp::spawn().await;
    let community = uuid::Uuid::now_v7();
    let user = uuid::Uuid::now_v7();
    let _ = sqlx::query("INSERT INTO communities (id, slug, name) VALUES ($1, 'smoke', 'Smoke')")
        .bind(community)
        .execute(&app.db)
        .await
        .unwrap();
    let _ = sqlx::query("INSERT INTO users (id, email) VALUES ($1, 'a@example.test')")
        .bind(user)
        .execute(&app.db)
        .await
        .unwrap();
    let _ = sqlx::query(
        "INSERT INTO players (id, community_id, user_id, display_name) VALUES ($1, $2, $3, 'A')",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(community)
    .bind(user)
    .execute(&app.db)
    .await
    .unwrap();

    // As the role tenant transactions run as: `courtpit_app`, or the owner that forced RLS binds.
    let mut tx = app.state.db.begin().await.unwrap();
    if RlsMode::detect(&app.state.db).await.unwrap() == RlsMode::AppRole {
        let _ = sqlx::query("SET LOCAL ROLE courtpit_app").execute(&mut *tx).await.unwrap();
    }
    let visible: i64 =
        sqlx::query_scalar("SELECT count(*) FROM players").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(visible, 0, "no app.community_id set => no rows");
}

#[tokio::test]
async fn global_transactions_see_every_community() {
    let app = TestApp::spawn().await;
    let alpha = app.community("alpha").await.id;
    let beta = app.community("beta").await.id;
    let _ = add_player(&app, alpha, "a@example.test").await;
    let _ = add_player(&app, beta, "b@example.test").await;
    let mut tx = GlobalTx::begin(&app.state.db).await.unwrap();
    let visible: i64 =
        sqlx::query_scalar("SELECT count(*) FROM players").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(visible, 2);
}

#[tokio::test]
async fn tenant_tables_force_row_level_security() {
    let app = TestApp::spawn().await;
    let unforced: Vec<String> = sqlx::query_scalar(
        "SELECT relname::text FROM pg_class
         WHERE relrowsecurity AND NOT relforcerowsecurity AND relkind = 'r'
         ORDER BY relname",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(unforced, Vec::<String>::new());
}
