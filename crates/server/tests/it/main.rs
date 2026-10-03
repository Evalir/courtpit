//! Integration tests. One binary (`it`) with a module per area keeps link times down.
#![expect(
    clippy::unwrap_used,
    reason = "a failed harness step should abort the test"
)]

mod auth;
#[expect(
    dead_code,
    reason = "helpers are shared; not every module uses every one"
)]
mod common;
mod directory;
mod health;
mod jobs;
mod league_entries;
mod leagues;
mod match_requests;
mod match_results;
mod matches;
mod oidc;
mod profiles;
mod proposals;
mod schema;
mod tenancy;
