//! A database whose login role may not create roles (`RlsMode::Owner`, as on Caster): the
//! migrations run without their role statements and tenant requests still work.

use axum::http::StatusCode;
use racquetcollective_server::{TenantTx, db};
use serde_json::json;

use crate::common::TestApp;

#[tokio::test]
async fn migrations_keep_their_checksums_and_rerun_as_a_no_op() {
    let app = TestApp::spawn_owner_mode().await;
    assert_eq!(db::RlsMode::detect(&app.db).await.unwrap(), db::RlsMode::Owner);
    let applied: Vec<(i64, Vec<u8>)> =
        sqlx::query_as("SELECT version, checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&app.db)
            .await
            .unwrap();
    let shipped: Vec<(i64, Vec<u8>)> = db::MIGRATOR
        .iter()
        .map(|migration| (migration.version, migration.checksum.to_vec()))
        .collect();
    assert_eq!(applied, shipped);
    assert_eq!(db::migrate(&app.db).await.unwrap(), db::RlsMode::Owner);
}

#[tokio::test]
async fn tenant_transactions_stay_on_the_login_role() {
    let app = TestApp::spawn_owner_mode().await;
    let community = app.community("owned").await.id;
    let mut tx = TenantTx::begin(&app.db, community).await.unwrap();
    let (setting, role): (String, String) =
        sqlx::query_as("SELECT current_setting('app.community_id'), current_user::text")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(setting, community.to_string());
    assert_eq!(role, "racquetcollective_owner");
}

#[tokio::test]
async fn players_sign_in_and_edit_their_profile() {
    let app = TestApp::spawn_owner_mode().await;
    let _ = app.community("owned").await;
    let session = app.login("ann@example.test", "owned").await;
    let body = app.patch_me(&session, json!({ "display_name": "Ann" })).await;
    assert_eq!(body["player"]["display_name"], "Ann");
    let _ = app.get("/api/v1/me").as_(&session).send().await.expect(StatusCode::OK);
}
