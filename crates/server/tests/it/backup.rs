//! Nightly backups: the job against an in-memory store and a fake dump, scheduling, config.

use std::{
    collections::BTreeMap,
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use chrono::{DateTime, Duration, TimeZone, Utc};
use racquetcollective_server::{
    AppState, Config,
    backup::{self, Backup, DumpReader, DumpSource, ObjectStore, StoreFuture, pg_dump::PgDump},
    clock::Clock,
    config::BackupConfig,
    jobs::{self, Job},
};
use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};

use crate::common::{TestApp, test_config};

const DUMP: &[u8] = b"PGDMP fake dump contents";

#[derive(Debug, Default)]
struct MemoryStore {
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl MemoryStore {
    fn with(keys: &[&str]) -> Arc<Self> {
        let store = Self::default();
        for key in keys {
            let _ = store.objects.lock().unwrap().insert((*key).to_owned(), b"old".to_vec());
        }
        Arc::new(store)
    }

    fn keys(&self) -> Vec<String> {
        self.objects.lock().unwrap().keys().cloned().collect()
    }
}

impl ObjectStore for MemoryStore {
    fn put_stream<'a>(&'a self, key: &'a str, mut body: DumpReader) -> StoreFuture<'a, u64> {
        Box::pin(async move {
            let mut data = Vec::new();
            let _ = body.read_to_end(&mut data).await?;
            let len = data.len() as u64;
            let _ = self.objects.lock().unwrap().insert(key.to_owned(), data);
            Ok(len)
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> StoreFuture<'a, Vec<String>> {
        Box::pin(async move {
            let keys = self.keys();
            Ok(keys.into_iter().filter(|key| key.starts_with(prefix)).collect())
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            let _ = self.objects.lock().unwrap().remove(key);
            Ok(())
        })
    }
}

/// Fails every read, like `pg_dump` exiting non-zero after writing some bytes.
struct Broken;

impl AsyncRead for Broken {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::other("pg_dump exited 1: connection refused")))
    }
}

#[derive(Debug)]
struct FakeDump {
    fails: bool,
}

impl DumpSource for FakeDump {
    fn start(&self) -> anyhow::Result<DumpReader> {
        if self.fails {
            return Ok(Box::new(io::Cursor::new(DUMP).chain(Broken)));
        }
        Ok(Box::new(io::Cursor::new(DUMP)))
    }
}

fn config(hour_utc: u32) -> Config {
    Config {
        backup: BackupConfig { hour_utc, retention_days: 7, ..BackupConfig::default() },
        ..test_config()
    }
}

fn backup_state(app: &TestApp, store: &Arc<MemoryStore>, fails: bool) -> AppState {
    let dump = Arc::new(FakeDump { fails });
    let store = store.clone();
    app.state.clone().with_backup(Some(Backup { dump, store }))
}

fn utc(month: u32, day: u32, hour: u32, min: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, month, day, hour, min, 0).unwrap()
}

async fn pending(app: &TestApp) -> Vec<(String, DateTime<Utc>)> {
    sqlx::query_as(
        "SELECT kind, run_at FROM jobs WHERE completed_at IS NULL AND failed_at IS NULL ORDER BY run_at",
    )
    .fetch_all(&app.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn the_job_uploads_prunes_and_schedules_the_next_night() {
    let app = TestApp::spawn_with(config(5)).await;
    let store = MemoryStore::with(&[
        "racquetcollective/2026/09/racquetcollective-20260920T030000Z.dump", /* past retention:
                                                                              * deleted */
        "racquetcollective/2026/10/racquetcollective-20261001T030000Z.dump", // recent: kept
        "racquetcollective/notes.txt",                                       // foreign: kept
        "racquetcollective/2026/09/racquetcollective-garbage.dump",          // unparseable: kept
        "racquetcollective/old/racquetcollective-20260920T030000Z.dump",     // wrong layout: kept
        "other/2026/09/racquetcollective-20260920T030000Z.dump",             /* outside the
                                                                              * prefix: kept */
    ]);
    let state = backup_state(&app, &store, false);
    app.clock.set(utc(10, 4, 10, 30));
    // Due at the pinned time, not the wall clock (which passes 10:30 on 2026-10-04).
    jobs::enqueue(&app.db, Job::BackupDatabase {}, utc(10, 4, 10, 30)).await.unwrap();

    assert_eq!(jobs::run_due(&state, "t").await.unwrap(), 1);

    let keys = store.keys();
    let uploaded: Vec<_> = keys
        .iter()
        .filter(|key| key.starts_with("racquetcollective/2026/10/racquetcollective-20261004T1030"))
        .collect();
    assert_eq!(uploaded.len(), 1, "{keys:?}");
    assert_eq!(store.objects.lock().unwrap()[uploaded[0]], DUMP);
    assert_eq!(keys.len(), 6, "one added, one pruned: {keys:?}");
    assert!(
        !keys.iter().any(|key| key.contains("20260920T030000Z.dump")
            && key.starts_with("racquetcollective/2026/09"))
    );
    // The next run: tomorrow at BACKUP_HOUR_UTC.
    assert_eq!(pending(&app).await, [("backup_database".to_owned(), utc(10, 5, 5, 0))]);
}

#[tokio::test]
async fn a_failed_dump_uploads_nothing_and_retries_instead_of_rescheduling() {
    let app = TestApp::spawn_with(config(3)).await;
    let store = MemoryStore::with(&[]);
    let state = backup_state(&app, &store, true);
    jobs::enqueue(&app.db, Job::BackupDatabase {}, Utc::now()).await.unwrap();
    assert_eq!(jobs::run_due(&state, "t").await.unwrap(), 1);
    assert!(store.keys().is_empty());
    let (err, attempts, completed): (Option<String>, i32, bool) =
        sqlx::query_as("SELECT last_error, attempts, completed_at IS NOT NULL FROM jobs")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert!(err.unwrap().contains("pg_dump exited 1"));
    assert_eq!((attempts, completed), (1, false), "backed off for a retry");
    assert_eq!(pending(&app).await.len(), 1, "no second job");
}

#[tokio::test]
async fn without_configuration_the_job_is_a_noop_that_completes() {
    let app = TestApp::spawn().await;
    jobs::enqueue(&app.db, Job::BackupDatabase {}, Utc::now()).await.unwrap();
    assert_eq!(jobs::run_due(&app.state, "t").await.unwrap(), 1);
    assert!(pending(&app).await.is_empty(), "nothing rescheduled");
    let completed: i64 =
        sqlx::query_scalar("SELECT count(*) FROM jobs WHERE completed_at IS NOT NULL")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(completed, 1);
    backup::ensure_scheduled(&app.state).await.unwrap();
    assert!(pending(&app).await.is_empty(), "not scheduled when disabled");
}

#[tokio::test]
async fn scheduling_is_insert_if_absent() {
    let app = TestApp::spawn_with(config(3)).await;
    let state = backup_state(&app, &MemoryStore::with(&[]), false);
    app.clock.set(utc(10, 4, 10, 0));
    backup::ensure_scheduled(&state).await.unwrap();
    let first = pending(&app).await;
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].1, utc(10, 5, 3, 0));

    // Later starts (and hourly ticks) leave the scheduled run where it is...
    app.clock.advance(Duration::hours(14));
    backup::ensure_scheduled(&state).await.unwrap();
    assert_eq!(pending(&app).await, first);

    // ...also while it is running (claimed), when its successor does not exist yet.
    app.clock.set(utc(10, 5, 3, 30));
    let running = jobs::claim(&app.db, "w", 1, app.clock.now()).await.unwrap();
    assert_eq!(running.len(), 1);
    backup::ensure_scheduled(&state).await.unwrap();
    let total: i64 =
        sqlx::query_scalar("SELECT count(*) FROM jobs").fetch_one(&app.db).await.unwrap();
    assert_eq!(total, 1);

    // Once it has finished without a successor (it failed for good), the next start heals.
    jobs::finish(&app.db, running[0].id, app.clock.now(), Ok(())).await.unwrap();
    backup::ensure_scheduled(&state).await.unwrap();
    assert_eq!(pending(&app).await.len(), 1);
}

#[tokio::test]
async fn partial_configuration_is_a_startup_error() {
    let mut config = Config::default();
    assert!(Backup::from_config(&config).unwrap().is_none(), "off by default");

    config.backup.s3_endpoint = Some("https://acct.r2.cloudflarestorage.com".into());
    config.backup.s3_bucket = Some("  ".into()); // blank counts as unset
    config.backup.s3_access_key = Some("key".into());
    let err = Backup::from_config(&config).unwrap_err().to_string();
    assert!(err.contains("BACKUP_S3_BUCKET, BACKUP_S3_SECRET_KEY"), "{err}");

    config.backup.s3_bucket = Some("bucket".into());
    config.backup.s3_secret_key = Some("secret".into());
    assert!(Backup::from_config(&config).unwrap().is_some());
}

/// The real thing, when `pg_dump` is installed: a custom-format dump of the test database.
#[tokio::test]
async fn pg_dump_streams_a_custom_format_dump() {
    let app = TestApp::spawn().await;
    let base = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://racquetcollective:racquetcollective@127.0.0.1/racquetcollective".to_owned()
    });
    let mut url = reqwest::Url::parse(&base).unwrap();
    url.set_path(app.db.connect_options().get_database().unwrap());
    let dump = PgDump::new(url.as_str()).unwrap();
    let mut reader = match dump.start() {
        Ok(reader) => reader,
        Err(err) => {
            eprintln!("skipping: {err}");
            return;
        }
    };
    let mut out = Vec::new();
    let _ = reader.read_to_end(&mut out).await.unwrap();
    assert!(out.starts_with(b"PGDMP"), "custom format magic");
    assert!(out.len() > 1000);
}
