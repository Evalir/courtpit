//! The match state machine and who may drive it.
//!
//! ```text
//! proposed ──accept──▶ scheduled ──report──▶ reported ──confirm / timeout──▶ confirmed
//!    │                     │                    │
//!    │                     │                    └──dispute──▶ disputed ──admin──▶ resolved
//!    │                     └──no-show / admin──▶ walkover
//!    └──decline all / admin──▶ cancelled
//! ```
//!
//! Any player on a side acts for that side. League and tournament ("competitive") matches can
//! only be cancelled by an admin; friendly matches by either side too.

use serde::{Deserialize, Serialize};

use crate::score::Side;

/// Lifecycle status of a match.
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

/// The facts about a match the rules depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchCtx {
    /// Current status.
    pub status: MatchStatus,
    /// Whether the match is friendly or competitive.
    pub kind: MatchKind,
    /// Side that reported the current score, if any.
    pub reported_by: Option<Side>,
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

/// Applies `event` by `actor` and returns the new status.
pub fn transition(
    ctx: &MatchCtx,
    actor: Actor,
    event: Event,
) -> Result<MatchStatus, TransitionError> {
    use MatchStatus as Status;
    let invalid = || TransitionError::InvalidState {
        from: ctx.status,
        event: event.name(),
    };
    let player = match actor {
        Actor::Player(side) => Some(side),
        _ => None,
    };
    let opposite_of = |side: Side, msg| match player {
        Some(actor_side) if actor_side != side => Ok(()),
        _ => Err(TransitionError::Forbidden(msg)),
    };

    match (ctx.status, event) {
        (Status::Proposed | Status::Scheduled, Event::Propose) => {
            let _ = player.ok_or(TransitionError::Forbidden("only players propose times"))?;
            Ok(ctx.status)
        }
        (Status::Proposed | Status::Scheduled, Event::AcceptProposal { proposed_by }) => {
            opposite_of(proposed_by, "only the other side can accept a proposal")?;
            Ok(Status::Scheduled)
        }
        (Status::Proposed | Status::Scheduled, Event::DeclineProposal { proposed_by }) => {
            opposite_of(proposed_by, "only the other side can decline a proposal")?;
            Ok(ctx.status)
        }
        (Status::Scheduled, Event::Report) => {
            let _ = player.ok_or(TransitionError::Forbidden("only players report scores"))?;
            Ok(Status::Reported)
        }
        (Status::Reported, Event::Confirm | Event::Dispute) => {
            let reporter = ctx.reported_by.ok_or_else(invalid)?;
            opposite_of(
                reporter,
                "only the other side can confirm or dispute a score",
            )?;
            Ok(if event == Event::Confirm {
                Status::Confirmed
            } else {
                Status::Disputed
            })
        }
        (Status::Reported, Event::ConfirmTimeout) => match actor {
            Actor::System => Ok(Status::Confirmed),
            _ => Err(TransitionError::Forbidden(
                "only the system confirms on timeout",
            )),
        },
        (Status::Disputed, Event::Resolve(resolution)) => match actor {
            Actor::Admin => Ok(match resolution {
                Resolution::Score => Status::Resolved,
                Resolution::Replay => Status::Scheduled,
                Resolution::Void => Status::Cancelled,
            }),
            _ => Err(TransitionError::Forbidden(
                "only an admin resolves disputes",
            )),
        },
        (Status::Proposed | Status::Scheduled, Event::Walkover) => match actor {
            Actor::Admin | Actor::System => Ok(Status::Walkover),
            Actor::Player(_) => Err(TransitionError::Forbidden("only an admin awards walkovers")),
        },
        (Status::Proposed | Status::Scheduled, Event::Cancel) => match (actor, ctx.kind) {
            (Actor::Admin, _) | (Actor::Player(_), MatchKind::Friendly) => Ok(Status::Cancelled),
            (Actor::Player(_), MatchKind::Competitive) => Err(TransitionError::Forbidden(
                "only an admin can cancel a league match",
            )),
            (Actor::System, _) => Err(TransitionError::Forbidden(
                "the system does not cancel matches",
            )),
        },
        _ => Err(invalid()),
    }
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
    use MatchStatus as Status;

    const PLAYER_A: Actor = Actor::Player(Side::A);
    const PLAYER_B: Actor = Actor::Player(Side::B);
    const ACTORS: [Actor; 4] = [PLAYER_A, PLAYER_B, Actor::Admin, Actor::System];

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

    fn ctx(status: MatchStatus, kind: MatchKind) -> MatchCtx {
        let reported_by = matches!(status, Status::Reported | Status::Disputed).then_some(Side::A);
        MatchCtx {
            status,
            kind,
            reported_by,
        }
    }

    /// The complete list of allowed (status, kind, actor, event) → status, with the score
    /// reported by side A. Everything not listed must be refused.
    fn expected(status: Status, kind: MatchKind, actor: Actor, event: Event) -> Option<Status> {
        let open = matches!(status, Status::Proposed | Status::Scheduled);
        let is_player = matches!(actor, Actor::Player(_));
        match event {
            Event::Propose if open && is_player => Some(status),
            Event::AcceptProposal { proposed_by }
                if open && actor == Actor::Player(proposed_by.other()) =>
            {
                Some(Status::Scheduled)
            }
            Event::DeclineProposal { proposed_by }
                if open && actor == Actor::Player(proposed_by.other()) =>
            {
                Some(status)
            }
            Event::Report if status == Status::Scheduled && is_player => Some(Status::Reported),
            Event::Confirm if status == Status::Reported && actor == PLAYER_B => {
                Some(Status::Confirmed)
            }
            Event::Dispute if status == Status::Reported && actor == PLAYER_B => {
                Some(Status::Disputed)
            }
            Event::ConfirmTimeout if status == Status::Reported && actor == Actor::System => {
                Some(Status::Confirmed)
            }
            Event::Resolve(resolution) if status == Status::Disputed && actor == Actor::Admin => {
                Some(match resolution {
                    Resolution::Score => Status::Resolved,
                    Resolution::Replay => Status::Scheduled,
                    Resolution::Void => Status::Cancelled,
                })
            }
            Event::Walkover if open && matches!(actor, Actor::Admin | Actor::System) => {
                Some(Status::Walkover)
            }
            Event::Cancel
                if open
                    && (actor == Actor::Admin || (is_player && kind == MatchKind::Friendly)) =>
            {
                Some(Status::Cancelled)
            }
            _ => None,
        }
    }

    #[test]
    fn exhaustive_transition_matrix() {
        let mut allowed = 0;
        for status in Status::ALL {
            for kind in [MatchKind::Friendly, MatchKind::Competitive] {
                for actor in ACTORS {
                    for event in events() {
                        let got = transition(&ctx(status, kind), actor, event).ok();
                        let want = expected(status, kind, actor, event);
                        assert_eq!(got, want, "{status:?} {kind:?} {actor:?} {event:?}");
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
        let mut match_ctx = ctx(Status::Proposed, MatchKind::Competitive);
        match_ctx.status = transition(
            &match_ctx,
            PLAYER_B,
            Event::AcceptProposal {
                proposed_by: Side::A,
            },
        )
        .unwrap();
        assert_eq!(match_ctx.status, Status::Scheduled);
        match_ctx.status = transition(&match_ctx, PLAYER_A, Event::Report).unwrap();
        match_ctx.reported_by = Some(Side::A);
        assert_eq!(match_ctx.status, Status::Reported);
        assert_eq!(
            transition(&match_ctx, PLAYER_A, Event::Confirm),
            Err(TransitionError::Forbidden(
                "only the other side can confirm or dispute a score"
            ))
        );
        assert_eq!(
            transition(&match_ctx, PLAYER_B, Event::Confirm),
            Ok(Status::Confirmed)
        );
    }

    #[test]
    fn terminal_states_accept_nothing() {
        for status in Status::ALL
            .into_iter()
            .filter(|status| status.is_terminal())
        {
            for actor in ACTORS {
                for event in events() {
                    let _ =
                        transition(&ctx(status, MatchKind::Friendly), actor, event).unwrap_err();
                }
            }
        }
        assert!(
            Status::Confirmed.has_result()
                && Status::Walkover.has_result()
                && !Status::Cancelled.has_result()
        );
    }

    #[test]
    fn invalid_state_vs_forbidden() {
        let match_ctx = ctx(Status::Confirmed, MatchKind::Friendly);
        assert!(matches!(
            transition(&match_ctx, PLAYER_A, Event::Report),
            Err(TransitionError::InvalidState { .. })
        ));
        let match_ctx = ctx(Status::Scheduled, MatchKind::Competitive);
        assert!(matches!(
            transition(&match_ctx, PLAYER_A, Event::Cancel),
            Err(TransitionError::Forbidden(_))
        ));
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
            serde_json::to_string(&Status::Walkover).unwrap(),
            "\"walkover\""
        );
        assert_eq!(
            serde_json::from_str::<Resolution>("\"replay\"").unwrap(),
            Resolution::Replay
        );
    }
}
