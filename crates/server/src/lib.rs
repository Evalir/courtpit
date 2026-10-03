//! Courtpit server: REST API, background jobs and admin CLI, backed by Postgres.
#![deny(unsafe_code)]

pub mod api;
pub mod app;
pub mod config;
pub mod error;
pub mod telemetry;

pub use app::{AppState, router};
pub use config::Config;
pub use error::{ApiError, ApiResult};
