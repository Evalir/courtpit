//! `/api/v1/matches`: friendly match creation, listing and cancellation.
//!
//! Scheduling proposals live in `api::proposals`; scores and admin decisions in
//! `api::match_results`. Creating a match may include the first proposal.

use std::collections::HashSet;

use axum::{Json, extract::State, http::StatusCode};
use chrono::{DateTime, Utc};
use courtpit_domain::{Discipline, Event, MatchFormat, MatchStatus};
use serde::Deserialize;
use sqlx::{Postgres, QueryBuilder};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    ApiError, ApiResult, AppState, Tenant, TenantTx,
    auth::CurrentPlayer,
    extract::{ApiJson, ApiPath, ApiQuery},
    matches::{self, DbMatchStatus, MATCH_COLUMNS, MatchRow, MatchView, NewMatch},
    models::{Page, PageParams, paginate},
    players,
};

use super::proposals;

pub(crate) const MAX_NOTE_LEN: usize = 500;

/// The community's default match format.
pub(crate) fn community_format(tenant: &Tenant) -> ApiResult<MatchFormat> {
    let format: MatchFormat = serde_json::from_value(tenant.default_match_format.0.clone())
        .map_err(|err| ApiError::Internal(anyhow::anyhow!("community match format: {err}")))?;
    format
        .validate()
        .map_err(|err| ApiError::Internal(anyhow::anyhow!("community match format: {err}")))?;
    Ok(format)
}

/// Trims optional free text, dropping it when empty and rejecting it when too long.
pub(crate) fn short_text(
    field: &str,
    raw: Option<String>,
    max: usize,
) -> ApiResult<Option<String>> {
    let text = raw
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());
    if text.as_ref().is_some_and(|text| text.chars().count() > max) {
        return Err(ApiError::validation(format!(
            "{field} must be at most {max} characters"
        )));
    }
    Ok(text)
}

/// Body of `POST /matches`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateMatch {
    /// Singles, doubles or mixed.
    pub discipline: Discipline,
    /// Your partner (doubles and mixed only). You play on side A.
    pub partner_id: Option<Uuid>,
    /// Side B: one player for singles, two for doubles and mixed.
    pub opponent_ids: Vec<Uuid>,
    /// Optional first proposal of time and place.
    pub proposed_time: Option<DateTime<Utc>>,
    /// Optional place of the first proposal.
    pub location: Option<String>,
}

/// Proposes a friendly match to other members. Friendly matches never count for rankings.
#[utoipa::path(post, path = "/api/v1/matches", tag = "matches", request_body = CreateMatch,
    security(("bearer" = [])),
    responses((status = 201, body = MatchView), (status = 403, body = crate::error::ErrorBody),
        (status = 422, body = crate::error::ErrorBody)))]
pub async fn create_match(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiJson(body): ApiJson<CreateMatch>,
) -> ApiResult<(StatusCode, Json<MatchView>)> {
    player.require_verified()?;
    let per_side = body.discipline.players_per_side();
    let side_a: Vec<Uuid> = std::iter::once(player.id).chain(body.partner_id).collect();
    if side_a.len() != per_side {
        return Err(ApiError::validation(if per_side == 1 {
            "singles has no partner"
        } else {
            "doubles needs a partner_id"
        }));
    }
    if body.opponent_ids.len() != per_side {
        return Err(ApiError::validation(format!(
            "{:?} needs {per_side} opponent(s)",
            body.discipline
        )));
    }
    let everyone: Vec<Uuid> = side_a.iter().chain(&body.opponent_ids).copied().collect();
    if everyone.iter().collect::<HashSet<_>>().len() != everyone.len() {
        return Err(ApiError::validation("a player can only appear once"));
    }
    if let Some(time) = body.proposed_time {
        proposals::check_time(&state, time)?;
    }
    let location = proposals::location(body.location)?;
    let format = community_format(&player.tenant)?;

    let mut tx = player.tenant.begin(&state.db).await?;
    players::require_active_members(&mut tx, &everyone).await?;
    let id = matches::insert(
        &mut tx,
        &NewMatch {
            discipline: body.discipline,
            side_a: &side_a,
            side_b: &body.opponent_ids,
            format,
            created_by: Some(player.id),
            league: None,
        },
    )
    .await?;
    if let Some(time) = body.proposed_time {
        let _ = matches::insert_proposal(&mut tx, id, player.id, time, location.as_deref()).await?;
    }
    let found = matches::load(&mut tx, id, false).await?;
    let view = matches::view_with_proposals(&mut tx, found).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(view)))
}

/// Filters for `GET /matches`.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct MatchQuery {
    /// Only matches in this status.
    pub status: Option<MatchStatus>,
    /// All matches of a league (visible to every member).
    pub league_id: Option<Uuid>,
    /// Admins only: every match of the community, not just your own.
    #[serde(default)]
    pub all: bool,
    /// Opaque cursor from a previous page.
    pub cursor: Option<String>,
    /// Page size.
    pub limit: Option<i64>,
}

/// Lists your matches (newest first), or a league's, or (admins) the whole community's.
#[utoipa::path(get, path = "/api/v1/matches", tag = "matches", params(MatchQuery),
    security(("bearer" = [])),
    responses((status = 200, body = Page<MatchView>),
        (status = 403, body = crate::error::ErrorBody)))]
pub async fn list_matches(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiQuery(query): ApiQuery<MatchQuery>,
) -> ApiResult<Json<Page<MatchView>>> {
    if query.all {
        player.require_admin()?;
    }
    let page = PageParams {
        cursor: query.cursor.clone(),
        limit: query.limit,
    };
    let limit = page.limit();
    let mut qb: QueryBuilder<'_, Postgres> = QueryBuilder::new(format!(
        "SELECT {MATCH_COLUMNS} FROM matches WHERE community_id = "
    ));
    let _ = qb.push_bind(player.tenant.id());
    match (query.league_id, query.all) {
        (Some(league), _) => {
            let _ = qb.push(" AND league_id = ").push_bind(league);
        }
        (None, true) => {}
        (None, false) => {
            let _ = qb
                .push(" AND (side_a_players || side_b_players) @> ARRAY[")
                .push_bind(player.id)
                .push("]::uuid[]");
        }
    }
    if let Some(status) = query.status {
        let _ = qb
            .push(" AND status = ")
            .push_bind(DbMatchStatus::from(status));
    }
    if let Some(cursor) = page.uuid_cursor()? {
        let _ = qb.push(" AND id < ").push_bind(cursor);
    }
    let _ = qb.push(" ORDER BY id DESC LIMIT ").push_bind(limit + 1);
    let mut tx = player.tenant.begin(&state.db).await?;
    let rows: Vec<MatchRow> = qb.build_query_as().fetch_all(&mut *tx).await?;
    tx.commit().await?;
    let page = paginate(rows, limit, |row| row.id.to_string());
    Ok(Json(Page {
        items: page.items.into_iter().map(MatchView::from).collect(),
        next_cursor: page.next_cursor,
    }))
}

/// Loads a match the caller may see, locked for update.
pub(crate) async fn load_visible(
    tx: &mut TenantTx,
    id: Uuid,
    player: &CurrentPlayer,
) -> ApiResult<MatchRow> {
    let found = matches::load(tx, id, true).await?;
    if matches::visible_to(&found, player) {
        Ok(found)
    } else {
        Err(ApiError::NotFound("match"))
    }
}

/// One match with its scheduling proposals.
#[utoipa::path(get, path = "/api/v1/matches/{id}", tag = "matches",
    params(("id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 200, body = MatchView), (status = 404, body = crate::error::ErrorBody)))]
pub async fn get_match(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<MatchView>> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let found = matches::load(&mut tx, id, false).await?;
    if !matches::visible_to(&found, &player) {
        return Err(ApiError::NotFound("match"));
    }
    let view = matches::view_with_proposals(&mut tx, found).await?;
    tx.commit().await?;
    Ok(Json(view))
}

/// Body of `POST /matches/{id}/cancel`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelMatch {
    /// Optional reason, recorded as the resolution note.
    pub note: Option<String>,
}

/// Calls a match off before it is played. Players may cancel friendlies; league and
/// tournament matches only an admin.
#[utoipa::path(post, path = "/api/v1/matches/{id}/cancel", tag = "matches",
    params(("id" = Uuid, Path)), request_body = CancelMatch, security(("bearer" = [])),
    responses((status = 200, body = MatchView), (status = 403, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody)))]
pub async fn cancel_match(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<CancelMatch>,
) -> ApiResult<Json<MatchView>> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let found = load_visible(&mut tx, id, &player).await?;
    let actor = matches::player_or_admin_actor(&found, &player)?;
    let status = found.transition(actor, Event::Cancel)?;
    let _ = sqlx::query(
        "UPDATE matches SET status = $3, resolved_by = $4, resolution_note = $5, updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(id)
    .bind(DbMatchStatus::from(status))
    .bind(player.id)
    .bind(short_text("note", body.note, MAX_NOTE_LEN)?)
    .execute(&mut *tx)
    .await?;
    let _ = sqlx::query(
        "UPDATE match_proposals SET status = 'superseded', updated_at = now()
         WHERE community_id = $1 AND match_id = $2 AND status = 'open'",
    )
    .bind(tx.community_id())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let found = matches::load(&mut tx, id, false).await?;
    let view = matches::view_with_proposals(&mut tx, found).await?;
    tx.commit().await?;
    Ok(Json(view))
}
