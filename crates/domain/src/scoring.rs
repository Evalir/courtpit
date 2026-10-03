//! Scoring rules (per community, overridable per league) and the ledger events they produce.
//!
//! The community ranking is a ledger: each scoring event appends a [`RankingEvent`]; a
//! player's ranking is the rolling 52-week sum of their events per discipline (computed by
//! the server). This module decides how many points each event is worth.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Discipline, PlayerId, ScoreSummary, Side};

/// Points for one league match, by result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LeagueMatchPoints {
    /// Points for a win in straight sets.
    pub win_straight: u32,
    /// Points for a win that went to a deciding set.
    pub win_deciding: u32,
    /// Points for a loss that went to a deciding set.
    pub loss_deciding: u32,
    /// Points for a loss in straight sets.
    pub loss_straight: u32,
    /// Points for a walkover win.
    pub walkover_win: u32,
    /// Points for a walkover loss.
    pub walkover_loss: u32,
}

impl Default for LeagueMatchPoints {
    fn default() -> Self {
        Self {
            win_straight: 3,
            win_deciding: 2,
            loss_deciding: 1,
            loss_straight: 0,
            walkover_win: 2,
            walkover_loss: 0,
        }
    }
}

/// End-of-season points by finishing position, scaled by the box's tier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SeasonPoints {
    /// Points for 1st, 2nd, ...; positions past the end get 0.
    pub position_points: Vec<u32>,
    /// Multiplier per tier (1 = top). Tiers not listed use the smallest listed multiplier.
    pub tier_multiplier: BTreeMap<u32, f64>,
}

impl Default for SeasonPoints {
    fn default() -> Self {
        Self {
            position_points: vec![100, 70, 50, 35, 25, 15, 10, 5],
            tier_multiplier: BTreeMap::from([(1, 1.0), (2, 0.7), (3, 0.5), (4, 0.35)]),
        }
    }
}

/// How far an entry got in a single-elimination tournament.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[non_exhaustive]
pub enum RoundReached {
    /// Won the tournament.
    Winner,
    /// Lost the final.
    Final,
    /// Lost in the semi-final.
    Semi,
    /// Lost in the quarter-final.
    Quarter,
    /// Lost in the round of 16.
    R16,
    /// Lost in the round of 32.
    R32,
    /// Lost earlier than the last 32.
    Earlier,
}

/// Percentage of the draw's base points per round reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RoundPointsPct {
    /// Percentage for the winner.
    pub winner: u32,
    #[serde(rename = "final")]
    /// Percentage for the losing finalist.
    pub final_: u32,
    /// Percentage for a semi-final loss.
    pub semi: u32,
    /// Percentage for a quarter-final loss.
    pub quarter: u32,
    /// Percentage for a round-of-16 loss.
    pub r16: u32,
    /// Percentage for a round-of-32 loss.
    pub r32: u32,
}

impl Default for RoundPointsPct {
    fn default() -> Self {
        Self {
            winner: 100,
            final_: 60,
            semi: 36,
            quarter: 18,
            r16: 9,
            r32: 4,
        }
    }
}

/// Tournament points: a base per draw size times the round-reached percentage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TournamentPoints {
    /// Percentage of the base points awarded per round reached.
    pub round_points_pct: RoundPointsPct,
    /// Base points by draw size. A draw size not listed uses the largest listed size below
    /// it (or the smallest listed one for draws smaller than all of them).
    pub base_by_draw_size: BTreeMap<u32, u32>,
}

impl Default for TournamentPoints {
    fn default() -> Self {
        Self {
            round_points_pct: RoundPointsPct::default(),
            base_by_draw_size: BTreeMap::from([(8, 100), (16, 150), (32, 250)]),
        }
    }
}

/// Whether mixed-doubles points count in their own ranking or in the doubles one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MixedPooling {
    #[default]
    /// Mixed points count in their own mixed ranking.
    Separate,
    /// Mixed points count in the doubles ranking.
    Doubles,
}

/// A community's scoring rules (spec §12). Every field defaults, so partial JSON overrides
/// (a league's `scoring_overrides` merged over the community config) deserialize.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScoringConfig {
    /// Points per league match result.
    pub league_match: LeagueMatchPoints,
    /// Points per end-of-season finishing position.
    pub league_season: SeasonPoints,
    /// Points per tournament round reached.
    pub tournament: TournamentPoints,
    /// Which ranking mixed-doubles points count towards.
    pub mixed_pooling: MixedPooling,
}

/// The result of a match as far as scoring cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Played out; `straight` when the loser won no set.
    Played {
        /// The side that won.
        winner: Side,
        /// Whether the loser won no set.
        straight: bool,
    },
    /// Awarded without play.
    Walkover {
        /// The side awarded the win.
        winner: Side,
    },
}

impl Outcome {
    /// The outcome of a validated score.
    #[must_use]
    pub fn from_summary(summary: &ScoreSummary) -> Self {
        Self::Played {
            winner: summary.winner,
            straight: summary.straight_sets(),
        }
    }

    /// The winning side.
    #[must_use]
    pub const fn winner(self) -> Side {
        match self {
            Self::Played { winner, .. } | Self::Walkover { winner } => winner,
        }
    }
}

impl LeagueMatchPoints {
    /// Points for (side A, side B).
    #[must_use]
    pub const fn points(&self, outcome: Outcome) -> (u32, u32) {
        let (win, loss) = match outcome {
            Outcome::Played { straight: true, .. } => (self.win_straight, self.loss_straight),
            Outcome::Played {
                straight: false, ..
            } => (self.win_deciding, self.loss_deciding),
            Outcome::Walkover { .. } => (self.walkover_win, self.walkover_loss),
        };
        match outcome.winner() {
            Side::A => (win, loss),
            Side::B => (loss, win),
        }
    }
}

impl SeasonPoints {
    /// Points for finishing at `position` (1-based) in a box of `tier`.
    #[must_use]
    pub fn points(&self, position: usize, tier: u32) -> u32 {
        let Some(&base) = position
            .checked_sub(1)
            .and_then(|i| self.position_points.get(i))
        else {
            return 0;
        };
        let multiplier = self.tier_multiplier.get(&tier).copied().unwrap_or_else(|| {
            self.tier_multiplier
                .values()
                .copied()
                .fold(f64::INFINITY, f64::min)
        });
        let multiplier = if multiplier.is_finite() {
            multiplier.max(0.0)
        } else {
            1.0
        };
        // Points are small integers; rounding to the nearest is the documented behaviour.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "small non-negative value, rounded"
        )]
        let points = (f64::from(base) * multiplier).round() as u32;
        points
    }
}

impl TournamentPoints {
    /// Points for reaching `round` in a draw of `draw_size`.
    #[must_use]
    pub fn points(&self, round: RoundReached, draw_size: u32) -> u32 {
        let base = self
            .base_by_draw_size
            .range(..=draw_size)
            .next_back()
            .or_else(|| self.base_by_draw_size.iter().next())
            .map_or(0, |(_, &base)| base);
        let pct = &self.round_points_pct;
        let pct = match round {
            RoundReached::Winner => pct.winner,
            RoundReached::Final => pct.final_,
            RoundReached::Semi => pct.semi,
            RoundReached::Quarter => pct.quarter,
            RoundReached::R16 => pct.r16,
            RoundReached::R32 => pct.r32,
            RoundReached::Earlier => 0,
        };
        // Integer maths, rounding half up.
        (base * pct + 50) / 100
    }
}

impl ScoringConfig {
    /// The ranking a `discipline`'s points count towards.
    #[must_use]
    pub const fn ranking_discipline(&self, discipline: Discipline) -> Discipline {
        match (discipline, self.mixed_pooling) {
            (Discipline::Mixed, MixedPooling::Doubles) => Discipline::Doubles,
            (kind, _) => kind,
        }
    }
}

/// What earned a ledger entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum RankingSource {
    /// Points from a league match.
    LeagueMatch,
    /// Points from a league season finishing position.
    LeagueSeason,
    /// Points from a tournament result.
    Tournament,
}

/// One ledger entry: `points` for `player` in the `discipline` ranking. The server adds the
/// source row id and the time it occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RankingEvent {
    /// The player who earned the points.
    pub player: PlayerId,
    /// The ranking the points count towards.
    pub discipline: Discipline,
    /// What earned the points.
    pub source: RankingSource,
    /// Points awarded.
    pub points: u32,
}

/// Ledger events for a league match: each player on a side gets that side's points (in
/// doubles each partner receives the full points). Events record the discipline actually
/// played; [`ScoringConfig::ranking_discipline`] pools mixed into doubles when the rankings
/// are computed, so changing `mixed_pooling` needs no ledger rewrite.
#[must_use]
pub fn league_match_events(
    points: &LeagueMatchPoints,
    discipline: Discipline,
    side_a: &[PlayerId],
    side_b: &[PlayerId],
    outcome: Outcome,
) -> Vec<RankingEvent> {
    let (pa, pb) = points.points(outcome);
    let event = |player: PlayerId, points| RankingEvent {
        player,
        discipline,
        source: RankingSource::LeagueMatch,
        points,
    };
    side_a
        .iter()
        .map(|&player| event(player, pa))
        .chain(side_b.iter().map(|&player| event(player, pb)))
        .collect()
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    const SPEC_JSON: &str = r#"{
      "league_match": { "win_straight": 3, "win_deciding": 2, "loss_deciding": 1, "loss_straight": 0,
                        "walkover_win": 2, "walkover_loss": 0 },
      "league_season": { "position_points": [100, 70, 50, 35, 25, 15, 10, 5],
                         "tier_multiplier": { "1": 1.0, "2": 0.7, "3": 0.5, "4": 0.35 } },
      "tournament": { "round_points_pct": { "winner": 100, "final": 60, "semi": 36, "quarter": 18,
                                            "r16": 9, "r32": 4 },
                      "base_by_draw_size": { "8": 100, "16": 150, "32": 250 } },
      "mixed_pooling": "separate"
    }"#;

    #[test]
    fn default_equals_the_spec_json() {
        let parsed: ScoringConfig = serde_json::from_str(SPEC_JSON).unwrap();
        assert_eq!(parsed, ScoringConfig::default());
        let round_trip: ScoringConfig =
            serde_json::from_value(serde_json::to_value(&parsed).unwrap()).unwrap();
        assert_eq!(round_trip, parsed);
    }

    #[test]
    fn partial_overrides_keep_defaults() {
        let config: ScoringConfig = serde_json::from_str(
            r#"{ "league_match": { "win_straight": 4 }, "mixed_pooling": "doubles" }"#,
        )
        .unwrap();
        assert_eq!(config.league_match.win_straight, 4);
        assert_eq!(config.league_match.win_deciding, 2);
        assert_eq!(config.league_season, SeasonPoints::default());
        assert_eq!(config.mixed_pooling, MixedPooling::Doubles);
        let _ = serde_json::from_str::<ScoringConfig>(r#"{ "typo": 1 }"#).unwrap_err();
    }

    #[test]
    fn league_match_points_table() {
        let points = LeagueMatchPoints::default();
        let played = |winner, straight| Outcome::Played { winner, straight };
        assert_eq!(points.points(played(Side::A, true)), (3, 0));
        assert_eq!(points.points(played(Side::A, false)), (2, 1));
        assert_eq!(points.points(played(Side::B, true)), (0, 3));
        assert_eq!(points.points(played(Side::B, false)), (1, 2));
        assert_eq!(points.points(Outcome::Walkover { winner: Side::B }), (0, 2));
    }

    #[test]
    fn outcome_from_a_score() {
        use crate::SetScore;
        let format = crate::MatchFormat::default();
        let summarize = |sets: Vec<SetScore>| {
            Outcome::from_summary(&crate::validate_score(&format, &crate::Score { sets }).unwrap())
        };
        assert_eq!(
            summarize(vec![SetScore::games(6, 1), SetScore::games(6, 2)]),
            Outcome::Played {
                winner: Side::A,
                straight: true
            }
        );
        assert_eq!(
            summarize(vec![
                SetScore::games(6, 1),
                SetScore::games(2, 6),
                SetScore::tiebreak(5, 10)
            ]),
            Outcome::Played {
                winner: Side::B,
                straight: false
            }
        );
    }

    #[test]
    fn season_points_by_position_and_tier() {
        let season = SeasonPoints::default();
        assert_eq!(season.points(1, 1), 100);
        assert_eq!(season.points(2, 1), 70);
        assert_eq!(season.points(1, 2), 70);
        assert_eq!(season.points(2, 2), 49);
        assert_eq!(season.points(3, 3), 25);
        assert_eq!(season.points(1, 4), 35);
        assert_eq!(season.points(4, 4), 12, "35 * 0.35 = 12.25");
        assert_eq!(
            season.points(1, 9),
            35,
            "unlisted tier uses the smallest multiplier"
        );
        assert_eq!(season.points(8, 1), 5);
        assert_eq!(season.points(9, 1), 0, "past the table");
        assert_eq!(season.points(0, 1), 0);
    }

    #[test]
    fn tournament_points_by_round_and_draw() {
        let tournament = TournamentPoints::default();
        assert_eq!(tournament.points(RoundReached::Winner, 8), 100);
        assert_eq!(tournament.points(RoundReached::Final, 8), 60);
        assert_eq!(tournament.points(RoundReached::Semi, 16), 54);
        assert_eq!(tournament.points(RoundReached::Quarter, 32), 45);
        assert_eq!(
            tournament.points(RoundReached::R16, 32),
            23,
            "22.5 rounds up"
        );
        assert_eq!(tournament.points(RoundReached::R32, 32), 10);
        assert_eq!(tournament.points(RoundReached::Earlier, 32), 0);
        assert_eq!(
            tournament.points(RoundReached::Winner, 24),
            150,
            "falls back to 16"
        );
        assert_eq!(
            tournament.points(RoundReached::Winner, 4),
            100,
            "smaller than all: 8"
        );
        assert_eq!(tournament.points(RoundReached::Winner, 64), 250);
    }

    #[test]
    fn match_events_give_each_partner_full_points() {
        let ids: Vec<PlayerId> = (0..4).map(|i| PlayerId(Uuid::from_u128(i))).collect();
        let outcome = Outcome::Played {
            winner: Side::B,
            straight: false,
        };
        let events = league_match_events(
            &LeagueMatchPoints::default(),
            Discipline::Mixed,
            &ids[..2],
            &ids[2..],
            outcome,
        );
        let pts: Vec<(PlayerId, u32)> = events
            .iter()
            .map(|event| (event.player, event.points))
            .collect();
        assert_eq!(
            pts,
            vec![(ids[0], 1), (ids[1], 1), (ids[2], 2), (ids[3], 2)]
        );
        assert!(
            events
                .iter()
                .all(|event| event.discipline == Discipline::Mixed
                    && event.source == RankingSource::LeagueMatch)
        );
    }

    #[test]
    fn mixed_pooling_maps_disciplines() {
        let separate = ScoringConfig::default();
        let pooled = ScoringConfig {
            mixed_pooling: MixedPooling::Doubles,
            ..ScoringConfig::default()
        };
        assert_eq!(
            separate.ranking_discipline(Discipline::Mixed),
            Discipline::Mixed
        );
        assert_eq!(
            pooled.ranking_discipline(Discipline::Mixed),
            Discipline::Doubles
        );
        for discipline in [Discipline::Singles, Discipline::Doubles] {
            assert_eq!(pooled.ranking_discipline(discipline), discipline);
        }
    }
}
