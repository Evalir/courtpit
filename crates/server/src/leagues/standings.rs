//! Box standings, computed from a league's matches that have a result.

use racquetcollective_domain::{
    BoxResult, EntryId, LeagueMatchPoints, MatchStatus, Outcome, StandingRow, standings,
};
use serde::Serialize;
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

use super::LeagueRow;
use crate::{
    ApiError, TenantTx,
    matches::{MATCH_COLUMNS, MatchRow, results::check_score},
    players::{Names, PlayerRef},
};

/// One line of a box table.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StandingLine {
    /// Rank within the box, starting at 1.
    pub position: u32,
    /// The league entry on this line.
    pub entry_id: Uuid,
    /// Players of the entry (one for singles, two for doubles).
    pub player_ids: Vec<Uuid>,
    /// Names of `player_ids`, in the same order.
    pub names: Vec<PlayerRef>,
    /// Matches with a result.
    pub played: u32,
    /// Matches won.
    pub won: u32,
    /// Matches lost.
    pub lost: u32,
    /// Table points under the league scoring rules.
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

/// A box with its table, best first.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DivisionStanding {
    /// The box.
    pub division_id: Uuid,
    /// Box name.
    pub name: String,
    /// Tier of the box; 1 is the top.
    pub tier: i32,
    /// The box table, best first.
    pub table: Vec<StandingLine>,
}

#[derive(FromRow)]
struct Division {
    id: Uuid,
    name: String,
    tier: i32,
}

#[derive(Clone, FromRow)]
struct PlacedEntry {
    id: Uuid,
    division_id: Uuid,
    player_ids: Vec<Uuid>,
}

/// The result of a league match for the table, if it has one.
fn box_result(
    match_row: &MatchRow,
    entries: &[PlacedEntry],
) -> Result<Option<BoxResult<EntryId>>, ApiError> {
    let entry_of = |players: &[Uuid]| {
        entries.iter().find(|entry| entry.player_ids == players).map(|entry| EntryId(entry.id))
    };
    let (Some(side_a_entry), Some(side_b_entry)) =
        (entry_of(&match_row.side_a_players), entry_of(&match_row.side_b_players))
    else {
        return Ok(None);
    };
    Ok(match (match_row.status(), match_row.winner_side, &match_row.score) {
        (MatchStatus::Walkover, Some(w), _) => Some(BoxResult {
            side_a: side_a_entry,
            side_b: side_b_entry,
            outcome: Outcome::Walkover { winner: w.into() },
            summary: None,
        }),
        (MatchStatus::Confirmed | MatchStatus::Resolved, Some(_), Some(score)) => {
            let summary = check_score(&match_row.match_format, score)?;
            Some(BoxResult {
                side_a: side_a_entry,
                side_b: side_b_entry,
                outcome: Outcome::from_summary(&summary),
                summary: Some(summary),
            })
        }
        _ => None,
    })
}

/// Standings of every box of `league`, top tier first. Every placed entry appears (withdrawn
/// ones keep the results they played).
pub async fn compute(
    tx: &mut TenantTx,
    league: &LeagueRow,
    points: &LeagueMatchPoints,
) -> Result<Vec<DivisionStanding>, ApiError> {
    let divisions: Vec<Division> = sqlx::query_as(
        "SELECT id, name, tier FROM league_divisions
         WHERE community_id = $1 AND league_id = $2 ORDER BY tier",
    )
    .bind(tx.community_id())
    .bind(league.id)
    .fetch_all(&mut **tx)
    .await?;
    let entries: Vec<PlacedEntry> = sqlx::query_as(
        "SELECT id, division_id, player_ids FROM league_entries
         WHERE community_id = $1 AND league_id = $2 AND division_id IS NOT NULL",
    )
    .bind(tx.community_id())
    .bind(league.id)
    .fetch_all(&mut **tx)
    .await?;
    let matches: Vec<MatchRow> = sqlx::query_as(&format!(
        "SELECT {MATCH_COLUMNS} FROM matches
         WHERE community_id = $1 AND league_id = $2
           AND status IN ('confirmed', 'resolved', 'walkover')"
    ))
    .bind(tx.community_id())
    .bind(league.id)
    .fetch_all(&mut **tx)
    .await?;
    let names = Names::load(tx, entries.iter().flat_map(|entry| entry.player_ids.clone())).await?;

    let mut out = Vec::with_capacity(divisions.len());
    for division in divisions {
        let members: Vec<PlacedEntry> =
            entries.iter().filter(|entry| entry.division_id == division.id).cloned().collect();
        let mut results = Vec::new();
        for match_row in
            matches.iter().filter(|match_row| match_row.division_id == Some(division.id))
        {
            results.extend(box_result(match_row, &members)?);
        }
        let ids: Vec<EntryId> = members.iter().map(|entry| EntryId(entry.id)).collect();
        let table = standings(&ids, &results, points)
            .into_iter()
            .zip(1..)
            .map(|(row, position): (StandingRow<EntryId>, u32)| {
                let player_ids = members
                    .iter()
                    .find(|entry| entry.id == row.entry.0)
                    .map(|entry| entry.player_ids.clone())
                    .unwrap_or_default();
                StandingLine {
                    position,
                    entry_id: row.entry.0,
                    names: names.refs(player_ids.iter().copied()),
                    player_ids,
                    played: row.played,
                    won: row.won,
                    lost: row.lost,
                    points: row.points,
                    sets_won: row.sets_won,
                    sets_lost: row.sets_lost,
                    games_won: row.games_won,
                    games_lost: row.games_lost,
                }
            })
            .collect();
        out.push(DivisionStanding {
            division_id: division.id,
            name: division.name,
            tier: division.tier,
            table,
        });
    }
    Ok(out)
}
