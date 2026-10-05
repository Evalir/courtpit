//! The jobs table and loop: claiming, retries, leases, dedupe.

use std::collections::HashSet;

use chrono::{Duration, Utc};
use racquetcollective_server::jobs::{self, Job};
use uuid::Uuid;

use crate::common::TestApp;

async fn insert_raw(app: &TestApp, kind: &str, n: usize) {
    let _ = sqlx::query(
        "INSERT INTO jobs (id, kind, payload)
         SELECT gen_random_uuid(), $1, '{}' FROM generate_series(1, $2)",
    )
    .bind(kind)
    .bind(n as i32)
    .execute(&app.db)
    .await
    .unwrap();
}

#[tokio::test]
async fn due_jobs_run_and_future_jobs_wait() {
    let app = TestApp::spawn().await;
    jobs::enqueue(&app.db, Job::Noop {}, Utc::now()).await.unwrap();
    jobs::enqueue(&app.db, Job::Noop {}, Utc::now() + Duration::hours(1)).await.unwrap();
    assert_eq!(jobs::run_due(&app.state, "t").await.unwrap(), 1);
    let (done, pending): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE completed_at IS NOT NULL),
                count(*) FILTER (WHERE completed_at IS NULL) FROM jobs",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!((done, pending), (1, 1));
    assert_eq!(jobs::run_due(&app.state, "t").await.unwrap(), 0);
}

#[tokio::test]
async fn advancing_the_clock_makes_future_jobs_due() {
    let app = TestApp::spawn().await;
    jobs::enqueue(&app.db, Job::Noop {}, Utc::now() + Duration::days(3)).await.unwrap();
    assert_eq!(jobs::run_due(&app.state, "t").await.unwrap(), 0);
    app.clock.advance(Duration::days(3) + Duration::seconds(1));
    assert_eq!(jobs::run_due(&app.state, "t").await.unwrap(), 1);
}

#[tokio::test]
async fn failures_back_off_record_errors_and_give_up() {
    let app = TestApp::spawn().await;
    insert_raw(&app, "kind_from_a_newer_release", 1).await;
    let _ = sqlx::query("UPDATE jobs SET max_attempts = 3").execute(&app.db).await.unwrap();
    for attempt in 1..=3 {
        assert_eq!(jobs::run_due(&app.state, "t").await.unwrap(), 1);
        let (attempts, err, in_future, failed): (i32, Option<String>, bool, bool) = sqlx::query_as(
            "SELECT attempts, last_error, run_at > now(), failed_at IS NOT NULL FROM jobs",
        )
        .fetch_one(&app.db)
        .await
        .unwrap();
        assert_eq!(attempts, attempt);
        assert!(err.unwrap().contains("kind_from_a_newer_release"));
        assert!(in_future, "backoff pushes run_at forward");
        assert_eq!(failed, attempt == 3);
        let _ = sqlx::query("UPDATE jobs SET run_at = now()").execute(&app.db).await.unwrap();
    }
    assert_eq!(jobs::run_due(&app.state, "t").await.unwrap(), 0, "failed jobs stay failed");
}

/// Two (here: four) pollers hammering the table never claim the same job twice.
#[tokio::test]
async fn concurrent_pollers_never_double_claim() {
    let app = TestApp::spawn().await;
    insert_raw(&app, "noop", 400).await;
    let mut tasks = Vec::new();
    for w in 0..4 {
        let db = app.db.clone();
        tasks.push(tokio::spawn(async move {
            let mut mine = Vec::new();
            loop {
                let batch = jobs::claim(&db, &format!("w{w}"), 7, Utc::now()).await.unwrap();
                if batch.is_empty() {
                    return mine;
                }
                mine.extend(batch.into_iter().map(|j| j.id));
                tokio::task::yield_now().await;
            }
        }));
    }
    let mut all: Vec<Uuid> = Vec::new();
    let mut per_worker = Vec::new();
    for task in tasks {
        let ids = task.await.unwrap();
        per_worker.push(ids.len());
        all.extend(ids);
    }
    let unique: HashSet<_> = all.iter().collect();
    assert_eq!(all.len(), 400, "every job claimed: {per_worker:?}");
    assert_eq!(unique.len(), 400, "no job claimed twice");
}

#[tokio::test]
async fn stale_leases_are_reclaimed() {
    let app = TestApp::spawn().await;
    insert_raw(&app, "noop", 1).await;
    assert_eq!(jobs::claim(&app.db, "crashed", 10, Utc::now()).await.unwrap().len(), 1);
    assert!(jobs::claim(&app.db, "other", 10, Utc::now()).await.unwrap().is_empty(), "lease held");
    let _ = sqlx::query("UPDATE jobs SET locked_at = now() - interval '1 hour'")
        .execute(&app.db)
        .await
        .unwrap();
    let again = jobs::claim(&app.db, "other", 10, Utc::now()).await.unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].attempts, 2);
}

/// A batch comes back in due order whatever plan Postgres picks: under a hash join the
/// `UPDATE ... RETURNING` yields rows in table order, which once ran a league's season finish
/// before the auto-confirm it was waiting on.
#[tokio::test]
async fn a_claimed_batch_is_in_due_order() {
    let app = TestApp::spawn().await;
    let now = Utc::now();
    let (due_last, due_first) = (Uuid::now_v7(), Uuid::now_v7());
    // Inserted first but due last, so table order and due order disagree.
    for (id, due) in
        [(due_last, now - Duration::minutes(1)), (due_first, now - Duration::minutes(2))]
    {
        let _ = sqlx::query("INSERT INTO jobs (id, kind, run_at) VALUES ($1, 'noop', $2)")
            .bind(id)
            .bind(due)
            .execute(&app.db)
            .await
            .unwrap();
    }
    let mut tx = app.db.begin().await.unwrap();
    for setting in ["enable_nestloop", "enable_mergejoin"] {
        let _ = sqlx::query(&format!("SET LOCAL {setting} = off")).execute(&mut *tx).await.unwrap();
    }
    let batch = jobs::claim(&mut *tx, "w", 10, now).await.unwrap();
    tx.rollback().await.unwrap();
    let order: Vec<Uuid> = batch.iter().map(|job| job.id).collect();
    assert_eq!(order, [due_first, due_last]);
}

#[tokio::test]
async fn loop_runs_jobs_and_stops_on_shutdown() {
    let app = TestApp::spawn().await;
    jobs::enqueue(&app.db, Job::Noop {}, Utc::now()).await.unwrap();
    let (tx, rx) = tokio::sync::watch::channel(false);
    let handle = jobs::spawn_loop(app.state.clone(), std::time::Duration::from_millis(20), rx);
    for _ in 0..100 {
        let done: i64 =
            sqlx::query_scalar("SELECT count(*) FROM jobs WHERE completed_at IS NOT NULL")
                .fetch_one(&app.db)
                .await
                .unwrap();
        if done == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    tx.send(true).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), handle).await.unwrap().unwrap();
    let done: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE completed_at IS NOT NULL")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(done, 1);
}

/// Dedupe keys only bind *waiting* jobs: a running job can have a successor queued under its
/// key, and if the running one then fails, the waiting successor supersedes its retry.
#[tokio::test]
async fn dedupe_applies_to_waiting_jobs_and_failed_runs_yield_to_successors() {
    let app = TestApp::spawn().await;
    let insert = |kind: &'static str| {
        sqlx::query("INSERT INTO jobs (id, kind, dedupe_key) VALUES (gen_random_uuid(), $1, 'k')")
            .bind(kind)
            .execute(&app.db)
    };
    let _ = insert("noop").await.unwrap();
    assert!(insert("noop").await.is_err(), "two waiting jobs with one key");
    let running = jobs::claim(&app.db, "w", 10, Utc::now()).await.unwrap();
    assert_eq!(running.len(), 1);
    let _ = insert("noop").await.unwrap();

    // The running job fails while its successor waits: no retry, it is superseded.
    jobs::finish(&app.db, running[0].id, Utc::now(), Err(anyhow::anyhow!("boom"))).await.unwrap();
    let (completed, err): (bool, Option<String>) =
        sqlx::query_as("SELECT completed_at IS NOT NULL, last_error FROM jobs WHERE id = $1")
            .bind(running[0].id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(completed);
    assert_eq!(err.as_deref(), Some("superseded after failure: boom"));
    assert_eq!(jobs::run_due(&app.state, "t").await.unwrap(), 1, "successor ran");

    // Without a waiting successor a failure is retried as usual.
    let _ = insert("noop").await.unwrap();
    let running = jobs::claim(&app.db, "w", 10, Utc::now()).await.unwrap();
    jobs::finish(&app.db, running[0].id, Utc::now(), Err(anyhow::anyhow!("boom"))).await.unwrap();
    let (completed, locked): (bool, bool) = sqlx::query_as(
        "SELECT completed_at IS NOT NULL, locked_at IS NOT NULL FROM jobs WHERE id = $1",
    )
    .bind(running[0].id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(!completed && !locked);
}

const fn budget() -> std::time::Duration {
    std::time::Duration::from_secs(60)
}

#[tokio::test]
async fn tick_returns_promptly_when_nothing_is_due() {
    let app = TestApp::spawn().await;
    let summary =
        tokio::time::timeout(std::time::Duration::from_secs(5), jobs::tick(&app.state, budget()))
            .await
            .unwrap()
            .unwrap();
    assert_eq!(summary, jobs::TickSummary { ran: 0, budget_spent: false });
}

#[tokio::test]
async fn tick_runs_due_jobs_and_leaves_future_ones() {
    let app = TestApp::spawn().await;
    for _ in 0..3 {
        jobs::enqueue(&app.db, Job::Noop {}, Utc::now()).await.unwrap();
    }
    jobs::enqueue(&app.db, Job::Noop {}, Utc::now() + Duration::hours(1)).await.unwrap();
    let summary = jobs::tick(&app.state, budget()).await.unwrap();
    assert_eq!((summary.ran, summary.budget_spent), (3, false));
    let (done, waiting, attempts): (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE completed_at IS NOT NULL),
                count(*) FILTER (WHERE completed_at IS NULL AND locked_at IS NULL),
                coalesce(sum(attempts) FILTER (WHERE completed_at IS NULL), 0)::bigint FROM jobs",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!((done, waiting, attempts), (3, 1, 0), "the future job is untouched");
}

#[tokio::test]
async fn a_spent_budget_claims_nothing() {
    let app = TestApp::spawn().await;
    insert_raw(&app, "noop", 3).await;
    let summary = jobs::tick(&app.state, std::time::Duration::ZERO).await.unwrap();
    assert_eq!((summary.ran, summary.budget_spent), (0, true));
    let (locked, attempts): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE locked_at IS NOT NULL), sum(attempts)::bigint FROM jobs",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!((locked, attempts), (0, 0), "no lease, no attempt burned");
    let claimable = jobs::claim(&app.db, "serve", 10, Utc::now()).await.unwrap();
    assert_eq!(claimable.len(), 3, "everything is still claimable right away");
}

/// Whatever the budget cuts off, every job is either done or untouched: none sits leased.
#[tokio::test]
async fn running_out_of_budget_midway_strands_no_lease() {
    let app = TestApp::spawn().await;
    insert_raw(&app, "noop", 400).await;
    let summary = jobs::tick(&app.state, std::time::Duration::from_millis(30)).await.unwrap();
    let (done, idle, locked, idle_attempts): (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE completed_at IS NOT NULL),
                count(*) FILTER (WHERE completed_at IS NULL AND locked_at IS NULL),
                count(*) FILTER (WHERE locked_at IS NOT NULL AND completed_at IS NULL),
                coalesce(sum(attempts) FILTER (WHERE completed_at IS NULL), 0)::bigint
         FROM jobs",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(done as usize, summary.ran);
    assert_eq!(done + idle, 400);
    assert_eq!((locked, idle_attempts), (0, 0));
}

#[tokio::test]
async fn tick_skips_jobs_leased_to_another_worker_and_survives_failures() {
    let app = TestApp::spawn().await;
    insert_raw(&app, "noop", 1).await;
    assert_eq!(jobs::claim(&app.db, "serve", 10, Utc::now()).await.unwrap().len(), 1);
    assert_eq!(jobs::tick(&app.state, budget()).await.unwrap().ran, 0);

    // A failing job is recorded and retried later; the tick itself still succeeds.
    insert_raw(&app, "kind_from_a_newer_release", 1).await;
    assert_eq!(jobs::tick(&app.state, budget()).await.unwrap().ran, 1);
    let (err, in_future): (Option<String>, bool) =
        sqlx::query_as("SELECT last_error, run_at > now() FROM jobs WHERE kind <> 'noop'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(err.unwrap().contains("kind_from_a_newer_release"));
    assert!(in_future);
}
