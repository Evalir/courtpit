//! League box standings from results.

use std::{collections::HashMap, hash::Hash};

use crate::{LeagueMatchPoints, Outcome, ScoreSummary, Side};

/// A finished match between two entries of a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxResult<T> {
    /// Entry on side A.
    pub side_a: T,
    /// Entry on side B.
    pub side_b: T,
    /// How the match ended.
    pub outcome: Outcome,
    /// Sets and games, for tie-breaks; `None` for walkovers.
    pub summary: Option<ScoreSummary>,
}

/// One line of the standings table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StandingRow<T> {
    /// The entry this row is for.
    pub entry: T,
    /// Matches played.
    pub played: u32,
    /// Matches won.
    pub won: u32,
    /// Matches lost.
    pub lost: u32,
    /// League points earned.
    pub points: u32,
    /// Sets won.
    pub sets_won: u32,
    /// Sets lost.
    pub sets_lost: u32,
    /// Games won.
    pub games_won: u32,
    /// Games lost.
    pub games_lost: u32,
}

impl<T> StandingRow<T> {
    const fn new(entry: T) -> Self {
        Self {
            entry,
            played: 0,
            won: 0,
            lost: 0,
            points: 0,
            sets_won: 0,
            sets_lost: 0,
            games_won: 0,
            games_lost: 0,
        }
    }

    fn set_diff(&self) -> i64 {
        i64::from(self.sets_won) - i64::from(self.sets_lost)
    }

    fn game_diff(&self) -> i64 {
        i64::from(self.games_won) - i64::from(self.games_lost)
    }
}

/// Standings for `entries`, best first. Order: points, wins, set difference, game
/// difference, then entry id (deterministic). Every entry appears, even with no results;
/// results naming unknown entries are ignored.
#[must_use]
pub fn standings<T: Ord + Copy + Hash>(
    entries: &[T],
    results: &[BoxResult<T>],
    points: &LeagueMatchPoints,
) -> Vec<StandingRow<T>> {
    let mut rows: HashMap<T, StandingRow<T>> =
        entries.iter().map(|&entry| (entry, StandingRow::new(entry))).collect();
    for result in results {
        if !(rows.contains_key(&result.side_a) && rows.contains_key(&result.side_b)) {
            continue;
        }
        let (pa, pb) = points.points(result.outcome);
        let winner = result.outcome.winner();
        for (entry, side, pts) in [(result.side_a, Side::A, pa), (result.side_b, Side::B, pb)] {
            let Some(row) = rows.get_mut(&entry) else {
                continue;
            };
            row.played += 1;
            row.points += pts;
            if side == winner {
                row.won += 1;
            } else {
                row.lost += 1;
            }
            if let Some(summary) = result.summary {
                let (sets_for, sets_against, games_for, games_against) = match side {
                    Side::A => (summary.sets_a, summary.sets_b, summary.games_a, summary.games_b),
                    Side::B => (summary.sets_b, summary.sets_a, summary.games_b, summary.games_a),
                };
                row.sets_won += u32::from(sets_for);
                row.sets_lost += u32::from(sets_against);
                row.games_won += u32::from(games_for);
                row.games_lost += u32::from(games_against);
            }
        }
    }
    let mut table: Vec<StandingRow<T>> = rows.into_values().collect();
    table.sort_by(|x, y| {
        y.points
            .cmp(&x.points)
            .then(y.won.cmp(&x.won))
            .then(y.set_diff().cmp(&x.set_diff()))
            .then(y.game_diff().cmp(&x.game_diff()))
            .then(x.entry.cmp(&y.entry))
    });
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MatchFormat, Score, SetScore, validate_score};

    fn played(entry_a: u8, entry_b: u8, sets: &[(u16, u16)]) -> BoxResult<u8> {
        let mut score = Score { sets: sets.iter().map(|&(x, y)| SetScore::games(x, y)).collect() };
        if score.sets.len() == 3 {
            let last = score.sets[2];
            score.sets[2] = SetScore::tiebreak(last.a, last.b);
        }
        let summary = validate_score(&MatchFormat::default(), &score).unwrap();
        BoxResult {
            side_a: entry_a,
            side_b: entry_b,
            outcome: Outcome::from_summary(&summary),
            summary: Some(summary),
        }
    }

    #[test]
    fn table_orders_by_points_then_tiebreaks() {
        let results = [
            played(1, 2, &[(6, 0), (6, 0)]),          // 1: 3, 2: 0
            played(3, 1, &[(6, 4), (4, 6), (10, 8)]), // 3: 2, 1: 1
            played(2, 3, &[(6, 3), (6, 3)]),          // 2: 3, 3: 0
            BoxResult {
                side_a: 4,
                side_b: 2,
                outcome: Outcome::Walkover { winner: Side::A },
                summary: None,
            }, // 4: 2
        ];
        let table = standings(&[1, 2, 3, 4, 5], &results, &LeagueMatchPoints::default());
        let order: Vec<(u8, u32)> = table.iter().map(|row| (row.entry, row.points)).collect();
        // 1 has 4 points; 2 has 3; 3 and 4 have 2 (3 has 1 win, 4 has 1 win; 3 set diff 1-2=-1,
        // 4 set diff 0 -> 4 first); 5 has nothing.
        assert_eq!(order, vec![(1, 4), (2, 3), (4, 2), (3, 2), (5, 0)]);
        let one = table[0];
        assert_eq!((one.played, one.won, one.lost), (2, 1, 1));
        assert_eq!((one.sets_won, one.sets_lost), (3, 2));
        assert_eq!((one.games_won, one.games_lost), (22, 11), "12-0 plus 10-11");
        let four = table[2];
        assert_eq!((four.played, four.sets_won, four.games_won), (1, 0, 0));
    }

    #[test]
    fn ties_fall_back_to_entry_order_and_unknown_entries_are_ignored() {
        let table = standings(
            &[9, 3, 7],
            &[played(1, 3, &[(6, 0), (6, 0)])],
            &LeagueMatchPoints::default(),
        );
        assert_eq!(table.iter().map(|row| row.entry).collect::<Vec<_>>(), vec![3, 7, 9]);
        assert!(table.iter().all(|row| row.played == 0));
    }
}
