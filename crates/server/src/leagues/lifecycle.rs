//! The date-driven league lifecycle, run by the `advance_league` job:
//! `draft → registration → active → finished`.
//!
//! Each run applies every step that is due at the clock's `now` (so a late job catches up),
//! then re-enqueues itself for the next date. Steps are idempotent per status, so retries and
//! duplicate deliveries are harmless.
//!
//! Two rules keep a season from ending badly: a league that cannot field two entries (or two
//! per box) is cancelled at its start date instead of activated, and a season does not finish
//! while reported or disputed matches are waiting for a result.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use racquetcollective_domain::{
    EntryId, Placed, PlayerId, Previous, RankingEvent, RankingSource, Seed, movements, place,
    playable, round_robin,
};
use rust_decimal::Decimal;
use sqlx::FromRow;
use uuid::Uuid;

use super::{LeagueRow, LeagueStatus, load, standings};
use crate::{
    ApiError, AppState, Tenant, TenantTx,
    jobs::{self, Job},
    matches::{self, LeagueSlot, NewMatch},
    notify, rankings,
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
    let stepped = step(&mut tx, &tenant, &league, now)
        .await
        .map_err(|err| anyhow::anyhow!("advancing league {league_id}: {err}"))?;
    let wake = stepped
        .retry_at
        .or_else(|| next_wake(&league, stepped.status, now));
    if let Some(at) = wake {
        let job = Job::AdvanceLeague {
            community_id,
            league_id,
        };
        jobs::enqueue(&mut *tx, job, at).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Where a run of [`step`] ended.
struct Stepped {
    /// The league's status now.
    status: LeagueStatus,
    /// Set when the season end is deferred by unresolved matches: when to look again.
    retry_at: Option<DateTime<Utc>>,
}

/// Applies every due step.
async fn step(
    tx: &mut TenantTx,
    tenant: &Tenant,
    league: &LeagueRow,
    now: DateTime<Utc>,
) -> Result<Stepped, ApiError> {
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
                activate(tx, tenant, league, now).await?
            }
            LeagueStatus::Active if now >= league.ends_at => {
                match close_season(tx, tenant, league, now, false).await? {
                    Closing::Finished => LeagueStatus::Finished,
                    Closing::Blocked(open) => {
                        let at = open.retry_at(now);
                        tracing::info!(
                            league = %league.id, unresolved = open.count, retry_at = %at,
                            "season end deferred: matches still reported or disputed"
                        );
                        return Ok(Stepped {
                            status,
                            retry_at: Some(at),
                        });
                    }
                }
            }
            _ => {
                return Ok(Stepped {
                    status,
                    retry_at: None,
                });
            }
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
/// creates every box's round-robin matches (status `proposed`; players self-schedule). A league
/// that cannot field two entries (or two per box) is cancelled instead, with its entrants
/// told by email; returns the resulting status.
async fn activate(
    tx: &mut TenantTx,
    tenant: &Tenant,
    league: &LeagueRow,
    now: DateTime<Utc>,
) -> Result<LeagueStatus, ApiError> {
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
    if let Err(unplayable) = playable(entries.len(), &boxes) {
        cancel_unplayable(tx, league, &unplayable.to_string(), now).await?;
        return Ok(LeagueStatus::Cancelled);
    }
    schedule(tx, tenant, league, &entries, boxes).await?;
    set_status(tx, league.id, LeagueStatus::Active).await?;
    let players: Vec<Uuid> = entries
        .iter()
        .flat_map(|entry| entry.player_ids.iter().copied())
        .collect();
    let event = notify::Event::LeagueStarted {
        league_id: league.id,
    };
    notify::tell(tx, players, event, now)
        .await
        .map_err(ApiError::Internal)?;
    tracing::info!(league = %league.id, entries = entries.len(), "league activated");
    Ok(LeagueStatus::Active)
}

/// Cancels a league that cannot be played, storing `reason`, and queues an email to every
/// player on any of its entries (including the incomplete ones just withdrawn). The jobs are
/// enqueued in this transaction, so they exist exactly when the cancellation does.
async fn cancel_unplayable(
    tx: &mut TenantTx,
    league: &LeagueRow,
    reason: &str,
    now: DateTime<Utc>,
) -> Result<(), ApiError> {
    let _ = sqlx::query(
        "UPDATE leagues SET status = 'cancelled', cancel_reason = $3, updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(league.id)
    .bind(reason)
    .execute(&mut **tx)
    .await?;
    let players: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT member FROM league_entries e, unnest(e.player_ids) AS member
         WHERE e.community_id = $1 AND e.league_id = $2",
    )
    .bind(tx.community_id())
    .bind(league.id)
    .fetch_all(&mut **tx)
    .await?;
    for &player_id in &players {
        let job = Job::NotifyLeagueCancelled {
            community_id: tx.community_id(),
            league_id: league.id,
            player_id,
        };
        jobs::enqueue(&mut **tx, job, now).await?;
    }
    tracing::info!(league = %league.id, notified = players.len(), reason, "league cancelled at start");
    Ok(())
}

/// Records each placed box as a division and creates its round-robin matches.
async fn schedule(
    tx: &mut TenantTx,
    tenant: &Tenant,
    league: &LeagueRow,
    entries: &[Confirmed],
    boxes: Vec<Placed<EntryId>>,
) -> Result<(), ApiError> {
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

/// League matches still waiting for a result at season end.
#[derive(Debug, Clone, Copy, FromRow)]
pub struct Unresolved {
    /// Matches in `reported` or `disputed`.
    pub count: i64,
    /// The earliest auto-confirm deadline among the reported ones.
    pub next_deadline: Option<DateTime<Utc>>,
}

impl Unresolved {
    /// When to look at the season again: just after the earliest auto-confirm deadline (so the
    /// confirmation runs first), at least a minute from `now` so an overdue deadline cannot
    /// spin the job; a day from `now` when only disputes (an admin's call) remain.
    fn retry_at(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        self.next_deadline.map_or_else(
            || now + Duration::hours(24),
            |deadline| (deadline + Duration::minutes(1)).max(now + Duration::minutes(1)),
        )
    }
}

/// How [`close_season`] ended.
#[derive(Debug, Clone, Copy)]
pub enum Closing {
    /// The league is now `finished`.
    Finished,
    /// Reported or disputed matches remain and the finish was not forced.
    Blocked(Unresolved),
}

/// Ends a season at or after `ends_at`: cancels unplayed matches, then finishes the league.
/// Reported and disputed matches have no result yet and would be left out of the final table,
/// so unless `force` is set the league stays as it is while any exist.
pub async fn close_season(
    tx: &mut TenantTx,
    tenant: &Tenant,
    league: &LeagueRow,
    now: DateTime<Utc>,
    force: bool,
) -> Result<Closing, ApiError> {
    let _ = sqlx::query(
        "UPDATE matches SET status = 'cancelled', resolution_note = 'season ended',
            updated_at = now()
         WHERE community_id = $1 AND league_id = $2 AND status IN ('proposed', 'scheduled')",
    )
    .bind(tx.community_id())
    .bind(league.id)
    .execute(&mut **tx)
    .await?;
    let open: Unresolved = sqlx::query_as(
        "SELECT count(*) AS count,
                min(confirm_deadline_at) FILTER (WHERE status = 'reported') AS next_deadline
         FROM matches
         WHERE community_id = $1 AND league_id = $2 AND status IN ('reported', 'disputed')",
    )
    .bind(tx.community_id())
    .bind(league.id)
    .fetch_one(&mut **tx)
    .await?;
    if open.count > 0 && !force {
        return Ok(Closing::Blocked(open));
    }
    finish(tx, tenant, league, now).await?;
    Ok(Closing::Finished)
}

/// `active → finished`: records final standings with promotion and relegation, and writes
/// season points to the ranking ledger (each partner in full). Matches without a result are
/// left out of the table.
async fn finish(
    tx: &mut TenantTx,
    tenant: &Tenant,
    league: &LeagueRow,
    now: DateTime<Utc>,
) -> Result<(), ApiError> {
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
