//! `courtpit-server` binary: `serve`, `migrate`, `create-community`.
#![deny(unsafe_code)]

use anyhow::Context;
use clap::{Parser, Subcommand};
use courtpit_server::{AppState, Config, router, telemetry};

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
    Migrate,
    /// Create a community (tenant).
    CreateCommunity,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    telemetry::init(cli.log_format);
    match cli.command {
        Command::Serve(config) => serve(config).await,
        Command::Migrate => anyhow::bail!("`migrate` is not implemented yet"),
        Command::CreateCommunity => anyhow::bail!("`create-community` is not implemented yet"),
    }
}

async fn serve(config: Config) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .with_context(|| format!("binding {}", config.bind))?;
    tracing::info!(addr = %config.bind, "listening");
    let app = router(AppState::new(config));
    axum::serve(listener, app)
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
                sig.recv().await;
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
