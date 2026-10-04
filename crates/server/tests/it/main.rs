//! Integration tests. One binary (`it`) with a module per area keeps link times down.
#![expect(
    clippy::unwrap_used,
    reason = "a failed harness step should abort the test"
)]

mod auth;
mod backup;
#[expect(
    dead_code,
    reason = "helpers are shared; not every module uses every one"
)]
mod common;
mod directory;
mod health;
mod jobs;
mod league_entries;
mod league_lifecycle;
mod league_season;
mod leagues;
mod match_requests;
mod match_results;
mod matches;
mod oidc;
mod player_names;
mod pooling;
mod profiles;
mod proposals;
mod rankings;
mod schema;
mod seed;
mod tenancy;
