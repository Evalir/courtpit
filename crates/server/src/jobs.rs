//! Background jobs: a Postgres `jobs` table and an in-process polling loop.
//!
//! Claiming uses `FOR UPDATE SKIP LOCKED` plus a lease (`locked_at`), so several instances can
//! poll concurrently without double-running a job, and a job whose worker died is retried once
//! its lease expires. Failures retry with exponential backoff and record `last_error`.

use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgExecutor, PgPool, types::Json};
use tokio::{sync::watch, task::JoinHandle};
use uuid::Uuid;

use crate::AppState;

/// How long a claimed job is leased before another worker may take it over.
pub const LEASE: Duration = Duration::from_secs(300);
/// Jobs claimed per poll.
const BATCH: i64 = 16;

/// Every job kind with its payload. Serialised as `kind` (text) + `payload` (jsonb).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum Job {
    /// Does nothing (health checks, tests).
    #[expect(
        clippy::empty_enum_variants_with_brackets,
        reason = "keeps the `{}` payload shape stable on the wire and in stored rows"
    )]
    Noop {},
}

impl Job {
    /// Key ensuring at most one pending job of this identity; re-enqueueing reschedules it.
    pub const fn dedupe_key(&self) -> Option<String> {
        match self {
            Self::Noop {} => None,
        }
    }

    fn into_parts(self) -> anyhow::Result<(String, Value)> {
        let mut value = serde_json::to_value(self)?;
        let kind = value
            .get("kind")
            .and_then(Value::as_str)
            .context("job serialised without kind")?
            .to_owned();
        let payload = value
            .get_mut("payload")
            .map_or(Value::Object(Default::default()), Value::take);
        Ok((kind, payload))
    }

    fn from_parts(kind: &str, payload: Value) -> anyhow::Result<Self> {
        serde_json::from_value(serde_json::json!({ "kind": kind, "payload": payload }))
            .with_context(|| format!("unknown or malformed job kind `{kind}`"))
    }
}

/// Enqueues `job` to run at `run_at` (inside the caller's transaction when given one).
/// A job with a dedupe key replaces the waiting (unclaimed) job with the same key (new time
/// and payload); a job that is currently running doesn't count, so it can enqueue its own
/// successor.
pub async fn enqueue<'e>(
    conn: impl PgExecutor<'e>,
    job: Job,
    run_at: DateTime<Utc>,
) -> anyhow::Result<()> {
    let dedupe = job.dedupe_key();
    let (kind, payload) = job.into_parts()?;
    let _ = sqlx::query(
        "INSERT INTO jobs (id, kind, payload, run_at, dedupe_key) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (dedupe_key)
            WHERE completed_at IS NULL AND failed_at IS NULL AND locked_at IS NULL
              AND dedupe_key IS NOT NULL
         DO UPDATE SET run_at = excluded.run_at, payload = excluded.payload",
    )
    .bind(Uuid::now_v7())
    .bind(kind)
    .bind(Json(payload))
    .bind(run_at)
    .bind(dedupe)
    .execute(conn)
    .await
    .context("enqueueing job")?;
    Ok(())
}

/// A job claimed by this worker.
#[derive(Debug, Clone, FromRow)]
pub struct ClaimedJob {
    /// Job id.
    pub id: Uuid,
    /// Job kind (the serialised `Job` tag).
    pub kind: String,
    /// Job payload as stored.
    pub payload: Json<Value>,
    /// Number of times this job has been claimed, including this claim.
    pub attempts: i32,
}

/// Claims up to `limit` jobs due at `now`. Concurrent callers never receive the same job.
pub async fn claim(
    db: &PgPool,
    worker: &str,
    limit: i64,
    now: DateTime<Utc>,
) -> Result<Vec<ClaimedJob>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE jobs SET locked_at = $4, locked_by = $1, attempts = attempts + 1
         WHERE id IN (
             SELECT id FROM jobs
             WHERE completed_at IS NULL AND failed_at IS NULL AND run_at <= $4
               AND (locked_at IS NULL OR locked_at < $4 - make_interval(secs => $3))
             ORDER BY run_at, id
             LIMIT $2
             FOR UPDATE SKIP LOCKED)
         RETURNING id, kind, payload, attempts",
    )
    .bind(worker)
    .bind(limit)
    .bind(LEASE.as_secs_f64())
    .bind(now)
    .fetch_all(db)
    .await
}

/// Runs one job's handler.
#[expect(
    clippy::unused_async,
    reason = "handlers are async by contract; the only job today has nothing to await"
)]
async fn execute(state: &AppState, job: Job) -> anyhow::Result<()> {
    match job {
        Job::Noop {} => {
            let _ = state;
            Ok(())
        }
    }
}

/// Records the outcome of a claimed job: completion, a backed-off retry, or (after a failure
/// with a successor already waiting under the same dedupe key) supersession.
pub async fn finish(
    db: &PgPool,
    id: Uuid,
    now: DateTime<Utc>,
    result: anyhow::Result<()>,
) -> Result<(), sqlx::Error> {
    match result {
        Ok(()) => {
            let _ = sqlx::query(
                "UPDATE jobs SET completed_at = $2, locked_at = NULL, last_error = NULL WHERE id = $1",
            )
            .bind(id)
            .bind(now)
            .execute(db)
            .await?;
        }
        Err(err) => {
            let message = format!("{err:#}");
            tracing::warn!(job = %id, error = %message, "job failed");
            // Retry later, unless a newer job with the same dedupe key is already waiting:
            // then that one supersedes this attempt.
            let retried = sqlx::query(
                "UPDATE jobs SET locked_at = NULL, last_error = $2,
                    run_at = $3 + make_interval(secs => least(power(2, attempts), 3600)),
                    failed_at = CASE WHEN attempts >= max_attempts THEN $3 END
                 WHERE id = $1 AND NOT EXISTS (
                     SELECT 1 FROM jobs w
                     WHERE w.dedupe_key = jobs.dedupe_key AND w.id <> jobs.id
                       AND w.completed_at IS NULL AND w.failed_at IS NULL
                       AND w.locked_at IS NULL)",
            )
            .bind(id)
            .bind(&message)
            .bind(now)
            .execute(db)
            .await?
            .rows_affected();
            if retried == 0 {
                let _ = sqlx::query(
                    "UPDATE jobs SET locked_at = NULL, completed_at = $3,
                        last_error = 'superseded after failure: ' || $2
                     WHERE id = $1",
                )
                .bind(id)
                .bind(&message)
                .bind(now)
                .execute(db)
                .await?;
            }
        }
    }
    Ok(())
}

/// Claims and runs jobs due by the state's clock until none are left; returns how many ran.
pub async fn run_due(state: &AppState, worker: &str) -> anyhow::Result<usize> {
    let mut total = 0;
    loop {
        let batch = claim(&state.db, worker, BATCH, state.clock.now()).await?;
        if batch.is_empty() {
            return Ok(total);
        }
        total += batch.len();
        for claimed in batch {
            let result = match Job::from_parts(&claimed.kind, claimed.payload.0) {
                Ok(job) => {
                    // A panicking handler fails its job instead of killing the loop.
                    let state = state.clone();
                    tokio::spawn(async move { execute(&state, job).await })
                        .await
                        .unwrap_or_else(|err| Err(anyhow::anyhow!("job panicked: {err}")))
                }
                Err(err) => Err(err),
            };
            finish(&state.db, claimed.id, state.clock.now(), result).await?;
        }
    }
}

/// Spawns the polling loop; it stops when `shutdown` flips to `true`.
pub fn spawn_loop(
    state: AppState,
    every: Duration,
    mut shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    let worker = format!("{}-{}", std::process::id(), Uuid::now_v7().simple());
    tokio::spawn(async move {
        tracing::info!(%worker, "job loop started");
        loop {
            match run_due(&state, &worker).await {
                Ok(n) if n > 0 => tracing::debug!(n, "ran jobs"),
                Ok(_) => {}
                Err(err) => tracing::error!(error = ?err, "job loop error"),
            }
            tokio::select! {
                () = tokio::time::sleep(every) => {}
                _ = shutdown.changed() => {}
            }
            if *shutdown.borrow() {
                tracing::info!("job loop stopped");
                return;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_and_payload_roundtrip() {
        let (kind, payload) = Job::Noop {}.into_parts().unwrap();
        assert_eq!(kind, "noop");
        assert_eq!(payload, serde_json::json!({}));
        assert_eq!(Job::from_parts(&kind, payload).unwrap(), Job::Noop {});
        let _ = Job::from_parts("from_the_future", serde_json::json!({})).unwrap_err();
    }
}
