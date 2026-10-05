//! Racquet Collective server: REST API, background jobs and admin CLI, backed by Postgres.
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg))]

// Used only by the `racquetcollective-server` binary target, which shares this manifest.
use tokio as _;

pub mod api;
pub mod app;
pub mod applinks;
pub mod auth;
pub mod backup;
pub mod clock;
pub mod communities;
pub mod config;
pub mod db;
pub mod error;
pub mod extract;
pub mod jobs;
pub mod leagues;
pub mod mailer;
pub mod matches;
pub mod models;
pub mod notify;
mod openapi;
pub mod players;
pub mod push;
pub mod rankings;
pub mod seed;
pub mod telemetry;
pub mod tenancy;
pub mod web;

pub use app::{AppState, router};
pub use config::Config;
pub use error::{ApiError, ApiResult};
pub use tenancy::{Tenant, TenantTx};
