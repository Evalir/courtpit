//! League entries: rows, eligibility checks and loaders.

use chrono::{DateTime, Utc};
use courtpit_domain::Discipline;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{ApiError, TenantTx, auth::CurrentPlayer, models::Gender};

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
}

impl EntryView {
    /// The view of `e` for `viewer`.
    pub fn for_viewer(entry: EntryRow, viewer: &CurrentPlayer) -> Self {
        let involved = viewer.role.is_admin()
            || entry.player_ids.contains(&viewer.id)
            || entry.invited_partner_id == Some(viewer.id);
        Self {
            id: entry.id,
            league_id: entry.league_id,
            division_id: entry.division_id,
            invited_partner_id: entry.invited_partner_id.filter(|_| involved),
            player_ids: entry.player_ids,
            created_by: entry.created_by,
            status: entry.status,
            looking_for_partner: entry.looking_for_partner,
            created_at: entry.created_at,
        }
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

/// Mixed doubles needs exactly one `female` and one `male` player; `other` and
/// `undisclosed` are not eligible (spec §9). With one player, checks they are eligible.
pub async fn check_mixed(
    tx: &mut TenantTx,
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
    let female = genders
        .iter()
        .filter(|gender| **gender == Gender::Female)
        .count();
    let male = genders
        .iter()
        .filter(|gender| **gender == Gender::Male)
        .count();
    let eligible = female + male == genders.len()
        && genders.len() == players.len()
        && female <= 1
        && male <= 1;
    if eligible {
        Ok(())
    } else if players.len() == 1 {
        Err(ApiError::validation(
            "mixed doubles needs one female and one male player; set your gender to female \
             or male on your profile to enter (other and undisclosed are not eligible)",
        ))
    } else {
        Err(ApiError::validation(
            "mixed doubles needs one female and one male player (other and undisclosed are \
             not eligible)",
        ))
    }
}
