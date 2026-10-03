//! Scores: report, confirm, dispute (players) and resolve, walkover (admins).

use axum::{Json, extract::State};
use chrono::Duration;
use courtpit_domain::{Event, Resolution, Score, Side};
use serde::Deserialize;
use sqlx::types::Json as SqlJson;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    ApiError, ApiResult, AppState, TenantTx,
    api::matches::{MAX_NOTE_LEN, load_visible, short_text},
    auth::CurrentPlayer,
    communities::CommunitySettings,
    extract::{ApiJson, ApiPath},
    matches::{self, DbMatchStatus, DbSide, MatchView, results},
};

async fn respond(tx: &mut TenantTx, id: Uuid) -> ApiResult<Json<MatchView>> {
    let found = matches::load(tx, id, false).await?;
    Ok(Json(matches::view_with_proposals(tx, found).await?))
}

/// Reports the score of a scheduled match. The other side then has the community's
/// confirmation window (default 3 days) to confirm or dispute; silence confirms.
#[utoipa::path(post, path = "/api/v1/matches/{id}/score", tag = "matches",
    params(("id" = Uuid, Path)), request_body = Score, security(("bearer" = [])),
    responses((status = 200, body = MatchView), (status = 403, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody), (status = 422, body = crate::error::ErrorBody)))]
pub async fn report_score(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(score): ApiJson<Score>,
) -> ApiResult<Json<MatchView>> {
    let window = Duration::days(
        CommunitySettings::of(&player.tenant)
            .confirm_window_days
            .into(),
    );
    let mut tx = player.tenant.begin(&state.db).await?;
    let found = load_visible(&mut tx, id, &player).await?;
    let actor = matches::player_actor(&found, &player)?;
    let _ = found.transition(actor, Event::Report)?;
    let summary = results::check_score(&found.match_format, &score)?;
    let now = state.clock.now();
    results::record_report(
        &mut tx,
        &found,
        player.id,
        &score,
        summary.winner,
        now,
        window,
    )
    .await?;
    let res = respond(&mut tx, id).await?;
    tx.commit().await?;
    Ok(res)
}

/// Confirms the other side's reported score. The result then stands.
#[utoipa::path(post, path = "/api/v1/matches/{id}/confirm", tag = "matches",
    params(("id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 200, body = MatchView), (status = 403, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody)))]
pub async fn confirm_score(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<MatchView>> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let found = load_visible(&mut tx, id, &player).await?;
    let actor = matches::player_actor(&found, &player)?;
    let status = found.transition(actor, Event::Confirm)?;
    matches::set_status(&mut tx, id, status).await?;
    let res = respond(&mut tx, id).await?;
    tx.commit().await?;
    Ok(res)
}

/// Body of `POST /matches/{id}/dispute`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DisputeBody {
    /// What is wrong with the reported score.
    pub note: Option<String>,
}

/// Disputes the other side's reported score; a community admin then decides.
#[utoipa::path(post, path = "/api/v1/matches/{id}/dispute", tag = "matches",
    params(("id" = Uuid, Path)), request_body = DisputeBody, security(("bearer" = [])),
    responses((status = 200, body = MatchView), (status = 403, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody)))]
pub async fn dispute_score(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<DisputeBody>,
) -> ApiResult<Json<MatchView>> {
    let note = short_text("note", body.note, MAX_NOTE_LEN)?;
    let mut tx = player.tenant.begin(&state.db).await?;
    let found = load_visible(&mut tx, id, &player).await?;
    let actor = matches::player_actor(&found, &player)?;
    let status = found.transition(actor, Event::Dispute)?;
    let _ = sqlx::query(
        "UPDATE matches SET status = $3, disputed_by = $4, dispute_note = $5, updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(id)
    .bind(DbMatchStatus::from(status))
    .bind(player.id)
    .bind(note)
    .execute(&mut *tx)
    .await?;
    let res = respond(&mut tx, id).await?;
    tx.commit().await?;
    Ok(res)
}

/// Body of `POST /admin/matches/{id}/resolve`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveBody {
    /// `score`: set the result; `replay`: back to scheduled; `void`: cancel the match.
    pub resolution: Resolution,
    /// Required with `score`.
    pub score: Option<Score>,
    /// Optional admin note explaining the decision.
    pub note: Option<String>,
}

/// Decides a disputed match (admin).
#[utoipa::path(post, path = "/api/v1/admin/matches/{id}/resolve", tag = "admin",
    params(("id" = Uuid, Path)), request_body = ResolveBody, security(("bearer" = [])),
    responses((status = 200, body = MatchView), (status = 403, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody), (status = 422, body = crate::error::ErrorBody)))]
pub async fn resolve_match(
    State(state): State<AppState>,
    admin: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<ResolveBody>,
) -> ApiResult<Json<MatchView>> {
    let note = short_text("note", body.note, MAX_NOTE_LEN)?;
    let mut tx = admin.tenant.begin(&state.db).await?;
    let found = matches::load(&mut tx, id, true).await?;
    let actor = matches::admin_actor(&found, &admin)?;
    let status = found.transition(actor, Event::Resolve(body.resolution))?;
    let (score, winner) = match (body.resolution, body.score) {
        (Resolution::Score, Some(score)) => {
            let winner = results::check_score(&found.match_format, &score)?.winner;
            (Some(score), Some(winner))
        }
        (Resolution::Score, None) => {
            return Err(ApiError::validation("resolution `score` needs a score"));
        }
        (_, Some(_)) => {
            return Err(ApiError::validation(
                "a score only goes with resolution `score`",
            ));
        }
        (_, None) => (None, None),
    };
    // A replay starts the result over: clear the report so the match can be reported again.
    let _ = sqlx::query(
        "UPDATE matches SET status = $3, score = $4, winner_side = $5, resolved_by = $6,
            resolution_note = $7, updated_at = now(),
            reported_by = CASE WHEN $3 = 'scheduled'::match_status THEN NULL ELSE reported_by END,
            reported_at = CASE WHEN $3 = 'scheduled'::match_status THEN NULL ELSE reported_at END,
            confirm_deadline_at = NULL
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(id)
    .bind(DbMatchStatus::from(status))
    .bind(score.map(SqlJson))
    .bind(winner.map(DbSide::from))
    .bind(admin.id)
    .bind(note)
    .execute(&mut *tx)
    .await?;
    let res = respond(&mut tx, id).await?;
    tx.commit().await?;
    Ok(res)
}

/// Body of `POST /admin/matches/{id}/walkover`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct WalkoverBody {
    /// The side awarded the match.
    pub winner_side: Side,
    /// Optional admin note explaining the walkover.
    pub note: Option<String>,
}

/// Awards an unplayed match to one side (no-show, missed deadline). Admin only.
#[utoipa::path(post, path = "/api/v1/admin/matches/{id}/walkover", tag = "admin",
    params(("id" = Uuid, Path)), request_body = WalkoverBody, security(("bearer" = [])),
    responses((status = 200, body = MatchView), (status = 403, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody)))]
pub async fn walkover_match(
    State(state): State<AppState>,
    admin: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<WalkoverBody>,
) -> ApiResult<Json<MatchView>> {
    let note = short_text("note", body.note, MAX_NOTE_LEN)?;
    let mut tx = admin.tenant.begin(&state.db).await?;
    let found = matches::load(&mut tx, id, true).await?;
    let actor = matches::admin_actor(&found, &admin)?;
    let status = found.transition(actor, Event::Walkover)?;
    let _ = sqlx::query(
        "UPDATE matches SET status = $3, score = NULL, winner_side = $4, resolved_by = $5,
            resolution_note = $6, updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(id)
    .bind(DbMatchStatus::from(status))
    .bind(DbSide::from(body.winner_side))
    .bind(admin.id)
    .bind(note)
    .execute(&mut *tx)
    .await?;
    let res = respond(&mut tx, id).await?;
    tx.commit().await?;
    Ok(res)
}
