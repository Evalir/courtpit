//! The date-driven league lifecycle, run by the `advance_league` job:
//! `draft → registration → active → finished`.
//!
//! Each run applies every step that is due at the clock's `now` (so a late job catches up),
//! then re-enqueues itself for the next date. Steps are idempotent per status, so retries and
//! duplicate deliveries are harmless.

use chrono::{DateTime, Utc};
use courtpit_domain::{EntryId, Seed, place, round_robin};
use rust_decimal::Decimal;
use sqlx::FromRow;
use uuid::Uuid;

use super::{LeagueRow, LeagueStatus, load};
use crate::{
    ApiError, AppState, Tenant, TenantTx,
    jobs::{self, Job},
    matches::{self, LeagueSlot, NewMatch},
};

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
    let seeds: Vec<Seed<EntryId>> = entries
        .iter()
        .map(|entry| Seed {
            id: EntryId(entry.id),
            utr: entry.utr,
            previous: None,
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
