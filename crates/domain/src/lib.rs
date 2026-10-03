//! Courtpit domain logic.
//!
//! Everything in this crate is pure: no IO, no async runtime, no database. Rules that decide
//! outcomes (score validation, match state transitions, pairings, scoring) live here so they
//! can be unit tested exhaustively. The server crate maps between these types and storage.
#![deny(unsafe_code)]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]

pub mod score;

pub use score::{
    FinalSet, MatchFormat, Score, ScoreError, ScoreSummary, SetScore, Side, validate_score,
};
