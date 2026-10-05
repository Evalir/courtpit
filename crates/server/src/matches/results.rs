//! Writing results: reports, confirmations, admin decisions, and the auto-confirm job.

use chrono::{DateTime, Duration, Utc};
use racquetcollective_domain::{
    Actor, Event, MatchFormat, MatchStatus, Score, ScoreSummary, Side, validate_score,
};
use sqlx::types::Json;
use uuid::Uuid;

use super::{DbMatchStatus, DbSide, MatchRow, load, supersede_open_proposals};
use crate::{
    ApiError, AppState, TenantTx,
    jobs::{self, Job},
};

/// Validates `score` against the match's format snapshot.
pub fn check_score(format: &MatchFormat, score: &Score) -> Result<ScoreSummary, ApiError> {
    validate_score(format, score).map_err(|err| ApiError::validation(err.to_string()))
}

/// A score reported by one of a match's players.
#[derive(Debug, Clone, Copy)]
pub struct ScoreReport<'a> {
    /// The reporting player.
    pub by: Uuid,
    /// The score, already checked with [`check_score`].
    pub score: &'a Score,
    /// The winning side, from [`check_score`].
    pub winner: Side,
    /// When it was reported.
    pub at: DateTime<Utc>,
}

/// Records a reported score on a locked `proposed` or `scheduled` match and schedules its
/// auto-confirmation. A played match has nothing left to negotiate, so any proposal still open
/// is superseded in the same transaction; `scheduled_at` and `location` are left as they are
/// (null for a match that was never scheduled).
pub async fn record_report(
    tx: &mut TenantTx,
    found: &MatchRow,
    report: &ScoreReport<'_>,
    window: Duration,
) -> Result<(), ApiError> {
    let deadline = report.at + window;
    let _ = sqlx::query(
        "UPDATE matches SET status = 'reported', score = $3, winner_side = $4, reported_by = $5,
            reported_at = $6, confirm_deadline_at = $7, disputed_by = NULL, dispute_note = NULL,
            updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(found.id)
    .bind(Json(report.score))
    .bind(DbSide::from(report.winner))
    .bind(report.by)
    .bind(report.at)
    .bind(deadline)
    .execute(&mut **tx)
    .await?;
    supersede_open_proposals(tx, found.id).await?;
    let job = Job::AutoConfirmMatch { community_id: tx.community_id(), match_id: found.id };
    jobs::enqueue(&mut **tx, job, deadline).await?;
    Ok(())
}

/// The job: confirms `match_id` if it is still reported and its window has passed.
/// Anything else (already confirmed, disputed, not yet due) is a no-op, so retries and
/// duplicate deliveries are harmless.
pub async fn auto_confirm(
    state: &AppState,
    community_id: Uuid,
    match_id: Uuid,
) -> anyhow::Result<()> {
    let mut tx = TenantTx::begin(&state.db, community_id).await?;
    let found = match load(&mut tx, match_id, true).await {
        Ok(found) => found,
        Err(ApiError::NotFound(_)) => return Ok(()),
        Err(err) => return Err(anyhow::anyhow!("loading match: {err}")),
    };
    let due = found.confirm_deadline_at.is_some_and(|deadline| deadline <= state.clock.now());
    if found.status() != MatchStatus::Reported || !due {
        return Ok(());
    }
    let status = found
        .transition(Actor::System, Event::ConfirmTimeout)
        .map_err(|err| anyhow::anyhow!("auto-confirm refused: {err}"))?;
    let _ = sqlx::query(
        "UPDATE matches SET status = $3, updated_at = now() WHERE community_id = $1 AND id = $2",
    )
    .bind(community_id)
    .bind(match_id)
    .bind(DbMatchStatus::from(status))
    .execute(&mut *tx)
    .await?;
    let row = load(&mut tx, match_id, false)
        .await
        .map_err(|err| anyhow::anyhow!("reloading match: {err}"))?;
    crate::rankings::on_match_result(&mut tx, &row, state.clock.now())
        .await
        .map_err(|err| anyhow::anyhow!("ranking events: {err}"))?;
    tx.commit().await?;
    tracing::info!(%match_id, "score auto-confirmed");
    Ok(())
}
