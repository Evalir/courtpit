//! Player rows and their public (redacted) projection.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, types::Json};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    ApiError, TenantTx,
    models::{Gender, PlayPref, PlayerRole, PlayerStatus},
};

/// Columns for [`PlayerRow`]; expects `players p JOIN users u`.
pub const PLAYER_COLUMNS: &str = "p.id, p.user_id, p.display_name, p.utr, p.gender, p.phone, \
    p.phone_visible, p.socials, p.socials_visible, p.racket, p.strings, p.tension_kg, \
    p.play_pref, p.preferred_locations, p.role, p.status, p.created_at, \
    (u.email_verified_at IS NOT NULL) AS email_verified";

/// A full player row (tenant-scoped; read inside a `TenantTx`).
#[derive(Debug, Clone, FromRow)]
pub struct PlayerRow {
    /// Player id.
    pub id: Uuid,
    /// Owning user account id.
    pub user_id: Uuid,
    /// Name shown to other players.
    pub display_name: String,
    /// Universal Tennis Rating, if set.
    pub utr: Option<Decimal>,
    /// Self-declared gender.
    pub gender: Gender,
    /// Contact phone number, if set.
    pub phone: Option<String>,
    /// Whether the phone number is shown to other players.
    pub phone_visible: bool,
    /// Social handles keyed by network.
    pub socials: Json<Value>,
    /// Whether social handles are shown to other players.
    pub socials_visible: bool,
    /// Racket model, if set.
    pub racket: Option<String>,
    /// String setup, if set.
    pub strings: Option<String>,
    /// String tension in kilograms, if set.
    pub tension_kg: Option<Decimal>,
    /// Preferred match format.
    pub play_pref: PlayPref,
    /// Places the player likes to play.
    pub preferred_locations: Json<Vec<String>>,
    /// Role within the community.
    pub role: PlayerRole,
    /// Membership status.
    pub status: PlayerStatus,
    /// When the player joined.
    pub created_at: DateTime<Utc>,
    /// Whether the account email is verified.
    pub email_verified: bool,
}

/// Loads one player of the transaction's community.
pub async fn load(tx: &mut TenantTx, id: Uuid) -> Result<Option<PlayerRow>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {PLAYER_COLUMNS} FROM players p JOIN users u ON u.id = p.user_id
         WHERE p.community_id = $1 AND p.id = $2"
    ))
    .bind(tx.community_id())
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
}

/// The owner's own view of their profile: everything, including gender and contact details.
#[derive(Debug, Serialize, ToSchema)]
pub struct PlayerProfile {
    /// Player id.
    pub id: Uuid,
    /// Name shown to other players.
    pub display_name: String,
    /// Universal Tennis Rating, if set.
    #[schema(value_type = Option<f64>)]
    pub utr: Option<Decimal>,
    /// Self-declared gender.
    pub gender: Gender,
    /// Contact phone number, if set.
    pub phone: Option<String>,
    /// Whether the phone number is shown to other players.
    pub phone_visible: bool,
    /// Social handles keyed by network.
    #[schema(value_type = std::collections::HashMap<String, String>)]
    pub socials: Value,
    /// Whether social handles are shown to other players.
    pub socials_visible: bool,
    /// Racket model, if set.
    pub racket: Option<String>,
    /// String setup, if set.
    pub strings: Option<String>,
    /// String tension in kilograms, if set.
    #[schema(value_type = Option<f64>)]
    pub tension_kg: Option<Decimal>,
    /// Preferred match format.
    pub play_pref: PlayPref,
    /// Places the player likes to play.
    pub preferred_locations: Vec<String>,
    /// Role within the community.
    pub role: PlayerRole,
    /// Membership status.
    pub status: PlayerStatus,
    /// When the player joined.
    pub created_at: DateTime<Utc>,
}

impl From<PlayerRow> for PlayerProfile {
    fn from(row: PlayerRow) -> Self {
        Self {
            id: row.id,
            display_name: row.display_name,
            utr: row.utr,
            gender: row.gender,
            phone: row.phone,
            phone_visible: row.phone_visible,
            socials: row.socials.0,
            socials_visible: row.socials_visible,
            racket: row.racket,
            strings: row.strings,
            tension_kg: row.tension_kg,
            play_pref: row.play_pref,
            preferred_locations: row.preferred_locations.0,
            role: row.role,
            status: row.status,
            created_at: row.created_at,
        }
    }
}

/// What other members see. Gender is never included; phone and socials only when the player
/// opted in *and* the viewer is a verified member.
#[derive(Debug, Serialize, ToSchema)]
pub struct PlayerPublic {
    /// Player id.
    pub id: Uuid,
    /// Name shown to other members.
    pub display_name: String,
    #[schema(value_type = Option<f64>)]
    /// Universal Tennis Rating, if set.
    pub utr: Option<Decimal>,
    /// Preferred match format.
    pub play_pref: PlayPref,
    /// Places the player likes to play.
    pub preferred_locations: Vec<String>,
    /// Racket model, if shared.
    pub racket: Option<String>,
    /// String setup, if shared.
    pub strings: Option<String>,
    #[schema(value_type = Option<f64>)]
    /// String tension in kilograms.
    pub tension_kg: Option<Decimal>,
    /// Phone number; only when the player opted in and the viewer is verified.
    pub phone: Option<String>,
    #[schema(value_type = Option<std::collections::HashMap<String, String>>)]
    /// Social handles; only when the player opted in and the viewer is verified.
    pub socials: Option<Value>,
    /// Role within the community.
    pub role: PlayerRole,
}

impl PlayerPublic {
    /// Redacts `p` for a viewer; `viewer_verified` gates contact details.
    pub fn redacted(row: PlayerRow, viewer_verified: bool) -> Self {
        let show_phone = viewer_verified && row.phone_visible;
        let show_socials = viewer_verified && row.socials_visible;
        Self {
            id: row.id,
            display_name: row.display_name,
            utr: row.utr,
            play_pref: row.play_pref,
            preferred_locations: row.preferred_locations.0,
            racket: row.racket,
            strings: row.strings,
            tension_kg: row.tension_kg,
            phone: row.phone.filter(|_| show_phone),
            socials: show_socials.then_some(row.socials.0),
            role: row.role,
        }
    }
}

/// Errors unless every id is an active, email-verified member of the transaction's community
/// (the players a match or entry may name).
pub async fn require_active_members(tx: &mut TenantTx, ids: &[Uuid]) -> Result<(), ApiError> {
    let found: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM players p JOIN users u ON u.id = p.user_id
         WHERE p.community_id = $1 AND p.id = ANY($2)
           AND p.status = 'active' AND u.email_verified_at IS NOT NULL",
    )
    .bind(tx.community_id())
    .bind(ids)
    .fetch_one(&mut **tx)
    .await?;
    if usize::try_from(found).ok() == Some(ids.len()) {
        Ok(())
    } else {
        Err(ApiError::validation(
            "every player must be an active, verified member of this community",
        ))
    }
}

/// A player id with the name to show for it. Views that list player ids carry these so
/// clients can render names without a request per player.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, FromRow, ToSchema)]
pub struct PlayerRef {
    /// Player id.
    pub id: Uuid,
    /// Name shown to other members ("Deleted player" for a deleted account).
    pub display_name: String,
}

/// Display names of some players of the transaction's community, loaded in one query.
#[derive(Debug, Default)]
pub struct Names(HashMap<Uuid, String>);

impl Names {
    /// Loads the names of `ids` (duplicates and ids that are not players here are ignored).
    pub async fn load(
        tx: &mut TenantTx,
        ids: impl IntoIterator<Item = Uuid>,
    ) -> Result<Self, sqlx::Error> {
        let mut ids: Vec<Uuid> = ids.into_iter().collect();
        ids.sort_unstable();
        ids.dedup();
        if ids.is_empty() {
            return Ok(Self::default());
        }
        let rows: Vec<PlayerRef> = sqlx::query_as(
            "SELECT id, display_name FROM players WHERE community_id = $1 AND id = ANY($2)",
        )
        .bind(tx.community_id())
        .bind(&ids)
        .fetch_all(&mut **tx)
        .await?;
        Ok(Self(
            rows.into_iter()
                .map(|row| (row.id, row.display_name))
                .collect(),
        ))
    }

    /// References for `ids` in first-seen order, without duplicates or unknown ids.
    pub fn refs(&self, ids: impl IntoIterator<Item = Uuid>) -> Vec<PlayerRef> {
        let mut refs: Vec<PlayerRef> = Vec::new();
        for id in ids {
            if refs.iter().any(|seen| seen.id == id) {
                continue;
            }
            if let Some(name) = self.0.get(&id) {
                refs.push(PlayerRef {
                    id,
                    display_name: name.clone(),
                });
            }
        }
        refs
    }
}
