//! Runtime configuration, read from the environment (or CLI flags) by clap.

use std::net::SocketAddr;

use axum_client_ip::ClientIpSource;

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
    /// Where the client IP comes from: `ConnectInfo` (the socket peer) unless behind a proxy you
    /// control, then e.g. `FlyClientIp` or `RightmostXForwardedFor`.
    #[arg(long, env = "COURTPIT_CLIENT_IP_SOURCE", default_value = "ConnectInfo")]
    pub client_ip_source: ClientIpSource,
    /// Session lifetime in days.
    #[arg(long, env = "COURTPIT_SESSION_TTL_DAYS", default_value_t = 30)]
    pub session_ttl_days: i64,
    /// Mark the session cookie `Secure` (disable only for plain-HTTP local development).
    #[arg(long, env = "COURTPIT_COOKIE_SECURE", default_value_t = true, action = clap::ArgAction::Set)]
    pub cookie_secure: bool,
    /// Auth requests (code requests, verifications, logins) allowed per client IP per hour.
    #[arg(long, env = "COURTPIT_AUTH_IP_LIMIT_PER_HOUR", default_value_t = 30)]
    pub auth_ip_limit_per_hour: u32,
    /// Email transport.
    #[arg(long, env = "COURTPIT_MAILER", value_enum, default_value_t)]
    pub mailer: MailerKind,
    /// Resend API key (required with `--mailer resend`).
    #[arg(long, env = "RESEND_API_KEY", hide_env_values = true)]
    pub resend_api_key: Option<String>,
    /// Sender address for outgoing email.
    #[arg(
        long,
        env = "COURTPIT_EMAIL_FROM",
        default_value = "Courtpit <no-reply@courtpit.app>"
    )]
    pub email_from: String,
    /// Database settings.
    #[command(flatten)]
    pub db: DbConfig,
}

/// Which [`crate::mailer::Mailer`] to use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum MailerKind {
    /// Log messages (development and tests).
    #[default]
    Log,
    /// Send through Resend's HTTP API.
    Resend,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([0, 0, 0, 0], 8080)),
            base_domain: "courtpit.app".to_owned(),
            tenant_cache_ttl_secs: 60,
            client_ip_source: ClientIpSource::ConnectInfo,
            session_ttl_days: 30,
            cookie_secure: true,
            auth_ip_limit_per_hour: 30,
            mailer: MailerKind::Log,
            resend_api_key: None,
            email_from: "Courtpit <no-reply@courtpit.app>".to_owned(),
            db: DbConfig {
                database_url: "postgres://courtpit:courtpit@127.0.0.1/courtpit".to_owned(),
                db_max_connections: 10,
            },
        }
    }
}
