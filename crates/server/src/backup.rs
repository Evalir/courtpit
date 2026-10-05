//! Nightly database backups: `pg_dump` streamed to S3-compatible object storage.
//!
//! The `backup_database` job (dedupe key `backup`) dumps the database, uploads it, prunes dumps
//! past retention and enqueues itself for the next `BACKUP_HOUR_UTC`. `serve` and `tick` call
//! [`ensure_scheduled`] on start so the first job exists. The dump source and the object store
//! are traits ([`DumpSource`], [`ObjectStore`]); production uses [`pg_dump::PgDump`] and
//! [`s3::S3Store`], tests substitute in-memory fakes.

pub mod pg_dump;
pub mod s3;

use std::{fmt::Debug, pin::Pin, sync::Arc};

use anyhow::Context;
use chrono::{DateTime, Duration, NaiveDateTime, NaiveTime, Utc};
use tokio::io::AsyncRead;

use crate::{
    AppState, Config,
    jobs::{self, Job},
};

/// Timestamp format inside object keys (`20261004T030000Z`); always 16 characters.
const STAMP: &str = "%Y%m%dT%H%M%SZ";
const STAMP_LEN: usize = 16;

/// A stream of dump bytes. A source that fails midway (for example `pg_dump` exiting non-zero)
/// must report it as a read error at the end, so the upload is aborted rather than completed.
pub type DumpReader = Box<dyn AsyncRead + Send + Unpin>;

/// Boxed future returned by [`ObjectStore`] methods (keeps the trait object-safe).
pub type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + Send + 'a>>;

/// Where a database dump comes from.
pub trait DumpSource: Send + Sync + Debug {
    /// Starts a dump and returns its bytes as a stream.
    fn start(&self) -> anyhow::Result<DumpReader>;
}

/// Object storage (S3-compatible).
pub trait ObjectStore: Send + Sync + Debug {
    /// Streams `body` to `key` without knowing its length; returns the bytes stored. On any
    /// failure nothing is left behind (a partial multipart upload is aborted).
    fn put_stream<'a>(&'a self, key: &'a str, body: DumpReader) -> StoreFuture<'a, u64>;
    /// All keys starting with `prefix`.
    fn list<'a>(&'a self, prefix: &'a str) -> StoreFuture<'a, Vec<String>>;
    /// Deletes one object.
    fn delete<'a>(&'a self, key: &'a str) -> StoreFuture<'a, ()>;
}

/// The configured dump source and object store; present on [`AppState`] when backups are on.
#[derive(Debug, Clone)]
pub struct Backup {
    /// Produces the dump.
    pub dump: Arc<dyn DumpSource>,
    /// Receives it.
    pub store: Arc<dyn ObjectStore>,
}

impl Backup {
    /// Builds the production backup from configuration: `None` when no `BACKUP_S3_*` setting is
    /// given, an error naming what is missing when only some are.
    pub fn from_config(config: &Config) -> anyhow::Result<Option<Self>> {
        let cfg = &config.backup;
        let endpoint = nonblank(cfg.s3_endpoint.as_deref());
        let bucket = nonblank(cfg.s3_bucket.as_deref());
        let access_key = nonblank(cfg.s3_access_key.as_deref());
        let secret_key = nonblank(cfg.s3_secret_key.as_deref());
        let (Some(endpoint), Some(bucket), Some(access_key), Some(secret_key)) =
            (endpoint, bucket, access_key, secret_key)
        else {
            let missing: Vec<&str> = [
                ("BACKUP_S3_ENDPOINT", endpoint),
                ("BACKUP_S3_BUCKET", bucket),
                ("BACKUP_S3_ACCESS_KEY", access_key),
                ("BACKUP_S3_SECRET_KEY", secret_key),
            ]
            .into_iter()
            .filter_map(|(name, value)| value.is_none().then_some(name))
            .collect();
            if missing.len() == 4 {
                return Ok(None);
            }
            anyhow::bail!(
                "incomplete backup configuration: {} not set (set all of BACKUP_S3_ENDPOINT, \
                 BACKUP_S3_BUCKET, BACKUP_S3_ACCESS_KEY and BACKUP_S3_SECRET_KEY to enable \
                 backups, or none of them to disable them)",
                missing.join(", ")
            );
        };
        let database_url =
            nonblank(cfg.dump_database_url.as_deref()).unwrap_or(config.db.session_url());
        Ok(Some(Self {
            dump: Arc::new(pg_dump::PgDump::new(database_url)?),
            store: Arc::new(s3::S3Store::new(
                endpoint,
                bucket,
                &cfg.s3_region,
                access_key,
                secret_key,
            )?),
        }))
    }
}

fn nonblank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

/// A key prefix that is empty or ends with `/`.
pub fn normalized_prefix(prefix: &str) -> String {
    let prefix = prefix.trim();
    if prefix.is_empty() || prefix.ends_with('/') {
        prefix.to_owned()
    } else {
        format!("{prefix}/")
    }
}

/// The object key of the dump taken at `at`:
/// `{prefix}{YYYY}/{MM}/racquetcollective-{YYYYMMDDTHHMMSSZ}.dump`. `prefix` is empty or ends with
/// `/` (see [`normalized_prefix`]).
pub fn object_key(prefix: &str, at: DateTime<Utc>) -> String {
    format!("{prefix}{}/racquetcollective-{}.dump", at.format("%Y/%m"), at.format(STAMP))
}

/// The time encoded in `key`, if it is exactly a key [`object_key`] would produce under
/// `prefix`. Anything else (other names, other layouts) is not ours: never pruned.
fn parse_key(prefix: &str, key: &str) -> Option<DateTime<Utc>> {
    let stem = key.strip_prefix(prefix)?.strip_suffix(".dump")?;
    let stamp = stem.get(stem.len().checked_sub(STAMP_LEN)?..)?;
    let at = NaiveDateTime::parse_from_str(stamp, STAMP).ok()?.and_utc();
    (object_key(prefix, at) == key).then_some(at)
}

/// The next instant strictly after `now` whose time of day is `hour`:00 UTC.
pub fn next_run_at(now: DateTime<Utc>, hour: u32) -> DateTime<Utc> {
    let at = NaiveTime::from_hms_opt(hour, 0, 0).unwrap_or(NaiveTime::MIN);
    let today = now.date_naive().and_time(at).and_utc();
    if today > now { today } else { today + Duration::days(1) }
}

/// Makes sure a backup job is pending, if backups are configured. Inserts only when none
/// exists, so calling it on every start (and hourly from `tick`) never moves a scheduled run.
pub async fn ensure_scheduled(state: &AppState) -> anyhow::Result<()> {
    if state.backup.is_none() {
        return Ok(());
    }
    let run_at = next_run_at(state.clock.now(), state.config.backup.hour_utc);
    if jobs::enqueue_if_absent(&state.db, Job::BackupDatabase {}, run_at).await? {
        tracing::info!(%run_at, "scheduled the database backup");
    }
    Ok(())
}

/// Deletes dumps under `prefix` taken before `cutoff`; returns how many. Only keys that parse
/// as ours are considered, so foreign objects sharing the bucket are never touched.
pub async fn prune(
    store: &dyn ObjectStore,
    prefix: &str,
    cutoff: DateTime<Utc>,
) -> anyhow::Result<usize> {
    let mut deleted = 0;
    for key in store.list(prefix).await.context("listing backups")? {
        if parse_key(prefix, &key).is_some_and(|taken| taken < cutoff) {
            store.delete(&key).await.with_context(|| format!("deleting {key}"))?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// The `backup_database` job: dump, upload, prune, reschedule. A failure before the upload
/// completes fails the job (retried with backoff); a failed prune only logs, so the next run
/// retries it instead of this one re-uploading a second dump.
pub(crate) async fn run(state: &AppState) -> anyhow::Result<()> {
    let Some(backup) = &state.backup else {
        tracing::info!("database backups are not configured; nothing to do");
        return Ok(());
    };
    let config = &state.config.backup;
    let prefix = normalized_prefix(&config.s3_prefix);
    let now = state.clock.now();
    let key = object_key(&prefix, now);
    let bytes = backup
        .store
        .put_stream(&key, backup.dump.start()?)
        .await
        .with_context(|| format!("backing up to {key}"))?;
    tracing::info!(%key, bytes, "database backup uploaded");

    prune_expired(backup.store.as_ref(), &prefix, now, config.retention_days).await;
    let next = next_run_at(now, config.hour_utc);
    jobs::enqueue(&state.db, Job::BackupDatabase {}, next).await
}

/// Prunes dumps older than `retention_days` before `now`, logging instead of failing.
async fn prune_expired(
    store: &dyn ObjectStore,
    prefix: &str,
    now: DateTime<Utc>,
    retention_days: i64,
) {
    // An absurd retention keeps everything.
    let Some(cutoff) =
        Duration::try_days(retention_days).and_then(|keep| now.checked_sub_signed(keep))
    else {
        return;
    };
    match prune(store, prefix, cutoff).await {
        Ok(0) => {}
        Ok(deleted) => tracing::info!(deleted, "pruned old database backups"),
        Err(err) => tracing::warn!(error = ?err, "pruning old database backups failed"),
    }
}

/// `text` cut to at most `max` characters, with an ellipsis when it was longer.
pub(crate) fn truncate(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(day: u32, hour: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, hour, min, 5).unwrap()
    }

    #[test]
    fn keys_have_a_fixed_layout_and_roundtrip() {
        let taken = at(4, 3, 0);
        let key = object_key("racquetcollective/", taken);
        assert_eq!(key, "racquetcollective/2026/10/racquetcollective-20261004T030005Z.dump");
        assert_eq!(parse_key("racquetcollective/", &key), Some(taken));
        assert_eq!(object_key("", taken), "2026/10/racquetcollective-20261004T030005Z.dump");
        assert_eq!(normalized_prefix("backups"), "backups/");
        assert_eq!(normalized_prefix(" b/ "), "b/");
        assert_eq!(normalized_prefix(""), "");
    }

    #[test]
    fn only_exact_keys_parse() {
        let key = "racquetcollective/2026/10/racquetcollective-20261004T030005Z.dump";
        for foreign in [
            "racquetcollective/notes.txt",
            "other/2026/10/racquetcollective-20261004T030005Z.dump",
            "racquetcollective/2026/09/racquetcollective-20261004T030005Z.dump",
            "racquetcollective/2026/10/racquetcollective-20261004T030005Z.dump.bak",
            "racquetcollective/2026/10/racquetcollective-20261304T030005Z.dump",
            "racquetcollective/2026/10/racquetcollective-.dump",
            "racquetcollective/2026/10/é.dump",
            "racquetcollective/extra/2026/10/racquetcollective-20261004T030005Z.dump",
        ] {
            assert_eq!(parse_key("racquetcollective/", foreign), None, "{foreign}");
        }
        assert!(parse_key("racquetcollective/", key).is_some());
    }

    #[test]
    fn next_run_is_strictly_after_now() {
        assert_eq!(
            next_run_at(at(4, 1, 0), 3),
            Utc.with_ymd_and_hms(2026, 10, 4, 3, 0, 0).unwrap()
        );
        // At or past the hour: tomorrow.
        let exactly = Utc.with_ymd_and_hms(2026, 10, 4, 3, 0, 0).unwrap();
        assert_eq!(next_run_at(exactly, 3), Utc.with_ymd_and_hms(2026, 10, 5, 3, 0, 0).unwrap());
        assert_eq!(
            next_run_at(at(4, 23, 59), 0),
            Utc.with_ymd_and_hms(2026, 10, 5, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn truncation_respects_characters() {
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(truncate("abcd", 3), "abc…");
        assert_eq!(truncate("éééé", 2), "éé…");
    }
}
