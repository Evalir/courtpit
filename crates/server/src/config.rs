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
    /// Push transport.
    #[arg(long, env = "COURTPIT_PUSH", value_enum, default_value_t)]
    pub push: PushKind,
    /// Expo access token, when the Expo project requires authenticated push requests.
    #[arg(long, env = "EXPO_ACCESS_TOKEN", hide_env_values = true)]
    pub expo_access_token: Option<String>,
    /// Sender address for outgoing email.
    #[arg(
        long,
        env = "COURTPIT_EMAIL_FROM",
        default_value = "Courtpit <no-reply@courtpit.app>"
    )]
    pub email_from: String,
    /// Accepted `aud` values for Sign in with Apple (bundle ids / service ids), comma-separated.
    #[arg(long, env = "COURTPIT_APPLE_CLIENT_IDS", value_delimiter = ',')]
    pub apple_client_ids: Vec<String>,
    /// Accepted `aud` values for Google sign-in (OAuth client ids), comma-separated.
    #[arg(long, env = "COURTPIT_GOOGLE_CLIENT_IDS", value_delimiter = ',')]
    pub google_client_ids: Vec<String>,
    /// Apple's JWKS endpoint.
    #[arg(
        long,
        env = "COURTPIT_APPLE_JWKS_URL",
        default_value = "https://appleid.apple.com/auth/keys"
    )]
    pub apple_jwks_url: String,
    /// Google's JWKS endpoint.
    #[arg(
        long,
        env = "COURTPIT_GOOGLE_JWKS_URL",
        default_value = "https://www.googleapis.com/oauth2/v3/certs"
    )]
    pub google_jwks_url: String,
    /// Run the background job loop in this process.
    #[arg(long, env = "COURTPIT_JOBS_ENABLED", default_value_t = true, action = clap::ArgAction::Set)]
    pub jobs_enabled: bool,
    /// Job loop poll interval in milliseconds.
    #[arg(long, env = "COURTPIT_JOB_POLL_MS", default_value_t = 1000)]
    pub job_poll_ms: u64,
    /// Directory of the exported web app (`npm run export:web` in `apps/mobile`), served for
    /// every path outside the API. Unset: the API only.
    #[arg(long, env = "COURTPIT_WEB_DIR")]
    pub web_dir: Option<std::path::PathBuf>,
    /// Database settings.
    #[command(flatten)]
    pub db: DbConfig,
    /// Nightly database backups to S3-compatible storage.
    #[command(flatten)]
    pub backup: BackupConfig,
}

/// Nightly `pg_dump` backups to S3-compatible object storage (Cloudflare R2). Backups are on
/// iff the endpoint, bucket and both keys are set; setting only some of them is a startup error.
#[derive(Debug, Clone, clap::Args)]
pub struct BackupConfig {
    /// S3 endpoint, e.g. `https://<account>.r2.cloudflarestorage.com` (path-style URLs).
    #[arg(long = "backup-s3-endpoint", env = "BACKUP_S3_ENDPOINT")]
    pub s3_endpoint: Option<String>,
    /// Bucket the dumps go to.
    #[arg(long = "backup-s3-bucket", env = "BACKUP_S3_BUCKET")]
    pub s3_bucket: Option<String>,
    /// Access key id.
    #[arg(
        long = "backup-s3-access-key",
        env = "BACKUP_S3_ACCESS_KEY",
        hide_env_values = true
    )]
    pub s3_access_key: Option<String>,
    /// Secret access key.
    #[arg(
        long = "backup-s3-secret-key",
        env = "BACKUP_S3_SECRET_KEY",
        hide_env_values = true
    )]
    pub s3_secret_key: Option<String>,
    /// Signing region (`auto` is what R2 expects).
    #[arg(
        long = "backup-s3-region",
        env = "BACKUP_S3_REGION",
        default_value = "auto"
    )]
    pub s3_region: String,
    /// Key prefix for dumps; only keys under it with our naming are ever listed for pruning.
    #[arg(
        long = "backup-s3-prefix",
        env = "BACKUP_S3_PREFIX",
        default_value = "courtpit/"
    )]
    pub s3_prefix: String,
    /// Days to keep dumps; older ones are deleted after a successful upload.
    #[arg(
        long = "backup-retention-days",
        env = "BACKUP_RETENTION_DAYS",
        default_value_t = 14,
        value_parser = clap::value_parser!(i64).range(1..)
    )]
    pub retention_days: i64,
    /// Hour of day (UTC, 0-23) the nightly backup runs at or after.
    #[arg(
        long = "backup-hour-utc",
        env = "BACKUP_HOUR_UTC",
        default_value_t = 3,
        value_parser = clap::value_parser!(u32).range(0..24)
    )]
    pub hour_utc: u32,
    /// Direct (non-pooled) connection URL for `pg_dump`; defaults to `DATABASE_DIRECT_URL`,
    /// then `DATABASE_URL`. A pooler in transaction mode (Neon's `-pooler` host) cannot be
    /// dumped.
    #[arg(
        long = "backup-database-url",
        env = "BACKUP_DATABASE_URL",
        hide_env_values = true
    )]
    pub dump_database_url: Option<String>,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            s3_endpoint: None,
            s3_bucket: None,
            s3_access_key: None,
            s3_secret_key: None,
            s3_region: "auto".to_owned(),
            s3_prefix: "courtpit/".to_owned(),
            retention_days: 14,
            hour_utc: 3,
            dump_database_url: None,
        }
    }
}

/// Which [`crate::push::Pusher`] to use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum PushKind {
    /// Log notifications (development and tests).
    #[default]
    Log,
    /// Send through Expo's push service.
    Expo,
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
            push: PushKind::Log,
            expo_access_token: None,
            email_from: "Courtpit <no-reply@courtpit.app>".to_owned(),
            jobs_enabled: true,
            job_poll_ms: 1000,
            apple_client_ids: Vec::new(),
            google_client_ids: Vec::new(),
            apple_jwks_url: "https://appleid.apple.com/auth/keys".to_owned(),
            google_jwks_url: "https://www.googleapis.com/oauth2/v3/certs".to_owned(),
            web_dir: None,
            db: DbConfig {
                database_url: "postgres://courtpit:courtpit@127.0.0.1/courtpit".to_owned(),
                database_direct_url: None,
                db_pooled: false,
                db_max_connections: 10,
            },
            backup: BackupConfig::default(),
        }
    }
}
