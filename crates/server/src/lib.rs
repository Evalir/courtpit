//! Courtpit server: REST API, background jobs and admin CLI, backed by Postgres.
#![deny(unsafe_code)]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]

// Used only by the `courtpit-server` binary target, which shares this manifest.
use tokio as _;

pub mod api;
pub mod app;
pub mod config;
pub mod db;
pub mod error;
pub mod telemetry;

pub use app::{AppState, router};
pub use config::Config;
pub use error::{ApiError, ApiResult};
