//! `/api/v1/matches/{id}/proposals`: proposing, counter-proposing, accepting and declining
//! a time and place. No chat in v1: the proposal flow is the whole negotiation.

use axum::{Json, extract::State, http::StatusCode};
use chrono::{DateTime, Duration, Utc};
use courtpit_domain::Event;
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    ApiError, ApiResult, AppState, TenantTx,
    api::matches::{load_visible, short_text},
    auth::CurrentPlayer,
    extract::{ApiJson, ApiPath},
    matches::{self, DbMatchStatus, MatchView, ProposalStatus},
};

/// Longest a proposal may look ahead.
const MAX_PROPOSAL_AHEAD_DAYS: i64 = 365;
const MAX_LOCATION_LEN: usize = 120;

pub(crate) fn location(raw: Option<String>) -> ApiResult<Option<String>> {
    short_text("location", raw, MAX_LOCATION_LEN)
}

/// Proposed times must be in the future and at most a year ahead.
pub(crate) fn check_time(state: &AppState, time: DateTime<Utc>) -> ApiResult<()> {
    let now = state.clock.now();
    if time <= now {
        return Err(ApiError::validation("proposed time must be in the future"));
    }
    if time > now + Duration::days(MAX_PROPOSAL_AHEAD_DAYS) {
        return Err(ApiError::validation(
            "proposed time must be within a year from now",
        ));
    }
    Ok(())
}

/// Body of `POST /matches/{id}/proposals`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposeTime {
    /// Proposed start time.
    pub time: DateTime<Utc>,
    /// Proposed place, if any.
    pub location: Option<String>,
}

/// Proposes (or counter-proposes) a time and place; supersedes any open proposal. Allowed
/// while the match is proposed or scheduled (rescheduling).
#[utoipa::path(post, path = "/api/v1/matches/{id}/proposals", tag = "matches",
    params(("id" = Uuid, Path)), request_body = ProposeTime, security(("bearer" = [])),
    responses((status = 201, body = MatchView), (status = 403, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody)))]
pub async fn propose(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<ProposeTime>,
) -> ApiResult<(StatusCode, Json<MatchView>)> {
    check_time(&state, body.time)?;
    let location = location(body.location)?;
    let mut tx = player.tenant.begin(&state.db).await?;
    let match_row = load_visible(&mut tx, id, &player).await?;
    let actor = matches::player_actor(&match_row, &player)?;
    let _ = match_row.transition(actor, Event::Propose)?;
    let _ =
        matches::insert_proposal(&mut tx, id, player.id, body.time, location.as_deref()).await?;
    let view = matches::view_with_proposals(&mut tx, match_row).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(view)))
}

#[derive(sqlx::FromRow)]
struct OpenProposal {
    proposed_by: Uuid,
    proposed_time: DateTime<Utc>,
    location: Option<String>,
    status: ProposalStatus,
}

async fn load_proposal(tx: &mut TenantTx, match_id: Uuid, id: Uuid) -> ApiResult<OpenProposal> {
    let proposal: OpenProposal = sqlx::query_as(
        "SELECT proposed_by, proposed_time, location, status FROM match_proposals
         WHERE community_id = $1 AND match_id = $2 AND id = $3",
    )
    .bind(tx.community_id())
    .bind(match_id)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(ApiError::NotFound("proposal"))?;
    if proposal.status != ProposalStatus::Open {
        return Err(ApiError::conflict("that proposal is no longer open"));
    }
    Ok(proposal)
}

async fn close_proposal(tx: &mut TenantTx, id: Uuid, status: ProposalStatus) -> ApiResult<()> {
    let _ = sqlx::query(
        "UPDATE match_proposals SET status = $3, updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(id)
    .bind(status)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Accepts the other side's proposal: the match is scheduled at that time and place.
#[utoipa::path(post, path = "/api/v1/matches/{id}/proposals/{proposal_id}/accept", tag = "matches",
    params(("id" = Uuid, Path), ("proposal_id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 200, body = MatchView), (status = 403, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody)))]
pub async fn accept_proposal(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath((id, proposal_id)): ApiPath<(Uuid, Uuid)>,
) -> ApiResult<Json<MatchView>> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let match_row = load_visible(&mut tx, id, &player).await?;
    let actor = matches::player_actor(&match_row, &player)?;
    let proposal = load_proposal(&mut tx, id, proposal_id).await?;
    let proposed_by = match_row
        .side_of(proposal.proposed_by)
        .ok_or_else(|| ApiError::conflict("the proposer no longer plays in this match"))?;
    let status = match_row.transition(actor, Event::AcceptProposal { proposed_by })?;
    if proposal.proposed_time <= state.clock.now() {
        return Err(ApiError::conflict("that proposal's time has passed"));
    }
    close_proposal(&mut tx, proposal_id, ProposalStatus::Accepted).await?;
    let _ = sqlx::query(
        "UPDATE matches SET status = $3, scheduled_at = $4, location = $5, updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(id)
    .bind(DbMatchStatus::from(status))
    .bind(proposal.proposed_time)
    .bind(proposal.location)
    .execute(&mut *tx)
    .await?;
    let match_row = matches::load(&mut tx, id, false).await?;
    let view = matches::view_with_proposals(&mut tx, match_row).await?;
    tx.commit().await?;
    Ok(Json(view))
}

/// Declines the other side's proposal. The match stays open for a counter-proposal.
#[utoipa::path(post, path = "/api/v1/matches/{id}/proposals/{proposal_id}/decline", tag = "matches",
    params(("id" = Uuid, Path), ("proposal_id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 200, body = MatchView), (status = 403, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody)))]
pub async fn decline_proposal(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath((id, proposal_id)): ApiPath<(Uuid, Uuid)>,
) -> ApiResult<Json<MatchView>> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let match_row = load_visible(&mut tx, id, &player).await?;
    let actor = matches::player_actor(&match_row, &player)?;
    let proposal = load_proposal(&mut tx, id, proposal_id).await?;
    let proposed_by = match_row
        .side_of(proposal.proposed_by)
        .ok_or_else(|| ApiError::conflict("the proposer no longer plays in this match"))?;
    let _ = match_row.transition(actor, Event::DeclineProposal { proposed_by })?;
    close_proposal(&mut tx, proposal_id, ProposalStatus::Declined).await?;
    let view = matches::view_with_proposals(&mut tx, match_row).await?;
    tx.commit().await?;
    Ok(Json(view))
}
