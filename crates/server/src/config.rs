//! Runtime configuration, read from the environment (or CLI flags) by clap.

use std::net::SocketAddr;

use crate::db::DbConfig;

/// Configuration for `courtpit-server serve`. Every field has an env var; see `.env.example`.
#[derive(Debug, Clone, clap::Args)]
pub struct Config {
    /// Address the HTTP server binds to.
    #[arg(long, env = "COURTPIT_BIND", default_value = "0.0.0.0:8080")]
    pub bind: SocketAddr,
    /// Database settings.
    #[command(flatten)]
    pub db: DbConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([0, 0, 0, 0], 8080)),
            db: DbConfig {
                database_url: "postgres://courtpit:courtpit@127.0.0.1/courtpit".to_owned(),
                db_max_connections: 10,
            },
        }
    }
}
