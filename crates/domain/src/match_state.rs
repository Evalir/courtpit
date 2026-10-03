//! The match state machine and who may drive it.
//!
//! ```text
//!    ┌───────────────── report ─────────────────┐
//!    │                                          ▼
//! proposed ──accept──▶ scheduled ──report──▶ reported ──confirm / timeout──▶ confirmed
//!    │                     │                    │
//!    │                     │                    └──dispute──▶ disputed ──admin──▶ resolved
//!    │                     └──no-show / admin──▶ walkover
//!    └──decline all / admin──▶ cancelled
//! ```
//!
//! Reporting a score implies the match was played, so a player may report straight from
//! `proposed` without agreeing a time first.
//!
//! [`MatchState`] is the machine: [`MatchState::step`] applies an [`Event`] by an [`Actor`] and
//! returns the next state. Any player on a side acts for that side. League and tournament
//! ("competitive") matches can only be cancelled by an admin; friendly matches by either side
//! too.

use serde::{Deserialize, Serialize};

use crate::score::Side;

/// Lifecycle status of a match: the flat tag of a [`MatchState`], as stored and serialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum MatchStatus {
    /// Time and place are still being negotiated.
    Proposed,
    /// A proposal was accepted; the match is waiting to be played.
    Scheduled,
    /// One side reported the score and the other has not answered yet.
    Reported,
    /// The reported score was confirmed, or the confirmation window lapsed.
    Confirmed,
    /// The other side disputed the reported score; an admin must decide.
    Disputed,
    /// An admin settled the dispute with a final score.
    Resolved,
    /// Awarded without play.
    Walkover,
    /// Called off; no result.
    Cancelled,
}

impl MatchStatus {
    /// Every status, for exhaustive tests and docs.
    pub const ALL: [Self; 8] = [
        Self::Proposed,
        Self::Scheduled,
        Self::Reported,
        Self::Confirmed,
        Self::Disputed,
        Self::Resolved,
        Self::Walkover,
        Self::Cancelled,
    ];

    /// No further transitions are possible.
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Confirmed | Self::Resolved | Self::Walkover | Self::Cancelled
        )
    }

    /// The result stands and counts for standings and rankings.
    pub const fn has_result(self) -> bool {
        matches!(self, Self::Confirmed | Self::Resolved | Self::Walkover)
    }
}

/// Where a match is in its lifecycle, carrying the facts later steps depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchState {
    /// Time and place are still being negotiated.
    Proposed,
    /// A proposal was accepted; the match is waiting to be played.
    Scheduled,
    /// A score was reported; only the other side may answer it.
    Reported {
        /// Side that reported the score.
        by: Side,
    },
    /// The reported score was confirmed, or the confirmation window lapsed.
    Confirmed,
    /// The reported score was disputed; an admin must decide.
    Disputed,
    /// An admin settled the dispute with a final score.
    Resolved,
    /// Awarded without play.
    Walkover,
    /// Called off; no result.
    Cancelled,
}

impl MatchState {
    /// Rebuilds the state from its stored parts. `None` if the match is reported but the
    /// reporter's side is unknown.
    pub const fn from_parts(status: MatchStatus, reported_by: Option<Side>) -> Option<Self> {
        Some(match status {
            MatchStatus::Proposed => Self::Proposed,
            MatchStatus::Scheduled => Self::Scheduled,
            MatchStatus::Reported => match reported_by {
                Some(by) => Self::Reported { by },
                None => return None,
            },
            MatchStatus::Confirmed => Self::Confirmed,
            MatchStatus::Disputed => Self::Disputed,
            MatchStatus::Resolved => Self::Resolved,
            MatchStatus::Walkover => Self::Walkover,
            MatchStatus::Cancelled => Self::Cancelled,
        })
    }

    /// The stored status tag.
    pub const fn status(self) -> MatchStatus {
        match self {
            Self::Proposed => MatchStatus::Proposed,
            Self::Scheduled => MatchStatus::Scheduled,
            Self::Reported { .. } => MatchStatus::Reported,
            Self::Confirmed => MatchStatus::Confirmed,
            Self::Disputed => MatchStatus::Disputed,
            Self::Resolved => MatchStatus::Resolved,
            Self::Walkover => MatchStatus::Walkover,
            Self::Cancelled => MatchStatus::Cancelled,
        }
    }

    /// Applies `event` by `actor` to a match of `kind` and returns the next state.
    pub fn step(
        self,
        kind: MatchKind,
        actor: Actor,
        event: Event,
    ) -> Result<Self, TransitionError> {
        match (self, event) {
            (Self::Proposed | Self::Scheduled, Event::Propose) => {
                actor.player("only players propose times").map(|_| self)
            }
            (Self::Proposed | Self::Scheduled, Event::AcceptProposal { proposed_by }) => actor
                .against(proposed_by, "only the other side can accept a proposal")
                .map(|()| Self::Scheduled),
            (Self::Proposed | Self::Scheduled, Event::DeclineProposal { proposed_by }) => actor
                .against(proposed_by, "only the other side can decline a proposal")
                .map(|()| self),
            (Self::Proposed | Self::Scheduled, Event::Walkover) => actor
                .officiating("only an admin awards walkovers")
                .map(|()| Self::Walkover),
            (Self::Proposed | Self::Scheduled, Event::Cancel) => {
                actor.may_cancel(kind).map(|()| Self::Cancelled)
            }
            (Self::Proposed | Self::Scheduled, Event::Report) => actor
                .player("only players report scores")
                .map(|by| Self::Reported { by }),
            (Self::Reported { by }, Event::Confirm) => {
                actor.against(by, ANSWER_REPORT).map(|()| Self::Confirmed)
            }
            (Self::Reported { by }, Event::Dispute) => {
                actor.against(by, ANSWER_REPORT).map(|()| Self::Disputed)
            }
            (Self::Reported { .. }, Event::ConfirmTimeout) => actor
                .exactly(Actor::System, "only the system confirms on timeout")
                .map(|()| Self::Confirmed),
            (Self::Disputed, Event::Resolve(resolution)) => actor
                .exactly(Actor::Admin, "only an admin resolves disputes")
                .map(|()| resolution.outcome()),
            _ => Err(TransitionError::InvalidState {
                from: self.status(),
                event: event.name(),
            }),
        }
    }
}

const ANSWER_REPORT: &str = "only the other side can confirm or dispute a score";

/// Friendly matches are informal; competitive ones belong to a league or tournament.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    /// Informal match outside any league or tournament.
    Friendly,
    /// Match that belongs to a league or tournament.
    Competitive,
}

/// Who is acting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    /// A player on this side of the match.
    Player(Side),
    /// A community admin.
    Admin,
    /// The job loop (timeouts, deadlines).
    System,
}

impl Actor {
    /// The acting player's side; anyone else is refused with `msg`.
    const fn player(self, msg: &'static str) -> Result<Side, TransitionError> {
        match self {
            Self::Player(side) => Ok(side),
            Self::Admin | Self::System => Err(TransitionError::Forbidden(msg)),
        }
    }

    /// Passes only for a player on the side opposite `side`.
    fn against(self, side: Side, msg: &'static str) -> Result<(), TransitionError> {
        match self {
            Self::Player(own) if own != side => Ok(()),
            _ => Err(TransitionError::Forbidden(msg)),
        }
    }

    /// Passes only for `who`.
    fn exactly(self, who: Self, msg: &'static str) -> Result<(), TransitionError> {
        if self == who {
            Ok(())
        } else {
            Err(TransitionError::Forbidden(msg))
        }
    }

    /// Passes for an admin or the system, never a player.
    const fn officiating(self, msg: &'static str) -> Result<(), TransitionError> {
        match self {
            Self::Admin | Self::System => Ok(()),
            Self::Player(_) => Err(TransitionError::Forbidden(msg)),
        }
    }

    /// Admins cancel any match; players only friendly ones.
    const fn may_cancel(self, kind: MatchKind) -> Result<(), TransitionError> {
        match (self, kind) {
            (Self::Admin, _) | (Self::Player(_), MatchKind::Friendly) => Ok(()),
            (Self::Player(_), MatchKind::Competitive) => Err(TransitionError::Forbidden(
                "only an admin can cancel a league match",
            )),
            (Self::System, _) => Err(TransitionError::Forbidden(
                "the system does not cancel matches",
            )),
        }
    }
}

/// What an admin decides about a disputed result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum Resolution {
    /// Set the score; the match is `resolved`.
    Score,
    /// The match is replayed; back to `scheduled`.
    Replay,
    /// The match is voided; `cancelled`.
    Void,
}

impl Resolution {
    /// Where a disputed match goes after this decision.
    const fn outcome(self) -> MatchState {
        match self {
            Self::Score => MatchState::Resolved,
            Self::Replay => MatchState::Scheduled,
            Self::Void => MatchState::Cancelled,
        }
    }
}

/// Something that happens to a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Make a (counter-)proposal of time and place.
    Propose,
    /// Accept a proposal made by `proposed_by`.
    AcceptProposal {
        /// Side that made the proposal being accepted.
        proposed_by: Side,
    },
    /// Decline a proposal made by `proposed_by` (the match stays open).
    DeclineProposal {
        /// Side that made the proposal being declined.
        proposed_by: Side,
    },
    /// Report the score.
    Report,
    /// Confirm the reported score.
    Confirm,
    /// Dispute the reported score.
    Dispute,
    /// The confirmation window passed without an answer.
    ConfirmTimeout,
    /// Admin decision on a dispute.
    Resolve(Resolution),
    /// Award the match without play (no-show, deadline).
    Walkover,
    /// Call the match off.
    Cancel,
}

impl Event {
    const fn name(&self) -> &'static str {
        match self {
            Self::Propose => "propose",
            Self::AcceptProposal { .. } => "accept",
            Self::DeclineProposal { .. } => "decline",
            Self::Report => "report",
            Self::Confirm => "confirm",
            Self::Dispute => "dispute",
            Self::ConfirmTimeout => "confirm timeout",
            Self::Resolve(_) => "resolve",
            Self::Walkover => "walkover",
            Self::Cancel => "cancel",
        }
    }
}

/// Why a transition is refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TransitionError {
    /// The event makes no sense in this status.
    #[error("cannot {event} a {from:?} match")]
    InvalidState {
        /// Status the match was in.
        from: MatchStatus,
        /// Name of the refused event.
        event: &'static str,
    },
    /// The event is valid but this actor may not perform it.
    #[error("{0}")]
    Forbidden(&'static str),
}

/// Which side `id` plays on, if any.
pub fn side_of<T: PartialEq>(id: &T, side_a: &[T], side_b: &[T]) -> Option<Side> {
    if side_a.contains(id) {
        Some(Side::A)
    } else if side_b.contains(id) {
        Some(Side::B)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use MatchState as State;

    const PLAYER_A: Actor = Actor::Player(Side::A);
    const PLAYER_B: Actor = Actor::Player(Side::B);
    const ACTORS: [Actor; 4] = [PLAYER_A, PLAYER_B, Actor::Admin, Actor::System];
    /// Every state, with the score reported by side A.
    const STATES: [State; 8] = [
        State::Proposed,
        State::Scheduled,
        State::Reported { by: Side::A },
        State::Confirmed,
        State::Disputed,
        State::Resolved,
        State::Walkover,
        State::Cancelled,
    ];

    fn events() -> Vec<Event> {
        let mut list = vec![
            Event::Propose,
            Event::Report,
            Event::Confirm,
            Event::Dispute,
            Event::ConfirmTimeout,
            Event::Walkover,
            Event::Cancel,
        ];
        for side in [Side::A, Side::B] {
            list.push(Event::AcceptProposal { proposed_by: side });
            list.push(Event::DeclineProposal { proposed_by: side });
        }
        for resolution in [Resolution::Score, Resolution::Replay, Resolution::Void] {
            list.push(Event::Resolve(resolution));
        }
        list
    }

    /// The complete list of allowed (state, kind, actor, event) → state, with the score
    /// reported by side A. Everything not listed must be refused.
    fn expected(state: State, kind: MatchKind, actor: Actor, event: Event) -> Option<State> {
        let open = matches!(state, State::Proposed | State::Scheduled);
        let reported = matches!(state, State::Reported { .. });
        let is_player = matches!(actor, Actor::Player(_));
        match event {
            Event::Propose if open && is_player => Some(state),
            Event::AcceptProposal { proposed_by }
                if open && actor == Actor::Player(proposed_by.other()) =>
            {
                Some(State::Scheduled)
            }
            Event::DeclineProposal { proposed_by }
                if open && actor == Actor::Player(proposed_by.other()) =>
            {
                Some(state)
            }
            Event::Report if open => match actor {
                Actor::Player(by) => Some(State::Reported { by }),
                Actor::Admin | Actor::System => None,
            },
            Event::Confirm if reported && actor == PLAYER_B => Some(State::Confirmed),
            Event::Dispute if reported && actor == PLAYER_B => Some(State::Disputed),
            Event::ConfirmTimeout if reported && actor == Actor::System => Some(State::Confirmed),
            Event::Resolve(resolution) if state == State::Disputed && actor == Actor::Admin => {
                Some(match resolution {
                    Resolution::Score => State::Resolved,
                    Resolution::Replay => State::Scheduled,
                    Resolution::Void => State::Cancelled,
                })
            }
            Event::Walkover if open && matches!(actor, Actor::Admin | Actor::System) => {
                Some(State::Walkover)
            }
            Event::Cancel
                if open
                    && (actor == Actor::Admin || (is_player && kind == MatchKind::Friendly)) =>
            {
                Some(State::Cancelled)
            }
            _ => None,
        }
    }

    #[test]
    fn exhaustive_transition_matrix() {
        let mut allowed = 0;
        for state in STATES {
            for kind in [MatchKind::Friendly, MatchKind::Competitive] {
                for actor in ACTORS {
                    for event in events() {
                        let got = state.step(kind, actor, event).ok();
                        let want = expected(state, kind, actor, event);
                        assert_eq!(got, want, "{state:?} {kind:?} {actor:?} {event:?}");
                        allowed += usize::from(got.is_some());
                    }
                }
            }
        }
        assert!(
            allowed > 40,
            "matrix should exercise many legal moves, got {allowed}"
        );
    }

    #[test]
    fn happy_path() {
        let kind = MatchKind::Competitive;
        let accept = Event::AcceptProposal {
            proposed_by: Side::A,
        };
        let state = State::Proposed.step(kind, PLAYER_B, accept).unwrap();
        assert_eq!(state, State::Scheduled);
        let state = state.step(kind, PLAYER_A, Event::Report).unwrap();
        assert_eq!(state, State::Reported { by: Side::A });
        assert_eq!(
            state.step(kind, PLAYER_A, Event::Confirm),
            Err(TransitionError::Forbidden(ANSWER_REPORT))
        );
        assert_eq!(
            state.step(kind, PLAYER_B, Event::Confirm),
            Ok(State::Confirmed)
        );
    }

    #[test]
    fn a_score_can_be_reported_straight_from_proposed() {
        for kind in [MatchKind::Friendly, MatchKind::Competitive] {
            for side in [Side::A, Side::B] {
                assert_eq!(
                    State::Proposed.step(kind, Actor::Player(side), Event::Report),
                    Ok(State::Reported { by: side })
                );
            }
            for actor in [Actor::Admin, Actor::System] {
                assert!(matches!(
                    State::Proposed.step(kind, actor, Event::Report),
                    Err(TransitionError::Forbidden(_))
                ));
            }
        }
    }

    #[test]
    fn terminal_states_accept_nothing() {
        for state in STATES
            .into_iter()
            .filter(|state| state.status().is_terminal())
        {
            for actor in ACTORS {
                for event in events() {
                    let _ = state.step(MatchKind::Friendly, actor, event).unwrap_err();
                }
            }
        }
        assert!(
            MatchStatus::Confirmed.has_result()
                && MatchStatus::Walkover.has_result()
                && !MatchStatus::Cancelled.has_result()
        );
    }

    #[test]
    fn invalid_state_vs_forbidden() {
        assert!(matches!(
            State::Confirmed.step(MatchKind::Friendly, PLAYER_A, Event::Report),
            Err(TransitionError::InvalidState { .. })
        ));
        assert!(matches!(
            State::Scheduled.step(MatchKind::Competitive, PLAYER_A, Event::Cancel),
            Err(TransitionError::Forbidden(_))
        ));
    }

    #[test]
    fn stored_parts_round_trip() {
        for state in STATES {
            let reported_by = match state {
                State::Reported { by } => Some(by),
                _ => None,
            };
            assert_eq!(State::from_parts(state.status(), reported_by), Some(state));
        }
        assert_eq!(State::from_parts(MatchStatus::Reported, None), None);
    }

    #[test]
    fn side_lookup() {
        assert_eq!(side_of(&1, &[1, 2], &[3, 4]), Some(Side::A));
        assert_eq!(side_of(&4, &[1, 2], &[3, 4]), Some(Side::B));
        assert_eq!(side_of(&5, &[1, 2], &[3, 4]), None);
    }

    #[test]
    fn status_json() {
        assert_eq!(
            serde_json::to_string(&MatchStatus::Walkover).unwrap(),
            "\"walkover\""
        );
        assert_eq!(
            serde_json::from_str::<Resolution>("\"replay\"").unwrap(),
            Resolution::Replay
        );
    }
}
