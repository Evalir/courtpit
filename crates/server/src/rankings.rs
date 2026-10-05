//! The ranking ledger: appending events and materialising the rolling 52-week rankings.

use chrono::{DateTime, Duration, Utc};
use racquetcollective_domain::{
    Discipline, MatchStatus, Outcome, PlayerId, RankingEvent, RankingSource, ScoringConfig,
    league_match_events,
};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    ApiError, AppState, TenantTx,
    jobs::{self, Job},
    leagues,
    matches::{DbDiscipline, MatchRow, results::check_score},
};

/// Rankings sum the points of this trailing window.
pub const fn window() -> Duration {
    Duration::weeks(52)
}

/// Postgres `ranking_source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "ranking_source", rename_all = "snake_case")]
pub enum DbRankingSource {
    /// Points from a league match result.
    LeagueMatch,
    /// Points from a league season finish.
    LeagueSeason,
    /// Points from a tournament result.
    Tournament,
}

impl From<RankingSource> for DbRankingSource {
    fn from(source: RankingSource) -> Self {
        match source {
            RankingSource::LeagueMatch => Self::LeagueMatch,
            RankingSource::LeagueSeason => Self::LeagueSeason,
            RankingSource::Tournament => Self::Tournament,
        }
    }
}

/// The community's raw `scoring_config`.
pub async fn community_scoring(tx: &mut TenantTx) -> Result<Value, sqlx::Error> {
    let config: sqlx::types::Json<Value> =
        sqlx::query_scalar("SELECT scoring_config FROM communities WHERE id = $1")
            .bind(tx.community_id())
            .fetch_one(&mut **tx)
            .await?;
    Ok(config.0)
}

/// Appends `events` (earned by `source_id`) to the ledger; re-appending is a no-op. Schedules
/// a rankings refresh.
pub async fn append(
    tx: &mut TenantTx,
    source_id: Uuid,
    events: &[RankingEvent],
    occurred_at: DateTime<Utc>,
) -> Result<(), ApiError> {
    for event in events {
        let _ = sqlx::query(
            "INSERT INTO ranking_events (id, community_id, player_id, discipline, source,
                source_id, points, occurred_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (source, source_id, player_id) DO NOTHING",
        )
        .bind(Uuid::now_v7())
        .bind(tx.community_id())
        .bind(event.player.0)
        .bind(DbDiscipline::from(event.discipline))
        .bind(DbRankingSource::from(event.source))
        .bind(source_id)
        .bind(i32::try_from(event.points).unwrap_or(i32::MAX))
        .bind(occurred_at)
        .execute(&mut **tx)
        .await?;
    }
    if !events.is_empty() {
        let job = Job::RefreshRankings { community_id: tx.community_id() };
        jobs::enqueue(&mut **tx, job, occurred_at).await?;
    }
    Ok(())
}

/// Ledger events for a league match that just got a result (confirmed, resolved or
/// walkover). Friendly matches earn nothing. Call inside the transaction that set the result.
pub async fn on_match_result(
    tx: &mut TenantTx,
    row: &MatchRow,
    now: DateTime<Utc>,
) -> Result<(), ApiError> {
    let Some(league_id) = row.league_id else {
        return Ok(());
    };
    let outcome = match (row.status(), row.winner_side, &row.score) {
        (MatchStatus::Walkover, Some(w), _) => Outcome::Walkover { winner: w.into() },
        (MatchStatus::Confirmed | MatchStatus::Resolved, Some(_), Some(score)) => {
            Outcome::from_summary(&check_score(&row.match_format, score)?)
        }
        _ => return Ok(()),
    };
    let league = leagues::load(tx, league_id, false).await?;
    let config = league.scoring(&community_scoring(tx).await?)?;
    let ids = |side: &[Uuid]| side.iter().copied().map(PlayerId).collect::<Vec<_>>();
    let events = league_match_events(
        &config.league_match,
        row.discipline.into(),
        &ids(&row.side_a_players),
        &ids(&row.side_b_players),
        outcome,
    );
    append(tx, row.id, &events, now).await
}

/// Rebuilds a community's rankings from the ledger as of `now`: per ranking discipline (mixed
/// folded into doubles when `mixed_pooling` says so), the sum of points earned in the last 52
/// weeks, ranked with ties sharing a rank.
pub async fn refresh(tx: &mut TenantTx, now: DateTime<Utc>) -> Result<(), ApiError> {
    let config: ScoringConfig = serde_json::from_value(community_scoring(tx).await?)
        .map_err(|err| ApiError::Internal(anyhow::anyhow!("community scoring config: {err}")))?;
    let mixed_into = DbDiscipline::from(config.ranking_discipline(Discipline::Mixed));
    let _ = sqlx::query("DELETE FROM rankings WHERE community_id = $1")
        .bind(tx.community_id())
        .execute(&mut **tx)
        .await?;
    let _ = sqlx::query(
        "INSERT INTO rankings (community_id, player_id, discipline, points_52w, rank, refreshed_at)
         SELECT $1, player_id, d, total, rank() OVER (PARTITION BY d ORDER BY total DESC), $2
         FROM (
             SELECT e.player_id,
                    CASE WHEN e.discipline = 'mixed' THEN $4 ELSE e.discipline END AS d,
                    sum(e.points)::int AS total
             FROM ranking_events e JOIN players p
               ON p.community_id = e.community_id AND p.id = e.player_id
             WHERE e.community_id = $1 AND e.occurred_at <= $2 AND e.occurred_at > $3
               AND p.status = 'active'
             GROUP BY 1, 2
         ) totals",
    )
    .bind(tx.community_id())
    .bind(now)
    .bind(now - window())
    .bind(mixed_into)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// The job: refreshes one community and schedules the next daily refresh, so points decay
/// out of the window even when nothing new is played.
pub async fn refresh_job(state: &AppState, community_id: Uuid) -> anyhow::Result<()> {
    let now = state.clock.now();
    let mut tx = TenantTx::begin(&state.db, community_id).await?;
    refresh(&mut tx, now).await.map_err(|err| anyhow::anyhow!("refreshing rankings: {err}"))?;
    tx.commit().await?;
    let next = Job::RefreshRankings { community_id };
    jobs::enqueue(&state.db, next, now + Duration::days(1)).await?;
    Ok(())
}
