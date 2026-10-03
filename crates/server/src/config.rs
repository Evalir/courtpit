//! Runtime configuration, read from the environment (or CLI flags) by clap.

use std::net::SocketAddr;

/// Configuration for `courtpit-server serve`. Every field has an env var; see `.env.example`.
#[derive(Debug, Clone, clap::Args)]
pub struct Config {
    /// Address the HTTP server binds to.
    #[arg(long, env = "COURTPIT_BIND", default_value = "0.0.0.0:8080")]
    pub bind: SocketAddr,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([0, 0, 0, 0], 8080)),
        }
    }
}
