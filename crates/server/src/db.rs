//! Database pool and migrations.

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
    /// Postgres connection URL.
    #[arg(long, env = "DATABASE_URL")]
    pub database_url: String,
    /// Maximum pool size.
    #[arg(long, env = "COURTPIT_DB_MAX_CONNECTIONS", default_value_t = 10)]
    pub db_max_connections: u32,
}

/// Opens a connection pool.
pub async fn connect(cfg: &DbConfig) -> anyhow::Result<PgPool> {
    let options: PgConnectOptions = cfg.database_url.parse().context("parsing DATABASE_URL")?;
    connect_with(options, cfg.db_max_connections).await
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

/// Applies pending migrations.
pub async fn migrate(pool: &PgPool) -> anyhow::Result<()> {
    MIGRATOR.run(pool).await.context("running migrations")
}
