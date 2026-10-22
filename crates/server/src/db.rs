//! Database pool and migrations.
//!
//! Production runs on Neon's pooled endpoint (pgbouncer, transaction pooling), so everything
//! the server does must survive a connection being handed to another client between
//! transactions. Anything that needs a real session (migrations' advisory lock, `pg_dump`) goes
//! through [`DbConfig::database_direct_url`] instead.
//!
//! Row-level security runs in one of two [`RlsMode`]s. Where the login role may create roles
//! (Neon, RDS, a superuser), the migrations create the `courtpit_app` role and [`TenantTx`]
//! drops to it. Where it may not (a managed Postgres that gives the app one owner role, such as
//! Caster), [`migrate`] runs the same migrations without their role statements
//! ([`RoleFreeMigrations`]) and tenant transactions stay on the owner role, which RLS still
//! binds because every tenant table forces it. Cross-tenant work opens a
//! [`GlobalTx`](crate::GlobalTx).
//!
//! [`TenantTx`]: crate::TenantTx

use std::{borrow::Cow, fmt, pin::Pin, time::Duration};

use anyhow::Context;
use sqlx::{
    PgPool,
    error::BoxDynError,
    migrate::{Migration, MigrationSource, Migrator},
    postgres::{PgConnectOptions, PgPoolOptions},
};

/// All migrations in `/migrations`, embedded at compile time.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// The exact role statements of the shipped migrations, removed by [`RoleFreeMigrations`].
///
/// Shipped migrations never change, so each text must be found as is. New migrations need no
/// grants: migration 1's default privileges cover every table the owner creates later.
const ROLE_STATEMENTS: &[(i64, &str)] = &[
    (
        20_261_002_000_001,
        "DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'courtpit_app') THEN
        CREATE ROLE courtpit_app NOLOGIN NOSUPERUSER NOBYPASSRLS NOINHERIT;
    END IF;
END
$$;
",
    ),
    (20_261_002_000_001, "GRANT courtpit_app TO CURRENT_USER;\n"),
    (20_261_002_000_001, "GRANT USAGE ON SCHEMA public TO courtpit_app;\n"),
    (
        20_261_002_000_001,
        "ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO courtpit_app;\n",
    ),
    (
        20_261_002_000_001,
        "GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO courtpit_app;\n",
    ),
    (20_261_002_000_002, "GRANT SELECT, INSERT, UPDATE, DELETE ON jobs TO courtpit_app;\n"),
    (20_261_003_000_001, "GRANT SELECT, INSERT, UPDATE, DELETE ON matches TO courtpit_app;\n"),
    (
        20_261_003_000_002,
        "GRANT SELECT, INSERT, UPDATE, DELETE ON match_proposals TO courtpit_app;\n",
    ),
    (
        20_261_003_000_004,
        "GRANT SELECT, INSERT, UPDATE, DELETE ON match_requests, match_request_joins TO \
         courtpit_app;\n",
    ),
    (
        20_261_003_000_005,
        "GRANT SELECT, INSERT, UPDATE, DELETE ON leagues, league_divisions, league_entries TO \
         courtpit_app;\n",
    ),
    (
        20_261_003_000_006,
        "GRANT SELECT, INSERT, UPDATE, DELETE ON ranking_events, rankings TO courtpit_app;\n",
    ),
    (
        20_261_003_000_007,
        "GRANT SELECT, INSERT, UPDATE, DELETE ON league_results TO courtpit_app;\n",
    ),
    (
        20_261_005_000_001,
        "GRANT SELECT, INSERT, UPDATE, DELETE ON device_tokens, notification_prefs TO \
         courtpit_app;\n",
    ),
];

/// How tenant transactions get row-level security on this database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RlsMode {
    /// The login role is a member of `courtpit_app`; [`TenantTx`](crate::TenantTx) drops to it.
    AppRole,
    /// No `courtpit_app` membership: tenant transactions run as the login role (the owner).
    Owner,
}

impl RlsMode {
    /// Reads the mode of the database `pool` connects to, as the login role.
    pub async fn detect(pool: &PgPool) -> sqlx::Result<Self> {
        let member: bool = sqlx::query_scalar(
            "SELECT CASE WHEN to_regrole('courtpit_app') IS NULL THEN false
                         ELSE pg_has_role(current_user, 'courtpit_app', 'MEMBER') END",
        )
        .fetch_one(pool)
        .await?;
        Ok(if member { Self::AppRole } else { Self::Owner })
    }

    /// The mode [`migrate`] runs in: [`Self::AppRole`] when the role is usable already, or when
    /// migration 1 has yet to run and the login role may create it.
    pub async fn for_migrations(pool: &PgPool) -> sqlx::Result<Self> {
        if Self::detect(pool).await? == Self::AppRole {
            return Ok(Self::AppRole);
        }
        let able: bool = sqlx::query_scalar(
            "SELECT rolsuper OR rolcreaterole FROM pg_roles WHERE rolname = current_user",
        )
        .fetch_one(pool)
        .await?;
        let has_history: bool =
            sqlx::query_scalar("SELECT to_regclass('_sqlx_migrations') IS NOT NULL")
                .fetch_one(pool)
                .await?;
        // A separate query: the table may not exist, and SQL does not short-circuit at parse.
        let first_applied = has_history && {
            let first = MIGRATOR.iter().next().map_or(0, |migration| migration.version);
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM _sqlx_migrations WHERE version = $1)")
                .bind(first)
                .fetch_one(pool)
                .await?
        };
        let fresh_and_able = able && !first_applied;
        Ok(if fresh_and_able { Self::AppRole } else { Self::Owner })
    }

    /// Logs what the mode means for tenant isolation; once, at startup.
    pub fn log(self) {
        match self {
            Self::AppRole => tracing::info!("tenant transactions run as `courtpit_app`"),
            Self::Owner => tracing::info!(
                "no `courtpit_app` role: tenant transactions run as the login role, which forced \
                 row-level security binds"
            ),
        }
    }
}

impl fmt::Display for RlsMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AppRole => "app_role",
            Self::Owner => "owner",
        })
    }
}

/// [`MIGRATOR`]'s migrations without their `courtpit_app` statements, each with its original
/// checksum, so a database migrated either way records the same history.
#[derive(Debug, Clone, Copy, Default)]
pub struct RoleFreeMigrations;

impl RoleFreeMigrations {
    /// The rewritten migrations; fails if a role statement is not found where it was shipped.
    pub fn migrations() -> Result<Vec<Migration>, String> {
        MIGRATOR
            .iter()
            .map(|migration| {
                let sql = ROLE_STATEMENTS
                    .iter()
                    .filter(|(version, _)| *version == migration.version)
                    .try_fold(migration.sql.to_string(), |sql, (_, statement)| {
                        sql.contains(statement).then(|| sql.replacen(statement, "", 1)).ok_or_else(
                            || {
                                format!(
                                    "migration {} no longer contains `{}`",
                                    migration.version,
                                    statement.trim()
                                )
                            },
                        )
                    })?;
                Ok(Migration { sql: Cow::Owned(sql), ..migration.clone() })
            })
            .collect()
    }
}

impl<'s> MigrationSource<'s> for RoleFreeMigrations {
    fn resolve(
        self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Migration>, BoxDynError>> + Send + 's>> {
        Box::pin(async { Self::migrations().map_err(BoxDynError::from) })
    }
}

/// Database connection settings.
#[derive(Debug, Clone, clap::Args)]
pub struct DbConfig {
    /// Postgres connection URL (on Neon: the pooled `-pooler` endpoint).
    #[arg(long, env = "DATABASE_URL")]
    pub database_url: String,
    /// Direct (non-pooler) Postgres URL for work that needs a real session, such as migrations
    /// (their advisory lock cannot live behind a transaction pooler). Defaults to
    /// `DATABASE_URL`.
    #[arg(long, env = "DATABASE_DIRECT_URL")]
    pub database_direct_url: Option<String>,
    /// `DATABASE_URL` points at a transaction pooler (pgbouncer, Neon's pooled endpoint):
    /// disables sqlx's prepared-statement cache.
    #[arg(long, env = "RACQUETCOLLECTIVE_DB_POOLED", default_value_t = false, action = clap::ArgAction::Set)]
    pub db_pooled: bool,
    /// Maximum pool size.
    #[arg(long, env = "RACQUETCOLLECTIVE_DB_MAX_CONNECTIONS", default_value_t = 10)]
    pub db_max_connections: u32,
}

impl DbConfig {
    /// URL for work that needs a real session: the direct URL when set, else `DATABASE_URL`.
    pub fn session_url(&self) -> &str {
        self.database_direct_url.as_deref().unwrap_or(&self.database_url)
    }
}

/// Opens the application pool on `DATABASE_URL`.
pub async fn connect(cfg: &DbConfig) -> anyhow::Result<PgPool> {
    let options: PgConnectOptions = cfg.database_url.parse().context("parsing DATABASE_URL")?;
    connect_with(pooled_options(options, cfg.db_pooled), cfg.db_max_connections).await
}

/// Opens a small pool on [`DbConfig::session_url`] for session-level work (migrations).
///
/// Refuses when `RACQUETCOLLECTIVE_DB_POOLED=true` and no direct URL is set: the only URL left is
/// the pooler's, which cannot hold the session advisory lock migrations rely on.
pub async fn connect_direct(cfg: &DbConfig) -> anyhow::Result<PgPool> {
    if cfg.db_pooled && cfg.database_direct_url.is_none() {
        anyhow::bail!(
            "RACQUETCOLLECTIVE_DB_POOLED=true but DATABASE_DIRECT_URL is not set: migrations take a \
             session-level advisory lock that a transaction pooler cannot hold. Set \
             DATABASE_DIRECT_URL to the direct (non-pooler) Postgres endpoint"
        );
    }
    let url = cfg.session_url();
    let options: PgConnectOptions = url.parse().context("parsing DATABASE_DIRECT_URL")?;
    connect_with(options, 2).await
}

/// Prepares connect options for a transaction pooler: no statement cache, so no named prepared
/// statements outlive a transaction on a server connection that another client may get next.
pub fn pooled_options(options: PgConnectOptions, pooled: bool) -> PgConnectOptions {
    if pooled { options.statement_cache_capacity(0) } else { options }
}

/// Opens a connection pool from explicit options.
pub async fn connect_with(options: PgConnectOptions, max: u32) -> anyhow::Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(max)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options)
        .await
        .context("connecting to Postgres")
}

/// Applies pending migrations in the database's [`RlsMode::for_migrations`] and returns it.
/// Needs a session connection (see [`connect_direct`]).
pub async fn migrate(pool: &PgPool) -> anyhow::Result<RlsMode> {
    let mode = RlsMode::for_migrations(pool).await.context("reading the RLS mode")?;
    match mode {
        RlsMode::AppRole => MIGRATOR.run(pool).await,
        RlsMode::Owner => {
            Migrator::new(RoleFreeMigrations)
                .await
                .context("preparing role-free migrations")?
                .run(pool)
                .await
        }
    }
    .context("running migrations")?;
    Ok(mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(direct: Option<&str>, pooled: bool) -> DbConfig {
        DbConfig {
            database_url: "postgres://u:p@pooler.invalid/db".to_owned(),
            database_direct_url: direct.map(str::to_owned),
            db_pooled: pooled,
            db_max_connections: 1,
        }
    }

    #[test]
    fn session_url_prefers_the_direct_url() {
        let both = config(Some("postgres://u:p@direct.invalid/db"), true);
        assert_eq!(both.session_url(), "postgres://u:p@direct.invalid/db");
        let only_pooled = config(None, false);
        assert_eq!(only_pooled.session_url(), "postgres://u:p@pooler.invalid/db");
    }

    #[test]
    fn role_free_migrations_drop_every_role_statement_and_keep_checksums() {
        let rewritten = RoleFreeMigrations::migrations().unwrap();
        assert_eq!(rewritten.len(), MIGRATOR.iter().len());
        for (new, old) in rewritten.iter().zip(MIGRATOR.iter()) {
            assert_eq!((new.version, &new.checksum), (old.version, &old.checksum));
            let mentions = new
                .sql
                .lines()
                .filter(|line| !line.trim_start().starts_with("--"))
                .any(|line| line.contains("courtpit_app"));
            assert!(!mentions, "migration {} still uses `courtpit_app`", new.version);
        }
    }

    #[tokio::test]
    async fn migrating_through_a_pooler_is_refused() {
        let err = connect_direct(&config(None, true)).await.unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("DATABASE_DIRECT_URL"), "{message}");
    }
}
