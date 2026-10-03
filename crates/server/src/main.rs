//! `courtpit-server` binary: `serve`, `migrate`, `create-community`.
#![deny(unsafe_code)]

use anyhow::Context;
use clap::{Parser, Subcommand};
use courtpit_server::{
    AppState, Config,
    communities::{NewCommunity, create_community},
    db, router, telemetry,
};

#[derive(Debug, Parser)]
#[command(
    name = "courtpit-server",
    version,
    about = "Courtpit API server and admin CLI"
)]
struct Cli {
    /// Log output format.
    #[arg(
        long,
        env = "COURTPIT_LOG_FORMAT",
        value_enum,
        default_value_t,
        global = true
    )]
    log_format: telemetry::LogFormat,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the HTTP API (and, later, the background job loop).
    Serve(Config),
    /// Apply pending database migrations.
    Migrate(db::DbConfig),
    /// Create a community (tenant), optionally with its owner.
    CreateCommunity(CreateCommunityArgs),
}

#[derive(Debug, clap::Args)]
struct CreateCommunityArgs {
    #[command(flatten)]
    db: db::DbConfig,
    /// URL slug: lowercase letters, digits and dashes (`{slug}.courtpit.app`).
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    telemetry::init(cli.log_format);
    match cli.command {
        Command::Serve(config) => serve(config).await,
        Command::Migrate(cfg) => {
            let pool = db::connect(&cfg).await?;
            db::migrate(&pool).await?;
            tracing::info!("migrations applied");
            Ok(())
        }
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
    }
}

async fn serve(config: Config) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .with_context(|| format!("binding {}", config.bind))?;
    let pool = db::connect(&config.db).await?;
    tracing::info!(addr = %config.bind, "listening");
    let app = router(AppState::from_config(config, pool)?);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("serving HTTP")
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
