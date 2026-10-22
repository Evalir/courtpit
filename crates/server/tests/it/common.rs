//! Shared test harness: per-test databases cloned from a migrated template, and a small
//! request builder over the in-process router.

use std::{
    hash::{DefaultHasher, Hash, Hasher},
    net::SocketAddr,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use axum::{
    Extension, Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use racquetcollective_server::{
    AppState, Config,
    clock::OffsetClock,
    communities::{Branding, CreatedCommunity, NewCommunity, create_community},
    db,
    mailer::LogMailer,
    push::LogPusher,
    tenancy::Community,
};
use serde_json::{Value, json};
use sqlx::{Connection, Executor, PgConnection, PgPool, postgres::PgConnectOptions};
use tokio::sync::OnceCell;
use tower::ServiceExt;
use uuid::Uuid;

const DEFAULT_URL: &str =
    "postgres://racquetcollective:racquetcollective@127.0.0.1/racquetcollective";
/// A login role that owns its database but may not create roles, like a managed Postgres's
/// app role (Caster): migrations run in `RlsMode::Owner`.
const OWNER_ROLE: &str = "racquetcollective_owner";
/// Advisory lock key serialising template creation across test processes.
const TEMPLATE_LOCK: i64 = 7_253_001;

static TEMPLATE: OnceCell<String> = OnceCell::const_new();
static OWNER_TEMPLATE: OnceCell<String> = OnceCell::const_new();

/// The login role the application under test connects as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DbRole {
    /// The admin role of `DATABASE_URL` (a superuser): `RlsMode::AppRole`.
    Admin,
    /// [`OWNER_ROLE`], which owns the database and may not create roles: `RlsMode::Owner`.
    Owner,
}

impl DbRole {
    /// [`Self::Owner`] when `RACQUETCOLLECTIVE_TEST_RLS_MODE=owner`, so the whole suite can run as
    /// a role that row-level security binds.
    fn from_env() -> Self {
        match std::env::var("RACQUETCOLLECTIVE_TEST_RLS_MODE").as_deref() {
            Ok("owner") => Self::Owner,
            _ => Self::Admin,
        }
    }

    fn template_name(self) -> String {
        let mut hasher = DefaultHasher::new();
        for migration in db::MIGRATOR.iter() {
            migration.version.hash(&mut hasher);
            migration.checksum.hash(&mut hasher);
        }
        let kind = match self {
            Self::Admin => "",
            Self::Owner => "owner_",
        };
        format!("racquetcollective_tpl_{kind}{:016x}", hasher.finish())
    }

    /// Options for the application pool on `database`. The owner always connects directly: the
    /// CI pooler knows only the admin role.
    fn app_options(self, database: &str) -> PgConnectOptions {
        match self {
            Self::Admin => app_options().database(database),
            Self::Owner => admin_options()
                .username(OWNER_ROLE)
                .password("racquetcollective")
                .database(database),
        }
    }

    /// The `OWNER` clause for databases this role's app runs on.
    fn owner_clause(self) -> String {
        match self {
            Self::Admin => String::new(),
            Self::Owner => format!(" OWNER {OWNER_ROLE}"),
        }
    }
}

/// Reads the first of `names` that is set, falling back to the default URL.
fn url_from_env(names: &[&str]) -> PgConnectOptions {
    let url = names
        .iter()
        .find_map(|name| std::env::var(name).ok())
        .unwrap_or_else(|| DEFAULT_URL.to_owned());
    PgConnectOptions::from_str(&url).expect("database URL must be a valid Postgres URL")
}

/// Options for admin work that needs a real session (advisory lock, `CREATE`/`DROP DATABASE`,
/// migrations): `DATABASE_DIRECT_URL`, else `DATABASE_URL`. Never goes through a pooler.
fn admin_options() -> PgConnectOptions {
    url_from_env(&["DATABASE_DIRECT_URL", "DATABASE_URL"])
}

/// Options for the application pool under test: `DATABASE_URL` (the pooler when testing
/// pooled), with sqlx's statement cache off when `RACQUETCOLLECTIVE_DB_POOLED=true`.
fn app_options() -> PgConnectOptions {
    let pooled = std::env::var("RACQUETCOLLECTIVE_DB_POOLED")
        .is_ok_and(|value| matches!(value.to_ascii_lowercase().as_str(), "true" | "1"));
    db::pooled_options(url_from_env(&["DATABASE_URL"]), pooled)
}

/// Ensures the migrated template for `role` exists (once per process) and returns its name.
async fn template(role: DbRole) -> &'static str {
    let cell = match role {
        DbRole::Admin => &TEMPLATE,
        DbRole::Owner => &OWNER_TEMPLATE,
    };
    cell.get_or_init(async || {
        let name = role.template_name();
        let mut conn = PgConnection::connect_with(&admin_options()).await.unwrap();
        let _ = sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(TEMPLATE_LOCK)
            .execute(&mut conn)
            .await
            .unwrap();

        // Clean up databases left behind by earlier runs; ignore ones still in use. Both
        // current templates stay.
        let stale: Vec<String> = sqlx::query_scalar(
            "SELECT datname FROM pg_database
             WHERE datname LIKE 'racquetcollective\\_test\\_%'
                OR (datname LIKE 'racquetcollective\\_tpl\\_%' AND datname NOT IN ($1, $2))",
        )
        .bind(DbRole::Admin.template_name())
        .bind(DbRole::Owner.template_name())
        .fetch_all(&mut conn)
        .await
        .unwrap();
        for db_name in stale {
            let _ = conn.execute(format!(r#"DROP DATABASE IF EXISTS "{db_name}""#).as_str()).await;
        }

        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)")
                .bind(&name)
                .fetch_one(&mut conn)
                .await
                .unwrap();
        if !exists {
            if role == DbRole::Owner {
                let _ = conn
                    .execute(
                        format!(
                            "DO $$ BEGIN
                                CREATE ROLE {OWNER_ROLE} LOGIN PASSWORD 'racquetcollective'
                                    NOSUPERUSER NOCREATEROLE NOCREATEDB NOBYPASSRLS;
                            EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL;
                            END $$"
                        )
                        .as_str(),
                    )
                    .await
                    .unwrap();
            }
            let build = format!("{name}_build");
            let _ = conn
                .execute(format!(r#"DROP DATABASE IF EXISTS "{build}""#).as_str())
                .await
                .unwrap();
            let owner = role.owner_clause();
            let _ = conn
                .execute(format!(r#"CREATE DATABASE "{build}"{owner}"#).as_str())
                .await
                .unwrap();
            let migrating = match role {
                DbRole::Admin => admin_options().database(&build),
                DbRole::Owner => role.app_options(&build),
            };
            let pool = db::connect_with(migrating, 2).await.unwrap();
            let mode = db::migrate(&pool).await.unwrap();
            assert_eq!(
                mode == db::RlsMode::Owner,
                role == DbRole::Owner,
                "unexpected RLS mode {mode}"
            );
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

/// Creates a fresh database for one test, cloned from `role`'s migrated template, and returns
/// its name.
async fn fresh_database(role: DbRole) -> String {
    let template = template(role).await;
    let name = format!("racquetcollective_test_{}", Uuid::now_v7().simple());
    let mut conn = PgConnection::connect_with(&admin_options()).await.unwrap();
    let owner = role.owner_clause();
    let sql = format!(r#"CREATE DATABASE "{name}" TEMPLATE "{template}"{owner}"#);
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
    name
}

/// An application under test with its own database.
pub(crate) struct TestApp {
    pub state: AppState,
    pub router: Router,
    pub db: PgPool,
    pub mailer: Arc<LogMailer>,
    /// Every push notification sent.
    pub pusher: Arc<LogPusher>,
    /// The app's clock; `advance` it to drive deadlines and jobs.
    pub clock: Arc<OffsetClock>,
}

/// A signed-in test user.
#[derive(Debug, Clone)]
pub(crate) struct Session {
    pub token: String,
    pub user_id: Uuid,
    pub player_id: Uuid,
    pub email: String,
    pub community: String,
}

/// Test defaults: generous rate limits, plain-HTTP cookies.
pub(crate) fn test_config() -> Config {
    Config { auth_ip_limit_per_hour: 10_000, cookie_secure: false, ..Config::default() }
}

impl TestApp {
    /// Spawns the app against a fresh, migrated database.
    pub(crate) async fn spawn() -> Self {
        Self::spawn_with(test_config()).await
    }

    /// Spawns with a custom configuration, as the role `RACQUETCOLLECTIVE_TEST_RLS_MODE` picks.
    pub(crate) async fn spawn_with(config: Config) -> Self {
        Self::spawn_as(config, DbRole::from_env()).await
    }

    /// Spawns against a database owned by [`OWNER_ROLE`] and migrated as it, so without the
    /// `courtpit_app` role, whatever `RACQUETCOLLECTIVE_TEST_RLS_MODE` says.
    pub(crate) async fn spawn_owner_mode() -> Self {
        Self::spawn_as(test_config(), DbRole::Owner).await
    }

    /// The app connects as `role`; [`Self::db`] is the admin role on the same database, which
    /// row-level security never restricts, for test setup and assertions.
    async fn spawn_as(config: Config, role: DbRole) -> Self {
        let name = fresh_database(role).await;
        let app_db = db::connect_with(role.app_options(&name), 5).await.unwrap();
        let db = match role {
            DbRole::Admin => app_db.clone(),
            DbRole::Owner => db::connect_with(admin_options().database(&name), 5).await.unwrap(),
        };
        let mailer = Arc::new(LogMailer::default());
        let pusher = Arc::new(LogPusher::default());
        let clock = OffsetClock::shared();
        let state = AppState::new(config, app_db, mailer.clone())
            .with_clock(clock.clone())
            .with_pusher(pusher.clone());
        // `oneshot` has no socket peer; stand in for the `ConnectInfo` that `serve` provides.
        let peer = ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0)));
        let web = state
            .config
            .web_dir
            .as_deref()
            .map(racquetcollective_server::web::WebApp::new)
            .transpose()
            .unwrap();
        let router = racquetcollective_server::router(state.clone(), web).layer(Extension(peer));
        Self { state, router, db, mailer, pusher, clock }
    }

    /// The 6-digit code in the last email sent to `email`.
    pub(crate) fn last_code(&self, email: &str) -> String {
        let mail = self.mailer.last_to(email).expect("no email sent");
        mail.text
            .split(|ch: char| !ch.is_ascii_digit())
            .find(|w| w.len() == 6)
            .expect("no code in email")
            .to_owned()
    }

    /// Signs `email` in to `community` through the OTP flow.
    pub(crate) async fn login(&self, email: &str, community: &str) -> Session {
        let _ = self
            .post("/api/v1/auth/otp/request")
            .community(community)
            .json(json!({ "email": email }))
            .send()
            .await
            .expect(StatusCode::ACCEPTED);
        let code = self.last_code(email);
        let body = self
            .post("/api/v1/auth/otp/verify")
            .community(community)
            .json(json!({ "email": email, "code": code }))
            .send()
            .await
            .expect(StatusCode::OK);
        Session {
            token: body["token"].as_str().unwrap().to_owned(),
            user_id: body["user_id"].as_str().unwrap().parse().unwrap(),
            player_id: body["player_id"].as_str().unwrap().parse().unwrap(),
            email: email.to_owned(),
            community: community.to_owned(),
        }
    }

    /// `PATCH /api/v1/me` expecting 200; returns the body.
    pub(crate) async fn patch_me(&self, session: &Session, body: Value) -> Value {
        self.patch("/api/v1/me").as_(session).json(body).send().await.expect(StatusCode::OK)
    }

    /// Promotes a player to community admin.
    pub(crate) async fn make_admin(&self, session: &Session) {
        let _ = sqlx::query("UPDATE players SET role = 'admin' WHERE id = $1")
            .bind(session.player_id)
            .execute(&self.db)
            .await
            .unwrap();
    }

    /// Starts building a request.
    pub(crate) fn req(&self, method: Method, path: &str) -> Req<'_> {
        Req { app: self, method, path: path.to_owned(), headers: Vec::new(), body: None }
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
            &self.state.db,
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
    /// Authenticates as `session` and scopes to its community.
    pub(crate) fn as_(self, session: &Session) -> Self {
        let community = session.community.clone();
        self.bearer(&session.token).community(&community)
    }

    pub(crate) fn bearer(self, token: &str) -> Self {
        self.header("authorization", &format!("Bearer {token}"))
    }

    /// Scopes the request to a community via the tenant header.
    pub(crate) fn community(self, slug: &str) -> Self {
        self.header("x-racquetcollective-community", slug)
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
        let body = self
            .body
            .map_or_else(Body::empty, |json| Body::from(serde_json::to_vec(&json).unwrap()));
        let res = self.app.router.clone().oneshot(builder.body(body).unwrap()).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
        };
        Res { status, headers, body }
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
        assert_eq!(self.status, status, "unexpected status; body: {:#}", self.body);
        self.body
    }
}
