//! Tracing setup.

use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Log output format.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum LogFormat {
    /// Human-readable, for local development.
    #[default]
    Pretty,
    /// One JSON object per line, for production log shipping.
    Json,
}

/// Installs the global tracing subscriber. `RUST_LOG` controls filtering (default `info`).
pub fn init(format: LogFormat) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let registry = tracing_subscriber::registry().with(filter);
    match format {
        LogFormat::Pretty => registry.with(fmt::layer()).init(),
        LogFormat::Json => registry
            .with(fmt::layer().json().flatten_event(true))
            .init(),
    }
}
