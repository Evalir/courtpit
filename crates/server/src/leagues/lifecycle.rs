//! The date-driven league lifecycle, run by the `advance_league` job:
//! `draft → registration → active → finished`.
//!
//! Each run applies every step that is due at the clock's `now` (so a late job catches up),
//! then re-enqueues itself for the next date. Steps are idempotent per status, so retries and
//! duplicate deliveries are harmless.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use courtpit_domain::{
    EntryId, PlayerId, Previous, RankingEvent, RankingSource, Seed, movements, place, round_robin,
};
use rust_decimal::Decimal;
use sqlx::FromRow;
use uuid::Uuid;

use super::{LeagueRow, LeagueStatus, load, standings};
use crate::{
    ApiError, AppState, Tenant, TenantTx,
    jobs::{self, Job},
    matches::{self, LeagueSlot, NewMatch},
    rankings,
};

/// Entries promoted / relegated per box at season end (spec default: top two, bottom two).
const PROMOTED: usize = 2;
const RELEGATED: usize = 2;

/// The job handler.
pub async fn advance(state: &AppState, community_id: Uuid, league_id: Uuid) -> anyhow::Result<()> {
    let Some(tenant) = Tenant::load(&state.db, community_id).await? else {
        return Ok(());
    };
    let now = state.clock.now();
    let mut tx = TenantTx::begin(&state.db, community_id).await?;
    let league = match load(&mut tx, league_id, true).await {
        Ok(league) => league,
        Err(ApiError::NotFound(_)) => return Ok(()),
        Err(err) => anyhow::bail!("loading league: {err}"),
    };
    let status = step(&mut tx, &tenant, &league, now)
        .await
        .map_err(|err| anyhow::anyhow!("advancing league {league_id}: {err}"))?;
    if let Some(at) = next_wake(&league, status, now) {
        let job = Job::AdvanceLeague {
            community_id,
            league_id,
        };
        jobs::enqueue(&mut *tx, job, at).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Applies every due step; returns the resulting status.
async fn step(
    tx: &mut TenantTx,
    tenant: &Tenant,
    league: &LeagueRow,
    now: DateTime<Utc>,
) -> Result<LeagueStatus, ApiError> {
    let mut status = league.status;
    loop {
        status = match status {
            LeagueStatus::Draft
                if league.published_at.is_some() && now >= league.registration_opens_at =>
            {
                set_status(tx, league.id, LeagueStatus::Registration).await?;
                LeagueStatus::Registration
            }
            LeagueStatus::Registration if now >= league.starts_at => {
                activate(tx, tenant, league).await?;
                LeagueStatus::Active
            }
            LeagueStatus::Active if now >= league.ends_at => {
                finish(tx, tenant, league, now).await?;
                LeagueStatus::Finished
            }
            _ => return Ok(status),
        };
    }
}

/// When the job should next look at the league, if ever.
fn next_wake(
    league: &LeagueRow,
    status: LeagueStatus,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let at = match status {
        LeagueStatus::Draft if league.published_at.is_some() => league.registration_opens_at,
        LeagueStatus::Registration => league.starts_at,
        LeagueStatus::Active => league.ends_at,
        _ => return None,
    };
    (at > now).then_some(at)
}

async fn set_status(tx: &mut TenantTx, id: Uuid, status: LeagueStatus) -> Result<(), ApiError> {
    let _ = sqlx::query(
        "UPDATE leagues SET status = $3, updated_at = now() WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(id)
    .bind(status)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[derive(Debug, FromRow)]
struct Confirmed {
    id: Uuid,
    player_ids: Vec<Uuid>,
    /// Mean UTR of the entry's players with a UTR (pairs are placed by their average).
    utr: Option<Decimal>,
}

/// `registration → active`: drops incomplete entries, places confirmed ones into boxes and
/// creates every box's round-robin matches (status `proposed`; players self-schedule).
async fn activate(tx: &mut TenantTx, tenant: &Tenant, league: &LeagueRow) -> Result<(), ApiError> {
    let _ = sqlx::query(
        "UPDATE league_entries SET status = 'withdrawn', invited_partner_id = NULL,
            updated_at = now()
         WHERE community_id = $1 AND league_id = $2
           AND status IN ('pending_partner', 'pending_payment')",
    )
    .bind(tx.community_id())
    .bind(league.id)
    .execute(&mut **tx)
    .await?;
    let entries: Vec<Confirmed> = sqlx::query_as(
        "SELECT e.id, e.player_ids,
                (SELECT round(avg(p.utr), 2) FROM players p
                  WHERE p.community_id = e.community_id AND p.id = ANY(e.player_ids)) AS utr
         FROM league_entries e
         WHERE e.community_id = $1 AND e.league_id = $2 AND e.status = 'confirmed'
         ORDER BY e.created_at, e.id",
    )
    .bind(tx.community_id())
    .bind(league.id)
    .fetch_all(&mut **tx)
    .await?;
    let previous = previous_results(tx, league).await?;
    let seeds: Vec<Seed<EntryId>> = entries
        .iter()
        .map(|entry| Seed {
            id: EntryId(entry.id),
            utr: entry.utr,
            previous: previous.get(&sorted(&entry.player_ids)).copied(),
        })
        .collect();
    let boxes = place(&seeds, league.box_size()).map_err(|err| ApiError::Internal(err.into()))?;
    let players_of = |id: EntryId| {
        entries
            .iter()
            .find(|entry| entry.id == id.0)
            .map(|entry| entry.player_ids.clone())
            .unwrap_or_default()
    };
    let format = league.format(tenant)?;
    for placed in boxes {
        let division_id = Uuid::now_v7();
        let _ = sqlx::query(
            "INSERT INTO league_divisions (id, community_id, league_id, name, tier, utr_min, utr_max)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(division_id)
        .bind(tx.community_id())
        .bind(league.id)
        .bind(format!("Box {}", placed.tier))
        .bind(i32::try_from(placed.tier).unwrap_or(i32::MAX))
        .bind(placed.utr_min)
        .bind(placed.utr_max)
        .execute(&mut **tx)
        .await?;
        let ids: Vec<Uuid> = placed.entries.iter().map(|entry| entry.0).collect();
        let _ = sqlx::query(
            "UPDATE league_entries SET division_id = $3, updated_at = now()
             WHERE community_id = $1 AND league_id = $2 AND id = ANY($4)",
        )
        .bind(tx.community_id())
        .bind(league.id)
        .bind(division_id)
        .bind(&ids)
        .execute(&mut **tx)
        .await?;
        for (round, pairs) in round_robin(&placed.entries).into_iter().enumerate() {
            for pair in pairs {
                let (side_a, side_b) = (players_of(pair.a), players_of(pair.b));
                let _ = matches::insert(
                    tx,
                    &NewMatch {
                        discipline: league.discipline(),
                        side_a: &side_a,
                        side_b: &side_b,
                        format,
                        created_by: None,
                        league: Some(LeagueSlot {
                            league_id: league.id,
                            division_id,
                            round: i32::try_from(round + 1).unwrap_or(i32::MAX),
                        }),
                    },
                )
                .await?;
            }
        }
    }
    set_status(tx, league.id, LeagueStatus::Active).await?;
    tracing::info!(league = %league.id, entries = entries.len(), "league activated");
    Ok(())
}

fn sorted(ids: &[Uuid]) -> Vec<Uuid> {
    let mut sorted_ids = ids.to_vec();
    sorted_ids.sort_unstable();
    sorted_ids
}

#[derive(FromRow)]
struct PreviousResult {
    player_ids: Vec<Uuid>,
    tier: i32,
    movement: i16,
}

/// Last season's tier and movement by (sorted) player set: a returning singles player or an
/// unchanged doubles pair. New pairs count as newcomers.
async fn previous_results(
    tx: &mut TenantTx,
    league: &LeagueRow,
) -> Result<HashMap<Vec<Uuid>, Previous>, ApiError> {
    let Some(previous) = league.previous_league_id else {
        return Ok(HashMap::new());
    };
    let rows: Vec<PreviousResult> = sqlx::query_as(
        "SELECT player_ids, tier, movement FROM league_results
         WHERE community_id = $1 AND league_id = $2",
    )
    .bind(tx.community_id())
    .bind(previous)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .into_iter()
        .map(|result| {
            let prev = Previous {
                tier: u32::try_from(result.tier).unwrap_or(1),
                movement: i8::try_from(result.movement).unwrap_or(0),
            };
            (sorted(&result.player_ids), prev)
        })
        .collect())
}

/// `active → finished`: closes unplayed matches, records final standings with promotion and
/// relegation, and writes season points to the ranking ledger (each partner in full).
async fn finish(
    tx: &mut TenantTx,
    tenant: &Tenant,
    league: &LeagueRow,
    now: DateTime<Utc>,
) -> Result<(), ApiError> {
    let _ = sqlx::query(
        "UPDATE matches SET status = 'cancelled', resolution_note = 'season ended',
            updated_at = now()
         WHERE community_id = $1 AND league_id = $2 AND status IN ('proposed', 'scheduled')",
    )
    .bind(tx.community_id())
    .bind(league.id)
    .execute(&mut **tx)
    .await?;
    let config = league.scoring(&tenant.scoring_config.0)?;
    let boxes = standings::compute(tx, league, &config.league_match).await?;
    let tiers = u32::try_from(boxes.len()).unwrap_or(u32::MAX);
    let discipline = league.discipline();
    let mut events = Vec::new();
    for division in &boxes {
        let tier = u32::try_from(division.tier).unwrap_or(u32::MAX);
        let moves = movements(division.table.len(), tier, tiers, PROMOTED, RELEGATED);
        for (line, movement) in division.table.iter().zip(moves) {
            let position = usize::try_from(line.position).unwrap_or(usize::MAX);
            let season_points = config.league_season.points(position, tier);
            let _ = sqlx::query(
                "INSERT INTO league_results (community_id, league_id, entry_id, division_id,
                    player_ids, tier, position, points, season_points, movement)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                 ON CONFLICT (league_id, entry_id) DO NOTHING",
            )
            .bind(tx.community_id())
            .bind(league.id)
            .bind(line.entry_id)
            .bind(division.division_id)
            .bind(&line.player_ids)
            .bind(division.tier)
            .bind(i32::try_from(line.position).unwrap_or(i32::MAX))
            .bind(i32::try_from(line.points).unwrap_or(i32::MAX))
            .bind(i32::try_from(season_points).unwrap_or(i32::MAX))
            .bind(i16::from(movement))
            .execute(&mut **tx)
            .await?;
            events.extend(line.player_ids.iter().map(|&player| RankingEvent {
                player: PlayerId(player),
                discipline,
                source: RankingSource::LeagueSeason,
                points: season_points,
            }));
        }
    }
    rankings::append(tx, league.id, &events, now).await?;
    set_status(tx, league.id, LeagueStatus::Finished).await?;
    tracing::info!(league = %league.id, boxes = boxes.len(), "league finished");
    Ok(())
}
