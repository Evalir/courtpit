//! League entries: rows, eligibility checks and loaders.

use chrono::{DateTime, Utc};
use courtpit_domain::Discipline;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    ApiError, Tenant, TenantTx,
    auth::CurrentPlayer,
    communities::CommunitySettings,
    models::Gender,
    players::{Names, PlayerRef},
};

/// Where an entry stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[sqlx(type_name = "entry_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum EntryStatus {
    /// Doubles/mixed entry still missing its second player.
    PendingPartner,
    /// Waiting for an entry fee (payments step; never set yet).
    PendingPayment,
    /// Complete and counted in the league.
    Confirmed,
    /// Withdrawn by its players or an admin; no longer counted.
    Withdrawn,
}

/// Columns for [`EntryRow`].
pub const ENTRY_COLUMNS: &str = "id, league_id, division_id, player_ids, created_by, status, \
    looking_for_partner, invited_partner_id, created_at";

/// A `league_entries` row.
#[derive(Debug, Clone, FromRow)]
pub struct EntryRow {
    /// Entry id.
    pub id: Uuid,
    /// League the entry is in.
    pub league_id: Uuid,
    /// Division the entry was placed in, once drawn.
    pub division_id: Option<Uuid>,
    /// The entry's players: one for singles or a solo entry, two for a pair.
    pub player_ids: Vec<Uuid>,
    /// Player who registered the entry.
    pub created_by: Uuid,
    /// Where the entry stands.
    pub status: EntryStatus,
    /// Solo entry listed as looking for a partner.
    pub looking_for_partner: bool,
    /// Player invited to complete the entry, if any.
    pub invited_partner_id: Option<Uuid>,
    /// When the entry was registered.
    pub created_at: DateTime<Utc>,
}

/// An entry as the API shows it.
#[derive(Debug, Serialize, ToSchema)]
pub struct EntryView {
    /// Entry id.
    pub id: Uuid,
    /// League the entry is in.
    pub league_id: Uuid,
    /// Division the entry was placed in, once drawn.
    pub division_id: Option<Uuid>,
    /// The entry's players: one for singles or a solo entry, two for a pair.
    pub player_ids: Vec<Uuid>,
    /// Player who registered the entry.
    pub created_by: Uuid,
    /// Where the entry stands.
    pub status: EntryStatus,
    /// Solo entry listed as looking for a partner.
    pub looking_for_partner: bool,
    /// The invited partner; shown only to the entry's players, the invitee and admins.
    pub invited_partner_id: Option<Uuid>,
    /// When the entry was registered.
    pub created_at: DateTime<Utc>,
    /// Names for the entry's players, its creator and (when shown) the invited partner.
    pub names: Vec<PlayerRef>,
}

impl EntryView {
    /// The views of `entries` for `viewer`, with every name loaded in one query.
    pub async fn for_viewer(
        tx: &mut TenantTx,
        entries: Vec<EntryRow>,
        viewer: &CurrentPlayer,
    ) -> Result<Vec<Self>, ApiError> {
        let mentioned = |entry: &EntryRow| -> Vec<Uuid> {
            entry
                .player_ids
                .iter()
                .copied()
                .chain([entry.created_by])
                .chain(entry.invited_partner_id)
                .collect()
        };
        let names = Names::load(tx, entries.iter().flat_map(mentioned)).await?;
        Ok(entries
            .into_iter()
            .map(|entry| {
                let involved = viewer.role.is_admin()
                    || entry.player_ids.contains(&viewer.id)
                    || entry.invited_partner_id == Some(viewer.id);
                let invited_partner_id = entry.invited_partner_id.filter(|_| involved);
                let names = names.refs(
                    entry
                        .player_ids
                        .iter()
                        .copied()
                        .chain([entry.created_by])
                        .chain(invited_partner_id),
                );
                Self {
                    id: entry.id,
                    league_id: entry.league_id,
                    division_id: entry.division_id,
                    invited_partner_id,
                    player_ids: entry.player_ids,
                    created_by: entry.created_by,
                    status: entry.status,
                    looking_for_partner: entry.looking_for_partner,
                    created_at: entry.created_at,
                    names,
                }
            })
            .collect())
    }

    /// The view of one entry for `viewer`.
    pub async fn one_for_viewer(
        tx: &mut TenantTx,
        entry: EntryRow,
        viewer: &CurrentPlayer,
    ) -> Result<Self, ApiError> {
        Self::for_viewer(tx, vec![entry], viewer)
            .await?
            .pop()
            .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("entry view vanished")))
    }
}

/// Loads an entry of `league`, locked.
pub async fn load(tx: &mut TenantTx, league: Uuid, id: Uuid) -> Result<EntryRow, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {ENTRY_COLUMNS} FROM league_entries
         WHERE community_id = $1 AND league_id = $2 AND id = $3 FOR UPDATE"
    ))
    .bind(tx.community_id())
    .bind(league)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(ApiError::NotFound("entry"))
}

/// Errors if `player` already plays in a live (not withdrawn) entry of `league`.
pub async fn require_not_entered(
    tx: &mut TenantTx,
    league: Uuid,
    player: Uuid,
    who: &str,
) -> Result<(), ApiError> {
    let entered: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM league_entries
         WHERE community_id = $1 AND league_id = $2 AND status <> 'withdrawn'
           AND player_ids @> ARRAY[$3]::uuid[])",
    )
    .bind(tx.community_id())
    .bind(league)
    .bind(player)
    .fetch_one(&mut **tx)
    .await?;
    if entered {
        Err(ApiError::conflict(format!(
            "{who} already in an entry of this league"
        )))
    } else {
        Ok(())
    }
}

/// Errors if `player` is already paired in `league`: a live entry with two players, or a
/// singles entry. A solo doubles entry still waiting for its partner does not count, so a
/// player looking for a partner can be invited (accepting withdraws their solo entry).
pub async fn require_unpaired(
    tx: &mut TenantTx,
    league: Uuid,
    player: Uuid,
    who: &str,
) -> Result<(), ApiError> {
    let paired: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM league_entries
         WHERE community_id = $1 AND league_id = $2 AND status <> 'withdrawn'
           AND player_ids @> ARRAY[$3]::uuid[]
           AND NOT (status = 'pending_partner' AND cardinality(player_ids) = 1))",
    )
    .bind(tx.community_id())
    .bind(league)
    .bind(player)
    .fetch_one(&mut **tx)
    .await?;
    if paired {
        Err(ApiError::conflict(format!(
            "{who} already in an entry of this league"
        )))
    } else {
        Ok(())
    }
}

/// Checks mixed-doubles eligibility under the community's `mixed_eligibility` setting
/// (spec §9; the domain crate holds the rule). With one player, checks they are eligible
/// to register looking for a partner. Other disciplines have no gender rule.
pub async fn check_mixed(
    tx: &mut TenantTx,
    tenant: &Tenant,
    discipline: Discipline,
    players: &[Uuid],
) -> Result<(), ApiError> {
    if discipline != Discipline::Mixed {
        return Ok(());
    }
    let genders: Vec<Gender> =
        sqlx::query_scalar("SELECT gender FROM players WHERE community_id = $1 AND id = ANY($2)")
            .bind(tx.community_id())
            .bind(players)
            .fetch_all(&mut **tx)
            .await?;
    if genders.len() != players.len() {
        return Err(ApiError::validation(
            "every player of a mixed entry must be a member of this community",
        ));
    }
    let genders: Vec<courtpit_domain::Gender> = genders.into_iter().map(Into::into).collect();
    CommunitySettings::of(tenant)
        .mixed_eligibility
        .check(&genders)
        .map_err(|err| ApiError::validation(err.to_string()))
}
