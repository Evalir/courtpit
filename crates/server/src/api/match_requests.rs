//! `/api/v1/match-requests`: open calls for a friendly match that others join. When the last
//! slot is taken the request is `filled` and a friendly match is created.
//!
//! Side assignment when filling: players are ordered creator, bring-your-own partner (if
//! any), then joiners by join time; the first half is side A. So in doubles without a
//! partner the creator plays with the first joiner against the next two.

use axum::{Json, extract::State, http::StatusCode};
use chrono::{DateTime, Duration, Utc};
use racquetcollective_domain::Discipline;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, QueryBuilder};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    ApiError, ApiResult, AppState, TenantTx,
    api::{matches::community_format, proposals},
    auth::CurrentPlayer,
    extract::{ApiJson, ApiPath, ApiQuery},
    matches::{self, DbDiscipline, NewMatch},
    models::{Page, PageParams, paginate},
    notify,
    players::{self, Names, PlayerRef},
};

/// Furthest ahead a request's window may end.
const MAX_AHEAD_DAYS: i64 = 90;

/// Postgres `match_request_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[sqlx(type_name = "match_request_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum RequestStatus {
    /// Still looking for players.
    Open,
    /// Every slot is taken and the match was created.
    Filled,
    /// Withdrawn by its creator or an admin.
    Cancelled,
}

const REQUEST_COLUMNS: &str = "r.id, r.created_by, r.discipline, r.partner_id, r.slots_open, \
    r.utr_min, r.utr_max, r.time_window_start, r.time_window_end, r.location, r.status, \
    r.match_id, r.created_at, \
    coalesce((SELECT array_agg(j.player_id ORDER BY j.joined_at, j.player_id) \
              FROM match_request_joins j WHERE j.request_id = r.id), '{}') AS joined";

#[derive(Debug, Clone, FromRow)]
struct RequestRow {
    id: Uuid,
    created_by: Uuid,
    discipline: DbDiscipline,
    partner_id: Option<Uuid>,
    slots_open: i32,
    utr_min: Option<Decimal>,
    utr_max: Option<Decimal>,
    time_window_start: DateTime<Utc>,
    time_window_end: DateTime<Utc>,
    location: Option<String>,
    status: RequestStatus,
    match_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    joined: Vec<Uuid>,
}

impl RequestRow {
    /// Creator, partner, then joiners in join order.
    fn participants(&self) -> Vec<Uuid> {
        std::iter::once(self.created_by)
            .chain(self.partner_id)
            .chain(self.joined.iter().copied())
            .collect()
    }
}

/// A match request as the API shows it.
#[derive(Debug, Serialize, ToSchema)]
pub struct MatchRequestView {
    /// Request id.
    pub id: Uuid,
    /// Player who opened the request.
    pub created_by: Uuid,
    /// Singles or doubles.
    pub discipline: Discipline,
    /// Creator, their partner (if they brought one), then joiners in join order.
    pub players: Vec<Uuid>,
    /// Players still needed to fill the match.
    pub slots_open: i32,
    /// Lowest UTR allowed to join, if bounded.
    #[schema(value_type = Option<f64>)]
    pub utr_min: Option<Decimal>,
    /// Highest UTR allowed to join, if bounded.
    #[schema(value_type = Option<f64>)]
    pub utr_max: Option<Decimal>,
    /// Start of the window in which the match should be played.
    pub time_window_start: DateTime<Utc>,
    /// End of the window in which the match should be played.
    pub time_window_end: DateTime<Utc>,
    /// Free-text place to play.
    pub location: Option<String>,
    /// Whether the request is open, filled or cancelled.
    pub status: RequestStatus,
    /// The match created when the request filled.
    pub match_id: Option<Uuid>,
    /// When the request was opened.
    pub created_at: DateTime<Utc>,
    /// Names of `players`, in the same order.
    pub names: Vec<PlayerRef>,
}

impl MatchRequestView {
    fn new(row: RequestRow, names: &Names) -> Self {
        let players = row.participants();
        Self {
            names: names.refs(players.iter().copied()),
            players,
            id: row.id,
            created_by: row.created_by,
            discipline: row.discipline.into(),
            slots_open: row.slots_open,
            utr_min: row.utr_min,
            utr_max: row.utr_max,
            time_window_start: row.time_window_start,
            time_window_end: row.time_window_end,
            location: row.location,
            status: row.status,
            match_id: row.match_id,
            created_at: row.created_at,
        }
    }
}

/// Views of `rows`, with every name loaded in one query.
async fn views(tx: &mut TenantTx, rows: Vec<RequestRow>) -> ApiResult<Vec<MatchRequestView>> {
    let names = Names::load(tx, rows.iter().flat_map(RequestRow::participants)).await?;
    Ok(rows
        .into_iter()
        .map(|row| MatchRequestView::new(row, &names))
        .collect())
}

/// The view of one request.
async fn load_view(tx: &mut TenantTx, id: Uuid) -> ApiResult<MatchRequestView> {
    let row = load(tx, id, false).await?;
    let names = Names::load(tx, row.participants()).await?;
    Ok(MatchRequestView::new(row, &names))
}

async fn load(tx: &mut TenantTx, id: Uuid, lock: bool) -> ApiResult<RequestRow> {
    let sql = format!(
        "SELECT {REQUEST_COLUMNS} FROM match_requests r WHERE r.community_id = $1 AND r.id = $2{}",
        if lock { " FOR UPDATE OF r" } else { "" }
    );
    sqlx::query_as(&sql)
        .bind(tx.community_id())
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(ApiError::NotFound("match request"))
}

fn utr(value: Option<f64>, field: &str) -> ApiResult<Option<Decimal>> {
    value
        .map(|value| {
            Decimal::try_from(value)
                .ok()
                .map(|rounded| rounded.round_dp(2))
                .filter(|rounded| *rounded >= Decimal::ONE && *rounded <= Decimal::new(1650, 2))
                .ok_or_else(|| ApiError::validation(format!("{field} must be 1.00–16.50")))
        })
        .transpose()
}

/// Body of `POST /match-requests`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    /// Singles or doubles.
    pub discipline: Discipline,
    /// Doubles only: bring your own partner and look for two opponents.
    pub partner_id: Option<Uuid>,
    /// Lowest UTR allowed to join (1.00-16.50).
    pub utr_min: Option<f64>,
    /// Highest UTR allowed to join (1.00-16.50).
    pub utr_max: Option<f64>,
    /// Start of the window in which the match should be played.
    pub time_window_start: DateTime<Utc>,
    /// End of the window in which the match should be played.
    pub time_window_end: DateTime<Utc>,
    /// Free-text place to play.
    pub location: Option<String>,
}

/// Opens a call for players: others in the UTR band join until every slot is taken.
#[utoipa::path(post, path = "/api/v1/match-requests", tag = "match-requests",
    request_body = CreateRequest, security(("bearer" = [])),
    responses((status = 201, body = MatchRequestView),
        (status = 403, body = crate::error::ErrorBody),
        (status = 422, body = crate::error::ErrorBody)))]
pub async fn create_request(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiJson(body): ApiJson<CreateRequest>,
) -> ApiResult<(StatusCode, Json<MatchRequestView>)> {
    player.require_verified()?;
    let now = state.clock.now();
    if body.time_window_end <= body.time_window_start {
        return Err(ApiError::validation(
            "the time window must end after it starts",
        ));
    }
    if body.time_window_end <= now || body.time_window_end > now + Duration::days(MAX_AHEAD_DAYS) {
        return Err(ApiError::validation(format!(
            "the time window must end in the future, within {MAX_AHEAD_DAYS} days"
        )));
    }
    let (utr_min, utr_max) = (utr(body.utr_min, "utr_min")?, utr(body.utr_max, "utr_max")?);
    if let (Some(lo), Some(hi)) = (utr_min, utr_max)
        && lo > hi
    {
        return Err(ApiError::validation("utr_min must not exceed utr_max"));
    }
    let per_side = body.discipline.players_per_side();
    if body.partner_id.is_some() && per_side == 1 {
        return Err(ApiError::validation("singles has no partner"));
    }
    if body.partner_id == Some(player.id) {
        return Err(ApiError::validation("you cannot partner yourself"));
    }
    let slots = 2 * per_side - 1 - usize::from(body.partner_id.is_some());
    let location = proposals::location(body.location)?;

    let mut tx = player.tenant.begin(&state.db).await?;
    let named: Vec<Uuid> = std::iter::once(player.id).chain(body.partner_id).collect();
    players::require_active_members(&mut tx, &named).await?;
    let id = Uuid::now_v7();
    let _ = sqlx::query(
        "INSERT INTO match_requests (id, community_id, created_by, discipline, partner_id,
            slots_open, utr_min, utr_max, time_window_start, time_window_end, location)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(id)
    .bind(tx.community_id())
    .bind(player.id)
    .bind(DbDiscipline::from(body.discipline))
    .bind(body.partner_id)
    .bind(i32::try_from(slots).unwrap_or(1))
    .bind(utr_min)
    .bind(utr_max)
    .bind(body.time_window_start)
    .bind(body.time_window_end)
    .bind(location)
    .execute(&mut *tx)
    .await?;
    let view = load_view(&mut tx, id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(view)))
}

/// Filters for `GET /match-requests`.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RequestQuery {
    /// Only requests of this discipline.
    pub discipline: Option<Discipline>,
    /// Only requests whose UTR band includes your UTR.
    #[serde(default)]
    pub fits_me: bool,
    /// Pagination cursor from a previous page.
    pub cursor: Option<String>,
    /// Maximum items per page.
    pub limit: Option<i64>,
}

/// Lists open requests whose window hasn't ended, soonest window first.
#[utoipa::path(get, path = "/api/v1/match-requests", tag = "match-requests",
    params(RequestQuery), security(("bearer" = [])),
    responses((status = 200, body = Page<MatchRequestView>)))]
pub async fn list_requests(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiQuery(query): ApiQuery<RequestQuery>,
) -> ApiResult<Json<Page<MatchRequestView>>> {
    let page = PageParams {
        cursor: query.cursor.clone(),
        limit: query.limit,
    };
    let limit = page.limit();
    let mut tx = player.tenant.begin(&state.db).await?;
    let mut qb: QueryBuilder<'_, Postgres> = QueryBuilder::new(format!(
        "SELECT {REQUEST_COLUMNS} FROM match_requests r
         WHERE r.status = 'open' AND r.community_id = "
    ));
    let _ = qb
        .push_bind(tx.community_id())
        .push(" AND r.time_window_end > ")
        .push_bind(state.clock.now());
    if let Some(discipline) = query.discipline {
        let _ = qb
            .push(" AND r.discipline = ")
            .push_bind(DbDiscipline::from(discipline));
    }
    if query.fits_me {
        let _ = qb
            .push(
                " AND EXISTS (SELECT 1 FROM players p WHERE p.community_id = r.community_id
              AND p.id = ",
            )
            .push_bind(player.id)
            .push(
                " AND (r.utr_min IS NULL OR p.utr >= r.utr_min)
              AND (r.utr_max IS NULL OR p.utr <= r.utr_max))",
            );
    }
    // Cursor = "<window_end rfc3339>|<id>" keeps the soonest-first order stable.
    if let Some(cursor) = page.cursor.as_deref() {
        let (end, id) = cursor
            .split_once('|')
            .and_then(|(end_text, id_text)| {
                Some((
                    end_text.parse::<DateTime<Utc>>().ok()?,
                    id_text.parse::<Uuid>().ok()?,
                ))
            })
            .ok_or_else(|| ApiError::BadRequest("invalid cursor".into()))?;
        let _ = qb
            .push(" AND (r.time_window_end, r.id) > (")
            .push_bind(end)
            .push(", ")
            .push_bind(id)
            .push(")");
    }
    let _ = qb
        .push(" ORDER BY r.time_window_end, r.id LIMIT ")
        .push_bind(limit + 1);
    let rows: Vec<RequestRow> = qb.build_query_as().fetch_all(&mut *tx).await?;
    let page = paginate(rows, limit, |row| {
        format!(
            "{}|{}",
            row.time_window_end
                .to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            row.id
        )
    });
    let items = views(&mut tx, page.items).await?;
    tx.commit().await?;
    Ok(Json(Page {
        items,
        next_cursor: page.next_cursor,
    }))
}

/// One match request.
#[utoipa::path(get, path = "/api/v1/match-requests/{id}", tag = "match-requests",
    params(("id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 200, body = MatchRequestView),
        (status = 404, body = crate::error::ErrorBody)))]
pub async fn get_request(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<MatchRequestView>> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let view = load_view(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

/// Takes a slot. Taking the last one fills the request and creates the match (status
/// `proposed`, with the creator proposing the start of the window when it is still ahead).
#[utoipa::path(post, path = "/api/v1/match-requests/{id}/join", tag = "match-requests",
    params(("id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 200, body = MatchRequestView),
        (status = 403, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody),
        (status = 422, body = crate::error::ErrorBody)))]
pub async fn join_request(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<MatchRequestView>> {
    player.require_verified()?;
    let now = state.clock.now();
    let mut tx = player.tenant.begin(&state.db).await?;
    let request = load(&mut tx, id, true).await?;
    if request.status != RequestStatus::Open || request.time_window_end <= now {
        return Err(ApiError::conflict("this request is no longer open"));
    }
    if request.participants().contains(&player.id) {
        return Err(ApiError::conflict("you are already in this request"));
    }
    let my_utr: Option<Decimal> =
        sqlx::query_scalar("SELECT utr FROM players WHERE community_id = $1 AND id = $2")
            .bind(tx.community_id())
            .bind(player.id)
            .fetch_one(&mut *tx)
            .await?;
    if request.utr_min.is_some() || request.utr_max.is_some() {
        let fits = my_utr.is_some_and(|utr| {
            request.utr_min.is_none_or(|lo| utr >= lo) && request.utr_max.is_none_or(|hi| utr <= hi)
        });
        if !fits {
            return Err(ApiError::validation(
                "your UTR is outside this request's band (set your UTR on your profile)",
            ));
        }
    }
    let _ = sqlx::query(
        "INSERT INTO match_request_joins (community_id, request_id, player_id, joined_at)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(tx.community_id())
    .bind(id)
    .bind(player.id)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    let slots_open: i32 = sqlx::query_scalar(
        "UPDATE match_requests SET slots_open = slots_open - 1, updated_at = now()
         WHERE community_id = $1 AND id = $2 RETURNING slots_open",
    )
    .bind(tx.community_id())
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if slots_open == 0 {
        fill(&player, &mut tx, id, now).await?;
    }
    let view = load_view(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

/// Creates the match for a request whose last slot was just taken.
async fn fill(
    player: &CurrentPlayer,
    tx: &mut TenantTx,
    id: Uuid,
    now: DateTime<Utc>,
) -> ApiResult<()> {
    let request = load(tx, id, false).await?;
    let everyone = request.participants();
    // Someone may have been banned since joining; don't create a match around them.
    players::require_active_members(tx, &everyone).await?;
    let discipline: Discipline = request.discipline.into();
    let (side_a, side_b) = everyone.split_at(discipline.players_per_side());
    let match_id = matches::insert(
        tx,
        &NewMatch {
            discipline,
            side_a,
            side_b,
            format: community_format(&player.tenant)?,
            created_by: Some(request.created_by),
            league: None,
        },
    )
    .await?;
    if request.time_window_start > now {
        let _ = matches::insert_proposal(
            tx,
            match_id,
            request.created_by,
            request.time_window_start,
            request.location.as_deref(),
        )
        .await?;
    }
    let _ = sqlx::query(
        "UPDATE match_requests SET status = 'filled', match_id = $3, updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(id)
    .bind(match_id)
    .execute(&mut **tx)
    .await?;
    let others = everyone.iter().copied().filter(|&other| other != player.id);
    notify::tell(tx, others, notify::Event::RequestFilled { match_id }, now).await?;
    Ok(())
}

/// Gives a slot back while the request is still open.
#[utoipa::path(post, path = "/api/v1/match-requests/{id}/leave", tag = "match-requests",
    params(("id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 200, body = MatchRequestView),
        (status = 404, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody)))]
pub async fn leave_request(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<MatchRequestView>> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let request = load(&mut tx, id, true).await?;
    if request.status != RequestStatus::Open {
        return Err(ApiError::conflict("this request is no longer open"));
    }
    if !request.joined.contains(&player.id) {
        return Err(ApiError::conflict(
            "you have not joined this request (creators cancel instead)",
        ));
    }
    let _ = sqlx::query(
        "DELETE FROM match_request_joins WHERE community_id = $1 AND request_id = $2
         AND player_id = $3",
    )
    .bind(tx.community_id())
    .bind(id)
    .bind(player.id)
    .execute(&mut *tx)
    .await?;
    let _ = sqlx::query(
        "UPDATE match_requests SET slots_open = slots_open + 1, updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let view = load_view(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

/// Withdraws an open request (its creator or an admin).
#[utoipa::path(post, path = "/api/v1/match-requests/{id}/cancel", tag = "match-requests",
    params(("id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 200, body = MatchRequestView),
        (status = 403, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody)))]
pub async fn cancel_request(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<MatchRequestView>> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let request = load(&mut tx, id, true).await?;
    if request.created_by != player.id && !player.role.is_admin() {
        return Err(ApiError::forbidden(
            "only the creator or an admin can cancel a request",
        ));
    }
    if request.status != RequestStatus::Open {
        return Err(ApiError::conflict("this request is no longer open"));
    }
    let _ = sqlx::query(
        "UPDATE match_requests SET status = 'cancelled', updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let view = load_view(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}
