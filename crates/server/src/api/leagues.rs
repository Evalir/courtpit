//! `/api/v1/leagues` (members) and `/api/v1/admin/leagues` (setup, publish, cancel, finish).

use axum::{Json, extract::State, http::StatusCode};
use chrono::{DateTime, Utc};
use racquetcollective_domain::{Discipline, MatchFormat};
use serde::Deserialize;
use serde_json::Value;
use sqlx::{Postgres, QueryBuilder, types::Json as SqlJson};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    ApiError, ApiResult, AppState, Tenant, TenantTx,
    auth::CurrentPlayer,
    extract::{ApiJson, ApiPath, ApiQuery},
    jobs::{self, Job},
    leagues::{
        self, LEAGUE_COLUMNS, LeagueRow, LeagueStatus, LeagueView,
        lifecycle::{self, Closing},
        standings::{self, DivisionStanding},
    },
    matches::{self, DbDiscipline, MATCH_COLUMNS, MatchRow, MatchView},
    models::{Page, PageParams, double_option, paginate},
};

/// The full set of league settings (creation, and the result of applying a patch).
#[derive(Debug, Clone)]
struct Settings {
    name: String,
    discipline: Discipline,
    registration_opens_at: DateTime<Utc>,
    registration_closes_at: DateTime<Utc>,
    starts_at: DateTime<Utc>,
    ends_at: DateTime<Utc>,
    match_format: Option<MatchFormat>,
    scoring_overrides: Option<Value>,
    box_min_size: i32,
    box_max_size: i32,
    previous_league_id: Option<Uuid>,
}

impl Settings {
    async fn validate(&mut self, tx: &mut TenantTx, tenant: &Tenant) -> ApiResult<()> {
        self.name = self.name.trim().to_owned();
        if self.name.is_empty() || self.name.chars().count() > 80 {
            return Err(ApiError::validation("name must be 1-80 characters"));
        }
        if !(self.registration_opens_at < self.registration_closes_at
            && self.registration_closes_at <= self.starts_at
            && self.starts_at < self.ends_at)
        {
            return Err(ApiError::validation(
                "dates must satisfy registration_opens_at < registration_closes_at \
                 <= starts_at < ends_at",
            ));
        }
        if let Some(format) = &self.match_format {
            format
                .validate()
                .map_err(|err| ApiError::validation(format!("match_format: {err}")))?;
        }
        if let Some(overrides) = &self.scoring_overrides {
            if !overrides.is_object() {
                return Err(ApiError::validation("scoring_overrides must be an object"));
            }
            let _ = leagues::scoring_with(&tenant.scoring_config.0, Some(overrides))
                .map_err(|err| ApiError::validation(format!("scoring_overrides: {err}")))?;
        }
        if !(2 <= self.box_min_size
            && self.box_min_size <= self.box_max_size
            && self.box_max_size <= 16)
        {
            return Err(ApiError::validation(
                "box sizes must satisfy 2 <= box_min_size <= box_max_size <= 16",
            ));
        }
        if let Some(prev) = self.previous_league_id {
            let previous = leagues::load(tx, prev, false)
                .await
                .map_err(|_| ApiError::validation("previous_league_id: no such league"))?;
            if previous.discipline() != self.discipline {
                return Err(ApiError::validation(
                    "previous_league_id must be a league of the same discipline",
                ));
            }
        }
        Ok(())
    }
}

/// Body of `POST /admin/leagues`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateLeague {
    /// Display name.
    pub name: String,
    /// Racket sport the league is played in.
    pub discipline: Discipline,
    /// When registration opens.
    pub registration_opens_at: DateTime<Utc>,
    /// When registration closes.
    pub registration_closes_at: DateTime<Utc>,
    /// When the season starts.
    pub starts_at: DateTime<Utc>,
    /// When the season ends.
    pub ends_at: DateTime<Utc>,
    /// Overrides the community's default match format.
    pub match_format: Option<MatchFormat>,
    /// Partial scoring config merged over the community's.
    #[schema(value_type = Option<Object>)]
    pub scoring_overrides: Option<Value>,
    /// Box size limits for placement (default 6–8).
    pub box_min_size: Option<i32>,
    /// Maximum box size.
    pub box_max_size: Option<i32>,
    /// Last season of this league; its promotion/relegation seeds the placement.
    pub previous_league_id: Option<Uuid>,
}

/// Creates a league in `draft` (admin). Publish it to open registration.
#[utoipa::path(post, path = "/api/v1/admin/leagues", tag = "admin", request_body = CreateLeague,
    security(("bearer" = [])),
    responses((status = 201, body = LeagueView), (status = 403, body = crate::error::ErrorBody),
        (status = 422, body = crate::error::ErrorBody)))]
pub async fn create_league(
    State(state): State<AppState>,
    admin: CurrentPlayer,
    ApiJson(body): ApiJson<CreateLeague>,
) -> ApiResult<(StatusCode, Json<LeagueView>)> {
    admin.require_admin()?;
    let mut settings = Settings {
        name: body.name,
        discipline: body.discipline,
        registration_opens_at: body.registration_opens_at,
        registration_closes_at: body.registration_closes_at,
        starts_at: body.starts_at,
        ends_at: body.ends_at,
        match_format: body.match_format,
        scoring_overrides: body.scoring_overrides,
        box_min_size: body.box_min_size.unwrap_or(6),
        box_max_size: body.box_max_size.unwrap_or(8),
        previous_league_id: body.previous_league_id,
    };
    let mut tx = admin.tenant.begin(&state.db).await?;
    settings.validate(&mut tx, &admin.tenant).await?;
    let id = Uuid::now_v7();
    let _ = sqlx::query(
        "INSERT INTO leagues (id, community_id, name, discipline, registration_opens_at,
            registration_closes_at, starts_at, ends_at, match_format, scoring_overrides,
            box_min_size, box_max_size, previous_league_id, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(id)
    .bind(tx.community_id())
    .bind(&settings.name)
    .bind(DbDiscipline::from(settings.discipline))
    .bind(settings.registration_opens_at)
    .bind(settings.registration_closes_at)
    .bind(settings.starts_at)
    .bind(settings.ends_at)
    .bind(settings.match_format.map(SqlJson))
    .bind(settings.scoring_overrides.map(SqlJson))
    .bind(settings.box_min_size)
    .bind(settings.box_max_size)
    .bind(settings.previous_league_id)
    .bind(admin.id)
    .execute(&mut *tx)
    .await?;
    let league = leagues::load(&mut tx, id, false).await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(LeagueView::new(league, &admin.tenant)?),
    ))
}

/// Body of `PATCH /admin/leagues/{id}`: absent fields are untouched, `null` clears
/// nullable ones.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchLeague {
    /// Display name.
    pub name: Option<String>,
    /// When registration opens.
    pub registration_opens_at: Option<DateTime<Utc>>,
    /// When registration closes.
    pub registration_closes_at: Option<DateTime<Utc>>,
    /// When the season starts.
    pub starts_at: Option<DateTime<Utc>>,
    /// When the season ends.
    pub ends_at: Option<DateTime<Utc>>,
    /// Match format override.
    #[serde(default, deserialize_with = "double_option")]
    pub match_format: Option<Option<MatchFormat>>,
    /// Scoring config overrides.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<Object>)]
    pub scoring_overrides: Option<Option<Value>>,
    /// Minimum box size.
    pub box_min_size: Option<i32>,
    /// Maximum box size.
    pub box_max_size: Option<i32>,
    /// The preceding season, if any.
    #[serde(default, deserialize_with = "double_option")]
    pub previous_league_id: Option<Option<Uuid>>,
}

/// Edits a league while it is still a draft (admin).
#[utoipa::path(patch, path = "/api/v1/admin/leagues/{id}", tag = "admin",
    params(("id" = Uuid, Path)), request_body = PatchLeague, security(("bearer" = [])),
    responses((status = 200, body = LeagueView), (status = 403, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody),
        (status = 422, body = crate::error::ErrorBody)))]
pub async fn patch_league(
    State(state): State<AppState>,
    admin: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(patch): ApiJson<PatchLeague>,
) -> ApiResult<Json<LeagueView>> {
    admin.require_admin()?;
    let mut tx = admin.tenant.begin(&state.db).await?;
    let existing = leagues::load(&mut tx, id, true).await?;
    if existing.status != LeagueStatus::Draft {
        return Err(ApiError::conflict("only draft leagues can be edited"));
    }
    if patch.previous_league_id == Some(Some(id)) {
        return Err(ApiError::validation("a league can't follow itself"));
    }
    let mut settings = Settings {
        name: patch.name.unwrap_or(existing.name.clone()),
        discipline: existing.discipline(),
        registration_opens_at: patch
            .registration_opens_at
            .unwrap_or(existing.registration_opens_at),
        registration_closes_at: patch
            .registration_closes_at
            .unwrap_or(existing.registration_closes_at),
        starts_at: patch.starts_at.unwrap_or(existing.starts_at),
        ends_at: patch.ends_at.unwrap_or(existing.ends_at),
        match_format: patch
            .match_format
            .unwrap_or(existing.match_format.as_ref().map(|json| json.0)),
        scoring_overrides: patch.scoring_overrides.unwrap_or(
            existing
                .scoring_overrides
                .as_ref()
                .map(|json| json.0.clone()),
        ),
        box_min_size: patch.box_min_size.unwrap_or(existing.box_min_size),
        box_max_size: patch.box_max_size.unwrap_or(existing.box_max_size),
        previous_league_id: patch
            .previous_league_id
            .unwrap_or(existing.previous_league_id),
    };
    settings.validate(&mut tx, &admin.tenant).await?;
    let _ = sqlx::query(
        "UPDATE leagues SET name = $3, registration_opens_at = $4, registration_closes_at = $5,
            starts_at = $6, ends_at = $7, match_format = $8, scoring_overrides = $9,
            box_min_size = $10, box_max_size = $11, previous_league_id = $12, updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(id)
    .bind(&settings.name)
    .bind(settings.registration_opens_at)
    .bind(settings.registration_closes_at)
    .bind(settings.starts_at)
    .bind(settings.ends_at)
    .bind(settings.match_format.map(SqlJson))
    .bind(settings.scoring_overrides.map(SqlJson))
    .bind(settings.box_min_size)
    .bind(settings.box_max_size)
    .bind(settings.previous_league_id)
    .execute(&mut *tx)
    .await?;
    let league = leagues::load(&mut tx, id, false).await?;
    tx.commit().await?;
    Ok(Json(LeagueView::new(league, &admin.tenant)?))
}

/// Makes a draft league public (admin). Registration opens at `registration_opens_at`
/// (immediately if that has passed); from then on the dates drive the lifecycle.
#[utoipa::path(post, path = "/api/v1/admin/leagues/{id}/publish", tag = "admin",
    params(("id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 200, body = LeagueView), (status = 403, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody)))]
pub async fn publish_league(
    State(state): State<AppState>,
    admin: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<LeagueView>> {
    admin.require_admin()?;
    let now = state.clock.now();
    let mut tx = admin.tenant.begin(&state.db).await?;
    let existing = leagues::load(&mut tx, id, true).await?;
    if existing.status != LeagueStatus::Draft || existing.published_at.is_some() {
        return Err(ApiError::conflict(
            "only unpublished drafts can be published",
        ));
    }
    if existing.registration_closes_at <= now {
        return Err(ApiError::conflict(
            "registration would already be closed; move the dates first",
        ));
    }
    let status = if existing.registration_opens_at <= now {
        LeagueStatus::Registration
    } else {
        LeagueStatus::Draft
    };
    let _ = sqlx::query(
        "UPDATE leagues SET published_at = $3, status = $4, updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(id)
    .bind(now)
    .bind(status)
    .execute(&mut *tx)
    .await?;
    // From here the dates drive the league; the job reschedules itself for each step.
    let job = Job::AdvanceLeague {
        community_id: tx.community_id(),
        league_id: id,
    };
    jobs::enqueue(&mut *tx, job, now).await?;
    let league = leagues::load(&mut tx, id, false).await?;
    tx.commit().await?;
    Ok(Json(LeagueView::new(league, &admin.tenant)?))
}

/// Cancels a league that hasn't finished (admin). Its unplayed matches are cancelled too.
#[utoipa::path(post, path = "/api/v1/admin/leagues/{id}/cancel", tag = "admin",
    params(("id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 200, body = LeagueView), (status = 403, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody)))]
pub async fn cancel_league(
    State(state): State<AppState>,
    admin: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<LeagueView>> {
    admin.require_admin()?;
    let mut tx = admin.tenant.begin(&state.db).await?;
    let existing = leagues::load(&mut tx, id, true).await?;
    if matches!(
        existing.status,
        LeagueStatus::Finished | LeagueStatus::Cancelled
    ) {
        return Err(ApiError::conflict("the league is already over"));
    }
    let _ = sqlx::query(
        "UPDATE leagues SET status = 'cancelled', updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let _ = sqlx::query(
        "UPDATE matches SET status = 'cancelled', resolved_by = $3,
            resolution_note = 'league cancelled', updated_at = now()
         WHERE community_id = $1 AND league_id = $2 AND status IN ('proposed', 'scheduled')",
    )
    .bind(tx.community_id())
    .bind(id)
    .bind(admin.id)
    .execute(&mut *tx)
    .await?;
    let league = leagues::load(&mut tx, id, false).await?;
    tx.commit().await?;
    Ok(Json(LeagueView::new(league, &admin.tenant)?))
}

/// Query of `POST /admin/leagues/{id}/finish`.
#[derive(Debug, Clone, Copy, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct FinishQuery {
    /// Finish even though matches are still reported or disputed; they are left out of the
    /// final table.
    #[serde(default)]
    pub force: bool,
}

/// Finishes an active league whose `ends_at` has passed (admin). Without `force`, refuses
/// (409 `unresolved_matches`) while any match is still reported or disputed; before the end
/// date it refuses with 409 `season_not_over`. Unplayed matches are cancelled and the final
/// standings, season points and promotion/relegation are recorded, as the lifecycle job does.
#[utoipa::path(post, path = "/api/v1/admin/leagues/{id}/finish", tag = "admin",
    params(("id" = Uuid, Path), FinishQuery), security(("bearer" = [])),
    responses((status = 200, body = LeagueView), (status = 403, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody)))]
pub async fn finish_league(
    State(state): State<AppState>,
    admin: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
    ApiQuery(query): ApiQuery<FinishQuery>,
) -> ApiResult<Json<LeagueView>> {
    admin.require_admin()?;
    let now = state.clock.now();
    let mut tx = admin.tenant.begin(&state.db).await?;
    let existing = leagues::load(&mut tx, id, true).await?;
    if existing.status != LeagueStatus::Active {
        return Err(ApiError::conflict("only an active league can be finished"));
    }
    if now < existing.ends_at {
        return Err(ApiError::conflict_code(
            "season_not_over",
            format!(
                "the season ends at {}; cancel the league instead to stop it earlier",
                existing.ends_at.to_rfc3339()
            ),
        ));
    }
    if let Closing::Blocked(open) =
        lifecycle::close_season(&mut tx, &admin.tenant, &existing, now, query.force).await?
    {
        let which = if open.count == 1 {
            "1 league match is".to_owned()
        } else {
            format!("{} league matches are", open.count)
        };
        return Err(ApiError::conflict_code(
            "unresolved_matches",
            format!(
                "{which} still reported or disputed; resolve them (see \
                 GET /api/v1/admin/leagues/{id}/unresolved) or finish with ?force=true to \
                 leave them out of the final table"
            ),
        ));
    }
    let league = leagues::load(&mut tx, id, false).await?;
    tx.commit().await?;
    Ok(Json(LeagueView::new(league, &admin.tenant)?))
}

/// The league's matches that still lack a result: `reported` (waiting for the other side or
/// the auto-confirm deadline) and `disputed` (waiting for an admin). They block the season
/// from finishing; newest first.
#[utoipa::path(get, path = "/api/v1/admin/leagues/{id}/unresolved", tag = "admin",
    params(("id" = Uuid, Path), PageParams), security(("bearer" = [])),
    responses((status = 200, body = Page<MatchView>), (status = 403, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody)))]
pub async fn unresolved_matches(
    State(state): State<AppState>,
    admin: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
    ApiQuery(page): ApiQuery<PageParams>,
) -> ApiResult<Json<Page<MatchView>>> {
    admin.require_admin()?;
    let limit = page.limit();
    let cursor = page.uuid_cursor()?;
    let mut tx = admin.tenant.begin(&state.db).await?;
    let _ = leagues::load(&mut tx, id, false).await?;
    let rows: Vec<MatchRow> = sqlx::query_as(&format!(
        "SELECT {MATCH_COLUMNS} FROM matches
         WHERE community_id = $1 AND league_id = $2 AND status IN ('reported', 'disputed')
           AND ($3::uuid IS NULL OR id < $3)
         ORDER BY id DESC LIMIT $4"
    ))
    .bind(tx.community_id())
    .bind(id)
    .bind(cursor)
    .bind(limit + 1)
    .fetch_all(&mut *tx)
    .await?;
    let page = paginate(rows, limit, |row| row.id.to_string());
    let items = matches::views(&mut tx, page.items).await?;
    tx.commit().await?;
    Ok(Json(Page {
        items,
        next_cursor: page.next_cursor,
    }))
}

/// Filters for `GET /leagues`.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LeagueQuery {
    /// Lifecycle status.
    pub status: Option<LeagueStatus>,
    /// Racket sport the league is played in.
    pub discipline: Option<Discipline>,
    /// Pagination cursor from a previous page.
    pub cursor: Option<String>,
    /// Maximum items per page.
    pub limit: Option<i64>,
}

const fn visible(league: &LeagueRow, viewer: &CurrentPlayer) -> bool {
    viewer.role.is_admin() || league.published_at.is_some()
}

/// Lists leagues, newest first. Unpublished drafts are visible to admins only.
#[utoipa::path(get, path = "/api/v1/leagues", tag = "leagues", params(LeagueQuery),
    security(("bearer" = [])), responses((status = 200, body = Page<LeagueView>)))]
pub async fn list_leagues(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiQuery(query): ApiQuery<LeagueQuery>,
) -> ApiResult<Json<Page<LeagueView>>> {
    let page = PageParams {
        cursor: query.cursor.clone(),
        limit: query.limit,
    };
    let limit = page.limit();
    let mut qb: QueryBuilder<'_, Postgres> = QueryBuilder::new(format!(
        "SELECT {LEAGUE_COLUMNS} FROM leagues WHERE community_id = "
    ));
    let _ = qb.push_bind(player.tenant.id());
    if !player.role.is_admin() {
        let _ = qb.push(" AND published_at IS NOT NULL");
    }
    if let Some(status) = query.status {
        let _ = qb.push(" AND status = ").push_bind(status);
    }
    if let Some(discipline) = query.discipline {
        let _ = qb
            .push(" AND discipline = ")
            .push_bind(DbDiscipline::from(discipline));
    }
    if let Some(cursor) = page.uuid_cursor()? {
        let _ = qb.push(" AND id < ").push_bind(cursor);
    }
    let _ = qb.push(" ORDER BY id DESC LIMIT ").push_bind(limit + 1);
    let mut tx = player.tenant.begin(&state.db).await?;
    let rows: Vec<LeagueRow> = qb.build_query_as().fetch_all(&mut *tx).await?;
    tx.commit().await?;
    let page = paginate(rows, limit, |league| league.id.to_string());
    Ok(Json(Page {
        items: page
            .items
            .into_iter()
            .map(|league| LeagueView::new(league, &player.tenant))
            .collect::<ApiResult<_>>()?,
        next_cursor: page.next_cursor,
    }))
}

/// One league.
#[utoipa::path(get, path = "/api/v1/leagues/{id}", tag = "leagues",
    params(("id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 200, body = LeagueView), (status = 404, body = crate::error::ErrorBody)))]
pub async fn get_league(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<LeagueView>> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let league = leagues::load(&mut tx, id, false).await?;
    tx.commit().await?;
    if !visible(&league, &player) {
        return Err(ApiError::NotFound("league"));
    }
    Ok(Json(LeagueView::new(league, &player.tenant)?))
}

/// Box tables computed from the league's confirmed, resolved and walkover matches (league
/// match points, then wins, set and game difference).
#[utoipa::path(get, path = "/api/v1/leagues/{id}/standings", tag = "leagues",
    params(("id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 200, body = Vec<DivisionStanding>),
        (status = 404, body = crate::error::ErrorBody)))]
pub async fn league_standings(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<Vec<DivisionStanding>>> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let league = leagues::load(&mut tx, id, false).await?;
    if !visible(&league, &player) {
        return Err(ApiError::NotFound("league"));
    }
    let config = league.scoring(&player.tenant.scoring_config.0)?;
    let table = standings::compute(&mut tx, &league, &config.league_match).await?;
    tx.commit().await?;
    Ok(Json(table))
}
