//! Database pool and migrations.
//!
//! Production runs on Neon's pooled endpoint (pgbouncer, transaction pooling), so everything
//! the server does must survive a connection being handed to another client between
//! transactions. Anything that needs a real session (migrations' advisory lock, `pg_dump`) goes
//! through [`DbConfig::database_direct_url`] instead.

use std::time::Duration;

use anyhow::Context;
use sqlx::{
    PgPool,
    migrate::Migrator,
    postgres::{PgConnectOptions, PgPoolOptions},
};

/// All migrations in `/migrations`, embedded at compile time.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

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
    #[arg(long, env = "COURTPIT_DB_POOLED", default_value_t = false, action = clap::ArgAction::Set)]
    pub db_pooled: bool,
    /// Maximum pool size.
    #[arg(long, env = "COURTPIT_DB_MAX_CONNECTIONS", default_value_t = 10)]
    pub db_max_connections: u32,
}

impl DbConfig {
    /// URL for work that needs a real session: the direct URL when set, else `DATABASE_URL`.
    pub fn session_url(&self) -> &str {
        self.database_direct_url
            .as_deref()
            .unwrap_or(&self.database_url)
    }
}

/// Opens the application pool on `DATABASE_URL`.
pub async fn connect(cfg: &DbConfig) -> anyhow::Result<PgPool> {
    let options: PgConnectOptions = cfg.database_url.parse().context("parsing DATABASE_URL")?;
    connect_with(
        pooled_options(options, cfg.db_pooled),
        cfg.db_max_connections,
    )
    .await
}

/// Opens a small pool on [`DbConfig::session_url`] for session-level work (migrations).
///
/// Refuses when `COURTPIT_DB_POOLED=true` and no direct URL is set: the only URL left is the
/// pooler's, which cannot hold the session advisory lock migrations rely on.
pub async fn connect_direct(cfg: &DbConfig) -> anyhow::Result<PgPool> {
    if cfg.db_pooled && cfg.database_direct_url.is_none() {
        anyhow::bail!(
            "COURTPIT_DB_POOLED=true but DATABASE_DIRECT_URL is not set: migrations take a \
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
    if pooled {
        options.statement_cache_capacity(0)
    } else {
        options
    }
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

/// Applies pending migrations. Needs a session connection (see [`connect_direct`]).
pub async fn migrate(pool: &PgPool) -> anyhow::Result<()> {
    MIGRATOR.run(pool).await.context("running migrations")
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
        assert_eq!(
            only_pooled.session_url(),
            "postgres://u:p@pooler.invalid/db"
        );
    }

    #[tokio::test]
    async fn migrating_through_a_pooler_is_refused() {
        let err = connect_direct(&config(None, true)).await.unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("DATABASE_DIRECT_URL"), "{message}");
    }
}
