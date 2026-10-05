//! League registration: `/api/v1/leagues/{id}/entries` and admin pairing of solo entries.
//!
//! No fees in this step, so a complete entry is `confirmed` straight away (the
//! `pending_payment` status is reserved for the payments step). Every change locks the league
//! row first, which serialises registration per league.

use axum::{Json, extract::State, http::StatusCode};
use courtpit_domain::Discipline;
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, QueryBuilder};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    ApiError, ApiResult, AppState, TenantTx,
    auth::CurrentPlayer,
    extract::{ApiJson, ApiPath, ApiQuery},
    leagues::{
        self, LEAGUE_COLUMNS, LeagueRow, LeagueStatus, LeagueView,
        entries::{self, ENTRY_COLUMNS, EntryRow, EntryStatus, EntryView},
    },
    players,
};

/// Loads a league the caller can see, locked, and requires registration to be open now.
async fn open_league(
    state: &AppState,
    tx: &mut TenantTx,
    id: Uuid,
    player: &CurrentPlayer,
) -> ApiResult<LeagueRow> {
    let league = leagues::load(tx, id, true).await?;
    if league.published_at.is_none() && !player.role.is_admin() {
        return Err(ApiError::NotFound("league"));
    }
    if !league.registration_open(state.clock.now()) {
        return Err(ApiError::conflict(
            "registration for this league is not open",
        ));
    }
    Ok(league)
}

async fn respond(
    tx: &mut TenantTx,
    league: Uuid,
    id: Uuid,
    viewer: &CurrentPlayer,
) -> ApiResult<Json<EntryView>> {
    let entry = entries::load(tx, league, id).await?;
    Ok(Json(EntryView::one_for_viewer(tx, entry, viewer).await?))
}

/// Body of `POST /leagues/{id}/entries`.
#[derive(Debug, Default, Clone, Copy, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Register {
    /// Doubles/mixed: invite this partner; the entry confirms when they accept.
    pub partner_id: Option<Uuid>,
    /// Doubles/mixed: list yourself for others (and admins) looking for a partner.
    #[serde(default)]
    pub looking_for_partner: bool,
}

/// Checks a doubles/mixed partner choice: an invited partner who may play with the caller
/// (a partner still on their own solo entry may be invited; accepting withdraws it), or a
/// solo listing as looking for one.
async fn check_partner(
    tx: &mut TenantTx,
    player: &CurrentPlayer,
    league: Uuid,
    discipline: Discipline,
    body: Register,
) -> ApiResult<()> {
    match body.partner_id {
        Some(partner) => {
            if partner == player.id {
                return Err(ApiError::validation("you cannot partner yourself"));
            }
            players::require_active_members(tx, &[partner]).await?;
            entries::require_unpaired(tx, league, partner, "your partner is").await?;
            entries::check_mixed(tx, &player.tenant, discipline, &[player.id, partner]).await
        }
        None if body.looking_for_partner => {
            entries::check_mixed(tx, &player.tenant, discipline, &[player.id]).await
        }
        None => Err(ApiError::validation(
            "name a partner_id or set looking_for_partner",
        )),
    }
}

/// Registers the caller. Singles entries are confirmed immediately; doubles and mixed
/// entries wait for the invited partner (or a pairing) as `pending_partner`.
#[utoipa::path(post, path = "/api/v1/leagues/{id}/entries", tag = "leagues",
    params(("id" = Uuid, Path)), request_body = Register, security(("bearer" = [])),
    responses((status = 201, body = EntryView), (status = 403, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody),
        (status = 422, body = crate::error::ErrorBody)))]
pub async fn register(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath(league_id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<Register>,
) -> ApiResult<(StatusCode, Json<EntryView>)> {
    player.require_verified()?;
    let mut tx = player.tenant.begin(&state.db).await?;
    let league = open_league(&state, &mut tx, league_id, &player).await?;
    let discipline = league.discipline();
    entries::require_not_entered(&mut tx, league_id, player.id, "you are").await?;
    let status = if discipline == Discipline::Singles {
        if body.partner_id.is_some() || body.looking_for_partner {
            return Err(ApiError::validation("singles entries have no partner"));
        }
        EntryStatus::Confirmed
    } else {
        check_partner(&mut tx, &player, league_id, discipline, body).await?;
        EntryStatus::PendingPartner
    };
    let id = Uuid::now_v7();
    let _ = sqlx::query(
        "INSERT INTO league_entries (id, community_id, league_id, player_ids, created_by, status,
            looking_for_partner, invited_partner_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(tx.community_id())
    .bind(league_id)
    .bind(vec![player.id])
    .bind(player.id)
    .bind(status)
    .bind(body.looking_for_partner && body.partner_id.is_none())
    .bind(body.partner_id)
    .execute(&mut *tx)
    .await?;
    let res = respond(&mut tx, league_id, id, &player).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, res))
}

/// Filters for `GET /leagues/{id}/entries`.
#[derive(Debug, Clone, Copy, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct EntryQuery {
    /// Defaults to every status except `withdrawn`.
    pub status: Option<EntryStatus>,
    /// Only solo entries looking for a partner.
    #[serde(default)]
    pub looking_for_partner: bool,
}

/// A league's entries, in registration order.
#[utoipa::path(get, path = "/api/v1/leagues/{id}/entries", tag = "leagues",
    params(("id" = Uuid, Path), EntryQuery), security(("bearer" = [])),
    responses((status = 200, body = Vec<EntryView>), (status = 404, body = crate::error::ErrorBody)))]
pub async fn list_entries(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath(league_id): ApiPath<Uuid>,
    ApiQuery(query): ApiQuery<EntryQuery>,
) -> ApiResult<Json<Vec<EntryView>>> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let league = leagues::load(&mut tx, league_id, false).await?;
    if league.published_at.is_none() && !player.role.is_admin() {
        return Err(ApiError::NotFound("league"));
    }
    let mut qb: QueryBuilder<'_, Postgres> = QueryBuilder::new(format!(
        "SELECT {ENTRY_COLUMNS} FROM league_entries WHERE community_id = "
    ));
    let _ = qb
        .push_bind(tx.community_id())
        .push(" AND league_id = ")
        .push_bind(league_id);
    let _ = match query.status {
        Some(status) => qb.push(" AND status = ").push_bind(status),
        None => qb.push(" AND status <> 'withdrawn'"),
    };
    if query.looking_for_partner {
        let _ = qb.push(" AND looking_for_partner AND cardinality(player_ids) = 1");
    }
    let _ = qb.push(" ORDER BY created_at, id");
    let rows: Vec<EntryRow> = qb.build_query_as().fetch_all(&mut *tx).await?;
    let views = EntryView::for_viewer(&mut tx, rows, &player).await?;
    tx.commit().await?;
    Ok(Json(views))
}

/// Completes a pending doubles/mixed entry with its second player and confirms it. Other
/// entries' invitations to either player lapse, so their creators can ask someone else.
async fn complete(tx: &mut TenantTx, entry: &EntryRow, partner: Uuid) -> ApiResult<()> {
    let pair = vec![entry.created_by, partner];
    let _ = sqlx::query(
        "UPDATE league_entries SET player_ids = $3, status = 'confirmed',
            looking_for_partner = false, invited_partner_id = NULL, updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(entry.id)
    .bind(&pair)
    .execute(&mut **tx)
    .await?;
    let _ = sqlx::query(
        "UPDATE league_entries SET invited_partner_id = NULL, updated_at = now()
         WHERE community_id = $1 AND league_id = $2 AND status = 'pending_partner'
           AND invited_partner_id = ANY($3)",
    )
    .bind(tx.community_id())
    .bind(entry.league_id)
    .bind(&pair)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Withdraws `player`'s own solo, still-pending entries in `league` (superseded when they
/// join someone else's entry).
async fn withdraw_solo(tx: &mut TenantTx, league: Uuid, player: Uuid) -> ApiResult<()> {
    let _ = sqlx::query(
        "UPDATE league_entries SET status = 'withdrawn', invited_partner_id = NULL,
            updated_at = now()
         WHERE community_id = $1 AND league_id = $2 AND status = 'pending_partner'
           AND player_ids = ARRAY[$3]::uuid[]",
    )
    .bind(tx.community_id())
    .bind(league)
    .bind(player)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Accepts a partner invitation; the entry is confirmed. Any solo entry of your own in the
/// league is withdrawn.
#[utoipa::path(post, path = "/api/v1/leagues/{id}/entries/{entry_id}/accept", tag = "leagues",
    params(("id" = Uuid, Path), ("entry_id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 200, body = EntryView), (status = 403, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody),
        (status = 422, body = crate::error::ErrorBody)))]
pub async fn accept_invite(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath((league_id, entry_id)): ApiPath<(Uuid, Uuid)>,
) -> ApiResult<Json<EntryView>> {
    player.require_verified()?;
    let mut tx = player.tenant.begin(&state.db).await?;
    let league = open_league(&state, &mut tx, league_id, &player).await?;
    let entry = entries::load(&mut tx, league_id, entry_id).await?;
    if entry.invited_partner_id != Some(player.id) {
        return Err(ApiError::forbidden("you were not invited to this entry"));
    }
    if entry.status != EntryStatus::PendingPartner {
        return Err(ApiError::conflict(
            "this entry is no longer waiting for a partner",
        ));
    }
    withdraw_solo(&mut tx, league_id, player.id).await?;
    entries::require_not_entered(&mut tx, league_id, player.id, "you are").await?;
    players::require_active_members(&mut tx, &[entry.created_by, player.id]).await?;
    let pair = [entry.created_by, player.id];
    entries::check_mixed(&mut tx, &player.tenant, league.discipline(), &pair).await?;
    complete(&mut tx, &entry, player.id).await?;
    let res = respond(&mut tx, league_id, entry_id, &player).await?;
    tx.commit().await?;
    Ok(res)
}

/// Declines a partner invitation. The entry stays pending for its creator.
#[utoipa::path(post, path = "/api/v1/leagues/{id}/entries/{entry_id}/decline", tag = "leagues",
    params(("id" = Uuid, Path), ("entry_id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 200, body = EntryView), (status = 403, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody)))]
pub async fn decline_invite(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath((league_id, entry_id)): ApiPath<(Uuid, Uuid)>,
) -> ApiResult<Json<EntryView>> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let _ = leagues::load(&mut tx, league_id, true).await?;
    let entry = entries::load(&mut tx, league_id, entry_id).await?;
    if entry.invited_partner_id != Some(player.id) {
        return Err(ApiError::forbidden("you were not invited to this entry"));
    }
    let _ = sqlx::query(
        "UPDATE league_entries SET invited_partner_id = NULL, updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(entry_id)
    .execute(&mut *tx)
    .await?;
    let res = respond(&mut tx, league_id, entry_id, &player).await?;
    tx.commit().await?;
    Ok(res)
}

/// Changes who a solo doubles/mixed entry waits for: invites a partner (replacing any open
/// invitation, e.g. after a decline) or lists the entry as looking for one. Only its creator,
/// only while registration is open.
#[utoipa::path(post, path = "/api/v1/leagues/{id}/entries/{entry_id}/partner", tag = "leagues",
    params(("id" = Uuid, Path), ("entry_id" = Uuid, Path)), request_body = Register,
    security(("bearer" = [])),
    responses((status = 200, body = EntryView), (status = 403, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody),
        (status = 422, body = crate::error::ErrorBody)))]
pub async fn change_partner(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath((league_id, entry_id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(body): ApiJson<Register>,
) -> ApiResult<Json<EntryView>> {
    player.require_verified()?;
    let mut tx = player.tenant.begin(&state.db).await?;
    let league = open_league(&state, &mut tx, league_id, &player).await?;
    let entry = entries::load(&mut tx, league_id, entry_id).await?;
    if entry.created_by != player.id {
        return Err(ApiError::forbidden(
            "only the player who registered the entry can change its partner",
        ));
    }
    if entry.status != EntryStatus::PendingPartner || entry.player_ids.len() != 1 {
        return Err(ApiError::conflict(
            "this entry is no longer waiting for a partner",
        ));
    }
    check_partner(&mut tx, &player, league_id, league.discipline(), body).await?;
    let _ = sqlx::query(
        "UPDATE league_entries SET invited_partner_id = $3, looking_for_partner = $4,
            updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(entry_id)
    .bind(body.partner_id)
    .bind(body.partner_id.is_none())
    .execute(&mut *tx)
    .await?;
    let res = respond(&mut tx, league_id, entry_id, &player).await?;
    tx.commit().await?;
    Ok(res)
}

/// One of the caller's entries or invitations, with its league.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyEntry {
    /// The league the entry is in.
    pub league: LeagueView,
    /// The entry, as its players see it.
    pub entry: EntryView,
}

/// The caller's live entries (their own, and entries inviting them as partner) in leagues
/// that have not finished or been cancelled, by league start. Bounded by the community's
/// live leagues, so not paginated.
#[utoipa::path(get, path = "/api/v1/me/entries", tag = "me", security(("bearer" = [])),
    responses((status = 200, body = Vec<MyEntry>)))]
pub async fn my_entries(
    State(state): State<AppState>,
    player: CurrentPlayer,
) -> ApiResult<Json<Vec<MyEntry>>> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let leagues: Vec<LeagueRow> = sqlx::query_as(&format!(
        "SELECT {LEAGUE_COLUMNS} FROM leagues
         WHERE community_id = $1 AND status IN ('draft', 'registration', 'active')
           AND id IN (SELECT league_id FROM league_entries
                      WHERE community_id = $1 AND status <> 'withdrawn'
                        AND (player_ids @> ARRAY[$2]::uuid[] OR invited_partner_id = $2))
         ORDER BY starts_at, id"
    ))
    .bind(tx.community_id())
    .bind(player.id)
    .fetch_all(&mut *tx)
    .await?;
    let ids: Vec<Uuid> = leagues.iter().map(|league| league.id).collect();
    let rows: Vec<EntryRow> = sqlx::query_as(&format!(
        "SELECT {ENTRY_COLUMNS} FROM league_entries
         WHERE community_id = $1 AND league_id = ANY($3) AND status <> 'withdrawn'
           AND (player_ids @> ARRAY[$2]::uuid[] OR invited_partner_id = $2)
         ORDER BY created_at, id"
    ))
    .bind(tx.community_id())
    .bind(player.id)
    .bind(&ids)
    .fetch_all(&mut *tx)
    .await?;
    let mut views = EntryView::for_viewer(&mut tx, rows, &player).await?;
    tx.commit().await?;
    let mut mine = Vec::with_capacity(views.len());
    for league in leagues {
        let id = league.id;
        let view = LeagueView::new(league, &player.tenant)?;
        let (here, rest): (Vec<_>, Vec<_>) =
            views.into_iter().partition(|entry| entry.league_id == id);
        views = rest;
        mine.extend(here.into_iter().map(|entry| MyEntry {
            league: view.clone(),
            entry,
        }));
    }
    Ok(Json(mine))
}

/// Withdraws an entry: its players while registration is open, admins until the league
/// finishes (matches already scheduled stay; admins award walkovers).
#[utoipa::path(post, path = "/api/v1/leagues/{id}/entries/{entry_id}/withdraw", tag = "leagues",
    params(("id" = Uuid, Path), ("entry_id" = Uuid, Path)), security(("bearer" = [])),
    responses((status = 200, body = EntryView), (status = 403, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody)))]
pub async fn withdraw(
    State(state): State<AppState>,
    player: CurrentPlayer,
    ApiPath((league_id, entry_id)): ApiPath<(Uuid, Uuid)>,
) -> ApiResult<Json<EntryView>> {
    let mut tx = player.tenant.begin(&state.db).await?;
    let league = leagues::load(&mut tx, league_id, true).await?;
    let entry = entries::load(&mut tx, league_id, entry_id).await?;
    let member = entry.player_ids.contains(&player.id);
    if !member && !player.role.is_admin() {
        return Err(ApiError::forbidden("this is not your entry"));
    }
    if entry.status == EntryStatus::Withdrawn {
        return Err(ApiError::conflict("the entry is already withdrawn"));
    }
    let allowed = if player.role.is_admin() {
        !matches!(
            league.status,
            LeagueStatus::Finished | LeagueStatus::Cancelled
        )
    } else {
        league.registration_open(state.clock.now())
    };
    if !allowed {
        return Err(ApiError::conflict(
            "entries can only be withdrawn while registration is open (ask an admin)",
        ));
    }
    let _ = sqlx::query(
        "UPDATE league_entries SET status = 'withdrawn', invited_partner_id = NULL,
            updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(entry_id)
    .execute(&mut *tx)
    .await?;
    let res = respond(&mut tx, league_id, entry_id, &player).await?;
    tx.commit().await?;
    Ok(res)
}

/// Body of `POST /admin/leagues/{id}/pair`.
#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PairBody {
    /// Two solo, pending entries. The first keeps its id and becomes the pair's entry; the
    /// second is withdrawn.
    pub entry_ids: [Uuid; 2],
}

/// Pairs two solo doubles/mixed entries into one confirmed entry (admin, before the draw).
#[utoipa::path(post, path = "/api/v1/admin/leagues/{id}/pair", tag = "admin",
    params(("id" = Uuid, Path)), request_body = PairBody, security(("bearer" = [])),
    responses((status = 200, body = EntryView), (status = 403, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody),
        (status = 409, body = crate::error::ErrorBody),
        (status = 422, body = crate::error::ErrorBody)))]
pub async fn pair_entries(
    State(state): State<AppState>,
    admin: CurrentPlayer,
    ApiPath(league_id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<PairBody>,
) -> ApiResult<Json<EntryView>> {
    admin.require_admin()?;
    let [first, second] = body.entry_ids;
    if first == second {
        return Err(ApiError::validation("pair two different entries"));
    }
    let mut tx = admin.tenant.begin(&state.db).await?;
    let league = leagues::load(&mut tx, league_id, true).await?;
    if league.status != LeagueStatus::Registration {
        return Err(ApiError::conflict("pairing happens before the draw"));
    }
    let first_entry = entries::load(&mut tx, league_id, first).await?;
    let second_entry = entries::load(&mut tx, league_id, second).await?;
    for entry in [&first_entry, &second_entry] {
        if entry.status != EntryStatus::PendingPartner || entry.player_ids.len() != 1 {
            return Err(ApiError::conflict(
                "both entries must be solo and waiting for a partner",
            ));
        }
    }
    let pair = [first_entry.created_by, second_entry.created_by];
    players::require_active_members(&mut tx, &pair).await?;
    entries::check_mixed(&mut tx, &admin.tenant, league.discipline(), &pair).await?;
    let _ = sqlx::query(
        "UPDATE league_entries SET status = 'withdrawn', invited_partner_id = NULL,
            looking_for_partner = false, updated_at = now()
         WHERE community_id = $1 AND id = $2",
    )
    .bind(tx.community_id())
    .bind(second_entry.id)
    .execute(&mut *tx)
    .await?;
    complete(&mut tx, &first_entry, second_entry.created_by).await?;
    let res = respond(&mut tx, league_id, first_entry.id, &admin).await?;
    tx.commit().await?;
    Ok(res)
}
