//! Player directory (`/api/v1/players`) and admin moderation (`/api/v1/admin/players`).

use axum::{Json, extract::State, http::StatusCode};
use rust_decimal::Decimal;
use serde::Deserialize;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use crate::{
    ApiError, ApiResult, AppState,
    auth::CurrentPlayer,
    extract::{ApiPath, ApiQuery},
    models::{Page, PageParams, PlayPref, PlayerRole, PlayerStatus, paginate},
    players::{self, PLAYER_COLUMNS, PlayerPublic, PlayerRow},
};

/// Directory filters.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DirectoryQuery {
    /// Minimum UTR (inclusive).
    #[param(value_type = Option<f64>)]
    pub utr_min: Option<Decimal>,
    /// Maximum UTR (inclusive).
    #[param(value_type = Option<f64>)]
    pub utr_max: Option<Decimal>,
    /// `singles` or `doubles` also match players who play `any`.
    pub play_pref: Option<PlayPref>,
    /// Case-insensitive substring of one of the player's preferred locations.
    pub location: Option<String>,
    /// Case-insensitive substring of the display name.
    #[serde(rename = "q")]
    pub search: Option<String>,
    /// Admins only: players in this status instead of active members (`banned`, `deleted`);
    /// these lists include unverified accounts.
    pub status: Option<PlayerStatus>,
    /// Opaque cursor from the previous page's `next_cursor`.
    pub cursor: Option<String>,
    /// Page size.
    pub limit: Option<i64>,
}

/// Lists verified, active players of the community (excluding the caller); admins may list
/// banned or deleted members instead.
#[utoipa::path(get, path = "/api/v1/players", tag = "players", params(DirectoryQuery),
    security(("bearer" = [])),
    responses((status = 200, body = Page<PlayerPublic>), (status = 401, body = crate::error::ErrorBody)))]
pub async fn list_players(
    State(state): State<AppState>,
    viewer: CurrentPlayer,
    ApiQuery(query): ApiQuery<DirectoryQuery>,
) -> ApiResult<Json<Page<PlayerPublic>>> {
    let page = PageParams { cursor: query.cursor.clone(), limit: query.limit };
    let limit = page.limit();
    let status = query.status.unwrap_or(PlayerStatus::Active);
    if status != PlayerStatus::Active {
        viewer.require_admin()?;
    }
    let mut qb: QueryBuilder<'_, Postgres> = QueryBuilder::new(format!(
        "SELECT {PLAYER_COLUMNS} FROM players p JOIN users u ON u.id = p.user_id
         WHERE p.status = "
    ));
    let _ = qb.push_bind(status);
    if status == PlayerStatus::Active {
        let _ = qb.push(" AND u.email_verified_at IS NOT NULL");
    }
    let _ = qb
        .push(" AND p.community_id = ")
        .push_bind(viewer.tenant.id())
        .push(" AND p.id <> ")
        .push_bind(viewer.id);
    if let Some(cursor) = page.uuid_cursor()? {
        let _ = qb.push(" AND p.id > ").push_bind(cursor);
    }
    if let Some(min) = query.utr_min {
        let _ = qb.push(" AND p.utr >= ").push_bind(min);
    }
    if let Some(max) = query.utr_max {
        let _ = qb.push(" AND p.utr <= ").push_bind(max);
    }
    match query.play_pref {
        Some(PlayPref::Any) | None => {}
        Some(pref) => {
            let _ = qb.push(" AND p.play_pref IN ('any', ").push_bind(pref).push(")");
        }
    }
    if let Some(loc) =
        query.location.as_deref().map(str::trim).filter(|loc_name| !loc_name.is_empty())
    {
        let _ = qb
            .push(
                " AND EXISTS (SELECT 1 FROM jsonb_array_elements_text(p.preferred_locations) l
              WHERE l ILIKE '%' || ",
            )
            .push_bind(loc.to_owned())
            .push(" || '%')");
    }
    if let Some(name) = query.search.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        let _ =
            qb.push(" AND p.display_name ILIKE '%' || ").push_bind(name.to_owned()).push(" || '%'");
    }
    let _ = qb.push(" ORDER BY p.id LIMIT ").push_bind(limit + 1);

    let mut tx = viewer.tenant.begin(&state.db).await?;
    let rows: Vec<PlayerRow> = qb.build_query_as().fetch_all(&mut *tx).await?;
    tx.commit().await?;
    let verified = viewer.user.is_verified();
    let page = paginate(rows, limit, |row| row.id.to_string());
    Ok(Json(Page {
        items: page.items.into_iter().map(|row| PlayerPublic::redacted(row, verified)).collect(),
        next_cursor: page.next_cursor,
    }))
}

/// One player's public profile. Unverified or inactive players are visible to admins only.
#[utoipa::path(get, path = "/api/v1/players/{id}", tag = "players",
    params(("id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 200, body = PlayerPublic), (status = 404, body = crate::error::ErrorBody)))]
pub async fn get_player(
    State(state): State<AppState>,
    viewer: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<PlayerPublic>> {
    let mut tx = viewer.tenant.begin(&state.db).await?;
    let row = players::load(&mut tx, id).await?;
    tx.commit().await?;
    let row = row
        .filter(|player| {
            viewer.role.is_admin()
                || player.id == viewer.id
                || (player.status == PlayerStatus::Active && player.email_verified)
        })
        .ok_or(ApiError::NotFound("player"))?;
    Ok(Json(PlayerPublic::redacted(row, viewer.user.is_verified())))
}

async fn set_status(
    state: &AppState,
    admin: &CurrentPlayer,
    id: Uuid,
    status: PlayerStatus,
) -> ApiResult<StatusCode> {
    admin.require_admin()?;
    if id == admin.id {
        return Err(ApiError::validation("you cannot change your own status"));
    }
    let mut tx = admin.tenant.begin(&state.db).await?;
    let target = players::load(&mut tx, id).await?.ok_or(ApiError::NotFound("player"))?;
    if target.status == PlayerStatus::Deleted {
        return Err(ApiError::conflict("player has deleted their account"));
    }
    let outranks = match target.role {
        PlayerRole::Player => true,
        PlayerRole::Admin => admin.role == PlayerRole::Owner,
        PlayerRole::Owner => false,
    };
    if !outranks {
        return Err(ApiError::forbidden("cannot moderate a player of equal or higher role"));
    }
    let _ = sqlx::query(
        "UPDATE players SET status = $3, updated_at = now() WHERE community_id = $1 AND id = $2",
    )
    .bind(admin.tenant.id())
    .bind(id)
    .bind(status)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    tracing::info!(admin = %admin.id, player = %id, ?status, "player status changed");
    Ok(StatusCode::NO_CONTENT)
}

/// Bans a player from the community (admin).
#[utoipa::path(post, path = "/api/v1/admin/players/{id}/ban", tag = "admin",
    params(("id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 204), (status = 403, body = crate::error::ErrorBody), (status = 404, body = crate::error::ErrorBody)))]
pub async fn ban_player(
    State(state): State<AppState>,
    admin: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<StatusCode> {
    set_status(&state, &admin, id, PlayerStatus::Banned).await
}

/// Lifts a ban (admin).
#[utoipa::path(post, path = "/api/v1/admin/players/{id}/unban", tag = "admin",
    params(("id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 204), (status = 403, body = crate::error::ErrorBody), (status = 404, body = crate::error::ErrorBody)))]
pub async fn unban_player(
    State(state): State<AppState>,
    admin: CurrentPlayer,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<StatusCode> {
    set_status(&state, &admin, id, PlayerStatus::Active).await
}
