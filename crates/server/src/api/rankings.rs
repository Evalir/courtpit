//! `/api/v1/rankings`: the community leaderboard per discipline, and the ledger behind it.

use axum::{Json, extract::State};
use chrono::{DateTime, Utc};
use racquetcollective_domain::Discipline;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, QueryBuilder};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    ApiError, ApiResult, AppState,
    auth::CurrentPlayer,
    extract::ApiQuery,
    matches::DbDiscipline,
    models::{Page, PageParams, paginate},
};

/// Query for `GET /rankings`.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RankingQuery {
    /// Discipline whose leaderboard to list.
    pub discipline: Discipline,
    /// Opaque cursor from the previous page.
    pub cursor: Option<String>,
    /// Maximum number of rows to return.
    pub limit: Option<i64>,
}

/// One leaderboard line.
#[derive(Debug, Serialize, FromRow, ToSchema)]
pub struct RankingRow {
    /// Position on the leaderboard (ties share a rank).
    pub rank: i32,
    /// The ranked player.
    pub player_id: Uuid,
    /// The player's display name.
    pub display_name: String,
    /// Points earned in the last 52 weeks.
    pub points_52w: i32,
    /// When the rankings were last rebuilt.
    pub refreshed_at: DateTime<Utc>,
}

/// The leaderboard for a discipline, best first (ties share a rank). With
/// `mixed_pooling = doubles`, mixed points appear under `doubles` and `mixed` is empty.
#[utoipa::path(get, path = "/api/v1/rankings", tag = "rankings", params(RankingQuery),
    security(("bearer" = [])), responses((status = 200, body = Page<RankingRow>)))]
pub async fn list_rankings(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiQuery(query): ApiQuery<RankingQuery>,
) -> ApiResult<Json<Page<RankingRow>>> {
    let page = PageParams { cursor: query.cursor.clone(), limit: query.limit };
    let limit = page.limit();
    let mut tx = player.tenant.begin(&state.db).await?;
    let mut qb: QueryBuilder<'_, Postgres> = QueryBuilder::new(
        "SELECT r.rank, r.player_id, p.display_name, r.points_52w, r.refreshed_at
         FROM rankings r JOIN players p ON p.community_id = r.community_id AND p.id = r.player_id
         WHERE r.community_id = ",
    );
    let _ = qb
        .push_bind(tx.community_id())
        .push(" AND r.discipline = ")
        .push_bind(DbDiscipline::from(query.discipline));
    // Cursor = "<rank>|<player id>".
    if let Some(cursor) = page.cursor.as_deref() {
        let (rank, id) = cursor
            .split_once('|')
            .and_then(|(rank_text, id_text)| {
                Some((rank_text.parse::<i32>().ok()?, id_text.parse::<Uuid>().ok()?))
            })
            .ok_or_else(|| ApiError::BadRequest("invalid cursor".into()))?;
        let _ = qb
            .push(" AND (r.rank, r.player_id) > (")
            .push_bind(rank)
            .push(", ")
            .push_bind(id)
            .push(")");
    }
    let _ = qb.push(" ORDER BY r.rank, r.player_id LIMIT ").push_bind(limit + 1);
    let rows: Vec<RankingRow> = qb.build_query_as().fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(paginate(rows, limit, |row| format!("{}|{}", row.rank, row.player_id))))
}

/// Query for `GET /rankings/events`.
#[derive(Debug, Clone, Copy, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct EventQuery {
    /// Player whose ledger to list.
    pub player_id: Uuid,
    /// Restrict to one discipline.
    pub discipline: Option<Discipline>,
}

/// One ledger entry.
#[derive(Debug, Serialize, FromRow, ToSchema)]
pub struct LedgerEntry {
    /// Ledger entry id.
    pub id: Uuid,
    /// The discipline played (before any mixed pooling).
    pub discipline: String,
    /// `league_match`, `league_season` or `tournament`.
    pub source: String,
    /// The league match, season or tournament that earned the points.
    pub source_id: Uuid,
    /// Points awarded.
    pub points: i32,
    /// When the points were earned.
    pub occurred_at: DateTime<Utc>,
}

/// A player's ledger, newest first: every point behind their ranking (last 100 entries).
#[utoipa::path(get, path = "/api/v1/rankings/events", tag = "rankings", params(EventQuery),
    security(("bearer" = [])), responses((status = 200, body = Vec<LedgerEntry>)))]
pub async fn ledger(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiQuery(query): ApiQuery<EventQuery>,
) -> ApiResult<Json<Vec<LedgerEntry>>> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let mut qb: QueryBuilder<'_, Postgres> = QueryBuilder::new(
        "SELECT id, discipline::text AS discipline, source::text AS source, source_id, points,
                occurred_at
         FROM ranking_events WHERE community_id = ",
    );
    let _ = qb.push_bind(tx.community_id()).push(" AND player_id = ").push_bind(query.player_id);
    if let Some(discipline) = query.discipline {
        let _ = qb.push(" AND discipline = ").push_bind(DbDiscipline::from(discipline));
    }
    let _ = qb.push(" ORDER BY occurred_at DESC, id DESC LIMIT 100");
    let rows: Vec<LedgerEntry> = qb.build_query_as().fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(rows))
}
