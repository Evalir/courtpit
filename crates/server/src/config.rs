//! Runtime configuration, read from the environment (or CLI flags) by clap.

use std::net::SocketAddr;

use crate::db::DbConfig;

/// Configuration for `courtpit-server serve`. Every field has an env var; see `.env.example`.
#[derive(Debug, Clone, clap::Args)]
pub struct Config {
    /// Address the HTTP server binds to.
    #[arg(long, env = "COURTPIT_BIND", default_value = "0.0.0.0:8080")]
    pub bind: SocketAddr,
    /// Base domain for `{slug}.{base}` tenant hosts.
    #[arg(long, env = "COURTPIT_BASE_DOMAIN", default_value = "courtpit.app")]
    pub base_domain: String,
    /// Seconds a resolved community stays cached.
    #[arg(long, env = "COURTPIT_TENANT_CACHE_TTL_SECS", default_value_t = 60)]
    pub tenant_cache_ttl_secs: u64,
    /// Database settings.
    #[command(flatten)]
    pub db: DbConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([0, 0, 0, 0], 8080)),
            base_domain: "courtpit.app".to_owned(),
            tenant_cache_ttl_secs: 60,
            db: DbConfig {
                database_url: "postgres://courtpit:courtpit@127.0.0.1/courtpit".to_owned(),
                db_max_connections: 10,
            },
        }
    }
}
