//! Behaviour that must hold when the pool sits behind a transaction pooler (pgbouncer, Neon's
//! `-pooler` endpoint): per-transaction state stays per-transaction while a few server
//! connections serve many clients. These also pass on a direct connection; the `test-pooled` CI
//! job runs the whole suite through pgbouncer.

use std::collections::HashSet;

use racquetcollective_server::TenantTx;
use uuid::Uuid;

use crate::{common::TestApp, tenancy::add_player};

/// Concurrent clients per test (the test pool has 5 connections, so they queue and reuse).
const TASKS: usize = 24;
/// Transactions per client.
const ROUNDS: usize = 25;

/// Many concurrent scoped transactions for two communities share a handful of server
/// connections; each must only ever see its own community's rows and setting.
#[tokio::test]
async fn concurrent_tenant_transactions_never_cross_communities() {
    let app = TestApp::spawn().await;
    let mut communities = Vec::new();
    for slug in ["alpha", "beta"] {
        let id = app.community(slug).await.id;
        let mut players = HashSet::new();
        for idx in 0..3 {
            let email = format!("{slug}{idx}@example.test");
            let _ = players.insert(add_player(&app, id, &email).await);
        }
        communities.push((id, players));
    }

    let mut tasks = Vec::new();
    for task in 0..TASKS {
        let db = app.db.clone();
        let (community, players) = communities[task % 2].clone();
        tasks.push(tokio::spawn(async move {
            for round in 0..ROUNDS {
                let mut tx = TenantTx::begin(&db, community).await.unwrap();
                // Let other clients' transactions interleave on the shared connections.
                tokio::task::yield_now().await;
                let seen: Vec<Uuid> =
                    sqlx::query_scalar("SELECT id FROM players").fetch_all(&mut *tx).await.unwrap();
                assert_eq!(seen.iter().copied().collect::<HashSet<_>>(), players);
                tokio::task::yield_now().await;
                let (setting, role): (String, String) = sqlx::query_as(
                    "SELECT current_setting('app.community_id'), current_user::text",
                )
                .fetch_one(&mut *tx)
                .await
                .unwrap();
                assert_eq!(setting, community.to_string());
                assert_eq!(role, "courtpit_app");
                // Alternate commit and rollback; both must release the settings.
                if round % 2 == 0 {
                    tx.commit().await.unwrap();
                }
            }
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
}

/// `SET LOCAL` and `set_config(.., true)` end with their transaction: whoever gets the
/// connection next (any client of the pooler) starts from the login role with no tenant.
#[tokio::test]
async fn transaction_settings_do_not_leak_to_the_next_transaction() {
    let app = TestApp::spawn().await;
    let community = app.community("leaky").await.id;

    let mut tasks = Vec::new();
    for task in 0..TASKS {
        let db = app.db.clone();
        tasks.push(tokio::spawn(async move {
            for round in 0..ROUNDS {
                let mut tx = TenantTx::begin(&db, community).await.unwrap();
                let _ = sqlx::query("SET LOCAL statement_timeout = '7s'")
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                if (task + round) % 2 == 0 {
                    tx.commit().await.unwrap();
                } else {
                    drop(tx);
                }
                // An unscoped statement on whatever connection comes next.
                let (tenant, role, timeout): (Option<String>, String, String) = sqlx::query_as(
                    "SELECT nullif(current_setting('app.community_id', true), ''),
                            current_user::text, current_setting('statement_timeout')",
                )
                .fetch_one(&db)
                .await
                .unwrap();
                assert_eq!(tenant, None, "tenant leaked into the next transaction");
                assert_ne!(role, "courtpit_app", "role leaked into the next transaction");
                assert_ne!(timeout, "7s", "SET LOCAL leaked into the next transaction");
            }
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
}
