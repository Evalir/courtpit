//! Shared test harness: per-test databases cloned from a migrated template, and a small
//! request builder over the in-process router.

use std::{
    hash::{DefaultHasher, Hash, Hasher},
    str::FromStr,
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use courtpit_server::{
    AppState, Config,
    communities::{Branding, CreatedCommunity, NewCommunity, create_community},
    db,
    tenancy::Community,
};
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::{Connection, Executor, PgConnection, PgPool, postgres::PgConnectOptions};
use tokio::sync::OnceCell;
use tower::ServiceExt;

const DEFAULT_URL: &str = "postgres://courtpit:courtpit@127.0.0.1/courtpit";
/// Advisory lock key serialising template creation across test processes.
const TEMPLATE_LOCK: i64 = 7_253_001;

static TEMPLATE: OnceCell<String> = OnceCell::const_new();

fn admin_options() -> PgConnectOptions {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_URL.to_owned());
    PgConnectOptions::from_str(&url).expect("DATABASE_URL must be a valid Postgres URL")
}

fn template_name() -> String {
    let mut hasher = DefaultHasher::new();
    for migration in db::MIGRATOR.iter() {
        migration.version.hash(&mut hasher);
        migration.checksum.hash(&mut hasher);
    }
    format!("courtpit_tpl_{:016x}", hasher.finish())
}

/// Ensures the migrated template exists (once per process) and returns its name.
async fn template() -> &'static str {
    TEMPLATE
        .get_or_init(async || {
            let name = template_name();
            let mut conn = PgConnection::connect_with(&admin_options()).await.unwrap();
            let _ = sqlx::query("SELECT pg_advisory_lock($1)")
                .bind(TEMPLATE_LOCK)
                .execute(&mut conn)
                .await
                .unwrap();

            // Clean up databases left behind by earlier runs; ignore ones still in use.
            let stale: Vec<String> = sqlx::query_scalar(
                "SELECT datname FROM pg_database
                 WHERE datname LIKE 'courtpit\\_test\\_%'
                    OR (datname LIKE 'courtpit\\_tpl\\_%' AND datname <> $1)",
            )
            .bind(&name)
            .fetch_all(&mut conn)
            .await
            .unwrap();
            for db_name in stale {
                let _ = conn
                    .execute(format!(r#"DROP DATABASE IF EXISTS "{db_name}""#).as_str())
                    .await;
            }

            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)")
                    .bind(&name)
                    .fetch_one(&mut conn)
                    .await
                    .unwrap();
            if !exists {
                let build = format!("{name}_build");
                let _ = conn
                    .execute(format!(r#"DROP DATABASE IF EXISTS "{build}""#).as_str())
                    .await
                    .unwrap();
                let _ = conn
                    .execute(format!(r#"CREATE DATABASE "{build}""#).as_str())
                    .await
                    .unwrap();
                let pool = db::connect_with(admin_options().database(&build), 2)
                    .await
                    .unwrap();
                db::migrate(&pool).await.unwrap();
                pool.close().await;
                let _ = conn
                    .execute(format!(r#"ALTER DATABASE "{build}" RENAME TO "{name}""#).as_str())
                    .await
                    .unwrap();
            }

            let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
                .bind(TEMPLATE_LOCK)
                .execute(&mut conn)
                .await
                .unwrap();
            name
        })
        .await
}

/// Creates a fresh database for one test, cloned from the migrated template.
async fn fresh_database() -> PgConnectOptions {
    let template = template().await;
    let name = format!("courtpit_test_{}", uuid::Uuid::now_v7().simple());
    let mut conn = PgConnection::connect_with(&admin_options()).await.unwrap();
    let sql = format!(r#"CREATE DATABASE "{name}" TEMPLATE "{template}""#);
    let mut attempts = 0;
    loop {
        match conn.execute(sql.as_str()).await {
            Ok(_) => break,
            // Concurrent clones can briefly collide on the template; retry.
            Err(err) if attempts < 20 => {
                attempts += 1;
                eprintln!("retrying CREATE DATABASE: {err}");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(err) => panic!("creating test database: {err}"),
        }
    }
    admin_options().database(&name)
}

/// An application under test with its own database.
pub(crate) struct TestApp {
    pub state: AppState,
    pub router: Router,
    pub db: PgPool,
}

impl TestApp {
    /// Spawns the app against a fresh, migrated database.
    pub(crate) async fn spawn() -> Self {
        let options = fresh_database().await;
        let db = db::connect_with(options, 5).await.unwrap();
        let state = AppState::new(Config::default(), db.clone());
        let router = courtpit_server::router(state.clone());
        Self { state, router, db }
    }

    /// Starts building a request.
    pub(crate) fn req(&self, method: Method, path: &str) -> Req<'_> {
        Req {
            app: self,
            method,
            path: path.to_owned(),
            headers: Vec::new(),
            body: None,
        }
    }

    pub(crate) fn get(&self, path: &str) -> Req<'_> {
        self.req(Method::GET, path)
    }

    pub(crate) fn post(&self, path: &str) -> Req<'_> {
        self.req(Method::POST, path)
    }

    pub(crate) fn patch(&self, path: &str) -> Req<'_> {
        self.req(Method::PATCH, path)
    }

    pub(crate) fn delete(&self, path: &str) -> Req<'_> {
        self.req(Method::DELETE, path)
    }

    /// Creates a community with the given slug (name derived from it), no owner.
    pub(crate) async fn community(&self, slug: &str) -> Community {
        self.community_with_owner(slug, None).await.community
    }

    /// Creates a community, optionally with an owner (verified user + `owner` player).
    pub(crate) async fn community_with_owner(
        &self,
        slug: &str,
        owner: Option<&str>,
    ) -> CreatedCommunity {
        create_community(
            &self.db,
            NewCommunity {
                slug: slug.to_owned(),
                name: format!("{slug} club"),
                custom_domain: None,
                branding: Branding {
                    display_name: Some(format!("{slug} club")),
                    ..Branding::default()
                },
                owner_email: owner.map(str::to_owned),
            },
        )
        .await
        .unwrap()
    }
}

/// Request builder.
pub(crate) struct Req<'a> {
    app: &'a TestApp,
    method: Method,
    path: String,
    headers: Vec<(String, String)>,
    body: Option<Value>,
}

impl Req<'_> {
    /// Scopes the request to a community via the tenant header.
    pub(crate) fn community(self, slug: &str) -> Self {
        self.header("x-courtpit-community", slug)
    }

    pub(crate) fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    pub(crate) fn json(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }

    pub(crate) async fn send(self) -> Res {
        let mut builder = Request::builder().method(self.method).uri(&self.path);
        for (name, value) in &self.headers {
            builder = builder.header(name, value);
        }
        if self.body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        let body = self.body.map_or_else(Body::empty, |json| {
            Body::from(serde_json::to_vec(&json).unwrap())
        });
        let res = self
            .app
            .router
            .clone()
            .oneshot(builder.body(body).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
        };
        Res {
            status,
            headers,
            body,
        }
    }
}

/// A response with its JSON body.
#[derive(Debug)]
pub(crate) struct Res {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Value,
}

impl Res {
    /// Asserts the status, printing the body on mismatch, and returns the body.
    #[track_caller]
    pub(crate) fn expect(self, status: StatusCode) -> Value {
        assert_eq!(
            self.status, status,
            "unexpected status; body: {:#}",
            self.body
        );
        self.body
    }
}
