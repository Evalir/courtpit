//! `racquetcollective-server` binary: `serve`, `tick`, `migrate`, `create-community`, `openapi`,
//! `seed`.

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Parser, Subcommand};
use racquetcollective_server::{
    AppState, Config,
    app::openapi,
    applinks::{self, AndroidApp, AppLinks},
    auth::rate_limit::rate_limit_notice,
    backup,
    communities::{NewCommunity, create_community},
    db, jobs, router, seed, telemetry,
    web::WebApp,
};

#[derive(Debug, Parser)]
#[command(
    name = "racquetcollective-server",
    version,
    about = "Racquet Collective API server and admin CLI"
)]
struct Cli {
    /// Log output format.
    #[arg(long, env = "RACQUETCOLLECTIVE_LOG_FORMAT", value_enum, default_value_t, global = true)]
    log_format: telemetry::LogFormat,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the HTTP API and the background job loop.
    Serve(Config),
    /// Run due background jobs once, then exit (for hosts that stop the server when idle).
    Tick(TickArgs),
    /// Apply pending database migrations.
    Migrate(db::DbConfig),
    /// Create a community (tenant), optionally with its owner.
    CreateCommunity(CreateCommunityArgs),
    /// Set the apps that open a community's links (universal links / app links).
    SetAppLinks(SetAppLinksArgs),
    /// Print the OpenAPI document as JSON (needs no database or configuration).
    Openapi {
        /// Write the document to this file instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Fill a development/staging database with a demo community (refused when
    /// `RACQUETCOLLECTIVE_ENV=production`).
    Seed(SeedArgs),
}

#[derive(Debug, clap::Args)]
struct TickArgs {
    #[command(flatten)]
    config: Config,
    /// Stop starting new jobs after this many seconds (a running job is always finished).
    #[arg(long, env = "RACQUETCOLLECTIVE_TICK_MAX_SECONDS", default_value_t = 300)]
    max_seconds: u64,
}

#[derive(Debug, clap::Args)]
struct SeedArgs {
    #[command(flatten)]
    db: db::DbConfig,
    /// Slug of the demo community; re-running with the same slug updates it in place.
    #[arg(long, default_value = "demo")]
    slug: String,
}

#[derive(Debug, clap::Args)]
struct CreateCommunityArgs {
    #[command(flatten)]
    db: db::DbConfig,
    /// URL slug: lowercase letters, digits and dashes (`{slug}.racquetcollective.app`).
    #[arg(long)]
    slug: String,
    /// Display name.
    #[arg(long)]
    name: String,
    /// Custom domain serving this community, if any.
    #[arg(long)]
    custom_domain: Option<String>,
    /// Branding JSON (display name, logo URL, colors, typography, feature flags).
    #[arg(long, default_value = "{}")]
    branding: String,
    /// Email of the owner; the user is created if needed and given the `owner` role.
    #[arg(long)]
    owner_email: Option<String>,
}

#[derive(Debug, clap::Args)]
struct SetAppLinksArgs {
    #[command(flatten)]
    db: db::DbConfig,
    /// The community's slug.
    #[arg(long)]
    slug: String,
    /// iOS app id, `{team id}.{bundle id}` (repeat for several).
    #[arg(long = "ios-app-id")]
    ios: Vec<String>,
    /// Android application id.
    #[arg(long)]
    android_package: Option<String>,
    /// SHA-256 fingerprint of the Android signing certificate (repeat for several).
    #[arg(long = "android-sha256", requires = "android_package")]
    android_sha256: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    telemetry::init(cli.log_format);
    match cli.command {
        Command::Serve(config) => serve(config).await,
        Command::Tick(args) => tick(args).await,
        Command::Migrate(cfg) => {
            let pool = db::connect_direct(&cfg).await?;
            db::migrate(&pool).await?;
            tracing::info!("migrations applied");
            Ok(())
        }
        Command::Openapi { out } => print_openapi(out.as_deref()),
        Command::CreateCommunity(args) => {
            let pool = db::connect(&args.db).await?;
            let branding = serde_json::from_str(&args.branding).context("parsing --branding")?;
            let created = create_community(
                &pool,
                NewCommunity {
                    slug: args.slug,
                    name: args.name,
                    custom_domain: args.custom_domain,
                    branding,
                    owner_email: args.owner_email,
                },
            )
            .await?;
            println!(
                "created community {} ({}){}",
                created.community.slug,
                created.community.id,
                created
                    .owner_player_id
                    .map(|player| format!(", owner player {player}"))
                    .unwrap_or_default()
            );
            Ok(())
        }
        Command::SetAppLinks(args) => {
            let pool = db::connect(&args.db).await?;
            let links = AppLinks {
                ios: args.ios,
                android: args
                    .android_package
                    .map(|package| AndroidApp {
                        package,
                        sha256_cert_fingerprints: args.android_sha256,
                    })
                    .into_iter()
                    .collect(),
            };
            anyhow::ensure!(
                applinks::store(&pool, &args.slug, &links).await?,
                "no community with slug `{}`",
                args.slug
            );
            println!("app links set for {}", args.slug);
            Ok(())
        }
        Command::Seed(args) => {
            seed::ensure_not_production(std::env::var("RACQUETCOLLECTIVE_ENV").ok().as_deref())?;
            let pool = db::connect(&args.db).await?;
            let config = Config { db: args.db, ..Config::default() };
            let state = AppState::from_config(config, pool)?;
            println!("{}", seed::seed(&state, &args.slug).await?);
            Ok(())
        }
    }
}

/// Prints the OpenAPI document (pretty JSON, trailing newline) to stdout or `out`.
fn print_openapi(out: Option<&Path>) -> anyhow::Result<()> {
    let mut json = serde_json::to_string_pretty(&openapi())?;
    json.push('\n');
    if let Some(path) = out {
        std::fs::write(path, json).with_context(|| format!("writing {}", path.display()))
    } else {
        print!("{json}");
        Ok(())
    }
}

async fn serve(config: Config) -> anyhow::Result<()> {
    let web = config.web_dir.as_deref().map(WebApp::new).transpose()?;
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .with_context(|| format!("binding {}", config.bind))?;
    let pool = db::connect(&config.db).await?;
    tracing::info!(addr = %config.bind, "listening");
    if let Some(notice) = rate_limit_notice(std::env::var("FLY_MACHINE_ID").ok().as_deref()) {
        tracing::warn!("{notice}");
    }
    let state = AppState::from_config(config, pool)?;
    backup::ensure_scheduled(&state).await?;
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let job_loop = state.config.jobs_enabled.then(|| {
        let every = std::time::Duration::from_millis(state.config.job_poll_ms);
        jobs::spawn_loop(state.clone(), every, stop_rx)
    });
    let app = router(state, web);
    let served =
        axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .with_graceful_shutdown(shutdown_signal())
            .await
            .context("serving HTTP");
    let _ = stop_tx.send(true);
    if let Some(handle) = job_loop {
        let _ = handle.await;
    }
    served
}

async fn tick(args: TickArgs) -> anyhow::Result<()> {
    let pool = db::connect(&args.config.db).await?;
    let state = AppState::from_config(args.config, pool)?;
    backup::ensure_scheduled(&state).await?;
    let budget = std::time::Duration::from_secs(args.max_seconds);
    let summary = jobs::tick(&state, budget).await?;
    tracing::info!(ran = summary.ran, budget_spent = summary.budget_spent, "tick finished");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::warn!(%err, "failed to listen for ctrl-c");
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                let _ = sig.recv().await;
            }
            Err(err) => tracing::warn!(%err, "failed to listen for SIGTERM"),
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory as _;

    use super::*;

    /// Flattened config structs share one argument namespace: a duplicate field name between
    /// them (say `database_url`) only panics when clap builds the command, i.e. at startup.
    #[test]
    fn the_cli_definition_is_consistent() {
        Cli::command().debug_assert();
    }
}
