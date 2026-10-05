//! Racquet Collective domain logic.
//!
//! Everything in this crate is pure: no IO, no async runtime, no database. Rules that decide
//! outcomes (score validation, match state transitions, pairings, scoring) live here so they
//! can be unit tested exhaustively. The server crate maps between these types and storage.
#![deny(unsafe_code)]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]

pub mod discipline;
pub mod eligibility;
pub mod ids;
pub mod pairing;
pub mod placement;
pub mod score;
pub mod scoring;
pub mod standings;

pub use discipline::Discipline;
pub use eligibility::{Gender, MixedEligibility, MixedError};
pub use ids::{CommunityId, DivisionId, EntryId, LeagueId, MatchId, PlayerId};
pub use pairing::{Pairing, round_robin, single_elimination};
pub use placement::{
    BoxSize, MIN_ENTRIES, Placed, PlacementError, Previous, Seed, Unplayable, box_sizes, movements,
    place, playable,
};
pub use scoring::{
    LeagueMatchPoints, MixedPooling, Outcome, RankingEvent, RankingSource, RoundReached,
    ScoringConfig, SeasonPoints, TournamentPoints, league_match_events,
};
pub use standings::{BoxResult, StandingRow, standings};

pub use score::{
    Deuce, FinalSet, MatchFormat, Score, ScoreError, ScoreSummary, SetScore, Side, validate_score,
};

pub mod match_state;

pub use match_state::{
    Actor, Event, MatchKind, MatchState, MatchStatus, Resolution, TransitionError, side_of,
};
