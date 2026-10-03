//! Match formats, set scores, validation and winner derivation.
//!
//! A score is a list of sets as reported, e.g. `6–4, 3–6, 10–7 (match tiebreak)`. Whether it is
//! legal depends on the [`MatchFormat`]: how many sets win the match, games per set, whether a
//! tiebreak is played at `games–games`, and how the deciding set is played.

use serde::{Deserialize, Serialize};

/// One side of a match. Doubles sides have two players; any of them acts for the side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[expect(
    clippy::min_ident_chars,
    reason = "A and B are the conventional side names and appear in the API"
)]
pub enum Side {
    /// The first side (the challenger or home side).
    A,
    /// The second side.
    B,
}

impl Side {
    /// The opposing side.
    #[must_use]
    pub const fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

/// How the deciding set (the last possible one) is played.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum FinalSet {
    /// A normal set.
    FullSet,
    /// A first-to-10, win-by-two match tiebreak, reported as one set with `match_tiebreak`.
    #[serde(rename = "match_tiebreak_10")]
    MatchTiebreak10,
    /// A pro set to 8 games (tiebreak at 8–8 when the format has tiebreaks).
    #[serde(rename = "pro_set_8")]
    ProSet8,
}

/// The rules of a match. Stored as JSON on communities, leagues and matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MatchFormat {
    /// Sets needed to win: 1, 2 (best of 3) or 3 (best of 5).
    pub sets_to_win: u8,
    /// Games needed to win a set (usually 6).
    pub games_per_set: u8,
    /// Play a tiebreak at `n–n`. Must equal `games_per_set`; `None` means advantage sets.
    #[serde(default)]
    pub tiebreak_at: Option<u8>,
    /// How the deciding set is played.
    pub final_set: FinalSet,
}

impl Default for MatchFormat {
    /// Best of three tiebreak sets with a 10-point match tiebreak instead of a third set.
    fn default() -> Self {
        Self {
            sets_to_win: 2,
            games_per_set: 6,
            tiebreak_at: Some(6),
            final_set: FinalSet::MatchTiebreak10,
        }
    }
}

/// Why a [`MatchFormat`] is unusable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FormatError {
    /// `sets_to_win` is outside 1 to 3.
    #[error("sets_to_win must be 1, 2 or 3")]
    SetsToWin,
    /// `games_per_set` is outside 1 to 9.
    #[error("games_per_set must be between 1 and 9")]
    GamesPerSet,
    /// `tiebreak_at` is set but differs from `games_per_set`.
    #[error("tiebreak_at must equal games_per_set when set")]
    TiebreakAt,
}

impl MatchFormat {
    /// Checks the format itself is sensible.
    pub fn validate(&self) -> Result<(), FormatError> {
        if !(1..=3).contains(&self.sets_to_win) {
            return Err(FormatError::SetsToWin);
        }
        if !(1..=9).contains(&self.games_per_set) {
            return Err(FormatError::GamesPerSet);
        }
        if self
            .tiebreak_at
            .is_some_and(|games| games != self.games_per_set)
        {
            return Err(FormatError::TiebreakAt);
        }
        Ok(())
    }

    /// Most sets a match can have.
    pub fn max_sets(&self) -> usize {
        usize::from(self.sets_to_win) * 2 - 1
    }
}

/// One set as reported. Games for normal sets, points for a match tiebreak.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[expect(
    clippy::min_ident_chars,
    reason = "`a` and `b` are the wire field names of the score API"
)]
pub struct SetScore {
    /// Games (or points) won by side A.
    pub a: u16,
    /// Games (or points) won by side B.
    pub b: u16,
    /// True when this "set" is a match tiebreak (points, not games).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub match_tiebreak: bool,
}

impl SetScore {
    /// A normal set.
    pub const fn games(side_a: u16, side_b: u16) -> Self {
        Self {
            a: side_a,
            b: side_b,
            match_tiebreak: false,
        }
    }

    /// A match tiebreak.
    pub const fn tiebreak(side_a: u16, side_b: u16) -> Self {
        Self {
            a: side_a,
            b: side_b,
            match_tiebreak: true,
        }
    }

    const fn winner_and_loser(&self) -> (Side, u16, u16) {
        if self.a >= self.b {
            (Side::A, self.a, self.b)
        } else {
            (Side::B, self.b, self.a)
        }
    }
}

/// A reported score.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Score {
    /// The sets in the order they were played.
    pub sets: Vec<SetScore>,
}

/// Why a score is illegal under a format.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ScoreError {
    /// The match format itself is invalid.
    #[error("invalid match format: {0}")]
    Format(#[from] FormatError),
    /// No sets were reported.
    #[error("a score needs at least one set")]
    Empty,
    /// A set score is not legal for its kind of set.
    #[error("set {set}: {games_a}-{games_b} is not a valid {kind}")]
    InvalidSet {
        /// One-based number of the offending set.
        set: usize,
        /// Games (or points) won by side A.
        games_a: u16,
        /// Games (or points) won by side B.
        games_b: u16,
        /// What the set should have been, e.g. `tiebreak set`.
        kind: &'static str,
    },
    /// The deciding set must be a match tiebreak but was reported as a normal set.
    #[error("set {set} must be reported as a match tiebreak")]
    MatchTiebreakExpected {
        /// One-based number of the offending set.
        set: usize,
    },
    /// A match tiebreak was reported where a normal set is required.
    #[error("set {set} cannot be a match tiebreak")]
    MatchTiebreakNotAllowed {
        /// One-based number of the offending set.
        set: usize,
    },
    /// A set was reported after one side had already won the match.
    #[error("set {set} was played after the match was already decided")]
    AfterMatchDecided {
        /// One-based number of the offending set.
        set: usize,
    },
    /// The sets reported do not decide the match.
    #[error("the match is incomplete: no side won {sets_to_win} sets")]
    Incomplete {
        /// Sets a side needed to win.
        sets_to_win: u8,
    },
}

/// The facts derived from a valid score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScoreSummary {
    /// The side that won the match.
    pub winner: Side,
    /// Sets won by side A.
    pub sets_a: u8,
    /// Sets won by side B.
    pub sets_b: u8,
    /// Games won by side A; a match tiebreak counts as one game for its winner.
    pub games_a: u16,
    /// Games won by side B (see `games_a`).
    pub games_b: u16,
}

impl ScoreSummary {
    /// Sets won by the loser.
    pub fn loser_sets(&self) -> u8 {
        self.sets_a.min(self.sets_b)
    }

    /// The winner dropped no set.
    pub fn straight_sets(&self) -> bool {
        self.loser_sets() == 0
    }
}

/// A normal set to `games`, with or without a tiebreak at `games–games`.
const fn valid_set(games: u16, tiebreak: bool, winner: u16, loser: u16) -> bool {
    if tiebreak {
        (winner == games && loser + 2 <= games)
            || (winner == games + 1 && (loser == games - 1 || loser == games))
    } else {
        winner >= games && winner - loser >= 2 && (winner == games || winner - loser == 2)
    }
}

/// First to 10 points, win by two.
const fn valid_match_tiebreak(winner: u16, loser: u16) -> bool {
    winner >= 10 && winner - loser >= 2 && (winner == 10 || winner - loser == 2)
}

/// Validates `score` against `format` and derives the winner.
pub fn validate_score(format: &MatchFormat, score: &Score) -> Result<ScoreSummary, ScoreError> {
    format.validate()?;
    if score.sets.is_empty() {
        return Err(ScoreError::Empty);
    }
    let to_win = format.sets_to_win;
    let games = u16::from(format.games_per_set);
    let tiebreak = format.tiebreak_at.is_some();
    let mut summary = ScoreSummary {
        winner: Side::A,
        sets_a: 0,
        sets_b: 0,
        games_a: 0,
        games_b: 0,
    };

    for (i, set) in score.sets.iter().enumerate() {
        let n = i + 1;
        if summary.sets_a == to_win || summary.sets_b == to_win {
            return Err(ScoreError::AfterMatchDecided { set: n });
        }
        let deciding = summary.sets_a == to_win - 1 && summary.sets_b == to_win - 1;
        let (winner, won, lost) = set.winner_and_loser();
        let invalid = |kind| ScoreError::InvalidSet {
            set: n,
            games_a: set.a,
            games_b: set.b,
            kind,
        };
        match (deciding, format.final_set) {
            (true, FinalSet::MatchTiebreak10) => {
                if !set.match_tiebreak {
                    return Err(ScoreError::MatchTiebreakExpected { set: n });
                }
                if !valid_match_tiebreak(won, lost) {
                    return Err(invalid("match tiebreak (first to 10, win by 2)"));
                }
            }
            _ if set.match_tiebreak => return Err(ScoreError::MatchTiebreakNotAllowed { set: n }),
            (true, FinalSet::ProSet8) => {
                if !valid_set(8, tiebreak, won, lost) {
                    return Err(invalid("pro set to 8"));
                }
            }
            _ => {
                if !valid_set(games, tiebreak, won, lost) {
                    return Err(invalid(if tiebreak {
                        "tiebreak set"
                    } else {
                        "advantage set"
                    }));
                }
            }
        }
        match winner {
            Side::A => summary.sets_a += 1,
            Side::B => summary.sets_b += 1,
        }
        let (ga, gb) = if set.match_tiebreak {
            if winner == Side::A { (1, 0) } else { (0, 1) }
        } else {
            (set.a, set.b)
        };
        summary.games_a += ga;
        summary.games_b += gb;
    }
    summary.winner = match (summary.sets_a == to_win, summary.sets_b == to_win) {
        (true, _) => Side::A,
        (_, true) => Side::B,
        _ => {
            return Err(ScoreError::Incomplete {
                sets_to_win: to_win,
            });
        }
    };
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn games(side_a: u16, side_b: u16) -> SetScore {
        SetScore::games(side_a, side_b)
    }
    fn tb(side_a: u16, side_b: u16) -> SetScore {
        SetScore::tiebreak(side_a, side_b)
    }
    fn check(format: &MatchFormat, sets: &[SetScore]) -> Result<ScoreSummary, ScoreError> {
        validate_score(
            format,
            &Score {
                sets: sets.to_vec(),
            },
        )
    }
    fn best_of_3_full() -> MatchFormat {
        MatchFormat {
            final_set: FinalSet::FullSet,
            ..MatchFormat::default()
        }
    }

    #[test]
    fn straight_sets_win() {
        let summary = check(&MatchFormat::default(), &[games(6, 4), games(6, 3)]).unwrap();
        assert_eq!(summary.winner, Side::A);
        assert!(summary.straight_sets());
        assert_eq!((summary.games_a, summary.games_b), (12, 7));
    }

    #[test]
    fn every_legal_tiebreak_set_score() {
        let format = best_of_3_full();
        for lost in 0..=4 {
            assert!(
                check(&format, &[games(6, lost), games(6, lost)]).is_ok(),
                "6-{lost}"
            );
            assert!(
                check(&format, &[games(lost, 6), games(lost, 6)]).is_ok(),
                "{lost}-6"
            );
        }
        let _ = check(&format, &[games(7, 5), games(7, 6)]).unwrap();
        assert_eq!(
            check(&format, &[games(5, 7), games(6, 7)]).unwrap().winner,
            Side::B
        );
    }

    #[test]
    fn illegal_tiebreak_set_scores() {
        let format = best_of_3_full();
        for (side_a, side_b) in [
            (6, 5),
            (6, 6),
            (7, 4),
            (8, 6),
            (7, 7),
            (5, 3),
            (0, 0),
            (7, 3),
        ] {
            let err = check(&format, &[games(side_a, side_b), games(6, 0)]).unwrap_err();
            assert!(
                matches!(err, ScoreError::InvalidSet { set: 1, .. }),
                "{side_a}-{side_b}: {err:?}"
            );
        }
    }

    #[test]
    fn advantage_sets_need_two_clear_games() {
        let format = MatchFormat {
            tiebreak_at: None,
            ..best_of_3_full()
        };
        for (side_a, side_b) in [(6, 4), (7, 5), (8, 6), (12, 10)] {
            assert!(
                check(&format, &[games(side_a, side_b), games(6, 0)]).is_ok(),
                "{side_a}-{side_b}"
            );
        }
        for (side_a, side_b) in [(7, 6), (8, 5), (6, 5), (9, 6)] {
            assert!(
                check(&format, &[games(side_a, side_b), games(6, 0)]).is_err(),
                "{side_a}-{side_b}"
            );
        }
    }

    #[test]
    fn match_tiebreak_decider() {
        let format = MatchFormat::default();
        let summary = check(&format, &[games(6, 4), games(3, 6), tb(10, 7)]).unwrap();
        assert_eq!(
            (summary.winner, summary.sets_a, summary.sets_b),
            (Side::A, 2, 1)
        );
        assert!(!summary.straight_sets());
        assert_eq!(
            (summary.games_a, summary.games_b),
            (10, 10),
            "tiebreak counts as one game"
        );
        let _ = check(&format, &[games(6, 4), games(3, 6), tb(11, 13)]).unwrap();
        for (side_a, side_b) in [(10, 9), (9, 7), (12, 9), (10, 10)] {
            let err = check(&format, &[games(6, 4), games(3, 6), tb(side_a, side_b)]).unwrap_err();
            assert!(
                matches!(err, ScoreError::InvalidSet { set: 3, .. }),
                "{side_a}-{side_b}"
            );
        }
    }

    #[test]
    fn match_tiebreak_flag_must_match_position() {
        let format = MatchFormat::default();
        assert_eq!(
            check(&format, &[games(6, 4), games(3, 6), games(6, 3)]).unwrap_err(),
            ScoreError::MatchTiebreakExpected { set: 3 }
        );
        assert_eq!(
            check(&format, &[tb(10, 3), games(6, 3)]).unwrap_err(),
            ScoreError::MatchTiebreakNotAllowed { set: 1 }
        );
        assert_eq!(
            check(&best_of_3_full(), &[games(6, 4), games(3, 6), tb(10, 3)]).unwrap_err(),
            ScoreError::MatchTiebreakNotAllowed { set: 3 }
        );
    }

    #[test]
    fn full_final_set() {
        let format = best_of_3_full();
        assert_eq!(
            check(&format, &[games(4, 6), games(6, 4), games(6, 7)])
                .unwrap()
                .winner,
            Side::B
        );
    }

    #[test]
    fn single_pro_set_to_eight() {
        let format = MatchFormat {
            sets_to_win: 1,
            final_set: FinalSet::ProSet8,
            ..MatchFormat::default()
        };
        for (side_a, side_b) in [(8, 0), (8, 6), (9, 7), (9, 8)] {
            assert!(
                check(&format, &[games(side_a, side_b)]).is_ok(),
                "{side_a}-{side_b}"
            );
        }
        for (side_a, side_b) in [(6, 4), (8, 7), (10, 8), (9, 6)] {
            assert!(
                check(&format, &[games(side_a, side_b)]).is_err(),
                "{side_a}-{side_b}"
            );
        }
        let adv = MatchFormat {
            tiebreak_at: None,
            ..format
        };
        let _ = check(&adv, &[games(10, 8)]).unwrap();
        let _ = check(&adv, &[games(9, 8)]).unwrap_err();
    }

    #[test]
    fn best_of_five() {
        let format = MatchFormat {
            sets_to_win: 3,
            ..best_of_3_full()
        };
        assert!(
            check(&format, &[games(6, 4), games(6, 4), games(6, 4)])
                .unwrap()
                .straight_sets()
        );
        let summary = check(
            &format,
            &[games(6, 4), games(3, 6), games(6, 4), games(6, 4)],
        )
        .unwrap();
        assert_eq!(
            (summary.sets_a, summary.sets_b, summary.loser_sets()),
            (3, 1, 1)
        );
        let summary = check(
            &format,
            &[
                games(6, 4),
                games(4, 6),
                games(6, 4),
                games(4, 6),
                games(5, 7),
            ],
        )
        .unwrap();
        assert_eq!(summary.winner, Side::B);
        assert!(matches!(
            check(&format, &[games(6, 4), games(6, 4)]).unwrap_err(),
            ScoreError::Incomplete { sets_to_win: 3 }
        ));
    }

    #[test]
    fn structural_errors() {
        let format = MatchFormat::default();
        assert_eq!(check(&format, &[]).unwrap_err(), ScoreError::Empty);
        assert_eq!(
            check(&format, &[games(6, 4)]).unwrap_err(),
            ScoreError::Incomplete { sets_to_win: 2 }
        );
        assert_eq!(
            check(&format, &[games(6, 4), games(6, 4), games(6, 4)]).unwrap_err(),
            ScoreError::AfterMatchDecided { set: 3 }
        );
    }

    #[test]
    fn bad_formats() {
        for format in [
            MatchFormat {
                sets_to_win: 0,
                ..MatchFormat::default()
            },
            MatchFormat {
                sets_to_win: 4,
                ..MatchFormat::default()
            },
            MatchFormat {
                games_per_set: 0,
                tiebreak_at: None,
                ..MatchFormat::default()
            },
            MatchFormat {
                tiebreak_at: Some(5),
                ..MatchFormat::default()
            },
        ] {
            assert!(
                matches!(
                    check(&format, &[games(6, 0), games(6, 0)]),
                    Err(ScoreError::Format(_))
                ),
                "{format:?}"
            );
        }
    }

    #[test]
    fn json_shapes_match_the_spec() {
        let format: MatchFormat = serde_json::from_str(
            r#"{"sets_to_win": 2, "games_per_set": 6, "tiebreak_at": 6, "final_set": "match_tiebreak_10"}"#,
        )
        .unwrap();
        assert_eq!(format, MatchFormat::default());
        let score: Score = serde_json::from_str(
            r#"{ "sets": [ {"a": 6, "b": 4}, {"a": 3, "b": 6}, {"a": 10, "b": 7, "match_tiebreak": true} ] }"#,
        )
        .unwrap();
        assert_eq!(validate_score(&format, &score).unwrap().winner, Side::A);
        assert_eq!(
            serde_json::to_string(&score.sets[0]).unwrap(),
            r#"{"a":6,"b":4}"#,
            "flag omitted when false"
        );
        let _ = serde_json::from_str::<MatchFormat>(r#"{"sets_to_win":2}"#).unwrap_err();
    }
}
